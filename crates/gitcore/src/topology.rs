//! 结构化拓扑层:把 git log --parents 输出转为 lane/edge 描述,供前端 SVG 绘图。
//!
//! 本模块**不**接触颜色、坐标、SVG,只产出纯结构化的 `GraphCommit` 列表。
//! 每个 commit 记录它落在哪条 lane、到下一行的连线(edges),前端据此画 node + 曲线。

use crate::{Error, LogEntry, LogOptions, Repo};

/// 一条图形连线:从当前行的 lane 走到下一行的 lane。
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct GraphEdge {
    /// 来源 lane 序号。
    pub from_lane: usize,
    /// 目标 lane 序号。
    pub to_lane: usize,
}

/// 拓扑图中的一个 commit 节点:包含提交信息、它在哪条 lane、以及离开它向下的连线。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct GraphCommit {
    /// 提交本体。
    pub entry: LogEntry,
    /// 父提交的完整 SHA(按 git 顺序:第一个是"主" parent)。
    pub parents: Vec<String>,
    /// 本 commit 落在哪条 lane(0 起)。
    pub lane: usize,
    /// 本行所有活跃 lane 到下一行的连线(每条活跃 lane 一条 edge)。
    pub edges: Vec<GraphEdge>,
}

/// 多 root 合并图中的一个 commit:lane 已按 root 段偏移,edges 含其他 root 的 pass-through 竖线。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct MergedGraphCommit {
    /// 提交本体。
    pub entry: LogEntry,
    /// 紧凑共享车道的 lane 序号。
    pub lane: usize,
    /// 本行所有活跃 lane 到下一行的连线(含其他 root 的 pass-through 竖线)。
    pub edges: Vec<GraphEdge>,
    /// 与 `edges` 平行:每条 edge 所在车道的 root(repo_index),供前端按 root 上色。
    pub edge_roots: Vec<usize>,
    /// 0=主仓,1..N=子仓 —— 前端按此给节点 / stripe 上色。
    pub repo_index: usize,
    /// stripe tooltip / 标签。
    pub repo_label: String,
    /// 该提交所属仓库的绝对路径,供前端定位操作。
    pub repo_path: String,
}

/// 一个 root(主仓或子仓)的元信息,供前端上色 / stripe / 仓库切换。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RootMeta {
    /// 0=主仓,1..N=子仓(原始 repo_index)。
    pub index: usize,
    /// "主仓" 或子仓相对路径。
    pub label: String,
    /// 仓库绝对路径。
    pub path: String,
}

/// 多 root 合并拓扑图:选中 root 的元信息 + 合并后的提交。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct MergedGraphLog {
    /// 选中 root 列表(含各自 lane 段偏移),供前端 laneToRoot 映射。
    pub roots: Vec<RootMeta>,
    /// 合并图提交(全局 lane / edges)。
    pub commits: Vec<MergedGraphCommit>,
}

/// 获取结构化拓扑图:每个 commit 的 lane 分配 + lane 间连线。
///
/// 数据来源 `git log --parents`,自己算 lane 布局,不依赖 `--graph` 的 ASCII。
pub(crate) fn log_topology(repo: &Repo, opts: &LogOptions) -> Result<Vec<GraphCommit>, Error> {
    let max_count_str = format!("-{}", opts.max_count);
    let mut args = vec![
        "log",
        "--topo-order",
        "--pretty=format:%H%x00%P%x00%h%x00%s%x00%an%x00%ar",
        &max_count_str,
    ];

    let author_str;
    if let Some(ref a) = opts.author {
        author_str = format!("--author={}", a);
        args.push(&author_str);
        args.push("--regexp-ignore-case");
    }

    let grep_str;
    if let Some(ref g) = opts.grep {
        grep_str = format!("--grep={}", g);
        args.push(&grep_str);
        args.push("--regexp-ignore-case");
    }

    let branch_str;
    if let Some(ref b) = opts.branch {
        branch_str = b.clone();
        args.push(&branch_str);
    } else {
        args.push("--all"); // 「全部分支」= 所有 ref(对标 WebStorm),否则只查 HEAD
    }

    let output = repo.git(&args)?;

    let raw_commits: Vec<RawCommit> = output
        .lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            let parts: Vec<&str> = line.split('\0').collect();
            if parts.len() != 6 {
                return None;
            }
            let parents: Vec<String> = if parts[1].is_empty() {
                vec![]
            } else {
                parts[1].split(' ').map(|s| s.to_string()).collect()
            };
            Some(RawCommit {
                full_sha: parts[0].to_string(),
                parents,
                short_sha: parts[2].to_string(),
                subject: parts[3].to_string(),
                author: parts[4].to_string(),
                date: parts[5].to_string(),
            })
        })
        .collect();

    Ok(compute_topology(&raw_commits))
}

/// 纯算法内核:在解析好的 RawCommit 上跑 lane assignment,返回 GraphCommit 列表。
/// 独立于 git 调用,方便单元测试直接构造数据验证。
fn compute_topology(raw_commits: &[RawCommit]) -> Vec<GraphCommit> {
    // lanes[i] = Some(sha) 表示 lane i 下一个等待的 commit 是 sha;None 表示空槽。
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut result: Vec<GraphCommit> = Vec::with_capacity(raw_commits.len());

    for raw in raw_commits {
        // --- 1. 找本 commit 的 lane(若还没 lane 则分配) ---
        let commit_lane = assign_commit_lane(&mut lanes, &raw.full_sha);

        // --- 2. 拍快照:含可能的重复 lane(供 edge 生成识别 merge) ---
        let lanes_before = lanes.clone();

        // --- 3. 清空重复 lane(多 child 合并到最左) ---
        clear_duplicate_lanes(&mut lanes, &raw.full_sha, commit_lane);

        // --- 4. 更新 lanes:parents 接替 ---
        if raw.parents.is_empty() {
            // 根 commit:lane 终止。
            lanes[commit_lane] = None;
        } else {
            // 第一个 parent 接替当前 lane。
            lanes[commit_lane] = Some(raw.parents[0].clone());

            // 其余 parent(merge 来源)各分新 lane,或合并已有 lane。
            for parent_sha in &raw.parents[1..] {
                place_additional_parent(&mut lanes, parent_sha);
            }
        }

        // --- 5. 拍快照(更新后的 lanes) ---
        let lanes_after = lanes.clone();

        // --- 6. 生成 edges ---
        let edges = build_edges(&lanes_before, &lanes_after, commit_lane, &raw.parents);

        let entry = LogEntry {
            full_sha: raw.full_sha.clone(),
            sha: raw.short_sha.clone(),
            message: raw.subject.clone(),
            author: raw.author.clone(),
            date: raw.date.clone(),
        };

        result.push(GraphCommit {
            entry,
            parents: raw.parents.clone(),
            lane: commit_lane,
            edges,
        });
    }

    result
}

/// 为 sha 找/分配一个 lane。若 sha 已在 lanes 中则返回第一个匹配位置;
/// 否则用空槽或新增。
fn assign_commit_lane(lanes: &mut Vec<Option<String>>, sha: &str) -> usize {
    if let Some(i) = lanes.iter().position(|slot| slot.as_deref() == Some(sha)) {
        return i;
    }
    // 分支头:用空槽。
    if let Some(empty) = lanes.iter().position(|slot| slot.is_none()) {
        lanes[empty] = Some(sha.to_string());
        empty
    } else {
        lanes.push(Some(sha.to_string()));
        lanes.len() - 1
    }
}

/// 把 commit_lane 之后重复出现的同一 sha 清空(多 child 合并到最左)。
fn clear_duplicate_lanes(lanes: &mut [Option<String>], sha: &str, commit_lane: usize) {
    for slot in lanes.iter_mut().skip(commit_lane + 1) {
        if slot.as_deref() == Some(sha) {
            *slot = None;
        }
    }
}

/// 为一个 merge parent 分新 lane 或消费已有 lane(汇入 merge)。
fn place_additional_parent(lanes: &mut Vec<Option<String>>, parent_sha: &str) {
    // 如果 parent 已在某条 lane 里等待,消费它(该 lane 汇入 merge)。
    if let Some(existing) = lanes
        .iter()
        .position(|slot| slot.as_deref() == Some(parent_sha))
    {
        lanes[existing] = None;
        return;
    }

    // 否则分一条新 lane。
    if let Some(empty) = lanes.iter().position(|slot| slot.is_none()) {
        lanes[empty] = Some(parent_sha.to_string());
    } else {
        lanes.push(Some(parent_sha.to_string()));
    }
}

/// 对照本行(before)与更新后(after)的 lanes 状态,每条活跃 lane 生成一条 from→to edge。
fn build_edges(
    lanes_before: &[Option<String>],
    lanes_after: &[Option<String>],
    commit_lane: usize,
    parents: &[String],
) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    // 记录已生成过 fork edge 的 "新 lane" 目标,避免重复。
    let mut new_lane_placed: Vec<usize> = Vec::new();

    for (from, slot) in lanes_before.iter().enumerate() {
        let Some(expected) = slot else {
            continue;
        };

        if from == commit_lane {
            // 本 commit 的 lane:第一个 parent 直传(留在同一 lane)。
            if !parents.is_empty() {
                edges.push(GraphEdge {
                    from_lane: from,
                    to_lane: from,
                });
            }
            // 其余 parent fork 到各自的 lane。
            for parent_sha in parents.iter().skip(1) {
                if let Some(to) = find_parent_in_after(lanes_after, parent_sha, from) {
                    if !new_lane_placed.contains(&to) {
                        edges.push(GraphEdge {
                            from_lane: from,
                            to_lane: to,
                        });
                        new_lane_placed.push(to);
                    }
                }
            }
            // 根 commit(parents 为空):不产生 edge。lane 字段已告诉前端位置。
        } else {
            // 非 commit lane:找它在 after 中的去向,优先同 lane 直传。
            if let Some(to) = find_slot_same_or_elsewhere(lanes_after, expected, from) {
                edges.push(GraphEdge {
                    from_lane: from,
                    to_lane: to,
                });
            } else {
                // 被本 commit 消费了(merge 汇入)。
                edges.push(GraphEdge {
                    from_lane: from,
                    to_lane: commit_lane,
                });
            }
        }
    }

    // 稳定排序:按 from 再按 to。
    edges.sort_by(|a, b| {
        a.from_lane
            .cmp(&b.from_lane)
            .then(a.to_lane.cmp(&b.to_lane))
    });
    edges
}

/// 在 lanes_after 中找 parent_sha(跳过 from_lane,因为那是第一个 parent 直传的槽)。
fn find_parent_in_after(
    lanes_after: &[Option<String>],
    parent_sha: &str,
    from_lane: usize,
) -> Option<usize> {
    lanes_after
        .iter()
        .enumerate()
        .position(|(i, slot)| i != from_lane && slot.as_deref() == Some(parent_sha))
}

/// 在 lanes_after 中找 expected sha,优先同 lane(from)直传;若已移位则返回新位置。
fn find_slot_same_or_elsewhere(
    lanes_after: &[Option<String>],
    expected: &str,
    from: usize,
) -> Option<usize> {
    // 同一 lane 还在 → 直传。
    if from < lanes_after.len() && lanes_after[from].as_deref() == Some(expected) {
        return Some(from);
    }
    // 否则找第一个匹配位置(lane 位移或 merge 汇入)。
    lanes_after
        .iter()
        .position(|slot| slot.as_deref() == Some(expected))
}

// ============================================================================
// 多 root 合并拓扑:把多仓合并图当"虚拟单仓",按 root 分段在全局 lanes 上算。
// ============================================================================

/// 一个参与合并的 root 的输入:原始 repo_index + 标签/路径 + 仓内 date-order 提交(带 %at)。
pub(crate) struct RepoInput {
    pub index: usize,
    pub label: String,
    pub path: String,
    /// 仓内 `--date-order` 序列(子在父前、同仓尽量按时间),每条带 author unix 时间戳(供跨仓归并)。
    pub commits: Vec<(i64, RawCommit)>,
}

/// 把多个 root 的仓内序列合并成一张**紧凑共享车道**图(对标 WebStorm 多 root)。
///
/// 先按 %at 多路归并(只从各仓队首出,严格保持仓内拓扑),再在合并流上跑**一套**车道分配:
/// 跨 root 的 full_sha 互不相同、parent 只引用同仓 → 车道天然按 root 分,但空槽跨 root 复用、
/// 紧凑排布(宽度 = 同时活跃车道数,而非各仓最大宽度之和,避免稀疏区留大空隙)。节点按
/// repo_index 上色,每条 edge 记 `edge_roots`(该 edge 所在车道的 root)供前端逐边上色。
/// 调用方需保证 `repos` 已按 repo_index 升序(主仓在前),且只含选中 root。
pub(crate) fn compute_multi_root_topology(repos: &[RepoInput]) -> MergedGraphLog {
    let roots: Vec<RootMeta> = repos
        .iter()
        .map(|r| RootMeta {
            index: r.index,
            label: r.label.clone(),
            path: r.path.clone(),
        })
        .collect();

    // 多路归并:按 %at 降序、同 ts repo_index 升序;返回 (repos 下标, RawCommit) 全局序列。
    let merged = merge_by_ts(repos);

    // 一套紧凑车道:lanes 记每槽等待的 sha,lane_root 平行记该槽当前 sha 属哪个 root(repo_index)。
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut lane_root: Vec<Option<usize>> = Vec::new();
    let mut commits = Vec::with_capacity(merged.len());

    for (pos, raw) in &merged {
        let repo = &repos[*pos];
        let ri = repo.index;

        let commit_lane = assign_lane(&mut lanes, &mut lane_root, &raw.full_sha, ri);
        let lanes_before = lanes.clone();
        let lane_root_before = lane_root.clone();
        clear_duplicate_lanes_root(&mut lanes, &mut lane_root, &raw.full_sha, commit_lane);
        if raw.parents.is_empty() {
            lanes[commit_lane] = None;
            lane_root[commit_lane] = None;
        } else {
            lanes[commit_lane] = Some(raw.parents[0].clone());
            lane_root[commit_lane] = Some(ri);
            for parent_sha in &raw.parents[1..] {
                place_parent(&mut lanes, &mut lane_root, parent_sha, ri);
            }
        }
        let lanes_after = lanes.clone();
        let edges = build_edges(&lanes_before, &lanes_after, commit_lane, &raw.parents);
        // 每条 edge 的 root:本 commit 自身车道归当前 root,其余 pass-through 车道归其原 owner。
        let edge_roots: Vec<usize> = edges
            .iter()
            .map(|e| {
                if e.from_lane == commit_lane {
                    ri
                } else {
                    lane_root_before
                        .get(e.from_lane)
                        .copied()
                        .flatten()
                        .unwrap_or(ri)
                }
            })
            .collect();

        commits.push(MergedGraphCommit {
            entry: LogEntry {
                full_sha: raw.full_sha.clone(),
                sha: raw.short_sha.clone(),
                message: raw.subject.clone(),
                author: raw.author.clone(),
                date: raw.date.clone(),
            },
            lane: commit_lane,
            edges,
            edge_roots,
            repo_index: ri,
            repo_label: repo.label.clone(),
            repo_path: repo.path.clone(),
        });
    }

    MergedGraphLog { roots, commits }
}

/// 多路归并各仓的 date-order 序列:每步取各仓队首中 %at 最大者(同 ts 取 repo_index 最小,主仓在前),
/// 只从队首出 → 仓内顺序严格保持(含「子在父前」)。返回 (repos 下标, RawCommit) 全局序列。
fn merge_by_ts(repos: &[RepoInput]) -> Vec<(usize, RawCommit)> {
    let total: usize = repos.iter().map(|r| r.commits.len()).sum();
    let mut out = Vec::with_capacity(total);
    let mut ptrs = vec![0usize; repos.len()];
    for _ in 0..total {
        let mut best: Option<usize> = None;
        for (pos, repo) in repos.iter().enumerate() {
            if ptrs[pos] >= repo.commits.len() {
                continue;
            }
            let ts = repo.commits[ptrs[pos]].0;
            match best {
                None => best = Some(pos),
                Some(b) => {
                    let bts = repos[b].commits[ptrs[b]].0;
                    if ts > bts || (ts == bts && repo.index < repos[b].index) {
                        best = Some(pos);
                    }
                }
            }
        }
        let pos = best.expect("还有未消费提交时各仓必有队首");
        out.push((pos, repos[pos].commits[ptrs[pos]].1.clone()));
        ptrs[pos] += 1;
    }
    out
}

/// 车道数硬上限:merge 重的仓会同时打开很多分支车道,封顶后图宽恒定、不溢出视口。
/// 超出的分支挤进最右一条「溢出车道」(覆盖),代价是极宽处车道为近似位置(GitKraken 等同款折叠)。
const MAX_LANES: usize = 8;

/// 紧凑版 assign_commit_lane:复用已有 lane / 空槽 / 扩展(封顶 MAX_LANES),并同步记录该 lane 的 root。
fn assign_lane(
    lanes: &mut Vec<Option<String>>,
    lane_root: &mut Vec<Option<usize>>,
    sha: &str,
    ri: usize,
) -> usize {
    if let Some(i) = lanes.iter().position(|s| s.as_deref() == Some(sha)) {
        lane_root[i] = Some(ri);
        return i;
    }
    if let Some(i) = lanes.iter().position(|s| s.is_none()) {
        lanes[i] = Some(sha.to_string());
        lane_root[i] = Some(ri);
        return i;
    }
    if lanes.len() < MAX_LANES {
        lanes.push(Some(sha.to_string()));
        lane_root.push(Some(ri));
        return lanes.len() - 1;
    }
    // 到上限:挤进最右一条溢出车道(覆盖),把图宽封顶。
    let overflow = MAX_LANES - 1;
    lanes[overflow] = Some(sha.to_string());
    lane_root[overflow] = Some(ri);
    overflow
}

/// 紧凑版 place_additional_parent:消费已有 lane(merge 汇入)或占空槽 / 扩展,同步记录 root。
fn place_parent(
    lanes: &mut Vec<Option<String>>,
    lane_root: &mut Vec<Option<usize>>,
    parent_sha: &str,
    ri: usize,
) {
    if let Some(i) = lanes.iter().position(|s| s.as_deref() == Some(parent_sha)) {
        lanes[i] = None;
        lane_root[i] = None;
        return;
    }
    if let Some(i) = lanes.iter().position(|s| s.is_none()) {
        lanes[i] = Some(parent_sha.to_string());
        lane_root[i] = Some(ri);
        return;
    }
    if lanes.len() < MAX_LANES {
        lanes.push(Some(parent_sha.to_string()));
        lane_root.push(Some(ri));
        return;
    }
    let overflow = MAX_LANES - 1;
    lanes[overflow] = Some(parent_sha.to_string());
    lane_root[overflow] = Some(ri);
}

/// 紧凑版 clear_duplicate_lanes:清 commit_lane 之后重复 sha,并同步清其 root。
fn clear_duplicate_lanes_root(
    lanes: &mut [Option<String>],
    lane_root: &mut [Option<usize>],
    sha: &str,
    commit_lane: usize,
) {
    for i in (commit_lane + 1)..lanes.len() {
        if lanes[i].as_deref() == Some(sha) {
            lanes[i] = None;
            lane_root[i] = None;
        }
    }
}

/// 解析多 root 取数的 git 输出(format `%H\0%P\0%h\0%s\0%an\0%ar\0%at`)为带时间戳的 RawCommit。
/// 放本模块以复用私有 RawCommit;供 `log::log_multi_root_topology` 调用。
pub(crate) fn parse_topo_raw_with_ts(output: &str) -> Vec<(i64, RawCommit)> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            let parts: Vec<&str> = line.split('\0').collect();
            if parts.len() != 7 {
                return None;
            }
            let parents: Vec<String> = if parts[1].is_empty() {
                vec![]
            } else {
                parts[1].split(' ').map(|s| s.to_string()).collect()
            };
            let ts = parts[6].parse::<i64>().unwrap_or(0);
            Some((
                ts,
                RawCommit {
                    full_sha: parts[0].to_string(),
                    parents,
                    short_sha: parts[2].to_string(),
                    subject: parts[3].to_string(),
                    author: parts[4].to_string(),
                    date: parts[5].to_string(),
                },
            ))
        })
        .collect()
}

// ============================================================================
// 以下为内部类型(仅测试用)。
// ============================================================================

#[derive(Clone)]
pub(crate) struct RawCommit {
    full_sha: String,
    parents: Vec<String>,
    short_sha: String,
    subject: String,
    author: String,
    date: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(sha: &str, parents: &[&str]) -> RawCommit {
        RawCommit {
            full_sha: sha.to_string(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            short_sha: sha[..8.min(sha.len())].to_string(),
            subject: String::new(),
            author: String::new(),
            date: String::new(),
        }
    }

    // ========== 线性历史 ==========

    #[test]
    fn linear_two_commits() {
        // A → B (B root)
        let raws = &[raw("A", &["B"]), raw("B", &[])];
        let result = compute_topology(raws);
        assert_eq!(result.len(), 2);

        // A 在 lane 0,直传到 B。
        assert_eq!(result[0].lane, 0);
        assert_eq!(
            result[0].edges,
            vec![GraphEdge {
                from_lane: 0,
                to_lane: 0
            }]
        );

        // B 在 lane 0,root commit 无 parent → 无 edge。
        assert_eq!(result[1].lane, 0);
        assert!(result[1].edges.is_empty(), "root commit 无 edge");
    }

    #[test]
    fn linear_three_commits() {
        // A → B → C (C root)
        let raws = &[raw("A", &["B"]), raw("B", &["C"]), raw("C", &[])];
        let result = compute_topology(raws);
        assert_eq!(result.len(), 3);

        // A, B 在 lane 0 有直传 edge;C root 无 edge。
        assert_eq!(result[0].lane, 0);
        assert_eq!(
            result[0].edges,
            vec![GraphEdge {
                from_lane: 0,
                to_lane: 0
            }]
        );
        assert_eq!(result[1].lane, 0);
        assert_eq!(
            result[1].edges,
            vec![GraphEdge {
                from_lane: 0,
                to_lane: 0
            }]
        );
        assert_eq!(result[2].lane, 0);
        assert!(result[2].edges.is_empty());
    }

    // ========== 简单分支 + merge ==========

    #[test]
    fn branch_and_merge() {
        // 图形:
        // *   M (merge parents: P, F)
        // |\
        // | * F (parent: B)
        // * | P (parent: B)
        // |/
        // * B (root)
        //
        // git log --topo-order: M, P, F, B
        let raws = &[
            raw("M", &["P", "F"]),
            raw("P", &["B"]),
            raw("F", &["B"]),
            raw("B", &[]),
        ];
        let result = compute_topology(raws);
        assert_eq!(result.len(), 4);

        // M: 新分支头,lane 0 → fork 出 lane 1 给 F。
        let m = &result[0];
        assert_eq!(m.lane, 0, "M 应在 lane 0");
        assert_eq!(m.parents, vec!["P", "F"]);
        assert_eq!(m.edges.len(), 2);
        assert!(m.edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 0
        }));
        assert!(m.edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 1
        }));

        // P: lane 0,直传;lane 1 pass-through(F 的 lane)。
        let p = &result[1];
        assert_eq!(p.lane, 0);
        assert_eq!(p.edges.len(), 2);
        assert!(p.edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 0
        }));
        assert!(p.edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 1
        }));

        // F: lane 1,直传;lane 0 pass-through(B 在 lane 0 等待)。
        let f = &result[2];
        assert_eq!(f.lane, 1);
        assert_eq!(f.edges.len(), 2);
        assert!(f.edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 0
        }));
        assert!(f.edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 1
        }));

        // B: 两条 lane 都指向 B,B 占 lane 0,lane 1 merge 到 lane 0。
        let b = &result[3];
        assert_eq!(b.lane, 0);
        assert_eq!(b.parents, Vec::<String>::new());
        assert_eq!(b.edges.len(), 1);
        assert!(b.edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 0
        }));
    }

    // ========== 多分支交叉 ==========

    #[test]
    fn cross_branches() {
        // * A (parents: B, C)
        // |\
        // | * C (parent: D)
        // * | B (parent: D)
        // |/
        // * D (root)
        let raws = &[
            raw("A", &["B", "C"]),
            raw("C", &["D"]),
            raw("B", &["D"]),
            raw("D", &[]),
        ];
        let result = compute_topology(raws);
        assert_eq!(result.len(), 4);

        // A: lane 0, fork 出 lane 1(C)。
        assert_eq!(result[0].lane, 0);
        assert!(result[0].edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 0
        }));
        assert!(result[0].edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 1
        }));

        // C: lane 1,直传;lane 0 pass-through(D)。
        assert_eq!(result[1].lane, 1);
        assert!(result[1].edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 0
        }));
        assert!(result[1].edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 1
        }));

        // B: lane 0,直传;lane 1 pass-through(D)。
        assert_eq!(result[2].lane, 0);
        assert!(result[2].edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 0
        }));
        assert!(result[2].edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 1
        }));

        // D: lane 0, lane 1 merge → 0。
        assert_eq!(result[3].lane, 0);
        assert!(result[3].edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 0
        }));
    }

    // ========== 八爪鱼 merge(3 parents) ==========

    #[test]
    fn octopus_merge() {
        // M (parents: P1, P2, P3) — fork 出三条 lane。
        let raws = &[
            raw("M", &["P1", "P2", "P3"]),
            raw("P3", &["B"]),
            raw("P2", &["B"]),
            raw("P1", &["B"]),
            raw("B", &[]),
        ];
        let result = compute_topology(raws);
        assert_eq!(result.len(), 5);

        // M: lane 0, fork 出 lane 1(P2) 和 lane 2(P3)。
        let m = &result[0];
        assert_eq!(m.lane, 0);
        assert_eq!(m.parents.len(), 3);
        assert_eq!(m.edges.len(), 3);
        assert!(m.edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 0
        }));
        assert!(m.edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 1
        }));
        assert!(m.edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 2
        }));

        // B: 三条 lane 汇入。
        let b = &result[4];
        assert_eq!(b.lane, 0);
        assert!(b.edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 0
        }));
        assert!(b.edges.contains(&GraphEdge {
            from_lane: 2,
            to_lane: 0
        }));
    }

    // ========== 多根 ==========

    #[test]
    fn multiple_roots() {
        // 两个独立分支,各有自己的 root。
        // A(parent:B) → B(root)  ← 分支 1
        // X(parent:Y) → Y(root)  ← 分支 2
        let raws = &[
            raw("A", &["B"]),
            raw("X", &["Y"]),
            raw("B", &[]),
            raw("Y", &[]),
        ];
        let result = compute_topology(raws);
        assert_eq!(result.len(), 4);

        // A: lane 0(新分支),直传。
        assert_eq!(result[0].lane, 0);
        assert_eq!(
            result[0].edges,
            vec![GraphEdge {
                from_lane: 0,
                to_lane: 0
            }]
        );

        // X: lane 1(另一新分支),lane 0 pass-through。
        assert_eq!(result[1].lane, 1);
        assert_eq!(result[1].edges.len(), 2);
        assert!(result[1].edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 0
        }));
        assert!(result[1].edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 1
        }));

        // B: lane 0 的 root,无 parent → 无 edge;lane 1 pass-through。
        assert_eq!(result[2].lane, 0);
        assert_eq!(
            result[2].edges,
            vec![GraphEdge {
                from_lane: 1,
                to_lane: 1
            }]
        );

        // Y: lane 1 的 root,无 parent → 无 edge。
        assert_eq!(result[3].lane, 1);
        assert!(result[3].edges.is_empty());
    }

    // ========== 分支交错(interleaved branches) ==========

    #[test]
    fn interleaved_branches() {
        // 两条分支交替推进,最后汇入同一个 root。
        // A → B → C(root)   ← lane 0
        // X → Y → C(root)   ← lane 1
        let raws = &[
            raw("A", &["B"]),
            raw("X", &["Y"]),
            raw("B", &["C"]),
            raw("Y", &["C"]),
            raw("C", &[]),
        ];
        let result = compute_topology(raws);
        assert_eq!(result.len(), 5);

        // A: lane 0。
        assert_eq!(result[0].lane, 0);
        // X: lane 1, lane 0 pass-through。
        assert_eq!(result[1].lane, 1);
        assert!(result[1].edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 0
        }));
        assert!(result[1].edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 1
        }));
        // B: lane 0。
        assert_eq!(result[2].lane, 0);
        // Y: lane 1。
        assert_eq!(result[3].lane, 1);
        // C: 汇入, lane 1 → 0 merge。
        assert_eq!(result[4].lane, 0);
        assert!(result[4].edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 0
        }));
    }

    // ========== 复杂场景:三条分支最终汇合 ==========

    #[test]
    fn triple_merge() {
        // * M (parents: A, B, C) — 三条分支
        // |\\
        // | | * C (parent: R)
        // | * | B (parent: R)
        // * | | A (parent: R)
        // |/ /
        // * R (root)
        let raws = &[
            raw("M", &["A", "B", "C"]),
            raw("C", &["R"]),
            raw("B", &["R"]),
            raw("A", &["R"]),
            raw("R", &[]),
        ];
        let result = compute_topology(raws);
        assert_eq!(result.len(), 5);

        // M: lane 0,fork 出 lane 1(B) 和 lane 2(C)。
        let m = &result[0];
        assert_eq!(m.lane, 0);
        assert_eq!(m.edges.len(), 3);
        assert!(m.edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 0
        }));
        assert!(m.edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 1
        }));
        assert!(m.edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 2
        }));

        // C: lane 2。
        assert_eq!(result[1].lane, 2);
        // B: lane 1。
        assert_eq!(result[2].lane, 1);
        // A: lane 0。
        assert_eq!(result[3].lane, 0);

        // R: 三条 lane 汇入,占最左 lane 0。
        let r = &result[4];
        assert_eq!(r.lane, 0);
        assert!(r.edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 0
        }));
        assert!(r.edges.contains(&GraphEdge {
            from_lane: 2,
            to_lane: 0
        }));
    }

    // ========== merge parent 已在另一条 lane(消费已有 lane) ==========

    #[test]
    fn additional_parent_already_in_lane() {
        // 构造场景:merge commit 的第二个 parent 已经在另一条活跃 lane 中等待。
        //
        // * M (parents: P, Q)  — Q 同时在 lane 1 等待
        // |\
        // | * Q (parent: B)
        // * | P (parent: B)
        // |/
        // * B
        //
        // 这里 M 为 tip 但 Q 已经在 lane 1(因为某种原因提前建立)。
        // 实际上正常 git log 不会这样,M 是最新的。所以改成:
        //
        // * A (单独在 lane 0, parent: M)
        // * M (parents: P, Q)
        // |\
        // | * Q (parent: B)
        // * | P (parent: B)
        // |/
        // * B (root)
        //
        // git log: A, M, P, Q, B
        let raws = &[
            raw("A", &["M"]),
            raw("M", &["P", "Q"]),
            raw("P", &["B"]),
            raw("Q", &["B"]),
            raw("B", &[]),
        ];
        let result = compute_topology(raws);
        assert_eq!(result.len(), 5);

        // A: lane 0。
        assert_eq!(result[0].lane, 0);
        // M: lane 0, fork 出 lane 1(Q)。
        assert_eq!(result[1].lane, 0);
        assert!(result[1].edges.contains(&GraphEdge {
            from_lane: 0,
            to_lane: 1
        }));
        // P: lane 0, lane 1 pass-through(Q 的 lane → B)。
        assert_eq!(result[2].lane, 0);
        // Q: lane 1。
        assert_eq!(result[3].lane, 1);
        // B: merge lane 1 → 0。
        assert_eq!(result[4].lane, 0);
        assert!(result[4].edges.contains(&GraphEdge {
            from_lane: 1,
            to_lane: 0
        }));
    }

    // ========== 多 root 合并拓扑 ==========

    fn repo_input(index: usize, commits: Vec<(i64, RawCommit)>) -> RepoInput {
        RepoInput {
            index,
            label: format!("repo{index}"),
            path: format!("/repo{index}"),
            commits,
        }
    }

    fn rts(ts: i64, sha: &str, parents: &[&str]) -> (i64, RawCommit) {
        (ts, raw(sha, parents))
    }

    #[test]
    fn multi_parse_topo_raw_with_ts() {
        // format `%H\x00%P\x00%h\x00%s\x00%an\x00%ar\x00%at`,7 字段;一条 merge(2 parents)、一条 root(无 parent)。
        // 用 \x00 而非 \0:\0 后紧跟数字(如 `\02 days`、`\01700000002`)会被 rustc 当作八进制转义而警告。
        let out = "AAA\x00BBB CCC\x00aaa\x00subj A\x00alice\x002 days ago\x001700000002\n\
                   BBB\x00\x00bbb\x00subj B\x00bob\x003 days ago\x001700000001";
        let parsed = parse_topo_raw_with_ts(out);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, 1700000002);
        assert_eq!(parsed[0].1.full_sha, "AAA");
        assert_eq!(parsed[0].1.parents, vec!["BBB", "CCC"]);
        assert_eq!(parsed[0].1.short_sha, "aaa");
        assert_eq!(parsed[0].1.subject, "subj A");
        assert_eq!(parsed[0].1.date, "2 days ago");
        assert_eq!(parsed[1].0, 1700000001);
        assert_eq!(parsed[1].1.parents, Vec::<String>::new());
    }

    // 取一组 commit 用到的最大 lane 号(含 edge 端点)。
    fn max_lane_of(commits: &[MergedGraphCommit]) -> usize {
        commits
            .iter()
            .flat_map(|c| {
                std::iter::once(c.lane).chain(c.edges.iter().flat_map(|e| [e.from_lane, e.to_lane]))
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn multi_two_repos_interleave_compact() {
        // repo0(main): A(ts3)→B(ts1)  repo1(sub): X(ts4)→Y(ts2)
        // 归并 X,A,Y,B;紧凑车道:X 先到占 lane0,A 占 lane1(总宽 2,不再各占一段)。
        let repos = &[
            repo_input(0, vec![rts(3, "A", &["B"]), rts(1, "B", &[])]),
            repo_input(1, vec![rts(4, "X", &["Y"]), rts(2, "Y", &[])]),
        ];
        let MergedGraphLog { roots, commits } = compute_multi_root_topology(repos);

        assert_eq!(
            roots.iter().map(|r| r.index).collect::<Vec<_>>(),
            vec![0, 1]
        );
        let seq: Vec<(&str, usize, usize)> = commits
            .iter()
            .map(|c| (c.entry.full_sha.as_str(), c.repo_index, c.lane))
            .collect();
        assert_eq!(
            seq,
            vec![("X", 1, 0), ("A", 0, 1), ("Y", 1, 0), ("B", 0, 1)]
        );
        assert_eq!(max_lane_of(&commits), 1, "紧凑布局总车道应为 2(lane 0/1)");

        // A 行两条 edge 的 root:lane0 pass-through 属 sub(1),lane1 自身属 main(0)。
        let a = &commits[1];
        assert_eq!(a.edges.len(), a.edge_roots.len());
        for (e, r) in a.edges.iter().zip(&a.edge_roots) {
            if e.from_lane == 0 {
                assert_eq!(*r, 1, "pass-through 边应归子仓");
            }
            if e.from_lane == 1 {
                assert_eq!(*r, 0, "自身边应归主仓");
            }
        }
    }

    #[test]
    fn multi_empty_sub_in_roots_no_commits() {
        // 子仓空 → 仍在 roots(供 chip),但无提交,主仓正常。
        let repos = &[
            repo_input(0, vec![rts(2, "A", &["B"]), rts(1, "B", &[])]),
            repo_input(1, vec![]),
        ];
        let MergedGraphLog { roots, commits } = compute_multi_root_topology(repos);
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[1].index, 1);
        assert_eq!(commits.len(), 2);
        assert!(commits.iter().all(|c| c.repo_index == 0 && c.lane == 0));
    }

    #[test]
    fn multi_octopus_compact() {
        // 仓内 octopus(3 parents)正常 fork;sub 单提交复用空槽,不溢出 / 不 panic。
        let repos = &[
            repo_input(
                0,
                vec![
                    rts(5, "M", &["P1", "P2", "P3"]),
                    rts(4, "P3", &["B"]),
                    rts(3, "P2", &["B"]),
                    rts(2, "P1", &["B"]),
                    rts(1, "B", &[]),
                ],
            ),
            repo_input(1, vec![rts(9, "X", &[])]),
        ];
        let MergedGraphLog { commits, .. } = compute_multi_root_topology(repos);
        // X(ts9) 最新先到 lane0;每条 edge 都有对应 root。
        assert_eq!(commits[0].entry.full_sha, "X");
        assert_eq!(commits[0].repo_index, 1);
        assert!(commits.iter().all(|c| c.edges.len() == c.edge_roots.len()));
    }

    #[test]
    fn multi_lane_cap_bounds_width() {
        // 13 路 octopus 本会开 13 条车道;MAX_LANES=8 应把图宽封顶在 lane 7。
        let parents: Vec<String> = (0..13).map(|i| format!("P{i}")).collect();
        let pref: Vec<&str> = parents.iter().map(|s| s.as_str()).collect();
        let mut commits = vec![rts(100, "M", &pref)];
        for (i, p) in parents.iter().enumerate() {
            commits.push(rts(50 - i as i64, p, &[]));
        }
        let repos = &[repo_input(0, commits)];
        let MergedGraphLog { commits, .. } = compute_multi_root_topology(repos);
        assert!(max_lane_of(&commits) < 8, "车道数应封顶在 MAX_LANES(8)");
    }

    #[test]
    fn multi_tie_ts_main_first() {
        // 同 %at:主仓(index0)排在子仓(index1)之前。
        let repos = &[
            repo_input(0, vec![rts(5, "A", &[])]),
            repo_input(1, vec![rts(5, "X", &[])]),
        ];
        let MergedGraphLog { commits, .. } = compute_multi_root_topology(repos);
        assert_eq!(commits[0].repo_index, 0);
        assert_eq!(commits[1].repo_index, 1);
    }

    #[test]
    fn multi_deselect_main_only_sub() {
        // 勾掉主仓 → 只传子仓(index1):roots 只剩它,提交全归 repo_index 1。
        let repos = &[repo_input(1, vec![rts(2, "X", &["Y"]), rts(1, "Y", &[])])];
        let MergedGraphLog { roots, commits } = compute_multi_root_topology(repos);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].index, 1);
        assert!(commits.iter().all(|c| c.lane == 0 && c.repo_index == 1));
        // 单 root 时 edge 全归该 root。
        assert!(commits.iter().all(|c| c.edge_roots.iter().all(|&r| r == 1)));
    }

    #[test]
    fn multi_single_repo_equivalent_to_compute_topology() {
        // 单 RepoInput 的 lane/edges 应与单仓 compute_topology 完全一致(紧凑布局对单仓即原算法)。
        let raws = &[
            raw("M", &["P", "F"]),
            raw("P", &["B"]),
            raw("F", &["B"]),
            raw("B", &[]),
        ];
        let single = compute_topology(raws);

        let repos = &[repo_input(
            0,
            vec![
                rts(4, "M", &["P", "F"]),
                rts(3, "P", &["B"]),
                rts(2, "F", &["B"]),
                rts(1, "B", &[]),
            ],
        )];
        let merged = compute_multi_root_topology(repos);

        assert_eq!(merged.commits.len(), single.len());
        for (m, s) in merged.commits.iter().zip(&single) {
            assert_eq!(m.entry.full_sha, s.entry.full_sha);
            assert_eq!(m.lane, s.lane, "lane 应与单仓一致");
            assert_eq!(m.edges, s.edges, "edges 应与单仓一致");
            assert_eq!(m.edges.len(), m.edge_roots.len());
            assert!(m.edge_roots.iter().all(|&r| r == 0));
        }
    }
}

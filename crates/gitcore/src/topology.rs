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

    let branch_str;
    if let Some(ref b) = opts.branch {
        branch_str = b.clone();
        args.push(&branch_str);
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
// 以下为内部类型(仅测试用)。
// ============================================================================

struct RawCommit {
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
}

use crate::{Error, Repo};

/// 一条提交记录。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct LogEntry {
    /// 短 SHA(8 位)。
    pub sha: String,
    /// 完整 SHA(40 位),供复制。
    pub full_sha: String,
    /// 提交消息第一行。
    pub message: String,
    /// 作者。
    pub author: String,
    /// 相对时间(如 "2 hours ago")。
    pub date: String,
}

/// log 查询选项。
#[derive(Debug, Clone)]
pub struct LogOptions {
    /// 最多返回多少条。
    pub max_count: usize,
    /// 跳过前 skip 条,只返回 [skip, skip+max_count) 增量窗口(滚动加载用)。
    /// 仅 `log_topology` / `log_merged` / `log_multi_root_topology` 三条 History
    /// 路径生效:lane 车道分配依赖完整前缀序列,内部仍全量计算,只是不回传前端已持有的部分。
    pub skip: usize,
    /// 指定分支或引用(如 "main"、"HEAD"),None 表示当前 HEAD。
    pub branch: Option<String>,
    /// 按作者筛选(映射 git log --author=<>)。
    pub author: Option<String>,
    /// 按提交消息筛选(映射 git log --grep=<>)。
    pub grep: Option<String>,
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            max_count: 50,
            skip: 0,
            branch: None,
            author: None,
            grep: None,
        }
    }
}

/// 把全量序列切成 `[skip, skip+max_count)` 增量窗口,返回 (窗口, anchor)。
/// anchor = 第 skip-1 条(前端已持有的最后一条)的 full_sha,供前端校验增量拼接点:
/// 不匹配说明两次请求间历史已变(新提交/rebase),前端应全量重载。skip=0(首页)无锚点。
/// 截尾必须做:多仓归并的全量是「每仓 skip+max_count」的总和,远超一页;且超出
/// skip+max_count 的尾部处在各仓取数耗尽区,随取数加深会变(不是稳定前缀),
/// 吐给前端会导致下次 loadMore 的 anchor 永远失配、退化为原地全量重载。
pub(crate) fn split_window<T>(
    mut all: Vec<T>,
    skip: usize,
    max_count: usize,
    full_sha: impl Fn(&T) -> &str,
) -> (Vec<T>, Option<String>) {
    let anchor = skip
        .checked_sub(1)
        .and_then(|i| all.get(i))
        .map(|c| full_sha(c).to_string());
    let mut window = if skip >= all.len() {
        Vec::new()
    } else {
        all.split_off(skip)
    };
    window.truncate(max_count);
    (window, anchor)
}

/// 获取提交历史。
pub(crate) fn log(repo: &Repo, opts: &LogOptions) -> Result<Vec<LogEntry>, Error> {
    let max_count_str = format!("-{}", opts.max_count);
    let mut args = vec![
        "log",
        "--pretty=format:%H%x00%h%x00%s%x00%an%x00%ai",
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
    let entries = output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\0').collect();
            if parts.len() != 5 {
                return None;
            }
            Some(LogEntry {
                full_sha: parts[0].to_string(),
                sha: parts[1].to_string(),
                message: parts[2].to_string(),
                author: parts[3].to_string(),
                date: parts[4].to_string(),
            })
        })
        .collect();
    Ok(entries)
}

/// 选定分支与当前 HEAD 的双向独有提交(Compare with Current)。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct BranchComparison {
    /// 在 other 但不在当前 HEAD 的提交(`HEAD..other`,= 合并 other 会带进来的)。
    pub incoming: Vec<LogEntry>,
    /// 在当前 HEAD 但不在 other 的提交(`other..HEAD`)。
    pub outgoing: Vec<LogEntry>,
}

/// 比较选定 ref(分支/远程分支)与当前 HEAD,列出双向各自独有的提交。
pub(crate) fn compare_commits(repo: &Repo, other: &str) -> Result<BranchComparison, Error> {
    Ok(BranchComparison {
        incoming: rev_range(repo, &format!("HEAD..{other}"))?,
        outgoing: rev_range(repo, &format!("{other}..HEAD"))?,
    })
}

/// 取某个 `A..B` 范围的提交列表(newest first)。
pub(crate) fn rev_range(repo: &Repo, range: &str) -> Result<Vec<LogEntry>, Error> {
    let output = repo.git(&["log", "--pretty=format:%H%x00%h%x00%s%x00%an%x00%ai", range])?;
    let entries = output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\0').collect();
            if parts.len() != 5 {
                return None;
            }
            Some(LogEntry {
                full_sha: parts[0].to_string(),
                sha: parts[1].to_string(),
                message: parts[2].to_string(),
                author: parts[3].to_string(),
                date: parts[4].to_string(),
            })
        })
        .collect();
    Ok(entries)
}

/// 列出子仓库在 `old..new` 区间的提交(供父仓 commit 详情展开子模块指针变化)。
/// 子仓未初始化、或该区间提交本地未拉取时返回错误,由上层降级处理(只显示指针变化)。
pub(crate) fn submodule_commits(
    main_repo: &Repo,
    sub_path: &std::path::Path,
    old: &str,
    new: &str,
) -> Result<Vec<LogEntry>, Error> {
    let sub = Repo::open(main_repo.workdir().join(sub_path))?;
    rev_range(&sub, &format!("{old}..{new}"))
}

/// 合并视图中的一条提交:可能来自主仓或某个子仓,带仓库标识。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct MergedLogEntry {
    /// 提交本体。
    pub entry: LogEntry,
    /// 仓库标识:空串 = 主仓,否则为子仓相对主仓的路径(供 UI 标签展示)。
    pub repo_label: String,
    /// 该提交所属仓库的绝对路径,供前端对该提交执行操作时定位到正确的仓库。
    pub repo_path: String,
}

/// 合并视图的一页:增量窗口 + 拼接锚点(见 `split_window`)。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct MergedLog {
    /// `[skip, skip+max_count)` 窗口内的提交。
    pub entries: Vec<MergedLogEntry>,
    /// 第 skip-1 条的 full_sha;skip=0 时为 None。
    pub anchor: Option<String>,
}

/// 与 `log` 相同,但额外解析 author unix 时间戳(`%at`),用于跨仓库合并时按时间排序。
/// 时间戳仅用于排序,不进入 `LogEntry`(避免改动其它构造点)。
/// 取数量为 skip+max_count:归并/切片发生在调用方,这里要把窗口前的前缀一并取回。
fn log_with_ts(repo: &Repo, opts: &LogOptions) -> Result<Vec<(i64, LogEntry)>, Error> {
    let max_count_str = format!("-{}", opts.skip + opts.max_count);
    let mut args = vec![
        "log",
        "--pretty=format:%H%x00%h%x00%s%x00%an%x00%ai%x00%at",
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
    let entries = output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\0').collect();
            if parts.len() != 6 {
                return None;
            }
            let ts = parts[5].parse::<i64>().unwrap_or(0);
            Some((
                ts,
                LogEntry {
                    full_sha: parts[0].to_string(),
                    sha: parts[1].to_string(),
                    message: parts[2].to_string(),
                    author: parts[3].to_string(),
                    date: parts[4].to_string(),
                },
            ))
        })
        .collect();
    Ok(entries)
}

/// 合并主仓与各已初始化子仓的提交历史,按提交时间降序排列,每条带仓库标识。
/// 子仓打开或日志读取失败时静默跳过该子仓,不影响其它仓库的展示。
/// 返回 `[skip, skip+max_count)` 增量窗口(归并在全量上做,保证窗口切分点稳定)。
pub(crate) fn log_merged(repo: &Repo, opts: &LogOptions) -> Result<MergedLog, Error> {
    use crate::submodule::{list_submodules, SubmoduleStatus};

    // 已初始化子仓(list_submodules 自身要 fork 一次 git,无法并行于后续抽取)。
    let subs: Vec<_> = list_submodules(repo)?
        .into_iter()
        .filter(|s| s.status != SubmoduleStatus::Uninitialized)
        .collect();

    let main_path = repo.workdir().to_string_lossy().to_string();

    // 主仓与各子仓的 log 各 fork 一次 git 且彼此独立 → 并行抽取,墙钟从串行 (1+N)×
    // 降到 ~1×。子仓打开/读取失败静默跳过(返回空),不影响其它仓库。
    #[allow(clippy::type_complexity)]
    let (main_res, sub_rows): (
        Result<Vec<(i64, LogEntry)>, Error>,
        Vec<Vec<(i64, LogEntry)>>,
    ) = std::thread::scope(|scope| {
        let main_handle = scope.spawn(|| log_with_ts(repo, opts));
        let sub_handles: Vec<_> = subs
            .iter()
            .map(|sub| {
                let abs = repo.workdir().join(&sub.path);
                scope.spawn(move || {
                    Repo::open(&abs)
                        .ok()
                        .and_then(|r| log_with_ts(&r, opts).ok())
                        .unwrap_or_default()
                })
            })
            .collect();
        let main = main_handle.join().unwrap_or_else(|_| Ok(Vec::new()));
        let collected = sub_handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect();
        (main, collected)
    });

    let mut rows: Vec<(i64, MergedLogEntry)> = Vec::new();
    for (ts, entry) in main_res? {
        rows.push((
            ts,
            MergedLogEntry {
                entry,
                repo_label: String::new(),
                repo_path: main_path.clone(),
            },
        ));
    }
    for (sub, entries) in subs.iter().zip(sub_rows) {
        let label = sub.path.to_string_lossy().to_string();
        let abs_str = repo.workdir().join(&sub.path).to_string_lossy().to_string();
        for (ts, entry) in entries {
            rows.push((
                ts,
                MergedLogEntry {
                    entry,
                    repo_label: label.clone(),
                    repo_path: abs_str.clone(),
                },
            ));
        }
    }

    // 时间降序(newest first),与单仓 log 一致。
    rows.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
    let all: Vec<MergedLogEntry> = rows.into_iter().map(|(_, e)| e).collect();
    let (entries, anchor) = split_window(all, opts.skip, opts.max_count, |e| &e.entry.full_sha);
    Ok(MergedLog { entries, anchor })
}

/// 取单仓的拓扑序 raw log(带 parents `%P` + 相对日期 `%ar` + author 时间戳 `%at`),
/// 供多 root 合并图取数。选项处理与 `log_topology` 对齐,format 额外带 `%at` 供跨仓归并。
/// 取数量为 skip+max_count:lane 依赖完整前缀,窗口切分在归并计算之后做。
fn topo_log_raw(repo: &Repo, opts: &LogOptions) -> Result<String, Error> {
    let max_count_str = format!("-{}", opts.skip + opts.max_count);
    // 用 --date-order 而非 --topo-order:跨仓按 %at 归并时,topo-order 会把某仓里
    // 一条新提交压到它 topo 前驱(常是合并进来的老侧支)之后,导致主/子仓的同期提交在
    // 合并流里被拉远(实测一条 4h 前的子仓提交被沉到一堆 5h 前提交之下)。date-order 仍
    // 保证「子在父前」(车道算法成立),但同仓内尽量按时间,合并后时间序更连贯、不丢「直觉位置」。
    let mut args = vec![
        "log",
        "--date-order",
        "--pretty=format:%H%x00%P%x00%h%x00%s%x00%an%x00%ar%x00%at",
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
        // 「全部分支」= 所有 ref(对标 WebStorm);否则默认只查 HEAD,会漏 submodule
        // 游离 HEAD 之外、分支上更新的提交。
        args.push("--all");
    }

    repo.git(&args)
}

/// 多 root 合并拓扑图:主仓 + 各已初始化子仓,按 root 分段画 lane(对标 WebStorm 多 root Log)。
/// **始终拉取全部已初始化仓**(焦点由前端灰显处理,后端不过滤)。
/// 复用 `log_merged` 的并行 fork 骨架;子仓打开/读取失败静默跳过,不影响其它仓库。
pub(crate) fn log_multi_root_topology(
    repo: &Repo,
    opts: &LogOptions,
) -> Result<crate::topology::MergedGraphLog, Error> {
    use crate::submodule::{list_submodules, SubmoduleStatus};
    use crate::topology::{compute_multi_root_topology, parse_topo_raw_with_ts, RepoInput};

    // 已初始化子仓:repo_index 主仓=0,子仓按此顺序 1..N。
    let subs: Vec<_> = list_submodules(repo)?
        .into_iter()
        .filter(|s| s.status != SubmoduleStatus::Uninitialized)
        .collect();

    let main_path = repo.workdir().to_string_lossy().to_string();

    // 并行 fork:主仓 + 各子仓各取一次 topo raw(复用 log_merged 的 scope 骨架)。
    let (main_raw, sub_raws): (Result<String, Error>, Vec<Option<String>>) =
        std::thread::scope(|scope| {
            let main_handle = scope.spawn(|| topo_log_raw(repo, opts));
            let sub_handles: Vec<_> = subs
                .iter()
                .map(|sub| {
                    let abs = repo.workdir().join(&sub.path);
                    scope.spawn(move || {
                        Repo::open(&abs)
                            .ok()
                            .and_then(|r| topo_log_raw(&r, opts).ok())
                    })
                })
                .collect();
            let main = main_handle.join().unwrap_or_else(|_| Ok(String::new()));
            let collected = sub_handles
                .into_iter()
                .map(|h| h.join().unwrap_or(None))
                .collect();
            (main, collected)
        });

    // 组装 RepoInput(按 repo_index 升序:主仓 0,然后各子仓 1..N)。
    let mut repos: Vec<RepoInput> = Vec::new();
    repos.push(RepoInput {
        index: 0,
        label: "主仓".to_string(),
        path: main_path,
        commits: parse_topo_raw_with_ts(&main_raw?),
    });
    for (i, (sub, raw_opt)) in subs.iter().zip(sub_raws).enumerate() {
        let Some(raw) = raw_opt else {
            continue; // 子仓打开失败 → 跳过(不进 roots,前端 chip 不列它)
        };
        repos.push(RepoInput {
            index: i + 1,
            label: sub.path.to_string_lossy().to_string(),
            path: repo.workdir().join(&sub.path).to_string_lossy().to_string(),
            commits: parse_topo_raw_with_ts(&raw),
        });
    }

    let mut graph = compute_multi_root_topology(&repos);
    let (window, anchor) = split_window(
        std::mem::take(&mut graph.commits),
        opts.skip,
        opts.max_count,
        |c| &c.entry.full_sha,
    );
    graph.commits = window;
    graph.anchor = anchor;
    Ok(graph)
}

/// 带分支拓扑图的一行 log:`graph` 是该行的图形前缀(如 `* `、`|\`、`| * `),
/// `entry` 仅 commit 行有(纯连接行为 None)。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct GraphRow {
    pub graph: String,
    pub entry: Option<LogEntry>,
}

/// 获取单个文件的提交历史(追踪重命名)。
pub(crate) fn file_history(
    repo: &Repo,
    file_path: &std::path::Path,
    opts: &LogOptions,
) -> Result<Vec<LogEntry>, Error> {
    let max_count_str = format!("-{}", opts.max_count);
    let mut args = vec![
        "log",
        "--follow",
        "--pretty=format:%H%x00%h%x00%s%x00%an%x00%ai",
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
    }

    args.push("--");
    let path_str = file_path.to_string_lossy().to_string();
    args.push(&path_str);

    let output = repo.git(&args)?;
    let entries = output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\0').collect();
            if parts.len() != 5 {
                return None;
            }
            Some(LogEntry {
                full_sha: parts[0].to_string(),
                sha: parts[1].to_string(),
                message: parts[2].to_string(),
                author: parts[3].to_string(),
                date: parts[4].to_string(),
            })
        })
        .collect();
    Ok(entries)
}

/// 获取带分支拓扑图的提交历史。图形列交给 `git log --graph` 生成,
/// 每个 commit 行用 NUL 分隔图形前缀与提交数据;无数据的行即纯连接行。
pub(crate) fn log_graph(repo: &Repo, opts: &LogOptions) -> Result<Vec<GraphRow>, Error> {
    let max_count_str = format!("-{}", opts.max_count);
    let mut args = vec![
        "log",
        "--graph",
        "--color=never",
        "--pretty=format:%x00%H%x00%h%x00%s%x00%an%x00%ai",
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
    }

    let output = repo.git(&args)?;
    let rows = output
        .lines()
        .map(|line| match line.split_once('\0') {
            // commit 行:NUL 前是图形前缀,NUL 后是 h\0s\0an\0ar。
            Some((graph, rest)) => {
                let parts: Vec<&str> = rest.split('\0').collect();
                let entry = if parts.len() == 5 {
                    Some(LogEntry {
                        full_sha: parts[0].to_string(),
                        sha: parts[1].to_string(),
                        message: parts[2].to_string(),
                        author: parts[3].to_string(),
                        date: parts[4].to_string(),
                    })
                } else {
                    None
                };
                GraphRow {
                    graph: graph.to_string(),
                    entry,
                }
            }
            // 纯连接行(无 format 展开)。
            None => GraphRow {
                graph: line.to_string(),
                entry: None,
            },
        })
        .collect();
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::split_window;

    fn shas(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_window_first_page_no_anchor() {
        let (win, anchor) = split_window(shas(&["a", "b", "c"]), 0, 3, |s| s);
        assert_eq!(win, shas(&["a", "b", "c"]));
        assert_eq!(anchor, None);
    }

    #[test]
    fn split_window_tail_with_anchor() {
        let (win, anchor) = split_window(shas(&["a", "b", "c", "d"]), 2, 2, |s| s);
        assert_eq!(win, shas(&["c", "d"]));
        assert_eq!(anchor, Some("b".to_string()));
    }

    #[test]
    fn split_window_at_end_returns_empty_with_anchor() {
        // 前端已加载全部,再翻一页:空窗口但 anchor 仍匹配 → 前端据此判定到底,不误触全量重载。
        let (win, anchor) = split_window(shas(&["a", "b"]), 2, 2, |s| s);
        assert!(win.is_empty());
        assert_eq!(anchor, Some("b".to_string()));
    }

    #[test]
    fn split_window_past_end_no_anchor() {
        // 历史被改写变短(rebase/reset):anchor 取不到 → 前端视为失配走全量重载。
        let (win, anchor) = split_window(shas(&["a", "b"]), 5, 2, |s| s);
        assert!(win.is_empty());
        assert_eq!(anchor, None);
    }

    #[test]
    fn split_window_truncates_to_max_count() {
        // 多仓归并的全量远超一页(每仓各取 skip+max_count):窗口必须截到 max_count,
        // 否则尾部落在各仓取数耗尽区,随取数加深不稳定,导致 loadMore anchor 永远失配。
        let (win, anchor) = split_window(shas(&["a", "b", "c", "d", "e", "f"]), 2, 2, |s| s);
        assert_eq!(win, shas(&["c", "d"]));
        assert_eq!(anchor, Some("b".to_string()));
    }
}

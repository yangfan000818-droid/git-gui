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
            branch: None,
            author: None,
            grep: None,
        }
    }
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
fn rev_range(repo: &Repo, range: &str) -> Result<Vec<LogEntry>, Error> {
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

/// 与 `log` 相同,但额外解析 author unix 时间戳(`%at`),用于跨仓库合并时按时间排序。
/// 时间戳仅用于排序,不进入 `LogEntry`(避免改动其它构造点)。
fn log_with_ts(repo: &Repo, opts: &LogOptions) -> Result<Vec<(i64, LogEntry)>, Error> {
    let max_count_str = format!("-{}", opts.max_count);
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
pub(crate) fn log_merged(repo: &Repo, opts: &LogOptions) -> Result<Vec<MergedLogEntry>, Error> {
    use crate::submodule::{list_submodules, SubmoduleStatus};

    let mut rows: Vec<(i64, MergedLogEntry)> = Vec::new();

    let main_path = repo.workdir().to_string_lossy().to_string();
    for (ts, entry) in log_with_ts(repo, opts)? {
        rows.push((
            ts,
            MergedLogEntry {
                entry,
                repo_label: String::new(),
                repo_path: main_path.clone(),
            },
        ));
    }

    for sub in list_submodules(repo)? {
        if sub.status == SubmoduleStatus::Uninitialized {
            continue;
        }
        let abs = repo.workdir().join(&sub.path);
        let Ok(sub_repo) = Repo::open(&abs) else {
            continue;
        };
        let Ok(sub_rows) = log_with_ts(&sub_repo, opts) else {
            continue;
        };
        let label = sub.path.to_string_lossy().to_string();
        let abs_str = abs.to_string_lossy().to_string();
        for (ts, entry) in sub_rows {
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
    Ok(rows.into_iter().map(|(_, e)| e).collect())
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

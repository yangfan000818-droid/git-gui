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

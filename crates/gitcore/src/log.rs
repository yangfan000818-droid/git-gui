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
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            max_count: 50,
            branch: None,
        }
    }
}

/// 获取提交历史。
pub(crate) fn log(repo: &Repo, opts: &LogOptions) -> Result<Vec<LogEntry>, Error> {
    let max_count_str = format!("-{}", opts.max_count);
    let mut args = vec![
        "log",
        "--pretty=format:%H%x00%h%x00%s%x00%an%x00%ar",
        &max_count_str,
    ];

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

/// 带分支拓扑图的一行 log:`graph` 是该行的图形前缀(如 `* `、`|\`、`| * `),
/// `entry` 仅 commit 行有(纯连接行为 None)。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct GraphRow {
    pub graph: String,
    pub entry: Option<LogEntry>,
}

/// 获取带分支拓扑图的提交历史。图形列交给 `git log --graph` 生成,
/// 每个 commit 行用 NUL 分隔图形前缀与提交数据;无数据的行即纯连接行。
pub(crate) fn log_graph(repo: &Repo, opts: &LogOptions) -> Result<Vec<GraphRow>, Error> {
    let max_count_str = format!("-{}", opts.max_count);
    let mut args = vec![
        "log",
        "--graph",
        "--color=never",
        "--pretty=format:%x00%H%x00%h%x00%s%x00%an%x00%ar",
        &max_count_str,
    ];

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

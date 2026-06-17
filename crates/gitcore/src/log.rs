use crate::{Error, Repo};

/// 一条提交记录。
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// 短 SHA(8 位)。
    pub sha: String,
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
        "--pretty=format:%h%x00%s%x00%an%x00%ar",
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
            if parts.len() != 4 {
                return None;
            }
            Some(LogEntry {
                sha: parts[0].to_string(),
                message: parts[1].to_string(),
                author: parts[2].to_string(),
                date: parts[3].to_string(),
            })
        })
        .collect();
    Ok(entries)
}

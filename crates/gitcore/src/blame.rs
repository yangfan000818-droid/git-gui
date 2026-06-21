use crate::{Error, Repo};
use std::collections::HashMap;
use std::path::Path;

/// blame 的一行:该行由谁、哪次提交引入。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct BlameLine {
    /// 短 sha(8 位)。
    pub sha: String,
    /// 完整 sha。
    pub full_sha: String,
    /// 作者名。
    pub author: String,
    /// 作者时间(unix 时间戳,前端格式化)。
    pub time: i64,
    /// 行号(从 1 开始)。
    pub line_no: usize,
    /// 行内容。
    pub content: String,
}

/// 逐行 blame 一个文件。解析 `git blame --porcelain`:
/// 每个 sha 组首行带完整 header(author/author-time 等),续行只有 sha 行,
/// 故缓存 sha→(author, time),续行查缓存。
pub(crate) fn blame(repo: &Repo, file_path: &Path) -> Result<Vec<BlameLine>, Error> {
    let path_str = file_path.to_string_lossy().to_string();
    let out = repo.git(&["blame", "--porcelain", "--", &path_str])?;

    let mut meta: HashMap<String, (String, i64)> = HashMap::new();
    let mut result = Vec::new();
    let mut cur_sha = String::new();
    let mut cur_line = 0usize;
    let mut pend_author: Option<String> = None;
    let mut pend_time: Option<i64> = None;

    for raw in out.lines() {
        let line = raw.trim_end_matches('\r');
        if let Some(content) = line.strip_prefix('\t') {
            // 内容行:组首先攒 author/time 存入缓存,续行查缓存。
            if let (Some(a), Some(t)) = (pend_author.take(), pend_time.take()) {
                meta.insert(cur_sha.clone(), (a, t));
            }
            let (author, time) = meta.get(&cur_sha).cloned().unwrap_or_default();
            let short = cur_sha.get(..8).unwrap_or(cur_sha.as_str()).to_string();
            result.push(BlameLine {
                sha: short,
                full_sha: cur_sha.clone(),
                author,
                time,
                line_no: cur_line,
                content: content.to_string(),
            });
        } else if let Some(a) = line.strip_prefix("author ") {
            pend_author = Some(a.to_string());
        } else if let Some(t) = line.strip_prefix("author-time ") {
            pend_time = t.parse().ok();
        } else {
            // sha 行:"<40hex> <orig> <final> [<num>]";其余 header(committer/summary/...) 忽略。
            let parts: Vec<&str> = line.split(' ').collect();
            if parts.len() >= 3
                && parts[0].len() == 40
                && parts[0].bytes().all(|b| b.is_ascii_hexdigit())
            {
                cur_sha = parts[0].to_string();
                cur_line = parts[2].parse().unwrap_or(0);
            }
        }
    }
    Ok(result)
}

use crate::{Error, Repo};
use std::path::PathBuf;

/// 指向一个 autostash 条目。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StashRef {
    /// 创建时写入的标签,用于定位与崩溃恢复。
    pub label: String,
}

/// autostash 贴回(pop)结果。
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PopResult {
    /// 干净贴回,stash 已删除。
    Clean,
    /// 贴回冲突,stash 保留,这些文件需处理。
    Conflict(Vec<PathBuf>),
}

/// gitcore 创建的 autostash 标签前缀(用于扫描识别)。
const AUTOSTASH_TAG: &str = "gitcore-autostash:";

/// 生成一个带进程标识的 autostash 标签。
pub(crate) fn autostash_label() -> String {
    format!("{AUTOSTASH_TAG}{}", std::process::id())
}

/// 工作区脏则 stash(含 untracked)并返回引用;干净返回 None。
pub(crate) fn autostash_push(repo: &Repo, label: &str) -> Result<Option<StashRef>, Error> {
    if repo.git(&["status", "--porcelain"])?.trim().is_empty() {
        return Ok(None);
    }
    repo.git(&["stash", "push", "--include-untracked", "-m", label])?;
    Ok(Some(StashRef {
        label: label.to_string(),
    }))
}

/// 按 label 定位并贴回;成功后 drop,冲突则保留 stash。
///
/// 用 `apply` + `drop` 而非 `pop`:失败时 stash 不会丢,用户改动有兜底。
pub(crate) fn autostash_pop(repo: &Repo, stash: &StashRef) -> Result<PopResult, Error> {
    let reff = locate(repo, &stash.label)?
        .ok_or_else(|| Error::Precondition(format!("找不到 autostash: {}", stash.label)))?;

    let applied = repo.git_checked(&["stash", "apply", &reff])?;
    if applied.success {
        repo.git(&["stash", "drop", &reff])?;
        Ok(PopResult::Clean)
    } else {
        Ok(PopResult::Conflict(crate::conflict::conflicted_files(
            repo,
        )?))
    }
}

/// 扫描 stash 列表,找回 gitcore 创建的 autostash(崩溃/中断后恢复用)。
pub(crate) fn find_autostash(repo: &Repo) -> Result<Option<StashRef>, Error> {
    let list = repo.git(&["stash", "list", "--format=%gs"])?;
    for line in list.lines() {
        if let Some(pos) = line.find(AUTOSTASH_TAG) {
            return Ok(Some(StashRef {
                label: line[pos..].trim().to_string(),
            }));
        }
    }
    Ok(None)
}

// 在 stash 列表里按 label 找到形如 stash@{N} 的引用。
fn locate(repo: &Repo, label: &str) -> Result<Option<String>, Error> {
    let list = repo.git(&["stash", "list", "--format=%gd %gs"])?;
    for line in list.lines() {
        if line.contains(label) {
            if let Some(reff) = line.split_whitespace().next() {
                return Ok(Some(reff.to_string()));
            }
        }
    }
    Ok(None)
}

// ========== 手动 stash 管理 ==========

/// 一条 stash 记录。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct StashEntry {
    /// stash@{N} 引用。
    pub reff: String,
    /// stash 消息。
    pub message: String,
    /// 来源分支。
    pub branch: String,
}

/// 列出所有 stash。
pub(crate) fn list_stashes(repo: &Repo) -> Result<Vec<StashEntry>, Error> {
    let output = repo.git(&["stash", "list", "--pretty=format:%gd%x00%gs%x00%h:%s"])?;
    let mut stashes = Vec::new();
    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\0').collect();
        if parts.len() < 2 {
            continue;
        }
        let reff = parts[0].to_string();
        // git stash 默认消息格式: "WIP on <branch>: <commit>"
        // 自定义消息: "<custom>"
        let (branch, message) = parse_stash_message(parts[1]);
        stashes.push(StashEntry {
            reff,
            message,
            branch,
        });
    }
    Ok(stashes)
}

/// 解析 stash 消息，提取分支名和实际消息。
fn parse_stash_message(raw: &str) -> (String, String) {
    if let Some(rest) = raw.strip_prefix("WIP on ") {
        let branch = rest
            .split(':')
            .next()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "?".into());
        (branch, raw.to_string())
    } else if let Some(rest) = raw.strip_prefix("On ") {
        // "On <branch>: <message>"
        let branch = rest
            .split(':')
            .next()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "?".into());
        (branch, raw.to_string())
    } else {
        ("?".into(), raw.to_string())
    }
}

/// 创建新的 stash（含 untracked 文件）。
pub(crate) fn stash_push(repo: &Repo, message: Option<&str>) -> Result<(), Error> {
    let mut args = vec!["stash", "push", "--include-untracked"];
    if let Some(msg) = message {
        if !msg.is_empty() {
            args.push("-m");
            args.push(msg);
        }
    }
    repo.git(&args)?;
    Ok(())
}

/// 应用指定 stash（保留 stash）。
pub(crate) fn stash_apply(repo: &Repo, reff: &str) -> Result<(), Error> {
    repo.git(&["stash", "apply", reff])?;
    Ok(())
}

/// 弹出指定 stash（apply + drop，冲突时保留）。
pub(crate) fn stash_pop(repo: &Repo, reff: &str) -> Result<PopResult, Error> {
    let out = repo.git_checked(&["stash", "pop", reff])?;
    if out.success {
        Ok(PopResult::Clean)
    } else {
        Ok(PopResult::Conflict(crate::conflict::conflicted_files(
            repo,
        )?))
    }
}

/// 丢弃指定 stash。
pub(crate) fn stash_drop(repo: &Repo, reff: &str) -> Result<(), Error> {
    repo.git(&["stash", "drop", reff])?;
    Ok(())
}

use crate::{Error, Repo};
use std::path::PathBuf;

/// 指向一个 autostash 条目。
#[derive(Debug, Clone)]
pub struct StashRef {
    /// 创建时写入的标签,用于定位与崩溃恢复。
    pub label: String,
}

/// autostash 贴回(pop)结果。
#[derive(Debug)]
pub enum PopResult {
    /// 干净贴回,stash 已删除。
    Clean,
    /// 贴回冲突,stash 保留,这些文件需处理。
    Conflict(Vec<PathBuf>),
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

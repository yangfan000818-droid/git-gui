//! 危险操作统一撤销:操作时捕获恢复凭据(`UndoAction`),撤销时执行对应恢复原语。
//! 凭据由前端内存持有、单次有效、不持久化;reflog 页仍是兜底(见 docs/undo-design.md)。

use crate::reset::ResetMode;
use crate::{Error, Repo};

/// 一次危险操作的撤销凭据(serde 跨 IPC,前端原样传回)。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind"))]
pub enum UndoAction {
    /// reset 的撤销:按**原 mode** 回到操作前 HEAD(soft 的撤销用 hard 会误杀工作区改动)。
    ResetTo { sha: String, mode: ResetMode },
    /// 删本地分支的撤销:在原 tip 重建同名分支。
    RestoreBranch { name: String, sha: String },
    /// stash drop 的撤销:把 stash commit 塞回 stash 列表。
    RestoreStash { sha: String, message: String },
    /// 丢弃文件改动的撤销:discard 的 stash 兜底按 sha 找回(apply 成功后删兜底条目)。
    RestoreDiscard { sha: String },
}

pub(crate) fn undo(repo: &Repo, action: &UndoAction) -> Result<(), Error> {
    match action {
        UndoAction::ResetTo { sha, mode } => crate::reset::reset(repo, sha, *mode),
        UndoAction::RestoreBranch { name, sha } => {
            repo.git(&["branch", name, sha])?;
            Ok(())
        }
        UndoAction::RestoreStash { sha, message } => {
            repo.git(&["stash", "store", "-m", message, sha])?;
            Ok(())
        }
        UndoAction::RestoreDiscard { sha } => {
            // 先贴回工作区,成功后按 sha 定位兜底条目删除;apply 失败(冲突等)不删,
            // 兜底仍留在 Stash 列表里可手动找回。
            repo.git(&["stash", "apply", sha])?;
            if let Some(reff) = find_stash_ref_by_sha(repo, sha)? {
                repo.git(&["stash", "drop", &reff])?;
            }
            Ok(())
        }
    }
}

/// 在 stash 列表中按 commit sha 找当前引用名(stash@{N} 会随增删漂移,sha 不会)。
fn find_stash_ref_by_sha(repo: &Repo, sha: &str) -> Result<Option<String>, Error> {
    let out = repo.git(&["stash", "list", "--pretty=format:%gd%x00%H"])?;
    for line in out.lines() {
        if let Some((reff, h)) = line.split_once('\0') {
            if h == sha {
                return Ok(Some(reff.to_string()));
            }
        }
    }
    Ok(None)
}

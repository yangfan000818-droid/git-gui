use crate::{Error, Repo};

/// reset 模式(对标 git reset --soft/--mixed/--hard)。
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ResetMode {
    /// 移动分支指针,改动保留在暂存区。
    Soft,
    /// 移动分支指针,改动退回工作区(未暂存);git 默认。
    Mixed,
    /// 移动分支指针,丢弃工作区与暂存区改动(不可恢复)。
    Hard,
}

/// 把当前分支重置到指定提交,返回撤销凭据(操作前 HEAD + 原 mode)。
pub(crate) fn reset_with_undo(
    repo: &Repo,
    sha: &str,
    mode: ResetMode,
) -> Result<crate::undo::UndoAction, Error> {
    let prev = repo.git(&["rev-parse", "HEAD"])?.trim().to_string();
    reset(repo, sha, mode)?;
    Ok(crate::undo::UndoAction::ResetTo { sha: prev, mode })
}

/// 把当前分支重置到指定提交。
pub(crate) fn reset(repo: &Repo, sha: &str, mode: ResetMode) -> Result<(), Error> {
    let flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
    };
    repo.git(&["reset", flag, sha])?;
    Ok(())
}

use crate::{Error, Repo};

/// reset 模式(对标 git reset --soft/--mixed/--hard)。
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub enum ResetMode {
    /// 移动分支指针,改动保留在暂存区。
    Soft,
    /// 移动分支指针,改动退回工作区(未暂存);git 默认。
    Mixed,
    /// 移动分支指针,丢弃工作区与暂存区改动(不可恢复)。
    Hard,
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

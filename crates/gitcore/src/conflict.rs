use crate::{Error, Repo};
use std::path::{Path, PathBuf};

/// 一个冲突文件的三个版本(三路合并的输入)。
#[derive(Debug, Clone)]
pub struct ThreeVersions {
    /// 共同祖先(stage 1);新增-新增冲突时为 None。
    pub base: Option<String>,
    /// 当前分支版本(stage 2,ours)。
    pub ours: Option<String>,
    /// 合入分支版本(stage 3,theirs)。
    pub theirs: Option<String>,
}

/// 列出当前所有冲突文件。
pub fn conflicted_files(repo: &Repo) -> Result<Vec<PathBuf>, Error> {
    let out = repo.git(&["diff", "--name-only", "--diff-filter=U"])?;
    Ok(out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect())
}

/// 取某个冲突文件的三个版本原文;某阶段不存在则为 None。
///
/// 直接从暂存区的三个阶段取,而非解析文件里的 `<<<<<<<` 标记——
/// 这是三栏合并 UI 的数据来源。
pub fn three_versions(repo: &Repo, path: &Path) -> Result<ThreeVersions, Error> {
    let p = path.to_string_lossy();
    Ok(ThreeVersions {
        base: show_stage(repo, 1, &p)?,
        ours: show_stage(repo, 2, &p)?,
        theirs: show_stage(repo, 3, &p)?,
    })
}

fn show_stage(repo: &Repo, stage: u8, path: &str) -> Result<Option<String>, Error> {
    let out = repo.git_checked(&["show", &format!(":{stage}:{path}")])?;
    Ok(out.success.then_some(out.stdout))
}

use crate::{Error, Repo};
use std::path::Path;

/// 暂存指定文件。
pub(crate) fn stage_files(repo: &Repo, paths: &[&Path]) -> Result<(), Error> {
    if paths.is_empty() {
        return Ok(());
    }
    let args: Vec<&str> = std::iter::once("add")
        .chain(paths.iter().map(|p| p.as_os_str().to_str().unwrap()))
        .collect();
    repo.git(&args)?;
    Ok(())
}

/// 取消暂存指定文件。
pub(crate) fn unstage_files(repo: &Repo, paths: &[&Path]) -> Result<(), Error> {
    if paths.is_empty() {
        return Ok(());
    }
    let args: Vec<&str> = std::iter::once("restore")
        .chain(std::iter::once("--staged"))
        .chain(paths.iter().map(|p| p.as_os_str().to_str().unwrap()))
        .collect();
    repo.git(&args)?;
    Ok(())
}

/// 暂存所有改动和未跟踪文件。
pub(crate) fn stage_all(repo: &Repo) -> Result<(), Error> {
    repo.git(&["add", "."])?;
    Ok(())
}

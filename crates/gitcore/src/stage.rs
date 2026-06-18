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

/// 回滚指定文件的改动:先用 stash 兜底(可在 Stash 视图 pop 找回),工作区随即回到 HEAD。
/// 含未跟踪文件(`--include-untracked`)。指定路径无改动时静默视作无操作。
pub(crate) fn discard_files(repo: &Repo, paths: &[&Path]) -> Result<(), Error> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = vec![
        "stash",
        "push",
        "--include-untracked",
        "-m",
        "git-gui discard 兜底",
        "--",
    ];
    args.extend(paths.iter().map(|p| p.as_os_str().to_str().unwrap()));
    let out = repo.git_checked(&args)?;
    if out.success
        || out.stdout.contains("No local changes")
        || out.stderr.contains("No local changes")
    {
        Ok(())
    } else {
        Err(Error::Git {
            args: args.iter().map(|s| s.to_string()).collect(),
            code: out.code,
            stderr: out.stderr,
        })
    }
}

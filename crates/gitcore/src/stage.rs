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

/// 取消暂存所有改动(把 index 重置回 HEAD,工作区不动)。index 为空时为 no-op。
pub(crate) fn unstage_all(repo: &Repo) -> Result<(), Error> {
    repo.git(&["reset"])?;
    Ok(())
}

/// 回滚指定文件的改动:先用 stash 兜底(可在 Stash 视图 pop 找回),工作区随即回到 HEAD。
/// 含未跟踪文件(`--include-untracked`)。指定路径无改动时静默视作无操作(返回 None)。
/// 成功时返回撤销凭据(兜底 stash 的 commit sha,供一键找回)。
pub(crate) fn discard_files(
    repo: &Repo,
    paths: &[&Path],
) -> Result<Option<crate::undo::UndoAction>, Error> {
    if paths.is_empty() {
        return Ok(None);
    }
    let mut args: Vec<&str> = vec![
        "stash",
        "push",
        "--include-untracked",
        "-m",
        "GitSail discard 兜底",
        "--",
    ];
    args.extend(paths.iter().map(|p| p.as_os_str().to_str().unwrap()));
    let out = repo.git_checked(&args)?;
    // 注意顺序:无改动时 push 也是 exit 0,必须先判 "No local changes" 再判 success,
    // 否则会去 rev-parse 不存在的 refs/stash。
    if out.stdout.contains("No local changes") || out.stderr.contains("No local changes") {
        Ok(None)
    } else if out.success {
        // 刚创建的兜底在栈顶;记 commit sha(stash@{N} 会漂移,sha 不会)。
        let sha = repo.git(&["rev-parse", "refs/stash"])?.trim().to_string();
        Ok(Some(crate::undo::UndoAction::RestoreDiscard { sha }))
    } else {
        Err(Error::Git {
            args: args.iter().map(|s| s.to_string()).collect(),
            code: out.code,
            stderr: out.stderr,
        })
    }
}

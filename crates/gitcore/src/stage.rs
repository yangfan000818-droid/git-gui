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
    // 按 stash 栈顶前后是否变化判断是否真的建出了兜底,不匹配 "No local changes" 文案:
    // 本地化的 git 输出译文,匹配不上会把栈顶一个无关的旧 stash 记成撤销凭据。
    let stash_top = || -> Result<Option<String>, Error> {
        let out = repo.git_checked(&["rev-parse", "-q", "--verify", "refs/stash"])?;
        Ok(out.success.then(|| out.stdout.trim().to_string()))
    };
    let before = stash_top()?;
    let out = repo.git_checked(&args)?;
    if !out.success {
        return Err(Error::Git {
            args: args.iter().map(|s| s.to_string()).collect(),
            code: out.code,
            stderr: out.stderr,
        });
    }
    // 无改动时 push 也是 exit 0 但不建 stash,栈顶不变。
    // 新建的兜底在栈顶;记 commit sha(stash@{N} 会漂移,sha 不会)。
    match stash_top()? {
        Some(sha) if before.as_ref() != Some(&sha) => {
            Ok(Some(crate::undo::UndoAction::RestoreDiscard { sha }))
        }
        _ => Ok(None),
    }
}

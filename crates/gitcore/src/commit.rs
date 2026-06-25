use crate::{Error, Repo};

/// commit 的选项。
#[derive(Debug, Clone, Default)]
pub struct CommitOptions {
    pub message: String,
    pub allow_empty: bool,
    /// 修改上一次提交(git commit --amend)而非新建。
    pub amend: bool,
    /// 跳过 git 钩子(--no-verify):pre-commit / commit-msg 不运行。
    pub no_verify: bool,
}

/// 创建提交,返回新提交的 SHA(前 8 位)。
pub(crate) fn commit(repo: &Repo, opts: &CommitOptions) -> Result<String, Error> {
    // 检查暂存区是否为空(amend 允许只改 message,故跳过此检查)。
    if !opts.allow_empty && !opts.amend {
        // git diff --cached --quiet: 退出码 0 表示无差异(暂存区空), 1 表示有差异
        let check = repo.git_checked(&["diff", "--cached", "--quiet"])?;
        if check.success {
            // 无差异,暂存区为空
            return Err(Error::Precondition("暂存区为空,无内容可提交".into()));
        }
    }

    let mut args = vec!["commit", "-m", &opts.message];
    if opts.amend {
        args.push("--amend");
    }
    if opts.allow_empty {
        args.push("--allow-empty");
    }
    if opts.no_verify {
        args.push("--no-verify");
    }

    repo.git(&args)?;

    // 获取新提交的 SHA
    let sha = repo.git(&["rev-parse", "--short=8", "HEAD"])?;
    Ok(sha.trim().to_string())
}

/// 只提交指定路径的工作区内容(供 Changelist 按组提交),忽略其它已暂存改动。
/// 先 `git add -- <paths>`(让未跟踪文件可被纳入,已跟踪幂等),再 `git commit -- <paths>`
/// ——`git commit -- <pathspec>` 只提交这些路径的工作区版本,其它已暂存内容保持不动。
pub(crate) fn commit_paths(
    repo: &Repo,
    message: &str,
    paths: &[String],
    no_verify: bool,
) -> Result<String, Error> {
    if paths.is_empty() {
        return Err(Error::Precondition("没有要提交的文件".into()));
    }

    let mut add_args: Vec<&str> = vec!["add", "--"];
    add_args.extend(paths.iter().map(|p| p.as_str()));
    repo.git(&add_args)?;

    let mut args: Vec<&str> = vec!["commit", "-m", message];
    if no_verify {
        args.push("--no-verify");
    }
    args.push("--");
    args.extend(paths.iter().map(|p| p.as_str()));
    repo.git(&args)?;

    let sha = repo.git(&["rev-parse", "--short=8", "HEAD"])?;
    Ok(sha.trim().to_string())
}

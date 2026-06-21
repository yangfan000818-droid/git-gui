use crate::{Error, Repo};

/// commit 的选项。
#[derive(Debug, Clone, Default)]
pub struct CommitOptions {
    pub message: String,
    pub allow_empty: bool,
    /// 修改上一次提交(git commit --amend)而非新建。
    pub amend: bool,
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

    repo.git(&args)?;

    // 获取新提交的 SHA
    let sha = repo.git(&["rev-parse", "--short=8", "HEAD"])?;
    Ok(sha.trim().to_string())
}

use crate::{Error, Repo};

/// push 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// 推送成功。
    Success,
    /// 当前分支没有 upstream,需要用户手动设置。
    NoUpstream,
    /// 非快进推送被拒绝(远端领先),需要先 pull。
    NonFastForward,
}

/// 推送当前分支到 upstream。
pub(crate) fn push(repo: &Repo) -> Result<PushOutcome, Error> {
    // 检查是否有 upstream
    let upstream = repo.git_checked(&["rev-parse", "--abbrev-ref", "@{u}"])?;
    if !upstream.success {
        return Ok(PushOutcome::NoUpstream);
    }

    // 执行 push
    let result = repo.git_checked(&["push"])?;
    if result.success {
        return Ok(PushOutcome::Success);
    }

    // 检查失败原因
    let stderr_lower = result.stderr.to_lowercase();
    if stderr_lower.contains("rejected") || stderr_lower.contains("non-fast-forward") {
        return Ok(PushOutcome::NonFastForward);
    }

    // 其他错误
    Err(Error::Git {
        args: vec!["push".to_string()],
        code: result.code,
        stderr: result.stderr,
    })
}

use crate::{CancelToken, Error, Progress, Repo};

/// push 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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

/// 流式推送:进度经 `on_progress` 上报,`cancel` 置位则中止。判定逻辑同 [`push`]。
pub(crate) fn push_streaming(
    repo: &Repo,
    on_progress: &mut dyn FnMut(Progress),
    cancel: &CancelToken,
) -> Result<PushOutcome, Error> {
    let upstream = repo.git_checked(&["rev-parse", "--abbrev-ref", "@{u}"])?;
    if !upstream.success {
        return Ok(PushOutcome::NoUpstream);
    }

    let result = repo.git_streaming(&["push", "--progress"], on_progress, cancel)?;
    if result.success {
        return Ok(PushOutcome::Success);
    }

    let stderr_lower = result.stderr.to_lowercase();
    if stderr_lower.contains("rejected") || stderr_lower.contains("non-fast-forward") {
        return Ok(PushOutcome::NonFastForward);
    }

    Err(Error::Git {
        args: vec!["push".to_string(), "--progress".to_string()],
        code: result.code,
        stderr: result.stderr,
    })
}

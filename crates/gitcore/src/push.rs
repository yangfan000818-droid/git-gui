use crate::{CancelToken, Error, LogEntry, Progress, Repo};

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

/// Push 对话框的预览:目标 upstream + 本地领先 upstream 的待推送提交。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PushPreview {
    /// upstream 引用名(如 origin/main);None = 当前分支无 upstream。
    pub upstream: Option<String>,
    /// 本地领先 upstream 的提交(`@{u}..HEAD`,newest first)。
    pub commits: Vec<LogEntry>,
}

/// 推送预览:取当前分支 upstream 及 `@{u}..HEAD` 待推送提交,供 Push 对话框做安全网。
/// 无 upstream 时返回 `upstream: None` + 空列表(由前端提示需先设上游)。
pub(crate) fn push_preview(repo: &Repo) -> Result<PushPreview, Error> {
    let upstream = repo.git_checked(&["rev-parse", "--abbrev-ref", "@{u}"])?;
    if !upstream.success {
        return Ok(PushPreview {
            upstream: None,
            commits: vec![],
        });
    }
    let up = upstream.stdout.trim().to_string();
    let commits = crate::log::rev_range(repo, "@{u}..HEAD")?;
    Ok(PushPreview {
        upstream: Some(up),
        commits,
    })
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
/// `force_with_lease` 为真时加 `--force-with-lease`(安全强制:仅当远端未被他人推进时才覆盖)。
pub(crate) fn push_streaming(
    repo: &Repo,
    force_with_lease: bool,
    on_progress: &mut dyn FnMut(Progress),
    cancel: &CancelToken,
) -> Result<PushOutcome, Error> {
    let upstream = repo.git_checked(&["rev-parse", "--abbrev-ref", "@{u}"])?;
    if !upstream.success {
        return Ok(PushOutcome::NoUpstream);
    }

    let mut args = vec!["push", "--progress"];
    if force_with_lease {
        args.push("--force-with-lease");
    }
    let result = repo.git_streaming(&args, on_progress, cancel)?;
    if result.success {
        return Ok(PushOutcome::Success);
    }

    let stderr_lower = result.stderr.to_lowercase();
    if stderr_lower.contains("rejected") || stderr_lower.contains("non-fast-forward") {
        return Ok(PushOutcome::NonFastForward);
    }

    Err(Error::Git {
        args: args.iter().map(|s| s.to_string()).collect(),
        code: result.code,
        stderr: result.stderr,
    })
}

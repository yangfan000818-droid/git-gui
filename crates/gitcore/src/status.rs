use crate::{Error, Repo};
use std::path::PathBuf;

/// 工作区某一刻的状态快照。
#[derive(Debug, Clone)]
pub struct RepoStatus {
    /// 当前分支;detached HEAD 为 None。
    pub branch: Option<String>,
    /// upstream(如 origin/main);未设置为 None。
    pub upstream: Option<String>,
    /// 落后 upstream 的提交数。
    pub behind: u32,
    /// 领先 upstream 的提交数。
    pub ahead: u32,
    /// 是否有未提交改动(含 untracked)。
    pub dirty: bool,
    /// 当前冲突文件。
    pub conflicted: Vec<PathBuf>,
}

pub(crate) fn status(repo: &Repo) -> Result<RepoStatus, Error> {
    let branch_raw = repo.git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch_raw = branch_raw.trim();
    let branch = (branch_raw != "HEAD").then(|| branch_raw.to_string());

    // 没有 upstream 时该命令非零退出,用 git_checked 容错。
    let up = repo.git_checked(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])?;
    let upstream = up.success.then(|| up.stdout.trim().to_string());

    let (behind, ahead) = if upstream.is_some() {
        parse_left_right(&repo.git(&["rev-list", "--left-right", "--count", "@{u}...HEAD"])?)?
    } else {
        (0, 0)
    };

    let dirty = !repo.git(&["status", "--porcelain"])?.trim().is_empty();
    let conflicted = crate::conflict::conflicted_files(repo)?;

    Ok(RepoStatus {
        branch,
        upstream,
        behind,
        ahead,
        dirty,
        conflicted,
    })
}

// 解析 "behind<TAB>ahead"(git rev-list --left-right --count @{u}...HEAD)。
fn parse_left_right(s: &str) -> Result<(u32, u32), Error> {
    let mut it = s.split_whitespace();
    match (
        it.next().and_then(|x| x.parse().ok()),
        it.next().and_then(|x| x.parse().ok()),
    ) {
        (Some(b), Some(a)) => Ok((b, a)),
        _ => Err(Error::Parse(format!("rev-list --count 输出异常: {s:?}"))),
    }
}

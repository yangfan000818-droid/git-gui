use crate::{Error, Repo};

/// 分支信息。
#[derive(Debug, Clone)]
pub struct BranchInfo {
    /// 分支名称。
    pub name: String,
    /// 当前分支。
    pub is_current: bool,
    /// upstream(如 origin/main)。
    pub upstream: Option<String>,
    /// 领先 upstream 的提交数。
    pub ahead: u32,
    /// 落后 upstream 的提交数。
    pub behind: u32,
}

/// 列出所有本地分支。
pub(crate) fn list_branches(repo: &Repo) -> Result<Vec<BranchInfo>, Error> {
    let output = repo.git(&[
        "for-each-ref",
        "--format=%(refname:short)%00%(upstream:short)%00%(upstream:track)%00%(HEAD)",
        "refs/heads/",
    ])?;

    let mut branches = Vec::new();
    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\0').collect();
        if parts.len() < 4 {
            continue;
        }
        let name = parts[0].to_string();
        let upstream = if parts[1].is_empty() {
            None
        } else {
            Some(parts[1].to_string())
        };
        let is_current = parts[3] == "*";

        let (ahead, behind) = parse_track(parts[2]);

        branches.push(BranchInfo {
            name,
            is_current,
            upstream,
            ahead,
            behind,
        });
    }
    Ok(branches)
}

/// 解析 upstream:track 字段，如 "ahead 1"、"behind 2"、"ahead 1, behind 3"。
fn parse_track(track: &str) -> (u32, u32) {
    let mut ahead = 0u32;
    let mut behind = 0u32;
    for part in track.split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

/// 创建新分支（不切换）。
pub(crate) fn create_branch(repo: &Repo, name: &str) -> Result<(), Error> {
    repo.git(&["branch", name])?;
    Ok(())
}

/// 切换到指定分支。
pub(crate) fn switch_branch(repo: &Repo, name: &str) -> Result<(), Error> {
    // 检查工作区是否脏
    let dirty = !repo.git(&["status", "--porcelain"])?.trim().is_empty();
    if dirty {
        return Err(Error::Precondition(
            "工作区有未提交改动，请先提交或暂存".into(),
        ));
    }
    repo.git(&["checkout", name])?;
    Ok(())
}

/// 删除分支（安全模式：不删除未合并的分支）。
pub(crate) fn delete_branch(repo: &Repo, name: &str) -> Result<(), Error> {
    // 不能删除当前分支
    let current = repo.git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    if current.trim() == name {
        return Err(Error::Precondition("不能删除当前分支".into()));
    }
    repo.git(&["branch", "-d", name])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_track_ahead_and_behind() {
        assert_eq!(parse_track(""), (0, 0));
        assert_eq!(parse_track("ahead 3"), (3, 0));
        assert_eq!(parse_track("behind 5"), (0, 5));
        assert_eq!(parse_track("ahead 1, behind 2"), (1, 2));
    }
}

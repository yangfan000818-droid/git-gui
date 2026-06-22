use crate::stash::{self, PopResult};
use crate::{Error, Repo};
use std::path::PathBuf;

/// 分支信息。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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
    /// 远程跟踪分支(refs/remotes/),非本地分支。
    pub is_remote: bool,
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
            is_remote: false,
        });
    }
    Ok(branches)
}

/// 列出所有远程跟踪分支(refs/remotes/),过滤掉 origin/HEAD 这类符号引用。
pub(crate) fn list_remote_branches(repo: &Repo) -> Result<Vec<BranchInfo>, Error> {
    let output = repo.git(&["for-each-ref", "--format=%(refname:short)", "refs/remotes/"])?;

    let mut branches = Vec::new();
    for line in output.lines() {
        let name = line.trim();
        // origin/HEAD 是符号引用(指向默认分支),不是真分支
        if name.is_empty() || name.ends_with("/HEAD") {
            continue;
        }
        branches.push(BranchInfo {
            name: name.to_string(),
            is_current: false,
            upstream: None,
            ahead: 0,
            behind: 0,
            is_remote: true,
        });
    }
    Ok(branches)
}

/// 从远程分支名推断本地分支名:去掉第一段远程名
/// (origin/feat → feat;origin/feature/x → feature/x)。
fn local_name_from_remote(remote_branch: &str) -> Option<&str> {
    remote_branch
        .split_once('/')
        .map(|(_, rest)| rest)
        .filter(|s| !s.is_empty())
}

/// 检出远程分支为本地跟踪分支(git checkout -b <local> --track <remote>)。
pub(crate) fn checkout_remote(repo: &Repo, remote_branch: &str) -> Result<(), Error> {
    // 工作区脏时拒绝(与 switch_branch 一致)
    let dirty = !repo.git(&["status", "--porcelain"])?.trim().is_empty();
    if dirty {
        return Err(Error::Precondition(
            "工作区有未提交改动，请先提交或暂存".into(),
        ));
    }
    let local = local_name_from_remote(remote_branch).ok_or_else(|| {
        Error::Precondition(format!(
            "无法从远程分支名 \"{remote_branch}\" 推断本地分支名"
        ))
    })?;
    // 本地已存在同名分支 → 报错引导直接切换,不覆盖
    let exists = repo
        .git_checked(&[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{local}"),
        ])?
        .success;
    if exists {
        return Err(Error::Precondition(format!(
            "本地已存在分支 \"{local}\"，请直接切换到它"
        )));
    }
    repo.git(&["checkout", "-b", local, "--track", remote_branch])?;
    Ok(())
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

/// 创建新分支（不切换）。`start` 为 None 时从当前 HEAD 创建,Some 时从指定起点(分支/提交)创建。
pub(crate) fn create_branch(repo: &Repo, name: &str, start: Option<&str>) -> Result<(), Error> {
    let mut args = vec!["branch", name];
    if let Some(s) = start {
        args.push(s);
    }
    repo.git(&args)?;
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

/// 脏工作区切换(smart checkout)的结果。
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum SwitchOutcome {
    /// 干净切换(原本干净,或暂存的改动已无冲突贴回新分支)。
    Switched,
    /// 已暂存改动并切到新分支,但贴回时冲突:改动带冲突标记留在工作区,
    /// stash 仍保留;需在改动列表中解决。
    StashConflict { files: Vec<PathBuf> },
}

/// 脏工作区智能切换(对标 WebStorm「Smart Checkout」):自动 stash → checkout → 贴回。
/// 工作区干净时等同普通切换。贴回冲突时改动安全留在工作区 + stash,交前端提示解决,
/// 不进合并 continue/abort 流(这并非合并进行中态)。
pub(crate) fn switch_branch_autostash(repo: &Repo, name: &str) -> Result<SwitchOutcome, Error> {
    // 干净 → None(不 stash);脏 → stash(含 untracked)后返回 Some,工作区随即清空。
    let label = stash::autostash_label();
    let stash = stash::autostash_push(repo, &label)?;

    // 工作区已清空,checkout 不会因「改动会被覆盖」而失败。
    let out = repo.git_checked(&["checkout", name])?;
    if !out.success {
        // 罕见失败:把改动贴回原分支,绝不把 stash 遗弃。
        if let Some(s) = &stash {
            let _ = stash::autostash_pop(repo, s);
        }
        return Err(Error::Git {
            args: vec!["checkout".into(), name.into()],
            code: out.code,
            stderr: out.stderr,
        });
    }

    // 已在新分支:贴回暂存的改动。
    match stash {
        None => Ok(SwitchOutcome::Switched),
        Some(s) => match stash::autostash_pop(repo, &s)? {
            PopResult::Clean => Ok(SwitchOutcome::Switched),
            PopResult::Conflict(files) => Ok(SwitchOutcome::StashConflict { files }),
        },
    }
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

/// 重命名分支(git branch -m;目标名已存在时 git 报错)。
pub(crate) fn rename_branch(repo: &Repo, old: &str, new: &str) -> Result<(), Error> {
    repo.git(&["branch", "-m", old, new])?;
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

    #[test]
    fn local_name_strips_remote_prefix() {
        assert_eq!(local_name_from_remote("origin/feat"), Some("feat"));
        assert_eq!(
            local_name_from_remote("origin/feature/x"),
            Some("feature/x")
        );
        assert_eq!(local_name_from_remote("upstream/main"), Some("main"));
        // 没有 '/' 或去掉前缀后为空 → None
        assert_eq!(local_name_from_remote("origin"), None);
        assert_eq!(local_name_from_remote("origin/"), None);
    }
}

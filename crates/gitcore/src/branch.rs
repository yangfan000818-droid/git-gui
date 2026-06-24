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

/// 判断 git checkout 失败是否因为"本地改动会被覆盖"(中/英文均支持)。
fn is_checkout_blocked_by_local_changes(stderr: &str) -> bool {
    stderr.contains("would be overwritten")
        || stderr.contains("将被检出")
        || stderr.contains("commit your changes or stash")
        || stderr.contains("提交或贮藏")
}

/// 把 checkout 的执行结果归一:成功→Ok;"改动会被覆盖"→可识别的 Precondition
/// (交前端做 Smart Checkout);其余错误原样抛出。
fn finish_checkout(result: Result<String, Error>) -> Result<(), Error> {
    match result {
        Ok(_) => Ok(()),
        Err(Error::Git { args, code, stderr }) => {
            if is_checkout_blocked_by_local_changes(&stderr) {
                Err(Error::Precondition(
                    "工作区有未提交改动会被覆盖，请先提交或暂存".into(),
                ))
            } else {
                Err(Error::Git { args, code, stderr })
            }
        }
        Err(e) => Err(e),
    }
}

/// 远程分支检出前置:推断本地分支名,并拒绝本地已存在同名分支(引导直接切换,不覆盖)。
/// 返回待创建的本地分支名。普通检出与 smart checkout 共用。
fn resolve_remote_checkout_target<'a>(
    repo: &Repo,
    remote_branch: &'a str,
) -> Result<&'a str, Error> {
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
    Ok(local)
}

/// 检出远程分支为本地跟踪分支(git checkout -b <local> --track <remote>)。
pub(crate) fn checkout_remote(repo: &Repo, remote_branch: &str) -> Result<(), Error> {
    let local = resolve_remote_checkout_target(repo, remote_branch)?;
    // 不预检脏区:让 git checkout 自己判。改动不冲突则成功(静默带到新分支),
    // 冲突则返回可识别错误给前端做 Smart Checkout。
    finish_checkout(repo.git(&["checkout", "-b", local, "--track", remote_branch]))
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

/// 切换到指定分支。不预检脏区:让 git checkout 自己判——改动与目标分支无冲突则成功,
/// 冲突则返回可识别错误给前端做 Smart Checkout(对标 WebStorm)。
pub(crate) fn switch_branch(repo: &Repo, name: &str) -> Result<(), Error> {
    finish_checkout(repo.git(&["checkout", name]))
}

/// 检出某个提交,进入 detached HEAD(对标 WebStorm Checkout Revision)。
/// 不预检脏区:让 git checkout 自己判,冲突才拒(与 [`switch_branch`] 一致)。
pub(crate) fn checkout_commit(repo: &Repo, sha: &str) -> Result<(), Error> {
    finish_checkout(repo.git(&["checkout", sha]))
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

/// 脏工作区智能检出(对标 WebStorm「Smart Checkout」)的通用实现:
/// autostash → 执行 `args` 指定的 checkout → 贴回暂存改动。
/// 工作区干净时等同普通检出。贴回冲突时改动安全留在工作区 + stash,交前端提示解决,
/// 不进合并 continue/abort 流(这并非合并进行中态)。
fn checkout_autostash(repo: &Repo, args: &[&str]) -> Result<SwitchOutcome, Error> {
    // 干净 → None(不 stash);脏 → stash(含 untracked)后返回 Some,工作区随即清空。
    let label = stash::autostash_label();
    let stash = stash::autostash_push(repo, &label)?;

    // 工作区已清空,checkout 不会因「改动会被覆盖」而失败。
    let out = repo.git_checked(args)?;
    if !out.success {
        // 罕见失败:把改动贴回原位置,绝不把 stash 遗弃。
        if let Some(s) = &stash {
            let _ = stash::autostash_pop(repo, s);
        }
        return Err(Error::Git {
            args: args.iter().map(|s| s.to_string()).collect(),
            code: out.code,
            stderr: out.stderr,
        });
    }

    // 已在新位置:贴回暂存的改动。
    match stash {
        None => Ok(SwitchOutcome::Switched),
        Some(s) => match stash::autostash_pop(repo, &s)? {
            PopResult::Clean => Ok(SwitchOutcome::Switched),
            PopResult::Conflict(files) => Ok(SwitchOutcome::StashConflict { files }),
        },
    }
}

/// 脏工作区智能切换本地分支(smart checkout)。
pub(crate) fn switch_branch_autostash(repo: &Repo, name: &str) -> Result<SwitchOutcome, Error> {
    checkout_autostash(repo, &["checkout", name])
}

/// 脏工作区智能检出远程分支为本地跟踪分支(smart checkout)。
pub(crate) fn checkout_remote_autostash(
    repo: &Repo,
    remote_branch: &str,
) -> Result<SwitchOutcome, Error> {
    let local = resolve_remote_checkout_target(repo, remote_branch)?;
    checkout_autostash(repo, &["checkout", "-b", local, "--track", remote_branch])
}

/// 脏工作区智能检出提交,进入 detached HEAD(smart checkout)。
pub(crate) fn checkout_commit_autostash(repo: &Repo, sha: &str) -> Result<SwitchOutcome, Error> {
    checkout_autostash(repo, &["checkout", sha])
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

    #[test]
    fn detects_checkout_blocked_by_local_changes() {
        // 英文 git(本机 Apple Git 实测原文)
        let en = "error: Your local changes to the following files would be overwritten by checkout:\n\ta.txt\nPlease commit your changes or stash them before you switch branches.\nAborting\n";
        assert!(is_checkout_blocked_by_local_changes(en));
        // 中文 git(对照 git 官方 zh_CN.po 译文)
        let zh = "error: 您对下列文件的本地修改将被检出操作覆盖:\n\ta.txt\n请在切换分支前提交或贮藏您的修改。\n正在终止\n";
        assert!(is_checkout_blocked_by_local_changes(zh));
        // 其它失败(分支不存在等)不应误判为脏区冲突
        let other = "error: pathspec 'nope' did not match any file(s) known to git";
        assert!(!is_checkout_blocked_by_local_changes(other));
    }
}

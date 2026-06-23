//! 交互式变基(对标 WebStorm "Interactively Rebase from Here")。
//!
//! 非交互地驱动 `git rebase -i`:用 `sequence.editor=cp` 把预先编译好的 todo
//! 覆盖进 git 生成的待办表,完全绕开交互式编辑器。reword/squash 改写提交信息
//! 不走 `reword`(那会再唤起编辑器),而是 `pick` + `exec git commit --amend -F <file>`——
//! 信息内容写进文件、文件名由我们掌控,既不经 shell 转义用户文本(无注入),
//! 也无需任何编辑器交互。冲突沿用 update 的 continue/abort/ConflictView 机制。

use crate::update::UpdateOutcome;
use crate::{Error, LogEntry, Repo};
use std::path::{Path, PathBuf};

/// 交互式变基里对单个提交的操作(对标 `git rebase -i` 的 todo 动作)。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub enum RebaseAction {
    /// 保留提交。
    Pick,
    /// 保留提交,但把提交信息改写为给定文本。
    Reword(String),
    /// 折叠进上一个保留的提交,并把合并后提交的信息设为给定文本。
    Squash(String),
    /// 折叠进上一个保留的提交,丢弃本提交信息(沿用上一个)。
    Fixup,
    /// 丢弃该提交。
    Drop,
}

/// 交互式变基的一个待办项:提交 SHA + 操作。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct RebaseItem {
    /// 提交 SHA(取 [`rebase_plan`] 返回的 `full_sha`)。
    pub sha: String,
    /// 对该提交执行的操作。
    pub action: RebaseAction,
}

/// 列出 `from_sha..HEAD`(含 from_sha)的提交,**oldest-first**(= git todo 顺序),
/// 供前端展示给用户编辑。from_sha 必须是 HEAD 的祖先。
pub(crate) fn rebase_plan(repo: &Repo, from_sha: &str) -> Result<Vec<LogEntry>, Error> {
    require_ancestor(repo, from_sha)?;
    // from_sha 含进区间:用 from_sha^..HEAD;from_sha 是根提交则没有父,列全部。
    let range = if has_parent(repo, from_sha)? {
        format!("{from_sha}^..HEAD")
    } else {
        "HEAD".to_string()
    };
    let out = repo.git(&[
        "log",
        "--reverse",
        "--pretty=format:%H%x00%h%x00%s%x00%an%x00%ar",
        &range,
    ])?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let p: Vec<&str> = line.split('\0').collect();
            (p.len() == 5).then(|| LogEntry {
                full_sha: p[0].to_string(),
                sha: p[1].to_string(),
                message: p[2].to_string(),
                author: p[3].to_string(),
                date: p[4].to_string(),
            })
        })
        .collect())
}

/// 从 `from_sha` 起按 `items` 给定的操作/顺序交互式变基。
/// 要求工作区干净、无进行中的整合、from_sha 是 HEAD 祖先。
/// 冲突时返回 [`UpdateOutcome::Conflicted`],交前端 ConflictView 解决后 `continue_update` 推进。
pub(crate) fn rebase_interactive(
    repo: &Repo,
    from_sha: &str,
    items: &[RebaseItem],
) -> Result<UpdateOutcome, Error> {
    crate::update::preflight(repo)?;
    if !repo.git(&["status", "--porcelain"])?.trim().is_empty() {
        return Err(Error::Precondition(
            "交互式变基前请先提交或暂存改动(工作区需干净)".into(),
        ));
    }
    require_ancestor(repo, from_sha)?;

    // 信息文件目录:每次清空重建,避免上次残留被误用。
    let msg_dir = git_dir(repo)?.join("girebase-msgs");
    let _ = std::fs::remove_dir_all(&msg_dir);
    std::fs::create_dir_all(&msg_dir)?;

    let (todo, msgs) = compile_todo(items, &msg_dir)?;
    for (path, content) in &msgs {
        std::fs::write(path, content)?;
    }
    let todo_path = msg_dir.join("todo");
    std::fs::write(&todo_path, &todo)?;

    // sequence.editor=cp '<todo>':git 调用 `cp '<todo>' <它生成的todo文件>`,即覆盖。
    // core.editor=true:万一某步唤起编辑器也直接通过,不阻塞。
    let seq_editor = format!("sequence.editor=cp '{}'", todo_path.display());
    let base_arg = if has_parent(repo, from_sha)? {
        format!("{from_sha}^")
    } else {
        "--root".to_string()
    };
    let args = vec![
        "-c",
        &seq_editor,
        "-c",
        "core.editor=true",
        "-c",
        "rerere.enabled=true",
        "-c",
        "merge.conflictStyle=zdiff3",
        "rebase",
        "-i",
        &base_arg,
    ];

    let out = repo.git_checked(&args)?;
    if out.success {
        let _ = std::fs::remove_dir_all(&msg_dir);
        return Ok(UpdateOutcome::Resolved);
    }

    // 非零退出:可能是某个提交冲突暂停。先让 rerere 重放已知解法。
    crate::update::auto_resolve_rerere(repo)?;
    let files = crate::conflict::conflicted_files(repo)?;
    if !files.is_empty() {
        return Ok(UpdateOutcome::Conflicted {
            files,
            autostash: None,
        });
    }
    // 无冲突标记但仍在变基中(如 rerere 全解决)→ 交 continue_update 收尾/推进下一步。
    if crate::update::rebase_in_progress(repo)? {
        return crate::update::continue_update(repo, None, false);
    }
    // 既不在变基中、又非成功:真失败(如 exec/hook 报错)。
    let _ = std::fs::remove_dir_all(&msg_dir);
    Err(Error::Git {
        args: args.iter().map(|s| s.to_string()).collect(),
        code: out.code,
        stderr: out.stderr,
    })
}

/// 把编辑后的待办项编译成 `git rebase -i` 的 todo 文本,以及需预先写入的信息文件。
/// reword/squash 用 `exec git commit --amend -F <file>` 落地,完全避开交互式编辑器。
fn compile_todo(
    items: &[RebaseItem],
    msg_dir: &Path,
) -> Result<(String, Vec<(PathBuf, String)>), Error> {
    let mut todo = String::new();
    let mut msgs: Vec<(PathBuf, String)> = Vec::new();
    let mut foldable = false; // 之前是否已有可被 fixup/squash 折叠的保留提交

    for item in items {
        match &item.action {
            RebaseAction::Drop => {}
            RebaseAction::Pick => {
                todo.push_str(&format!("pick {}\n", item.sha));
                foldable = true;
            }
            RebaseAction::Reword(msg) => {
                push_amend(&mut todo, &mut msgs, msg_dir, "pick", &item.sha, msg)?;
                foldable = true;
            }
            RebaseAction::Fixup => {
                if !foldable {
                    return Err(no_fold_target());
                }
                todo.push_str(&format!("fixup {}\n", item.sha));
            }
            RebaseAction::Squash(msg) => {
                if !foldable {
                    return Err(no_fold_target());
                }
                // 先无信息折叠(fixup 不唤起编辑器),再把合并结果的信息改写为 msg。
                push_amend(&mut todo, &mut msgs, msg_dir, "fixup", &item.sha, msg)?;
            }
        }
    }

    if todo.is_empty() {
        return Err(Error::Precondition("没有要保留的提交(不能丢弃全部)".into()));
    }
    Ok((todo, msgs))
}

// 发出 `<verb> <sha>` + 紧随的 `exec git commit --amend -F <信息文件>`。
fn push_amend(
    todo: &mut String,
    msgs: &mut Vec<(PathBuf, String)>,
    msg_dir: &Path,
    verb: &str,
    sha: &str,
    msg: &str,
) -> Result<(), Error> {
    if msg.trim().is_empty() {
        return Err(Error::Precondition("改写的提交信息不能为空".into()));
    }
    let f = msg_dir.join(format!("msg-{}", msgs.len()));
    todo.push_str(&format!("{verb} {sha}\n"));
    // 路径由我们掌控(git-dir 下),单引号包裹即可;信息内容在文件里,不经 shell。
    todo.push_str(&format!("exec git commit --amend -F '{}'\n", f.display()));
    msgs.push((f, msg.to_string()));
    Ok(())
}

fn no_fold_target() -> Error {
    Error::Precondition("第一个保留的提交不能是 squash/fixup(没有可折叠的上一个提交)".into())
}

fn require_ancestor(repo: &Repo, from_sha: &str) -> Result<(), Error> {
    if repo
        .git_checked(&["merge-base", "--is-ancestor", from_sha, "HEAD"])?
        .success
    {
        Ok(())
    } else {
        Err(Error::Precondition(
            "只能对当前分支历史中的提交交互式变基".into(),
        ))
    }
}

fn has_parent(repo: &Repo, sha: &str) -> Result<bool, Error> {
    Ok(repo
        .git_checked(&["rev-parse", "--verify", "--quiet", &format!("{sha}^")])?
        .success)
}

fn git_dir(repo: &Repo) -> Result<PathBuf, Error> {
    let gd = PathBuf::from(repo.git(&["rev-parse", "--git-dir"])?.trim());
    Ok(if gd.is_absolute() {
        gd
    } else {
        repo.workdir().join(gd)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(v: &[(&str, RebaseAction)]) -> Vec<RebaseItem> {
        v.iter()
            .map(|(sha, a)| RebaseItem {
                sha: (*sha).to_string(),
                action: a.clone(),
            })
            .collect()
    }

    #[test]
    fn compile_pick_drop_reword() {
        let dir = Path::new("/tmp/msgs");
        let (todo, msgs) = compile_todo(
            &items(&[
                ("aaa", RebaseAction::Pick),
                ("bbb", RebaseAction::Reword("new b".into())),
                ("ccc", RebaseAction::Drop),
            ]),
            dir,
        )
        .unwrap();
        assert_eq!(
            todo,
            "pick aaa\npick bbb\nexec git commit --amend -F '/tmp/msgs/msg-0'\n"
        );
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1, "new b");
    }

    #[test]
    fn compile_squash_folds_with_fixup_then_amend() {
        let dir = Path::new("/tmp/msgs");
        let (todo, msgs) = compile_todo(
            &items(&[
                ("aaa", RebaseAction::Pick),
                ("bbb", RebaseAction::Squash("combined".into())),
            ]),
            dir,
        )
        .unwrap();
        assert_eq!(
            todo,
            "pick aaa\nfixup bbb\nexec git commit --amend -F '/tmp/msgs/msg-0'\n"
        );
        assert_eq!(msgs[0].1, "combined");
    }

    #[test]
    fn first_fixup_without_target_errors() {
        let dir = Path::new("/tmp/msgs");
        assert!(compile_todo(&items(&[("aaa", RebaseAction::Fixup)]), dir).is_err());
        assert!(compile_todo(&items(&[("aaa", RebaseAction::Squash("x".into()))]), dir).is_err());
    }

    #[test]
    fn dropping_all_errors() {
        let dir = Path::new("/tmp/msgs");
        assert!(compile_todo(
            &items(&[("aaa", RebaseAction::Drop), ("bbb", RebaseAction::Drop)]),
            dir
        )
        .is_err());
    }

    #[test]
    fn empty_reword_message_errors() {
        let dir = Path::new("/tmp/msgs");
        assert!(compile_todo(&items(&[("aaa", RebaseAction::Reword("  ".into()))]), dir).is_err());
    }
}

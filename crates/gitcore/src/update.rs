use crate::stash::{self, PopResult, StashRef};
use crate::{CancelToken, Error, Progress, Repo};
use std::path::PathBuf;

/// 整合策略:合并或变基。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationStrategy {
    Merge,
    Rebase,
}

/// Update 的可调选项。
#[derive(Debug, Clone)]
pub struct UpdateOptions {
    pub strategy: IntegrationStrategy,
    /// 整合时忽略纯空白差异,减少伪冲突。
    pub ignore_whitespace: bool,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            strategy: IntegrationStrategy::Merge,
            ignore_whitespace: true,
        }
    }
}

/// fetch 之后、整合之前的计划:会发生什么,但还没动手。
#[derive(Debug, Clone)]
pub struct UpdatePlan {
    pub upstream: String,
    pub behind: u32,
    pub ahead: u32,
    /// 本地无领先提交 → 可直接快进。
    pub can_fast_forward: bool,
    /// 工作区脏 → 整合前会自动 stash。
    pub will_autostash: bool,
}

/// Update 状态机的终态。
#[derive(Debug)]
pub enum UpdateOutcome {
    /// 已是最新,什么都没做。
    AlreadyUpToDate,
    /// 直接快进了 N 个提交。
    FastForwarded { commits: u32 },
    /// 通过合并/变基整合了 N 个提交。
    Integrated {
        commits: u32,
        strategy: IntegrationStrategy,
    },
    /// 整合产生冲突,停下等人解决;autostash 仍保留(若有)。
    Conflicted {
        files: Vec<PathBuf>,
        autostash: Option<StashRef>,
    },
    /// 整合成功,但 autostash 贴回时冲突。
    StashRestoreConflict { files: Vec<PathBuf> },
    /// 冲突解决后整合完成。
    Resolved,
}

/// 预检 + fetch + 计算计划,不改动工作区。
pub(crate) fn plan_update(repo: &Repo, _opts: &UpdateOptions) -> Result<UpdatePlan, Error> {
    preflight(repo)?;
    let upstream = require_upstream(repo)?;
    repo.git(&["fetch", "--prune"])?;
    let (behind, ahead) = ahead_behind(repo)?;
    Ok(UpdatePlan {
        upstream,
        behind,
        ahead,
        can_fast_forward: ahead == 0,
        will_autostash: is_dirty(repo)?,
    })
}

/// 执行完整 Update 流程。
pub(crate) fn execute_update(
    repo: &Repo,
    opts: &UpdateOptions,
    cancel: &CancelToken,
) -> Result<UpdateOutcome, Error> {
    preflight(repo)?;
    require_upstream(repo)?;
    fetch(repo, cancel)?;

    let (behind, ahead) = ahead_behind(repo)?;
    if behind == 0 {
        return Ok(UpdateOutcome::AlreadyUpToDate);
    }

    // ① 脏工作区保护:先 stash。
    let label = stash::autostash_label();
    let autostash = stash::autostash_push(repo, &label)?;

    // ② 整合:本地无领先提交则快进,否则按策略合并/变基。
    let integration = if ahead == 0 {
        fast_forward(repo)
    } else {
        match opts.strategy {
            IntegrationStrategy::Merge => merge(repo, opts.ignore_whitespace),
            IntegrationStrategy::Rebase => rebase(repo, opts.ignore_whitespace),
        }
    };
    // 整合非冲突失败(如 pre-merge-commit hook 拒绝):不能让 `?` 直接抛错,
    // 否则刚 push 的 autostash 会被遗弃在 stash 里(打脸 README「失败不丢改动」)。
    // 先撤销半完成整合 + 还原脏改动,再把原始失败抛回。
    let conflicted = match integration {
        Ok(c) => c,
        Err(cause) => return recover_or_strand(repo, autostash, cause),
    };

    if conflicted {
        // rerere 可能已重放解法:把已无冲突标记的文件自动标记为已解决。
        auto_resolve_rerere(repo)?;
        let remaining = crate::conflict::conflicted_files(repo)?;
        if remaining.is_empty() {
            // 全部被 rerere 解决 → 无需人工,直接完成整合。
            return continue_update(repo, autostash);
        }
        // 还有真冲突:autostash 保留,交给上层 / UI 处理。
        return Ok(UpdateOutcome::Conflicted {
            files: remaining,
            autostash,
        });
    }

    // ① 还原脏工作区。
    if let Some(stash) = autostash {
        if let PopResult::Conflict(files) = stash::autostash_pop(repo, &stash)? {
            return Ok(UpdateOutcome::StashRestoreConflict { files });
        }
    }

    Ok(if ahead == 0 {
        UpdateOutcome::FastForwarded { commits: behind }
    } else {
        UpdateOutcome::Integrated {
            commits: behind,
            strategy: opts.strategy,
        }
    })
}

/// 冲突解决完毕后,完成整合并还原 autostash。
pub(crate) fn continue_update(
    repo: &Repo,
    autostash: Option<StashRef>,
) -> Result<UpdateOutcome, Error> {
    let remaining = crate::conflict::conflicted_files(repo)?;
    if !remaining.is_empty() {
        // 还有没解决的,原样退回。
        return Ok(UpdateOutcome::Conflicted {
            files: remaining,
            autostash,
        });
    }

    match in_progress(repo)? {
        Some(Integration::Merge) => {
            repo.git(&["-c", "rerere.enabled=true", "commit", "--no-edit"])?;
        }
        Some(Integration::Rebase) => {
            // 跳过 editor,用现成 message 继续。
            repo.git(&[
                "-c",
                "rerere.enabled=true",
                "-c",
                "core.editor=true",
                "rebase",
                "--continue",
            ])?;
        }
        None => {}
    }

    if let Some(stash) = autostash {
        if let PopResult::Conflict(files) = stash::autostash_pop(repo, &stash)? {
            return Ok(UpdateOutcome::StashRestoreConflict { files });
        }
    }
    Ok(UpdateOutcome::Resolved)
}

/// 放弃整合,回到 Update 之前的状态(含还原 autostash)。
pub(crate) fn abort_update(repo: &Repo, autostash: Option<StashRef>) -> Result<(), Error> {
    match in_progress(repo)? {
        Some(Integration::Merge) => {
            repo.git(&["merge", "--abort"])?;
        }
        Some(Integration::Rebase) => {
            repo.git(&["rebase", "--abort"])?;
        }
        None => {}
    }
    // 整合已撤销,贴回脏改动通常是干净的。
    if let Some(stash) = autostash {
        stash::autostash_pop(repo, &stash)?;
    }
    Ok(())
}

/// 未完成整合的恢复信息:待解决冲突文件 + 扫回的 autostash。
pub type PendingConflicts = (Vec<PathBuf>, Option<StashRef>);

/// 检测未完成的整合(中断/崩溃后):有进行中的 merge/rebase + 冲突文件时,
/// 返回冲突文件 + 扫回的 autostash;否则 None。
pub(crate) fn resume(repo: &Repo) -> Result<Option<PendingConflicts>, Error> {
    if in_progress(repo)?.is_none() {
        return Ok(None);
    }
    let files = crate::conflict::conflicted_files(repo)?;
    if files.is_empty() {
        return Ok(None);
    }
    Ok(Some((files, stash::find_autostash(repo)?)))
}

// ---- 内部步骤 ----

/// 整合非冲突失败时的收尾:撤销半完成的整合 + 还原 autostash,然后把原始失败抛回。
///
/// hook 在「已合并待提交」处拒绝会留下 MERGE_HEAD,必须先 abort,否则工作区/索引
/// 被合并结果占着,脏改动贴不回来。还原失败(abort 出错 / 贴回冲突)时退而求其次:
/// 点名 stash 的位置用 Precondition 报出来,让用户能手工取回,绝不无声丢改动。
fn recover_or_strand(
    repo: &Repo,
    autostash: Option<StashRef>,
    cause: Error,
) -> Result<UpdateOutcome, Error> {
    match in_progress(repo)? {
        Some(Integration::Merge) => {
            repo.git(&["merge", "--abort"])?;
        }
        Some(Integration::Rebase) => {
            repo.git(&["rebase", "--abort"])?;
        }
        None => {}
    }

    if let Some(stash) = autostash {
        match stash::autostash_pop(repo, &stash) {
            Ok(PopResult::Clean) => {}
            Ok(PopResult::Conflict(_)) | Err(_) => {
                return Err(Error::Precondition(format!(
                    "整合失败且改动未能自动还原,已保留在 stash「{}」,请手动 `git stash pop` 取回",
                    stash.label
                )));
            }
        }
    }

    Err(cause)
}

// 可取消的 fetch:取消 → Err(Cancelled);失败 → Err(Git)。execute_update 内部用,
// 进度不上报(UI 进度走 fetch_streaming);带 --progress 是为给取消轮询留时机点。
fn fetch(repo: &Repo, cancel: &CancelToken) -> Result<(), Error> {
    let mut ignore = |_p: Progress| {};
    let out = repo.git_streaming(&["fetch", "--prune", "--progress"], &mut ignore, cancel)?;
    if out.success {
        Ok(())
    } else {
        Err(Error::Git {
            args: vec!["fetch".into(), "--prune".into(), "--progress".into()],
            code: out.code,
            stderr: out.stderr,
        })
    }
}

// rerere 重放后:把已无冲突标记的文件自动 add(视为已解决)。
fn auto_resolve_rerere(repo: &Repo) -> Result<(), Error> {
    for file in crate::conflict::conflicted_files(repo)? {
        let content = std::fs::read_to_string(repo.workdir().join(&file))?;
        if !content.lines().any(|l| l.starts_with("<<<<<<<")) {
            let p = file
                .to_str()
                .ok_or_else(|| Error::Parse("路径含非法字符".into()))?;
            repo.git(&["add", "--", p])?;
        }
    }
    Ok(())
}

enum Integration {
    Merge,
    Rebase,
}

fn git_dir(repo: &Repo) -> Result<PathBuf, Error> {
    let gd = PathBuf::from(repo.git(&["rev-parse", "--git-dir"])?.trim());
    Ok(if gd.is_absolute() {
        gd
    } else {
        repo.workdir().join(gd)
    })
}

// 是否有正在进行的 merge / rebase。
fn in_progress(repo: &Repo) -> Result<Option<Integration>, Error> {
    let base = git_dir(repo)?;
    if base.join("MERGE_HEAD").exists() {
        Ok(Some(Integration::Merge))
    } else if base.join("rebase-merge").exists() || base.join("rebase-apply").exists() {
        Ok(Some(Integration::Rebase))
    } else {
        Ok(None)
    }
}

// 预检:不能有正在进行的 merge / rebase(防止重入)。
fn preflight(repo: &Repo) -> Result<(), Error> {
    if let Some(kind) = in_progress(repo)? {
        let what = match kind {
            Integration::Merge => "合并",
            Integration::Rebase => "变基",
        };
        return Err(Error::Precondition(format!(
            "已有{what}进行中,请先解决或 abort"
        )));
    }
    Ok(())
}

fn require_upstream(repo: &Repo) -> Result<String, Error> {
    let up = repo.git_checked(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])?;
    if up.success {
        Ok(up.stdout.trim().to_string())
    } else {
        Err(Error::Precondition("当前分支没有设置 upstream".into()))
    }
}

fn ahead_behind(repo: &Repo) -> Result<(u32, u32), Error> {
    let counts = repo.git(&["rev-list", "--left-right", "--count", "@{u}...HEAD"])?;
    let mut it = counts.split_whitespace();
    match (
        it.next().and_then(|x| x.parse().ok()),
        it.next().and_then(|x| x.parse().ok()),
    ) {
        (Some(b), Some(a)) => Ok((b, a)),
        _ => Err(Error::Parse(format!(
            "rev-list --count 输出异常: {counts:?}"
        ))),
    }
}

fn is_dirty(repo: &Repo) -> Result<bool, Error> {
    Ok(!repo.git(&["status", "--porcelain"])?.trim().is_empty())
}

// 返回 Ok(true)=产生冲突,Ok(false)=干净完成。
fn fast_forward(repo: &Repo) -> Result<bool, Error> {
    let out = repo.git_checked(&["merge", "--ff-only", "@{u}"])?;
    if out.success {
        Ok(false)
    } else {
        // ff-only 不会冲突:要么成功,要么因不能快进而失败。
        Err(Error::Git {
            args: vec!["merge".into(), "--ff-only".into(), "@{u}".into()],
            code: out.code,
            stderr: out.stderr,
        })
    }
}

fn merge(repo: &Repo, ignore_whitespace: bool) -> Result<bool, Error> {
    // -c merge.conflictStyle=zdiff3:让冲突标记带上 base 段,供魔法棒判断。
    let mut args = vec![
        "-c",
        "rerere.enabled=true",
        "-c",
        "merge.conflictStyle=zdiff3",
        "merge",
        "--no-edit",
    ];
    if ignore_whitespace {
        args.push("-Xignore-space-change"); // ④ 整合阶段就消解空白伪冲突
    }
    args.push("@{u}");
    let out = repo.git_checked(&args)?;
    finish_integration(repo, out, &args)
}

fn rebase(repo: &Repo, ignore_whitespace: bool) -> Result<bool, Error> {
    let mut args = vec![
        "-c",
        "rerere.enabled=true",
        "-c",
        "merge.conflictStyle=zdiff3",
        "rebase",
    ];
    if ignore_whitespace {
        args.push("-Xignore-space-change"); // 与 merge 一致:消解空白伪冲突
    }
    args.push("@{u}");
    let out = repo.git_checked(&args)?;
    finish_integration(repo, out, &args)
}

// 整合命令收尾:成功=false,有冲突文件=true,否则视为真失败=Err。
fn finish_integration(repo: &Repo, out: crate::git::Output, args: &[&str]) -> Result<bool, Error> {
    if out.success {
        Ok(false)
    } else if !crate::conflict::conflicted_files(repo)?.is_empty() {
        Ok(true)
    } else {
        Err(Error::Git {
            args: args.iter().map(|s| s.to_string()).collect(),
            code: out.code,
            stderr: out.stderr,
        })
    }
}

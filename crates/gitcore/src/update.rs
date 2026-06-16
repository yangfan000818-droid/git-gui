use crate::stash::{self, PopResult, StashRef};
use crate::{Error, Repo};
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
pub(crate) fn execute_update(repo: &Repo, opts: &UpdateOptions) -> Result<UpdateOutcome, Error> {
    preflight(repo)?;
    require_upstream(repo)?;
    repo.git(&["fetch", "--prune"])?;

    let (behind, ahead) = ahead_behind(repo)?;
    if behind == 0 {
        return Ok(UpdateOutcome::AlreadyUpToDate);
    }

    // ① 脏工作区保护:先 stash。
    let label = format!("gitcore-autostash:{}", std::process::id());
    let autostash = stash::autostash_push(repo, &label)?;

    // ② 整合:本地无领先提交则快进,否则按策略合并/变基。
    let conflicted = if ahead == 0 {
        fast_forward(repo)?
    } else {
        match opts.strategy {
            IntegrationStrategy::Merge => merge(repo, opts.ignore_whitespace)?,
            IntegrationStrategy::Rebase => rebase(repo)?,
        }
    };

    if conflicted {
        // ③ 停在冲突:autostash 保留,交给上层 / UI 处理。
        return Ok(UpdateOutcome::Conflicted {
            files: crate::conflict::conflicted_files(repo)?,
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

// ---- 内部步骤 ----

// 预检:不能有正在进行的 merge / rebase(防止重入)。
fn preflight(repo: &Repo) -> Result<(), Error> {
    let git_dir = PathBuf::from(repo.git(&["rev-parse", "--git-dir"])?.trim());
    let base = if git_dir.is_absolute() {
        git_dir
    } else {
        repo.workdir().join(git_dir)
    };
    if base.join("MERGE_HEAD").exists() {
        return Err(Error::Precondition(
            "已有合并进行中,请先解决或 abort".into(),
        ));
    }
    if base.join("rebase-merge").exists() || base.join("rebase-apply").exists() {
        return Err(Error::Precondition(
            "已有变基进行中,请先解决或 abort".into(),
        ));
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
    let mut args = vec!["merge", "--no-edit"];
    if ignore_whitespace {
        args.push("-Xignore-space-change"); // ④ 整合阶段就消解空白伪冲突
    }
    args.push("@{u}");
    let out = repo.git_checked(&args)?;
    finish_integration(repo, out, &args)
}

fn rebase(repo: &Repo) -> Result<bool, Error> {
    let args = vec!["rebase", "@{u}"];
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

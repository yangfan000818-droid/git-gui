//! gitcore — UI 无关的 git 编排核心库。
//!
//! 所有前端(TUI / 未来 GUI)都依赖本 crate,不直接调用 git。
//! 设计:spawn git CLI + plumbing 命令拿可解析输出;每个写操作尽量可回退。

mod conflict;
mod error;
mod git;
mod stash;
mod status;
mod update;

use std::path::{Path, PathBuf};

pub use conflict::{conflicted_files, three_versions, ThreeVersions};
pub use error::Error;
pub use stash::{PopResult, StashRef};
pub use status::RepoStatus;
pub use update::{IntegrationStrategy, UpdateOptions, UpdateOutcome, UpdatePlan};

/// 一个 git 工作区的句柄;所有操作相对它执行。
#[derive(Debug, Clone)]
pub struct Repo {
    workdir: PathBuf,
}

impl Repo {
    /// 绑定到一个已存在的 git 工作区。
    pub fn open(workdir: impl Into<PathBuf>) -> Result<Self, Error> {
        let repo = Repo {
            workdir: workdir.into(),
        };
        repo.git(&["rev-parse", "--git-dir"])?; // 验证确实是 git 仓库
        Ok(repo)
    }

    /// 工作区根目录。
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// 读取当前状态(分支 / upstream / ahead-behind / dirty / 冲突)。
    pub fn status(&self) -> Result<RepoStatus, Error> {
        status::status(self)
    }

    /// 预检 + fetch + 计算将发生什么,不改动工作区。
    pub fn plan_update(&self, opts: &UpdateOptions) -> Result<UpdatePlan, Error> {
        update::plan_update(self, opts)
    }

    /// 执行完整 Update 流程(autostash → 整合 → restore)。
    pub fn execute_update(&self, opts: &UpdateOptions) -> Result<UpdateOutcome, Error> {
        update::execute_update(self, opts)
    }

    // 跑一个必须成功的 git 子命令,非零退出 → Err。
    pub(crate) fn git(&self, args: &[&str]) -> Result<String, Error> {
        git::run(&self.workdir, args)
    }

    // 跑一个 git 子命令,返回原始结果,不把非零当错误(用于可能冲突的整合)。
    pub(crate) fn git_checked(&self, args: &[&str]) -> Result<git::Output, Error> {
        git::run_checked(&self.workdir, args)
    }
}

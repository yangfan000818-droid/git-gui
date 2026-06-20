//! gitcore — UI 无关的 git 编排核心库。
//!
//! 所有前端(TUI / 未来 GUI)都依赖本 crate,不直接调用 git。
//! 设计:spawn git CLI + plumbing 命令拿可解析输出;每个写操作尽量可回退。

mod branch;
mod commit;
mod config;
mod conflict;
mod diff;
mod diff3;
mod error;
mod git;
mod hunk;
mod log;
mod push;
mod resolve;
mod stage;
mod stash;
mod status;
mod submodule;
mod topology;
mod update;

use std::path::{Path, PathBuf};

pub use branch::BranchInfo;
pub use commit::CommitOptions;
pub use config::{parse_repos_config, RepoConfig};
pub use conflict::{conflicted_files, three_versions, ThreeVersions};
pub use diff::DiffOptions;
pub use error::Error;
pub use git::{CancelToken, Progress};
pub use hunk::{DiffLine, FileDiff, Hunk, LineKind};
pub use log::{GraphRow, LogEntry, LogOptions};
pub use push::PushOutcome;
pub use resolve::{
    parse_conflicts, rebuild, refine_segments, Choice, ConflictHunk, Resolution, Segment,
};
pub use stash::{PopResult, StashEntry, StashRef};
pub use status::{FileState, FileStatus, RepoStatus};
pub use submodule::{Submodule, SubmoduleStatus};
pub use topology::{GraphCommit, GraphEdge};
pub use update::{IntegrationStrategy, PendingConflicts, UpdateOptions, UpdateOutcome, UpdatePlan};

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

    /// 同 [`Repo::plan_update`],但 fetch 阶段支持取消(cancel 置位后中止)和进度回调。
    pub fn plan_update_streaming(
        &self,
        opts: &UpdateOptions,
        on_progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<UpdatePlan, Error> {
        update::plan_update_streaming(self, opts, on_progress, cancel)
    }

    /// 执行完整 Update 流程(autostash → 整合 → restore)。`cancel` 置位可在 fetch
    /// 阶段中止并返回 `Error::Cancelled`(此时尚未 autostash,工作区不受影响)。
    pub fn execute_update(
        &self,
        opts: &UpdateOptions,
        cancel: &CancelToken,
    ) -> Result<UpdateOutcome, Error> {
        update::execute_update(self, opts, cancel)
    }

    /// 同 [`Repo::execute_update`],但 fetch 阶段进度经 `on_progress` 上报,供 UI 进度条用。
    pub fn execute_update_streaming(
        &self,
        opts: &UpdateOptions,
        on_progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<UpdateOutcome, Error> {
        update::execute_update_streaming(self, opts, on_progress, cancel)
    }

    /// 读取一个冲突文件,解析成片段序列(含三版本)。
    pub fn read_conflict(&self, path: &Path) -> Result<Vec<Segment>, Error> {
        resolve::read_conflict(self, path)
    }

    /// 写回某文件的解决结果并标记为已解决(git add)。
    pub fn resolve_file(&self, path: &Path, text: &str) -> Result<(), Error> {
        resolve::write_resolution(self, path, text)
    }

    /// 冲突解决后完成整合,并还原 autostash。
    pub fn continue_update(
        &self,
        autostash: Option<StashRef>,
        recurse_submodules: bool,
    ) -> Result<UpdateOutcome, Error> {
        update::continue_update(self, autostash, recurse_submodules)
    }

    /// 放弃整合,回到 Update 之前的状态(含还原 autostash)。
    pub fn abort_update(&self, autostash: Option<StashRef>) -> Result<(), Error> {
        update::abort_update(self, autostash)
    }

    /// 检测未完成的整合(中断/崩溃后):返回待解决冲突文件 + 扫回的 autostash。
    pub fn resume_conflicts(&self) -> Result<Option<PendingConflicts>, Error> {
        update::resume(self)
    }

    /// 暂存指定文件。
    pub fn stage(&self, paths: &[&Path]) -> Result<(), Error> {
        stage::stage_files(self, paths)
    }

    /// 取消暂存指定文件。
    pub fn unstage(&self, paths: &[&Path]) -> Result<(), Error> {
        stage::unstage_files(self, paths)
    }

    /// 回滚指定文件的改动(stash 兜底,可在 Stash 视图 pop 找回)。
    pub fn discard(&self, paths: &[&Path]) -> Result<(), Error> {
        stage::discard_files(self, paths)
    }

    /// 暂存所有改动和未跟踪文件。
    pub fn stage_all(&self) -> Result<(), Error> {
        stage::stage_all(self)
    }

    /// 创建提交,返回新提交的 SHA(前 8 位)。
    pub fn commit(&self, opts: &CommitOptions) -> Result<String, Error> {
        commit::commit(self, opts)
    }

    /// 推送当前分支到 upstream。
    pub fn push(&self) -> Result<PushOutcome, Error> {
        push::push(self)
    }

    /// 流式 fetch:进度经 `on_progress` 上报,`cancel` 置位则中止并返回 `Error::Cancelled`。
    /// 供需要进度条 / 可取消的长操作用;内部 update 流程仍走即时 fetch。
    pub fn fetch_streaming(
        &self,
        on_progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<(), Error> {
        let out = self.git_streaming(&["fetch", "--prune", "--progress"], on_progress, cancel)?;
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

    /// 流式推送:进度经 `on_progress` 上报,`cancel` 置位则中止。判定同 [`Repo::push`]。
    pub fn push_streaming(
        &self,
        on_progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<PushOutcome, Error> {
        push::push_streaming(self, on_progress, cancel)
    }

    /// 获取提交历史。
    pub fn log(&self, opts: &LogOptions) -> Result<Vec<LogEntry>, Error> {
        log::log(self, opts)
    }

    /// 获取带分支拓扑图的提交历史(每行 = 图形前缀 + 可选 commit)。
    pub fn log_graph(&self, opts: &LogOptions) -> Result<Vec<GraphRow>, Error> {
        log::log_graph(self, opts)
    }

    /// 检查 git 是否在 PATH 中可用。不可用返回友好错误供 UI 展示。
    pub fn check_git() -> Result<(), Error> {
        git::check_available()
    }

    /// 获取结构化拓扑图:每个 commit 的 lane 分配 + lane 间连线,供前端 SVG 绘图。
    pub fn log_topology(&self, opts: &LogOptions) -> Result<Vec<GraphCommit>, Error> {
        topology::log_topology(self, opts)
    }

    /// 获取 diff 输出。
    pub fn diff(&self, opts: &DiffOptions) -> Result<String, Error> {
        diff::diff(self, opts)
    }

    /// 解析未暂存改动(工作区 vs 暂存区)为结构化 diff(文件 → hunk → 行)。
    pub fn unstaged_diff(&self) -> Result<Vec<FileDiff>, Error> {
        let text = self.git(&[
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicprefix=false",
            "diff",
            "--no-color",
        ])?;
        let mut files = hunk::parse(&text);
        // git diff 不含未跟踪文件,单独列出并补成"全新增"的 FileDiff。
        let untracked = self.git(&[
            "-c",
            "core.quotepath=false",
            "ls-files",
            "--others",
            "--exclude-standard",
        ])?;
        for path in untracked.lines() {
            if let Some(fd) = hunk::untracked_file(self, path) {
                files.push(fd);
            }
        }
        Ok(files)
    }

    /// 解析已暂存改动(暂存区 vs HEAD)为结构化 diff。
    pub fn staged_diff(&self) -> Result<Vec<FileDiff>, Error> {
        let text = self.git(&[
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicprefix=false",
            "diff",
            "--cached",
            "--no-color",
        ])?;
        Ok(hunk::parse(&text))
    }

    /// 暂存某文件的某个 hunk(`file`/`hunk` 取自 [`Repo::unstaged_diff`])。
    pub fn stage_hunk(&self, file: &FileDiff, hunk: &Hunk) -> Result<(), Error> {
        hunk::stage_hunk(self, file, hunk)
    }

    /// 取消暂存某文件的某个 hunk(`file`/`hunk` 取自 [`Repo::staged_diff`])。
    pub fn unstage_hunk(&self, file: &FileDiff, hunk: &Hunk) -> Result<(), Error> {
        hunk::unstage_hunk(self, file, hunk)
    }

    /// 暂存某 hunk 中选中的行(`selected` 为 `hunk.lines` 下标;取自 [`Repo::unstaged_diff`])。
    pub fn stage_lines(
        &self,
        file: &FileDiff,
        hunk: &Hunk,
        selected: &[usize],
    ) -> Result<(), Error> {
        hunk::stage_lines(self, file, hunk, selected)
    }

    /// 取消暂存某 hunk 中选中的行(`selected` 为 `hunk.lines` 下标;取自 [`Repo::staged_diff`])。
    pub fn unstage_lines(
        &self,
        file: &FileDiff,
        hunk: &Hunk,
        selected: &[usize],
    ) -> Result<(), Error> {
        hunk::unstage_lines(self, file, hunk, selected)
    }

    /// 查看指定提交的完整内容(message + diff)。
    pub fn show_commit(&self, sha: &str) -> Result<String, Error> {
        diff::show_commit(self, sha)
    }

    /// 解析某个 commit 的改动为结构化 diff(按文件 → hunk → 行)。
    pub fn commit_files(&self, sha: &str) -> Result<Vec<FileDiff>, Error> {
        hunk::commit_files(self, sha)
    }

    /// 获取指定提交的完整消息(多行)。
    pub fn commit_message(&self, sha: &str) -> Result<String, Error> {
        diff::commit_message(self, sha)
    }

    /// 列出所有 stash。
    pub fn stashes(&self) -> Result<Vec<StashEntry>, Error> {
        stash::list_stashes(self)
    }

    /// 创建新的 stash。
    pub fn stash_push(&self, message: Option<&str>) -> Result<(), Error> {
        stash::stash_push(self, message)
    }

    /// 应用指定 stash。
    pub fn stash_apply(&self, reff: &str) -> Result<(), Error> {
        stash::stash_apply(self, reff)
    }

    /// 弹出指定 stash。
    pub fn stash_pop(&self, reff: &str) -> Result<PopResult, Error> {
        stash::stash_pop(self, reff)
    }

    /// 丢弃指定 stash。
    pub fn stash_drop(&self, reff: &str) -> Result<(), Error> {
        stash::stash_drop(self, reff)
    }

    /// 列出所有子仓库。
    pub fn submodules(&self) -> Result<Vec<Submodule>, Error> {
        submodule::list_submodules(self)
    }

    /// 列出所有本地分支。
    pub fn branches(&self) -> Result<Vec<BranchInfo>, Error> {
        branch::list_branches(self)
    }

    /// 创建新分支。
    pub fn create_branch(&self, name: &str) -> Result<(), Error> {
        branch::create_branch(self, name)
    }

    /// 切换到指定分支。
    pub fn switch_branch(&self, name: &str) -> Result<(), Error> {
        branch::switch_branch(self, name)
    }

    /// 删除分支(安全模式)。
    pub fn delete_branch(&self, name: &str) -> Result<(), Error> {
        branch::delete_branch(self, name)
    }

    // 跑一个必须成功的 git 子命令,非零退出 → Err。
    pub(crate) fn git(&self, args: &[&str]) -> Result<String, Error> {
        git::run(&self.workdir, args)
    }

    // 跑一个 git 子命令,返回原始结果,不把非零当错误(用于可能冲突的整合)。
    pub(crate) fn git_checked(&self, args: &[&str]) -> Result<git::Output, Error> {
        git::run_checked(&self.workdir, args)
    }

    // 跑一个 git 子命令并把 patch 等内容写入其 stdin(供 git apply 用)。
    pub(crate) fn git_with_stdin(&self, args: &[&str], input: &str) -> Result<String, Error> {
        git::run_with_stdin(&self.workdir, args, input)
    }

    // 流式跑一个 git 子命令(进度回调 + 可取消),供 fetch / push 等长操作复用。
    pub(crate) fn git_streaming(
        &self,
        args: &[&str],
        on_progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<git::Output, Error> {
        git::run_streaming(&self.workdir, args, on_progress, cancel)
    }
}

//! gitcore — UI 无关的 git 编排核心库。
//!
//! 所有前端都依赖本 crate,不直接调用 git。
//! 设计:spawn git CLI + plumbing 命令拿可解析输出;每个写操作尽量可回退。

mod blame;
mod branch;
mod clean;
mod clone;
mod commit;
mod config;
mod conflict;
mod diff;
mod diff3;
mod error;
mod git;
mod hunk;
mod log;
mod precommit;
mod push;
mod rebase;
mod reflog;
mod remote;
mod reset;
mod resolve;
mod stage;
mod stash;
mod status;
mod submodule;
mod tags;
mod topology;
mod update;

use std::path::{Path, PathBuf};

pub use blame::BlameLine;
pub use branch::{BranchInfo, SwitchOutcome};
pub use clone::clone_streaming;
pub use commit::CommitOptions;
pub use config::{parse_repos_config, RepoConfig};
pub use conflict::{conflicted_files, three_versions, ConflictFile, ConflictKind, ThreeVersions};
pub use diff::DiffOptions;
pub use diff3::{MergeRegion, RegionKind};
pub use error::Error;
pub use git::{CancelToken, Progress};
pub use hunk::{DiffLine, FileDiff, Hunk, LineKind};
pub use log::{BranchComparison, GraphRow, LogEntry, LogOptions, MergedLogEntry};
pub use precommit::{PrecommitReport, PrecommitWarning, WarningKind};
pub use push::{PushOutcome, PushPreview};
pub use rebase::{RebaseAction, RebaseItem};
pub use reflog::ReflogEntry;
pub use remote::RemoteInfo;
pub use reset::ResetMode;
pub use resolve::{
    parse_conflicts, rebuild, refine_segments, Choice, ConflictHunk, Resolution, Segment, Side,
};
pub use stash::{PopResult, StashEntry, StashRef};
pub use status::{FileState, FileStatus, RepoStatus};
pub use submodule::{Submodule, SubmoduleStatus};
pub use tags::TagInfo;
pub use topology::{GraphCommit, GraphEdge, MergedGraphCommit, MergedGraphLog, RootMeta};
pub use update::{
    ConflictState, IntegrationKind, IntegrationStrategy, PendingConflicts, SubmoduleUpdate,
    UpdateOptions, UpdateOutcome,
};

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

    /// 报告当前冲突态(进行中整合类型 + 分类后的冲突文件 + autostash)。
    /// 总是返回(无冲突则 files 空),供主界面在 refresh 时探测与重入。
    pub fn conflict_state(&self) -> Result<ConflictState, Error> {
        update::conflict_state(self)
    }

    /// 保留工作区现有版本并标记已解决(modify/delete 保留改动、delete/modify 保留对方)。
    pub fn resolve_keep(&self, path: &Path) -> Result<(), Error> {
        resolve::resolve_keep(self, path)
    }

    /// 接受删除:从工作区与索引移除该文件并标记已解决。
    pub fn resolve_remove(&self, path: &Path) -> Result<(), Error> {
        resolve::resolve_remove(self, path)
    }

    /// 取某一侧整份内容(checkout --ours/--theirs)并标记已解决(二进制 / add-add 选边)。
    pub fn resolve_take_side(&self, path: &Path, side: Side) -> Result<(), Error> {
        resolve::resolve_take_side(self, path, side)
    }

    /// 整文件对齐三路 diff(供 WebStorm 式三栏合并编辑器渲染)。
    pub fn merge_file_regions(&self, path: &Path) -> Result<Vec<MergeRegion>, Error> {
        let v = conflict::three_versions(self, path)?;
        Ok(diff3::merge_regions(
            v.ours.as_deref().unwrap_or(""),
            v.base.as_deref().unwrap_or(""),
            v.theirs.as_deref().unwrap_or(""),
        ))
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

    /// 完成 stash 还原冲突的解决:校验无残留冲突后丢弃已贴回的 autostash,改动留在工作区。
    pub fn finish_stash_restore(&self, autostash: Option<StashRef>) -> Result<(), Error> {
        update::finish_stash_restore(self, autostash)
    }

    /// 放弃 stash 还原:reset --hard 回到整合后状态,保留 stash(原始改动可重试)。
    pub fn abort_stash_restore(&self) -> Result<(), Error> {
        update::abort_stash_restore(self)
    }

    /// 检测未完成的整合(中断/崩溃后):返回待解决冲突文件 + 扫回的 autostash。
    pub fn resume_conflicts(&self) -> Result<Option<PendingConflicts>, Error> {
        update::resume(self)
    }

    /// Cherry-pick 一个提交到当前分支。
    pub fn cherry_pick(&self, sha: &str) -> Result<UpdateOutcome, Error> {
        update::cherry_pick(self, sha)
    }

    /// Revert 一个提交(生成反向提交)。
    pub fn revert(&self, sha: &str) -> Result<UpdateOutcome, Error> {
        update::revert(self, sha)
    }

    /// 把当前分支重置到指定提交(soft/mixed/hard)。
    pub fn reset(&self, sha: &str, mode: ResetMode) -> Result<(), Error> {
        reset::reset(self, sha, mode)
    }

    /// 列出所有远程仓库。
    pub fn list_remotes(&self) -> Result<Vec<RemoteInfo>, Error> {
        remote::list_remotes(self)
    }

    /// 添加一个远程仓库。
    pub fn add_remote(&self, name: &str, url: &str) -> Result<(), Error> {
        remote::add_remote(self, name, url)
    }

    /// 删除一个远程仓库。
    pub fn remove_remote(&self, name: &str) -> Result<(), Error> {
        remote::remove_remote(self, name)
    }

    /// 修改一个远程仓库的 URL。
    pub fn set_remote_url(&self, name: &str, url: &str) -> Result<(), Error> {
        remote::set_remote_url(self, name, url)
    }

    /// 预览 `git clean` 将删除的未跟踪文件(dry-run)。
    pub fn clean_preview(&self, directories: bool) -> Result<Vec<std::path::PathBuf>, Error> {
        clean::clean_preview(self, directories)
    }

    /// 强制清理未跟踪文件,返回删除数。
    pub fn clean_force(&self, directories: bool) -> Result<usize, Error> {
        clean::clean_force(self, directories)
    }

    /// 取 HEAD reflog(最近 max_count 条),供查看/恢复历史状态。
    pub fn reflog(&self, max_count: usize) -> Result<Vec<ReflogEntry>, Error> {
        reflog::reflog(self, max_count)
    }

    /// 列出所有 tag。
    pub fn tags(&self) -> Result<Vec<TagInfo>, Error> {
        tags::list_tags(self)
    }

    /// 创建 tag。target 为 None 时打在 HEAD;message 为 Some 时为注释标签。
    pub fn create_tag(
        &self,
        name: &str,
        target: Option<&str>,
        message: Option<&str>,
    ) -> Result<(), Error> {
        tags::create_tag(self, name, target, message)
    }

    /// 删除 tag。
    pub fn delete_tag(&self, name: &str) -> Result<(), Error> {
        tags::delete_tag(self, name)
    }

    /// 把一个 tag 推送到默认远程。
    pub fn push_tag(&self, name: &str) -> Result<(), Error> {
        tags::push_tag(self, name)
    }

    /// 把所有本地 tag 推送到默认远程(一键全推,无需逐条)。
    pub fn push_all_tags(&self) -> Result<(), Error> {
        tags::push_all_tags(self)
    }

    /// 把另一个分支合并到当前分支(对标 WebStorm "Merge into current")。
    pub fn merge_branch(&self, branch: &str, opts: &UpdateOptions) -> Result<UpdateOutcome, Error> {
        update::merge_branch(self, branch, opts)
    }

    /// 把当前分支变基到另一个分支上(对标 WebStorm "Rebase current onto")。
    pub fn rebase_branch(
        &self,
        branch: &str,
        opts: &UpdateOptions,
    ) -> Result<UpdateOutcome, Error> {
        update::rebase_branch(self, branch, opts)
    }

    /// 列出 `from_sha..HEAD`(含 from_sha)的提交(oldest-first),供交互式变基编辑。
    pub fn rebase_plan(&self, from_sha: &str) -> Result<Vec<LogEntry>, Error> {
        rebase::rebase_plan(self, from_sha)
    }

    /// 从 `from_sha` 起按给定操作交互式变基(对标 WebStorm "Interactively Rebase from Here")。
    pub fn rebase_interactive(
        &self,
        from_sha: &str,
        items: &[RebaseItem],
    ) -> Result<UpdateOutcome, Error> {
        rebase::rebase_interactive(self, from_sha, items)
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

    /// 取消暂存所有改动。
    pub fn unstage_all(&self) -> Result<(), Error> {
        stage::unstage_all(self)
    }

    /// 创建提交,返回新提交的 SHA(前 8 位)。
    pub fn commit(&self, opts: &CommitOptions) -> Result<String, Error> {
        commit::commit(self, opts)
    }

    /// 只提交指定路径(供 Changelist 按组提交),忽略其它已暂存改动。返回新提交 SHA(前 8 位)。
    pub fn commit_paths(
        &self,
        message: &str,
        paths: &[String],
        no_verify: bool,
    ) -> Result<String, Error> {
        commit::commit_paths(self, message, paths, no_verify)
    }

    /// 推送当前分支到 upstream。
    pub fn push(&self) -> Result<PushOutcome, Error> {
        push::push(self)
    }

    /// Push 对话框预览:目标 upstream + 待推送提交(`@{u}..HEAD`)。
    pub fn push_preview(&self) -> Result<PushPreview, Error> {
        push::push_preview(self)
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
    /// `force_with_lease` 为真时用 `--force-with-lease`(安全强制推送)。
    pub fn push_streaming(
        &self,
        force_with_lease: bool,
        on_progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<PushOutcome, Error> {
        push::push_streaming(self, force_with_lease, on_progress, cancel)
    }

    /// 获取提交历史。
    pub fn log(&self, opts: &LogOptions) -> Result<Vec<LogEntry>, Error> {
        log::log(self, opts)
    }

    /// 获取带分支拓扑图的提交历史(每行 = 图形前缀 + 可选 commit)。
    pub fn log_graph(&self, opts: &LogOptions) -> Result<Vec<GraphRow>, Error> {
        log::log_graph(self, opts)
    }

    /// 合并主仓与各子仓的提交历史(按时间降序,每条带仓库标识)。
    pub fn log_merged(&self, opts: &LogOptions) -> Result<Vec<MergedLogEntry>, Error> {
        log::log_merged(self, opts)
    }

    /// 获取单个文件的提交历史(追踪重命名)。
    pub fn file_history(
        &self,
        file_path: &Path,
        opts: &LogOptions,
    ) -> Result<Vec<LogEntry>, Error> {
        log::file_history(self, file_path, opts)
    }

    /// 比较选定分支(或任意 ref)与当前 HEAD 的双向独有提交(Compare with Current)。
    pub fn compare_commits(&self, other: &str) -> Result<BranchComparison, Error> {
        log::compare_commits(self, other)
    }

    /// 列出子仓库在 `old..new` 区间的提交(父仓 commit 详情展开子模块变化)。
    pub fn submodule_commits(
        &self,
        sub_path: &Path,
        old: &str,
        new: &str,
    ) -> Result<Vec<LogEntry>, Error> {
        log::submodule_commits(self, sub_path, old, new)
    }

    /// 检查 git 是否在 PATH 中可用。不可用返回友好错误供 UI 展示。
    pub fn check_git() -> Result<(), Error> {
        git::check_available()
    }

    /// 获取结构化拓扑图:每个 commit 的 lane 分配 + lane 间连线,供前端 SVG 绘图。
    pub fn log_topology(&self, opts: &LogOptions) -> Result<Vec<GraphCommit>, Error> {
        topology::log_topology(self, opts)
    }

    /// 获取多 root 合并拓扑图(主仓 + 各已初始化子仓,按 root 分段画 lane)。
    /// 始终拉取全部已初始化仓;焦点(灰显)由前端处理。
    pub fn log_multi_root_topology(&self, opts: &LogOptions) -> Result<MergedGraphLog, Error> {
        log::log_multi_root_topology(self, opts)
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
        let text = self.cached_diff_text()?;
        Ok(hunk::parse(&text))
    }

    /// 已暂存改动(暂存区 vs HEAD)的原始 diff 文本,供 AI 生成提交信息等用途。
    pub fn staged_diff_text(&self) -> Result<String, Error> {
        self.cached_diff_text()
    }

    /// 提交前检查:扫描暂存内容,返回潜在问题(敏感信息 / 冲突标记 / 大文件 /
    /// 调试残留 / TODO),供前端在提交前提示。复用 `staged_diff` 的结构化解析。
    pub fn precommit_check(&self) -> Result<PrecommitReport, Error> {
        crate::precommit::check(self)
    }

    /// `git diff --cached` 原始文本(staged_diff 与 staged_diff_text 共用)。
    fn cached_diff_text(&self) -> Result<String, Error> {
        self.git(&[
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicprefix=false",
            "diff",
            "--cached",
            "--no-color",
        ])
    }

    /// 单个文件的未暂存 diff(懒加载用):普通改动走 git diff,未跟踪文件补成"全新增"。
    /// 文件无未暂存改动时返回 None。
    pub fn file_unstaged_diff(&self, file: &Path) -> Result<Option<FileDiff>, Error> {
        let p = file
            .to_str()
            .ok_or_else(|| Error::Parse("文件路径含非 UTF-8 字符".into()))?;
        let text = self.git(&[
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicprefix=false",
            "diff",
            "--no-color",
            "--",
            p,
        ])?;
        if let Some(fd) = hunk::parse(&text).into_iter().next() {
            return Ok(Some(fd));
        }
        // git diff 不含未跟踪文件,补成"全新增"。
        Ok(hunk::untracked_file(self, p))
    }

    /// 单个文件的已暂存 diff(懒加载用)。文件无已暂存改动时返回 None。
    pub fn file_staged_diff(&self, file: &Path) -> Result<Option<FileDiff>, Error> {
        let p = file
            .to_str()
            .ok_or_else(|| Error::Parse("文件路径含非 UTF-8 字符".into()))?;
        let text = self.git(&[
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicprefix=false",
            "diff",
            "--cached",
            "--no-color",
            "--",
            p,
        ])?;
        Ok(hunk::parse(&text).into_iter().next())
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

    /// 获取某提交中单个文件的 diff。改名前匹配不到或该提交未改动此文件 → None。
    pub fn commit_file_diff(&self, sha: &str, file_path: &Path) -> Result<Option<FileDiff>, Error> {
        hunk::commit_file_diff(self, sha, file_path)
    }

    /// 选定分支(或任意 ref)与当前工作区的差异文件列表(Show Diff with Working Tree)。
    pub fn diff_with_workdir(&self, rev: &str) -> Result<Vec<FileDiff>, Error> {
        hunk::diff_with_workdir(self, rev)
    }

    /// 读取任意 revision 下某文件的全文(`git show <rev>:<path>`),供文件查看器。
    /// 超过 1MB 返回 Precondition 错误(提示改用命令行),避免一次性塞进 UI 卡死。
    pub fn cat_file_at_rev(&self, revision: &str, path: &Path) -> Result<String, Error> {
        let p = path
            .to_str()
            .ok_or_else(|| Error::Parse("文件路径含非 UTF-8 字符".into()))?;
        let spec = format!("{revision}:{p}");
        let size = self.git_checked(&["cat-file", "-s", &spec])?;
        if size.success {
            if let Ok(bytes) = size.stdout.trim().parse::<u64>() {
                if bytes > 1_000_000 {
                    return Err(Error::Precondition(format!(
                        "文件过大（{} KB），请在命令行查看",
                        bytes / 1024
                    )));
                }
            }
        }
        self.git(&["show", &spec])
    }

    /// 工作区文件相对 HEAD 的 diff(供文件查看器的行内变更标记)。
    /// 已跟踪且无改动返回 None;未跟踪文件补成"全新增"(`git diff HEAD` 不含未跟踪)。
    pub fn file_diff_vs_head(&self, file: &Path) -> Result<Option<FileDiff>, Error> {
        let p = file
            .to_str()
            .ok_or_else(|| Error::Parse("文件路径含非 UTF-8 字符".into()))?;
        let text = self.git(&[
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicprefix=false",
            "diff",
            "HEAD",
            "--no-color",
            "--",
            p,
        ])?;
        if let Some(fd) = hunk::parse(&text).into_iter().next() {
            return Ok(Some(fd));
        }
        let tracked = self
            .git_checked(&["ls-files", "--error-unmatch", "--", p])?
            .success;
        if tracked {
            Ok(None)
        } else {
            Ok(hunk::untracked_file(self, p))
        }
    }

    /// 逐行 blame 一个文件(每行的作者/提交)。
    pub fn blame(&self, file_path: &Path) -> Result<Vec<BlameLine>, Error> {
        blame::blame(self, file_path)
    }

    /// 获取指定提交的完整消息(多行)。
    pub fn commit_message(&self, sha: &str) -> Result<String, Error> {
        diff::commit_message(self, sha)
    }

    /// 列出所有 stash。
    pub fn stashes(&self) -> Result<Vec<StashEntry>, Error> {
        stash::list_stashes(self)
    }

    /// 创建新的 stash；可选 paths 仅储藏部分文件。
    pub fn stash_push(&self, message: Option<&str>, paths: Option<&[String]>) -> Result<(), Error> {
        stash::stash_push(self, message, paths)
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

    /// 初始化并更新子仓库到父仓库记录的提交(git submodule update --init)。
    pub fn submodule_update(&self, path: &Path) -> Result<(), Error> {
        submodule::update_submodule(self, path)
    }

    /// 把子仓更新到它当前分支的 upstream 并留在该分支(对标 WebStorm,不 detach)。
    pub fn update_submodule_on_branch(
        &self,
        sub_path: &Path,
        opts: &UpdateOptions,
    ) -> Result<SubmoduleUpdate, Error> {
        update::update_submodule_on_branch(self, sub_path, opts)
    }

    /// 同步子仓库的 URL 配置(git submodule sync)。
    pub fn submodule_sync(&self, path: &Path) -> Result<(), Error> {
        submodule::sync_submodule(self, path)
    }

    /// 列出所有本地分支。
    pub fn branches(&self) -> Result<Vec<BranchInfo>, Error> {
        branch::list_branches(self)
    }

    /// 创建新分支。`start` 为 None 时从当前 HEAD,Some 时从指定起点(分支/提交)。
    pub fn create_branch(&self, name: &str, start: Option<&str>) -> Result<(), Error> {
        branch::create_branch(self, name, start)
    }

    /// 切换到指定分支。
    pub fn switch_branch(&self, name: &str) -> Result<(), Error> {
        branch::switch_branch(self, name)
    }

    /// 脏工作区智能切换(smart checkout):自动 stash → checkout → 贴回。
    pub fn switch_branch_autostash(&self, name: &str) -> Result<SwitchOutcome, Error> {
        branch::switch_branch_autostash(self, name)
    }

    /// 检出某个提交,进入 detached HEAD(对标 WebStorm Checkout Revision)。
    pub fn checkout_commit(&self, sha: &str) -> Result<(), Error> {
        branch::checkout_commit(self, sha)
    }

    /// 脏工作区智能检出提交(smart checkout):自动 stash → checkout → 贴回。
    pub fn checkout_commit_autostash(&self, sha: &str) -> Result<SwitchOutcome, Error> {
        branch::checkout_commit_autostash(self, sha)
    }

    /// 删除分支(安全模式)。
    pub fn delete_branch(&self, name: &str) -> Result<(), Error> {
        branch::delete_branch(self, name)
    }

    /// 删除远程分支(git push --delete,网络操作、不可逆)。
    pub fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<(), Error> {
        branch::delete_remote_branch(self, remote, branch)
    }

    /// 重命名分支(目标名已存在时报错)。
    pub fn rename_branch(&self, old: &str, new: &str) -> Result<(), Error> {
        branch::rename_branch(self, old, new)
    }

    /// 列出所有远程跟踪分支(refs/remotes/,过滤 origin/HEAD)。
    pub fn remote_branches(&self) -> Result<Vec<BranchInfo>, Error> {
        branch::list_remote_branches(self)
    }

    /// 检出远程分支为本地跟踪分支(脏工作区/本地同名已存在时报错)。
    pub fn checkout_remote(&self, remote_branch: &str) -> Result<(), Error> {
        branch::checkout_remote(self, remote_branch)
    }

    /// 脏工作区智能检出远程分支(smart checkout):自动 stash → checkout -b --track → 贴回。
    pub fn checkout_remote_autostash(&self, remote_branch: &str) -> Result<SwitchOutcome, Error> {
        branch::checkout_remote_autostash(self, remote_branch)
    }

    // 跑一个必须成功的 git 子命令,非零退出 → Err。
    pub(crate) fn git(&self, args: &[&str]) -> Result<String, Error> {
        git::run(&self.workdir, args)
    }

    // 跑一个 git 子命令,返回原始结果,不把非零当错误(用于可能冲突的整合)。
    pub(crate) fn git_checked(&self, args: &[&str]) -> Result<git::Output, Error> {
        git::run_checked(&self.workdir, args)
    }

    // 跑一个 git 子命令并附加环境变量(如 LC_ALL=C 强制英文输出),非零退出 → Err。
    pub(crate) fn git_env(&self, args: &[&str], env: &[(&str, &str)]) -> Result<String, Error> {
        git::run_env(&self.workdir, args, env)
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

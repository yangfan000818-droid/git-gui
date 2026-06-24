use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitcore::{
    BranchComparison, BranchInfo, CancelToken, CommitOptions, FileDiff, Hunk, PendingConflicts,
    PopResult, Progress, RebaseItem, ReflogEntry, Repo, RepoStatus, ResetMode, Segment, StashEntry,
    StashRef, SubmoduleUpdate, SwitchOutcome, TagInfo, UpdateOptions, UpdateOutcome,
};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

// ── 项目历史管理 ──

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ProjectHistory {
    last_project: Option<String>,
    recent_projects: Vec<String>,
}

impl ProjectHistory {
    fn load(app: &AppHandle) -> Self {
        let path = Self::config_path(app);
        if let Ok(content) = fs::read_to_string(&path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    fn save(&self, app: &AppHandle) -> Result<(), String> {
        let path = Self::config_path(app);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, content).map_err(|e| e.to_string())
    }

    fn config_path(app: &AppHandle) -> PathBuf {
        app.path()
            .app_data_dir()
            .expect("无法获取应用数据目录")
            .join("projects.json")
    }

    fn add_project(&mut self, path: String) {
        // 移除重复项
        self.recent_projects.retain(|p| p != &path);
        // 插入到开头
        self.recent_projects.insert(0, path.clone());
        // 最多保留 10 个
        self.recent_projects.truncate(10);
        // 更新上次打开
        self.last_project = Some(path);
    }
}

#[tauri::command]
fn get_last_project(app: AppHandle) -> Option<String> {
    ProjectHistory::load(&app).last_project
}

#[tauri::command]
fn get_recent_projects(app: AppHandle) -> Vec<String> {
    ProjectHistory::load(&app).recent_projects
}

#[tauri::command]
fn add_recent_project(app: AppHandle, path: String) -> Result<(), String> {
    let mut history = ProjectHistory::load(&app);
    history.add_project(path);
    history.save(&app)
}

#[tauri::command]
fn remove_recent_project(app: AppHandle, path: String) -> Result<(), String> {
    let mut history = ProjectHistory::load(&app);
    history.recent_projects.retain(|p| p != &path);
    if history.last_project.as_ref() == Some(&path) {
        history.last_project = history.recent_projects.first().cloned();
    }
    history.save(&app)
}

// ── 全局设置(settings.json):更新策略等,一次配置全局生效,更新时不再逐次弹选 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppSettings {
    /// 整合策略:"Merge" | "Rebase"。
    update_strategy: String,
    /// 整合时忽略纯空白差异(减少伪冲突)。
    ignore_whitespace: bool,
    /// 提交时跳过 git 钩子(--no-verify)。默认 false = 不跳过。
    #[serde(default)]
    skip_hooks: bool,

    // ── 外观设置 ──
    /// 主题预设: "neon-dark" | "light" | "dracula" | "nord" | "solarized-dark" |
    ///            "monokai" | "tokyo-night" | "github-dark"
    #[serde(default = "default_theme")]
    theme: String,
    /// 布局密度: "compact" | "comfortable" | "spacious"
    #[serde(default = "default_density")]
    density: String,
    /// 字号: "small" | "medium" | "large"
    #[serde(default = "default_font_size")]
    font_size: String,
    /// 是否启用过渡动画
    #[serde(default = "default_true")]
    animations_enabled: bool,
    /// 是否显示扫描线纹理
    #[serde(default = "default_true")]
    scanline_enabled: bool,
    /// Glow 强度: "off" | "subtle" | "medium" | "strong"
    #[serde(default = "default_glow")]
    glow_intensity: String,
}

fn default_theme() -> String {
    "neon-dark".into()
}
fn default_density() -> String {
    "comfortable".into()
}
fn default_font_size() -> String {
    "medium".into()
}
fn default_true() -> bool {
    true
}
fn default_glow() -> String {
    "medium".into()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            update_strategy: "Merge".into(),
            ignore_whitespace: true,
            skip_hooks: false,
            theme: default_theme(),
            density: default_density(),
            font_size: default_font_size(),
            animations_enabled: true,
            scanline_enabled: true,
            glow_intensity: default_glow(),
        }
    }
}

impl AppSettings {
    fn load(app: &AppHandle) -> Self {
        let path = Self::config_path(app);
        if let Ok(content) = fs::read_to_string(&path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    fn save(&self, app: &AppHandle) -> Result<(), String> {
        let path = Self::config_path(app);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, content).map_err(|e| e.to_string())
    }

    fn config_path(app: &AppHandle) -> PathBuf {
        app.path()
            .app_data_dir()
            .expect("无法获取应用数据目录")
            .join("settings.json")
    }
}

#[tauri::command]
fn get_settings(app: AppHandle) -> AppSettings {
    AppSettings::load(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    settings.save(&app)
}

// ── 取消注册表:op_id → CancelToken,供前端取消长操作 ──

struct CancelRegistry(Mutex<HashMap<String, CancelToken>>);

impl CancelRegistry {
    fn insert(&self, op_id: String, token: CancelToken) {
        self.0.lock().unwrap().insert(op_id, token);
    }
    fn get(&self, op_id: &str) -> Option<CancelToken> {
        self.0.lock().unwrap().get(op_id).cloned()
    }
    fn remove(&self, op_id: &str) {
        self.0.lock().unwrap().remove(op_id);
    }
}

impl Default for CancelRegistry {
    fn default() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

// ── 文件监视:debounce 500ms 后 emit "repo-changed" ──

struct WatchState {
    watcher: Mutex<Option<RecommendedWatcher>>,
}

impl Default for WatchState {
    fn default() -> Self {
        Self {
            watcher: Mutex::new(None),
        }
    }
}

#[tauri::command]
fn start_watch(app: AppHandle, path: String, state: State<'_, WatchState>) -> Result<(), String> {
    // 停掉旧的 watcher(释放即停)并重新开始监视。
    *state.watcher.lock().unwrap() = None;

    let (tx, rx) = mpsc::channel::<()>();

    // 后台线程:收到事件后等 500ms 无新事件才 emit。
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let mut pending = false;
        let mut last = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(()) => {
                    pending = true;
                    last = Instant::now();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if pending && last.elapsed() >= Duration::from_millis(500) {
                        let _ = app_handle.emit("repo-changed", ());
                        pending = false;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            if event.kind.is_access() {
                return;
            }
            // git 每次读操作(status/diff)都会瞬时创建/删除 .git/index.lock 等锁文件;
            // 递归监视 .git 会把这些纯锁文件事件当成"仓库变更" → 触发 refresh,
            // refresh 又跑 status/diff 再生成锁文件 → 自激成无限刷新(diff 区一闪一闪)。
            // 锁文件本身不是有意义的状态变更(真正的变更是锁释放后落盘的目标文件,
            // 如 index 的 rename 事件路径不含 .lock,仍会被捕获),整事件路径全是 *.lock 时跳过。
            if !event.paths.is_empty()
                && event
                    .paths
                    .iter()
                    .all(|p| p.extension().and_then(|e| e.to_str()) == Some("lock"))
            {
                return;
            }
            // .git/objects 与 .git/logs(含子仓 .git/modules/*/{objects,logs})下的写入是
            // 高频噪声:对象落盘、reflog 追加。它们不代表用户可见的仓库状态变化——真正的
            // 变化(分支移动/HEAD 切换/合并)一定同时落到被保留的 .git/refs、.git/HEAD、
            // .git/MERGE_HEAD 上。递归监视 .git 时这两类事件量极大(fetch/gc 尤甚),会把刷新打成
            // 风暴;而每次刷新又要对主仓+每个子仓各 spawn 5~7 个 git 进程(Windows 上进程
            // 创建昂贵)→ 直接卡顿。整事件路径都落在 git 内部的 objects/logs 时跳过。
            let is_git_noise = |p: &PathBuf| {
                let mut saw_git = false;
                let mut saw_noise = false;
                for c in p.components() {
                    match c.as_os_str().to_str() {
                        Some(".git") => saw_git = true,
                        Some("objects") | Some("logs") => saw_noise = true,
                        _ => {}
                    }
                }
                saw_git && saw_noise
            };
            // 自激刷新的根因:我们自己每次刷新都要跑的 `git status`/`git diff` 会刷新 stat cache
            // 并回写 .git/index(经 index.lock → rename 到 index;子仓为 .git/modules/*/index)。
            // 递归监视 .git 会把这个"自己造成的写"当成仓库变更 → 再刷新 → 再写 index →
            // 自持循环(尤其文件刚被编辑器/同步盘/杀软触碰、处于 git racy 态时,每次 status 都回写)。
            // index 写入无需靠 watcher 反映:App 自身的暂存/提交/丢弃等操作都在命令返回后显式 refresh。
            // 故整事件路径都是 .git 下 index 系列文件时跳过。
            // (代价:命令行外部 `git add` 不再即时反映,需等下一个事件;权衡可接受。)
            let is_index_write = |p: &PathBuf| {
                let in_git = p
                    .components()
                    .any(|c| c.as_os_str().to_str() == Some(".git"));
                let is_index = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n == "index" || n == "index.lock" || n.starts_with("sharedindex."))
                    .unwrap_or(false);
                in_git && is_index
            };
            if !event.paths.is_empty()
                && event
                    .paths
                    .iter()
                    .all(|p| is_git_noise(p) || is_index_write(p))
            {
                return;
            }
            let _ = tx.send(());
        }
    })
    .map_err(|e| e.to_string())?;

    // 监视工作目录(非递归:只关心直接文件变更)
    watcher
        .watch(Path::new(&path), RecursiveMode::NonRecursive)
        .map_err(|e| e.to_string())?;

    // 监视 .git(递归:objects/refs/HEAD/index 都在里)
    let git_dir = Path::new(&path).join(".git");
    if git_dir.is_dir() {
        watcher
            .watch(&git_dir, RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;
    }

    *state.watcher.lock().unwrap() = Some(watcher);
    Ok(())
}

// ── Changes 视图命令 ──

#[tauri::command]
async fn repo_status(path: String) -> Result<RepoStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.status().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_unstaged_diff(path: String) -> Result<Vec<FileDiff>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.unstaged_diff().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_staged_diff(path: String) -> Result<Vec<FileDiff>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.staged_diff().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 单个文件的未暂存 diff(选中文件时懒加载)。
#[tauri::command]
async fn repo_file_unstaged_diff(path: String, file: String) -> Result<Option<FileDiff>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.file_unstaged_diff(Path::new(&file))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 单个文件的已暂存 diff(选中文件时懒加载)。
#[tauri::command]
async fn repo_file_staged_diff(path: String, file: String) -> Result<Option<FileDiff>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.file_staged_diff(Path::new(&file))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_stage(path: String, files: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let paths: Vec<&Path> = files.iter().map(|s| Path::new(s.as_str())).collect();
        repo.stage(&paths).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_unstage(path: String, files: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let paths: Vec<&Path> = files.iter().map(|s| Path::new(s.as_str())).collect();
        repo.unstage(&paths).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_discard(path: String, files: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let paths: Vec<&Path> = files.iter().map(|s| Path::new(s.as_str())).collect();
        repo.discard(&paths).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_stage_hunk(path: String, file: FileDiff, hunk: Hunk) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.stage_hunk(&file, &hunk).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_unstage_hunk(path: String, file: FileDiff, hunk: Hunk) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.unstage_hunk(&file, &hunk).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_stage_lines(
    path: String,
    file: FileDiff,
    hunk: Hunk,
    selected: Vec<usize>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.stage_lines(&file, &hunk, &selected)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_unstage_lines(
    path: String,
    file: FileDiff,
    hunk: Hunk,
    selected: Vec<usize>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.unstage_lines(&file, &hunk, &selected)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_commit(
    app: AppHandle,
    path: String,
    message: String,
    amend: bool,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let opts = CommitOptions {
            message,
            amend,
            no_verify: AppSettings::load(&app).skip_hooks,
            ..Default::default()
        };
        repo.commit(&opts).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Stash 管理命令(对标 WebStorm Stash / Unstash Changes) ──

#[tauri::command]
async fn repo_stashes(path: String) -> Result<Vec<StashEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.stashes().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 把当前工作区改动(含未跟踪)储藏起来;message 为空则用 git 默认描述。
#[tauri::command]
async fn repo_stash_push(path: String, message: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.stash_push(message.as_deref().filter(|m| !m.is_empty()))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 应用指定 stash(保留 stash)。
#[tauri::command]
async fn repo_stash_apply(path: String, reff: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.stash_apply(&reff).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 弹出指定 stash(应用 + 删除;冲突则保留 stash)。
#[tauri::command]
async fn repo_stash_pop(path: String, reff: String) -> Result<PopResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.stash_pop(&reff).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 丢弃指定 stash。
#[tauri::command]
async fn repo_stash_drop(path: String, reff: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.stash_drop(&reff).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 初始化/更新子仓库到父仓库记录的提交。可能较慢(需 clone/fetch),故 spawn_blocking。
#[tauri::command]
async fn repo_submodule_update(path: String, sub_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.submodule_update(Path::new(&sub_path))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 把子仓更新到它当前分支的 upstream 并留在该分支(对标 WebStorm,不 detach)。
/// detached / 无 upstream 跳过;冲突回退。供"全部更新"逐子仓调用。
#[tauri::command]
async fn repo_update_submodule_on_branch(
    path: String,
    sub_path: String,
    options: UpdateOptions,
) -> Result<SubmoduleUpdate, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.update_submodule_on_branch(Path::new(&sub_path), &options)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 同步子仓库的 URL 配置(git submodule sync)。
#[tauri::command]
async fn repo_submodule_sync(path: String, sub_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.submodule_sync(Path::new(&sub_path))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 主仓库:fetch 远程(只下载不合并,无冲突风险)。
#[tauri::command]
async fn repo_fetch(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let cancel = CancelToken::default();
        let mut noop = |_p: Progress| {};
        repo.fetch_streaming(&mut noop, &cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 主仓库:push 当前分支到 upstream。返回 PushOutcome 交前端决定文案与后续
/// (被拒于"远端领先"时,前端可走「更新后推送」)。
#[tauri::command]
async fn repo_push(path: String) -> Result<gitcore::PushOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.push().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Update 视图命令 ──

/// 执行完整 Update 流程(autostash → 整合 → restore)。
/// 长操作:async + spawn_blocking,进度经 "update-progress" 事件推送,取消走 CancelRegistry。
#[tauri::command]
async fn execute_update(
    app: AppHandle,
    path: String,
    op_id: String,
    options: UpdateOptions,
    state: State<'_, CancelRegistry>,
) -> Result<UpdateOutcome, String> {
    let cancel = CancelToken::default();
    state.insert(op_id.clone(), cancel.clone());

    let res = tauri::async_runtime::spawn_blocking(move || -> Result<UpdateOutcome, String> {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let mut on_progress = |p: Progress| {
            let _ = app.emit("update-progress", p);
        };
        repo.execute_update_streaming(&options, &mut on_progress, &cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?;

    state.remove(&op_id);
    res
}

/// 从远程 URL clone 仓库到 parent_dir 下(子目录名由 URL 推断),返回 clone 出的仓库路径。
/// 长操作:async + spawn_blocking,进度经 "clone-progress" 事件推送,取消走 CancelRegistry。
#[tauri::command]
async fn repo_clone(
    app: AppHandle,
    url: String,
    parent_dir: String,
    op_id: String,
    state: State<'_, CancelRegistry>,
) -> Result<String, String> {
    let cancel = CancelToken::default();
    state.insert(op_id.clone(), cancel.clone());
    let res = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let mut on_progress = |p: Progress| {
            let _ = app.emit("clone-progress", p);
        };
        gitcore::clone_streaming(&url, Path::new(&parent_dir), &mut on_progress, &cancel)
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?;
    state.remove(&op_id);
    res
}

/// 取消某个进行中的长操作(查 CancelRegistry 并 token.cancel())。
#[tauri::command]
fn cancel_op(op_id: String, state: State<'_, CancelRegistry>) {
    if let Some(t) = state.get(&op_id) {
        t.cancel();
    }
}

/// 读取仓库中的文件内容(供冲突解决 textarea 展示)。
#[tauri::command]
fn read_repo_file(path: String, file_path: String) -> Result<String, String> {
    let workdir = Path::new(&path);
    std::fs::read_to_string(workdir.join(&file_path))
        .map_err(|e| format!("读取 {file_path} 失败: {e}"))
}

/// 写回某文件的冲突解决结果并 git add。
#[tauri::command]
async fn resolve_conflict_file(
    path: String,
    file_path: String,
    text: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.resolve_file(Path::new(&file_path), &text)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 读取冲突文件的片段序列(已 refine),供三栏视图渲染。
#[tauri::command]
async fn read_conflict_segments(path: String, file_path: String) -> Result<Vec<Segment>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.read_conflict(Path::new(&file_path))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 冲突解决后完成整合,并还原 autostash。
#[tauri::command]
async fn continue_update_cmd(
    path: String,
    autostash: Option<StashRef>,
    recurse_submodules: bool,
) -> Result<UpdateOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.continue_update(autostash, recurse_submodules)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 放弃整合,回到 Update 之前的状态(含还原 autostash)。
#[tauri::command]
async fn abort_update_cmd(path: String, autostash: Option<StashRef>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.abort_update(autostash).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 检测未完成的整合(中断/崩溃后):返回待解决冲突文件 + 扫回的 autostash。
#[tauri::command]
async fn resume_conflicts(path: String) -> Result<Option<PendingConflicts>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.resume_conflicts().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Cherry-pick 一个提交到当前分支。
#[tauri::command]
async fn repo_cherry_pick(path: String, sha: String) -> Result<UpdateOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.cherry_pick(&sha).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Revert 一个提交(生成反向提交)。
#[tauri::command]
async fn repo_revert(path: String, sha: String) -> Result<UpdateOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.revert(&sha).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 把当前分支重置到指定提交(soft/mixed/hard,对标 WebStorm Reset Current Branch to Here)。
#[tauri::command]
async fn repo_reset(path: String, sha: String, mode: ResetMode) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.reset(&sha, mode).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 取 HEAD reflog(最近 max_count 条),供查看/恢复历史状态。
#[tauri::command]
async fn repo_reflog(path: String, max_count: usize) -> Result<Vec<ReflogEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.reflog(max_count).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Tag 管理命令 ──

#[tauri::command]
async fn repo_tags(path: String) -> Result<Vec<TagInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.tags().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 创建 tag。target 为 None 打在 HEAD;message 非空则为注释标签。
#[tauri::command]
async fn repo_create_tag(
    path: String,
    name: String,
    target: Option<String>,
    message: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.create_tag(
            &name,
            target.as_deref(),
            message.as_deref().filter(|m| !m.is_empty()),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_delete_tag(path: String, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.delete_tag(&name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 把一个 tag 推送到默认远程。
#[tauri::command]
async fn repo_push_tag(path: String, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.push_tag(&name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 列出仓库所有本地分支。
#[tauri::command]
async fn repo_branches(path: String) -> Result<Vec<BranchInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.branches().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 切换到指定分支(工作区脏时返回错误,引导先提交/暂存)。
#[tauri::command]
async fn repo_switch_branch(path: String, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.switch_branch(&name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 脏工作区智能切换(smart checkout):自动 stash → checkout → 贴回。
#[tauri::command]
async fn repo_switch_branch_autostash(path: String, name: String) -> Result<SwitchOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.switch_branch_autostash(&name)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 检出某个提交,进入 detached HEAD(对标 WebStorm Checkout Revision)。
#[tauri::command]
async fn repo_checkout_commit(path: String, sha: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.checkout_commit(&sha).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 脏工作区智能检出提交(smart checkout):自动 stash → checkout → 贴回。
#[tauri::command]
async fn repo_checkout_commit_autostash(
    path: String,
    sha: String,
) -> Result<SwitchOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.checkout_commit_autostash(&sha)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 新建分支(仅创建,不切换)。start_point 为 None 时从当前 HEAD,Some 时从指定分支/提交。
#[tauri::command]
async fn repo_create_branch(
    path: String,
    name: String,
    start_point: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.create_branch(&name, start_point.as_deref())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 删除分支(安全模式:拒删当前分支和未合并分支)。
#[tauri::command]
async fn repo_delete_branch(path: String, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.delete_branch(&name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 重命名分支(目标名已存在时返回错误)。
#[tauri::command]
async fn repo_rename_branch(
    path: String,
    old_name: String,
    new_name: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.rename_branch(&old_name, &new_name)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 列出远程跟踪分支(refs/remotes/,过滤 origin/HEAD)。
#[tauri::command]
async fn repo_remote_branches(path: String) -> Result<Vec<BranchInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.remote_branches().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 把另一个分支合并到当前分支(git merge <branch>;脏工作区自动 autostash)。
/// 返回 UpdateOutcome:Resolved/FastForwarded/Integrated 表示干净完成,Conflicted 需进 ConflictView。
#[tauri::command]
async fn repo_merge_branch(
    path: String,
    branch: String,
    options: UpdateOptions,
) -> Result<UpdateOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.merge_branch(&branch, &options)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 把当前分支变基到另一个分支(git rebase <branch>;脏工作区自动 autostash)。
#[tauri::command]
async fn repo_rebase_branch(
    path: String,
    branch: String,
    options: UpdateOptions,
) -> Result<UpdateOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.rebase_branch(&branch, &options)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 列出 from_sha..HEAD(含 from_sha)的提交,供交互式变基编辑(oldest-first)。
#[tauri::command]
async fn repo_rebase_plan(
    path: String,
    from_sha: String,
) -> Result<Vec<gitcore::LogEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.rebase_plan(&from_sha).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 从 from_sha 起按给定操作交互式变基(reword/squash/fixup/drop/重排);
/// 冲突时返回 Conflicted,交前端 ConflictView 复用 continue/abort 推进。
#[tauri::command]
async fn repo_rebase_interactive(
    path: String,
    from_sha: String,
    items: Vec<RebaseItem>,
) -> Result<UpdateOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.rebase_interactive(&from_sha, &items)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 检出远程分支为本地跟踪分支(脏工作区/本地同名已存在时返回错误)。
#[tauri::command]
async fn repo_checkout_remote(path: String, remote_branch: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.checkout_remote(&remote_branch)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 脏工作区智能检出远程分支(smart checkout):自动 stash → checkout -b --track → 贴回。
#[tauri::command]
async fn repo_checkout_remote_autostash(
    path: String,
    remote_branch: String,
) -> Result<SwitchOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.checkout_remote_autostash(&remote_branch)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── History 视图命令 ──

#[tauri::command]
async fn repo_log_graph(
    path: String,
    max_count: usize,
    branch: Option<String>,
) -> Result<Vec<gitcore::GraphRow>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let opts = gitcore::LogOptions {
            max_count,
            branch,
            author: None,
            grep: None,
        };
        repo.log_graph(&opts).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_file_history(
    path: String,
    file_path: String,
    max_count: usize,
) -> Result<Vec<gitcore::LogEntry>, String> {
    // git log --follow fork 子进程,大历史可能慢;同步 command 跑主线程会冻结 UI,故 spawn_blocking。
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let opts = gitcore::LogOptions {
            max_count,
            branch: None,
            author: None,
            grep: None,
        };
        repo.file_history(Path::new(&file_path), &opts)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_commit_file_diff(
    path: String,
    sha: String,
    file_path: String,
) -> Result<Option<FileDiff>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.commit_file_diff(&sha, Path::new(&file_path))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_blame(path: String, file_path: String) -> Result<Vec<gitcore::BlameLine>, String> {
    // git blame 大文件可能慢;同步 command 跑主线程会冻结 UI,故 spawn_blocking。
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.blame(Path::new(&file_path)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_log_topology(
    path: String,
    max_count: usize,
    branch: Option<String>,
    author: Option<String>,
    grep: Option<String>,
) -> Result<Vec<gitcore::GraphCommit>, String> {
    // git log + 拓扑计算会 fork 子进程;同步 command 跑在主线程会冻结 UI,故 spawn_blocking。
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let opts = gitcore::LogOptions {
            max_count,
            branch,
            author,
            grep,
        };
        repo.log_topology(&opts).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 合并主仓与各子仓的提交历史(按时间排序,每条带仓库标识),用于含子模块的仓库。
#[tauri::command]
async fn repo_log_merged(
    path: String,
    max_count: usize,
    author: Option<String>,
    grep: Option<String>,
) -> Result<Vec<gitcore::MergedLogEntry>, String> {
    // 主仓 + 各子仓串行 fork git(2+2N 次);同步 command 跑在主线程会冻结 UI,故 spawn_blocking。
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let opts = gitcore::LogOptions {
            max_count,
            branch: None,
            author,
            grep,
        };
        repo.log_merged(&opts).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_commit_files(path: String, sha: String) -> Result<Vec<FileDiff>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.commit_files(&sha).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 列出子仓库在 old..new 区间的提交(父仓 commit 详情展开子模块指针变化)。
#[tauri::command]
async fn repo_submodule_commits(
    path: String,
    sub_path: String,
    old_sha: String,
    new_sha: String,
) -> Result<Vec<gitcore::LogEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.submodule_commits(Path::new(&sub_path), &old_sha, &new_sha)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 选定分支(或任意 ref)与当前工作区的差异(Show Diff with Working Tree)。
#[tauri::command]
async fn repo_diff_with_workdir(path: String, rev: String) -> Result<Vec<FileDiff>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.diff_with_workdir(&rev).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 选定分支与当前 HEAD 的双向独有提交(Compare with Current)。
#[tauri::command]
async fn repo_compare_commits(path: String, other: String) -> Result<BranchComparison, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.compare_commits(&other).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn repo_commit_message(path: String, sha: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        repo.commit_message(&sha).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 检查 git 是否可用:Windows 不自带 git,缺 git 时给友好提示。
#[tauri::command]
fn check_git() -> Result<(), String> {
    gitcore::Repo::check_git().map_err(|e| e.to_string())
}

// ── 启动 ──

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(CancelRegistry::default())
        .manage(WatchState::default())
        .invoke_handler(tauri::generate_handler![
            get_last_project,
            get_recent_projects,
            add_recent_project,
            remove_recent_project,
            get_settings,
            save_settings,
            repo_status,
            repo_unstaged_diff,
            repo_staged_diff,
            repo_file_unstaged_diff,
            repo_file_staged_diff,
            repo_stage,
            repo_unstage,
            repo_discard,
            repo_stage_hunk,
            repo_unstage_hunk,
            repo_stage_lines,
            repo_unstage_lines,
            repo_commit,
            repo_stashes,
            repo_stash_push,
            repo_stash_apply,
            repo_stash_pop,
            repo_stash_drop,
            repo_submodule_update,
            repo_update_submodule_on_branch,
            repo_submodule_sync,
            repo_fetch,
            repo_push,
            execute_update,
            repo_clone,
            cancel_op,
            read_repo_file,
            resolve_conflict_file,
            read_conflict_segments,
            continue_update_cmd,
            abort_update_cmd,
            resume_conflicts,
            repo_cherry_pick,
            repo_revert,
            repo_reset,
            repo_reflog,
            repo_tags,
            repo_create_tag,
            repo_delete_tag,
            repo_push_tag,
            repo_branches,
            repo_switch_branch,
            repo_checkout_commit,
            repo_checkout_commit_autostash,
            repo_switch_branch_autostash,
            repo_create_branch,
            repo_delete_branch,
            repo_rename_branch,
            repo_remote_branches,
            repo_checkout_remote,
            repo_checkout_remote_autostash,
            repo_merge_branch,
            repo_rebase_branch,
            repo_rebase_plan,
            repo_rebase_interactive,
            repo_log_graph,
            repo_file_history,
            repo_commit_file_diff,
            repo_blame,
            repo_log_topology,
            repo_log_merged,
            start_watch,
            check_git,
            repo_commit_files,
            repo_submodule_commits,
            repo_diff_with_workdir,
            repo_compare_commits,
            repo_commit_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitcore::{
    CancelToken, Choice, CommitOptions, FileDiff, Hunk, PendingConflicts, Progress, Repo,
    RepoStatus, Segment, StashRef, UpdateOptions, UpdateOutcome, UpdatePlan,
};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, State};

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

// ── 文件监视:debounce 300ms 后 emit "repo-changed" ──

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

    // 后台线程:收到事件后等 300ms 无新事件才 emit。
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let mut pending = false;
        let mut last = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(300)) {
                Ok(()) => {
                    pending = true;
                    last = Instant::now();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if pending && last.elapsed() >= Duration::from_millis(300) {
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
fn repo_status(path: String) -> Result<RepoStatus, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.status().map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_unstaged_diff(path: String) -> Result<Vec<FileDiff>, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.unstaged_diff().map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_staged_diff(path: String) -> Result<Vec<FileDiff>, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.staged_diff().map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_stage(path: String, files: Vec<String>) -> Result<(), String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    let paths: Vec<&Path> = files.iter().map(|s| Path::new(s.as_str())).collect();
    repo.stage(&paths).map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_unstage(path: String, files: Vec<String>) -> Result<(), String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    let paths: Vec<&Path> = files.iter().map(|s| Path::new(s.as_str())).collect();
    repo.unstage(&paths).map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_discard(path: String, files: Vec<String>) -> Result<(), String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    let paths: Vec<&Path> = files.iter().map(|s| Path::new(s.as_str())).collect();
    repo.discard(&paths).map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_stage_hunk(path: String, file: FileDiff, hunk: Hunk) -> Result<(), String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.stage_hunk(&file, &hunk).map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_unstage_hunk(path: String, file: FileDiff, hunk: Hunk) -> Result<(), String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.unstage_hunk(&file, &hunk).map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_stage_lines(
    path: String,
    file: FileDiff,
    hunk: Hunk,
    selected: Vec<usize>,
) -> Result<(), String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.stage_lines(&file, &hunk, &selected)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_unstage_lines(
    path: String,
    file: FileDiff,
    hunk: Hunk,
    selected: Vec<usize>,
) -> Result<(), String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.unstage_lines(&file, &hunk, &selected)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_commit(path: String, message: String) -> Result<String, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    let opts = CommitOptions {
        message,
        ..Default::default()
    };
    repo.commit(&opts).map_err(|e| e.to_string())
}

// ── Update 视图命令 ──

/// 预检 + fetch + 计算计划,不改动工作区。fetch 阶段可取消。
#[tauri::command]
async fn plan_update(
    app: AppHandle,
    path: String,
    op_id: String,
    options: UpdateOptions,
    state: State<'_, CancelRegistry>,
) -> Result<UpdatePlan, String> {
    let cancel = CancelToken::default();
    state.insert(op_id.clone(), cancel.clone());

    let res = tauri::async_runtime::spawn_blocking(move || -> Result<UpdatePlan, String> {
        let repo = Repo::open(&path).map_err(|e| e.to_string())?;
        let mut on_progress = |p: Progress| {
            let _ = app.emit("update-progress", p);
        };
        repo.plan_update_streaming(&options, &mut on_progress, &cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?;

    state.remove(&op_id);
    res
}

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
fn resolve_conflict_file(path: String, file_path: String, text: String) -> Result<(), String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.resolve_file(Path::new(&file_path), &text)
        .map_err(|e| e.to_string())
}

/// 读取冲突文件的片段序列(已 refine),供三栏视图渲染。
#[tauri::command]
fn read_conflict_segments(path: String, file_path: String) -> Result<Vec<Segment>, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.read_conflict(Path::new(&file_path))
        .map_err(|e| e.to_string())
}

/// 按用户选择重建冲突文件文本并写回 + git add。
#[tauri::command]
fn resolve_conflict(path: String, file_path: String, choices: Vec<Choice>) -> Result<(), String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    let segments = repo
        .read_conflict(Path::new(&file_path))
        .map_err(|e| e.to_string())?;
    let text = gitcore::rebuild(&segments, &choices);
    repo.resolve_file(Path::new(&file_path), &text)
        .map_err(|e| e.to_string())
}

/// 冲突解决后完成整合,并还原 autostash。
#[tauri::command]
fn continue_update_cmd(
    path: String,
    autostash: Option<StashRef>,
    recurse_submodules: bool,
) -> Result<UpdateOutcome, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.continue_update(autostash, recurse_submodules)
        .map_err(|e| e.to_string())
}

/// 放弃整合,回到 Update 之前的状态(含还原 autostash)。
#[tauri::command]
fn abort_update_cmd(path: String, autostash: Option<StashRef>) -> Result<(), String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.abort_update(autostash).map_err(|e| e.to_string())
}

/// 检测未完成的整合(中断/崩溃后):返回待解决冲突文件 + 扫回的 autostash。
#[tauri::command]
fn resume_conflicts(path: String) -> Result<Option<PendingConflicts>, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.resume_conflicts().map_err(|e| e.to_string())
}

// ── History 视图命令 ──

#[tauri::command]
fn repo_log_graph(
    path: String,
    max_count: usize,
    branch: Option<String>,
) -> Result<Vec<gitcore::GraphRow>, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    let opts = gitcore::LogOptions { max_count, branch };
    repo.log_graph(&opts).map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_log_topology(
    path: String,
    max_count: usize,
    branch: Option<String>,
) -> Result<Vec<gitcore::GraphCommit>, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    let opts = gitcore::LogOptions { max_count, branch };
    repo.log_topology(&opts).map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_commit_files(path: String, sha: String) -> Result<Vec<FileDiff>, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.commit_files(&sha).map_err(|e| e.to_string())
}

#[tauri::command]
fn repo_commit_message(path: String, sha: String) -> Result<String, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.commit_message(&sha).map_err(|e| e.to_string())
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
        .manage(CancelRegistry::default())
        .manage(WatchState::default())
        .invoke_handler(tauri::generate_handler![
            repo_status,
            repo_unstaged_diff,
            repo_staged_diff,
            repo_stage,
            repo_unstage,
            repo_discard,
            repo_stage_hunk,
            repo_unstage_hunk,
            repo_stage_lines,
            repo_unstage_lines,
            repo_commit,
            plan_update,
            execute_update,
            cancel_op,
            read_repo_file,
            resolve_conflict_file,
            read_conflict_segments,
            resolve_conflict,
            continue_update_cmd,
            abort_update_cmd,
            resume_conflicts,
            repo_log_graph,
            repo_log_topology,
            start_watch,
            check_git,
            repo_commit_files,
            repo_commit_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

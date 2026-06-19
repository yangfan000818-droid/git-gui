use gitcore::{Repo, RepoStatus};

// walking skeleton:打开指定仓库,返回结构化状态(分支 / ahead-behind / dirty / 文件)。
// gitcore 是同步阻塞 API,每次 spawn git 子进程;Tauri 在独立线程执行同步 command,
// 不会卡住 UI 主线程。后续长操作(update / fetch)再换 async + spawn_blocking + event。
#[tauri::command]
fn repo_status(path: String) -> Result<RepoStatus, String> {
    let repo = Repo::open(&path).map_err(|e| e.to_string())?;
    repo.status().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![repo_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

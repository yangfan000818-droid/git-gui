use std::path::Path;

use gitcore::{CommitOptions, FileDiff, Hunk, Repo, RepoStatus};

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

//! CLI 模式:`tui status` / `tui update`,便于脚本化与自动化验证。

use gitcore::{Repo, UpdateOptions, UpdateOutcome};
use std::process::ExitCode;

fn open() -> Result<Repo, ExitCode> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    Repo::open(&cwd).map_err(|e| {
        eprintln!("打开仓库失败: {e}");
        ExitCode::FAILURE
    })
}

pub fn status() -> ExitCode {
    let repo = match open() {
        Ok(r) => r,
        Err(c) => return c,
    };
    match repo.status() {
        Ok(st) => {
            println!(
                "分支:      {}",
                st.branch.as_deref().unwrap_or("(detached)")
            );
            println!("upstream:  {}", st.upstream.as_deref().unwrap_or("(none)"));
            println!("领先/落后: +{} / -{}", st.ahead, st.behind);
            println!(
                "工作区:    {}",
                if st.dirty {
                    "有未提交改动"
                } else {
                    "干净"
                }
            );
            if !st.conflicted.is_empty() {
                println!("冲突文件:  {} 个", st.conflicted.len());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("读取状态失败: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn update() -> ExitCode {
    let repo = match open() {
        Ok(r) => r,
        Err(c) => return c,
    };
    match repo.execute_update(&UpdateOptions::default()) {
        Ok(outcome) => {
            println!("{}", describe(&outcome));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("更新失败: {e}");
            ExitCode::FAILURE
        }
    }
}

/// outcome → 人话,CLI 与 TUI 共用。
pub fn describe(outcome: &UpdateOutcome) -> String {
    match outcome {
        UpdateOutcome::AlreadyUpToDate => "已是最新。".into(),
        UpdateOutcome::FastForwarded { commits } => format!("已快进 {commits} 个提交。"),
        UpdateOutcome::Integrated { commits, strategy } => {
            format!("已整合 {commits} 个提交({strategy:?})。")
        }
        UpdateOutcome::Conflicted { files, .. } => {
            format!("整合冲突,需手动解决 {} 个文件。", files.len())
        }
        UpdateOutcome::StashRestoreConflict { files } => {
            format!("整合成功,但暂存改动贴回时冲突 {} 个文件。", files.len())
        }
        UpdateOutcome::Resolved => "冲突已解决,整合完成。".into(),
    }
}

//! 最小前端壳:验证 gitcore 端到端可用。
//!
//! 当前是命令行形态(`tui status` / `tui update`);
//! 下一步替换为 ratatui 全屏交互界面。

use gitcore::{Repo, UpdateOptions, UpdateOutcome};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "status".into());
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());

    let repo = match Repo::open(&cwd) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("打开仓库失败: {e}");
            return ExitCode::FAILURE;
        }
    };

    match cmd.as_str() {
        "status" => print_status(&repo),
        "update" => run_update(&repo),
        other => {
            eprintln!("未知命令: {other}(可用: status | update)");
            ExitCode::FAILURE
        }
    }
}

fn print_status(repo: &Repo) -> ExitCode {
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

fn run_update(repo: &Repo) -> ExitCode {
    match repo.execute_update(&UpdateOptions::default()) {
        Ok(outcome) => {
            match outcome {
                UpdateOutcome::AlreadyUpToDate => println!("已是最新。"),
                UpdateOutcome::FastForwarded { commits } => println!("已快进 {commits} 个提交。"),
                UpdateOutcome::Integrated { commits, strategy } => {
                    println!("已整合 {commits} 个提交({strategy:?})。")
                }
                UpdateOutcome::Conflicted { files, .. } => {
                    println!("整合冲突,需手动解决 {} 个文件:", files.len());
                    for f in files {
                        println!("  {}", f.display());
                    }
                }
                UpdateOutcome::StashRestoreConflict { files } => {
                    println!("整合成功,但暂存改动贴回时冲突 {} 个文件。", files.len())
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("更新失败: {e}");
            ExitCode::FAILURE
        }
    }
}

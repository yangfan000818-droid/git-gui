//! git-gui 前端入口。
//!
//! 无参数 → ratatui 全屏 TUI;`status` / `update` → CLI 模式(便于脚本化与验证)。

mod app;
mod branch_ui;
mod cli;
mod conflict_ui;
mod diff_ui;
mod keys;
mod log_ui;
mod scroll;
mod stage_ui;
mod stash_ui;
mod submodule_ui;

use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => app::run(),
        Some("status") => cli::status(),
        Some("update") => cli::update(),
        Some(other) => {
            eprintln!("未知命令: {other}(可用: status | update,或无参数进入 TUI)");
            ExitCode::FAILURE
        }
    }
}

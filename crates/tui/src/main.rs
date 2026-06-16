//! git-gui 前端入口。
//!
//! 无参数 → ratatui 全屏 TUI;`status` / `update` → CLI 模式(便于脚本化与验证)。

mod app;
mod cli;

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

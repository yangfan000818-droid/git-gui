//! 系统剪贴板:spawn 平台命令并写入 stdin(零 Rust 依赖,契合项目 spawn CLI 风格)。

use std::io::Write;
use std::process::{Command, Stdio};

/// 复制文本到系统剪贴板,返回是否成功。按平台依次尝试可用命令。
pub fn copy(text: &str) -> bool {
    candidates()
        .iter()
        .any(|(cmd, args)| try_copy(cmd, args, text))
}

fn candidates() -> &'static [(&'static str, &'static [&'static str])] {
    match std::env::consts::OS {
        "macos" => &[("pbcopy", &[])],
        "windows" => &[("clip", &[])],
        // Linux/BSD:Wayland 优先,回退 X11。
        _ => &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ],
    }
}

fn try_copy(cmd: &str, args: &[&str], text: &str) -> bool {
    let mut child = match Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false, // 该命令不存在 → 试下一个
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
        // stdin 在此 drop,关闭管道,命令读到 EOF 后结束。
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

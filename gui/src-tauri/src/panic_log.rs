//! panic 日志落盘:全局钩子把 panic 信息 + backtrace 追加写入日志文件,
//! 崩溃后用户反馈时有据可查。不引入遥测,纯本地文件。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 日志超过此大小时轮转为 `.old`,防止反复 panic 无限增长。
const ROTATE_BYTES: u64 = 256 * 1024;

/// 安装全局 panic 钩子,写入 `log_dir/panic.log`;保留原钩子(stderr 输出)。
pub fn install(log_dir: PathBuf) {
    let path = log_dir.join("panic.log");
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // force_capture 不依赖 RUST_BACKTRACE 环境变量,release 下也能拿到帧。
        let backtrace = std::backtrace::Backtrace::force_capture();
        let _ = write_entry(&path, &info.to_string(), &backtrace.to_string());
        prev(info);
    }));
}

/// 追加一条 panic 记录;目录不存在自动创建,文件过大先轮转。
fn write_entry(path: &Path, info: &str, backtrace: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let too_big = fs::metadata(path)
        .map(|m| m.len() > ROTATE_BYTES)
        .unwrap_or(false);
    if too_big {
        let _ = fs::rename(path, path.with_extension("log.old"));
    }
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        f,
        "==== panic @ epoch {epoch} (GitSail v{}) ====\n{info}\n{backtrace}",
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gitsail-panic-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn write_entry_creates_dir_and_appends() {
        let dir = tmp_dir("append");
        let path = dir.join("panic.log");
        write_entry(&path, "panicked at 'boom'", "bt-1").unwrap();
        write_entry(&path, "panicked at 'again'", "bt-2").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("boom"));
        assert!(content.contains("again"));
        assert!(content.contains(env!("CARGO_PKG_VERSION")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_entry_rotates_when_too_big() {
        let dir = tmp_dir("rotate");
        let path = dir.join("panic.log");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, vec![b'x'; (ROTATE_BYTES + 1) as usize]).unwrap();
        write_entry(&path, "fresh", "bt").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("fresh"));
        assert!(content.len() < 1024, "轮转后应是新文件");
        assert!(path.with_extension("log.old").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}

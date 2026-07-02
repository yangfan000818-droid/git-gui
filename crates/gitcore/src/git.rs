use crate::error::Error;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 创建一个预置 Windows `CREATE_NO_WINDOW` flag 的 git Command,
/// 避免每次 spawn 闪黑框。
fn git_command() -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut cmd = Command::new("git");
        let _ = cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(windows))]
    {
        Command::new("git")
    }
}

/// 检查 git 是否在 PATH 中可用。不可用返回友好错误。
pub(crate) fn check_available() -> Result<(), Error> {
    match git_command().arg("--version").output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(_) => Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "git 未正确安装(非零退出)",
        ))),
        Err(_) => Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "git 不在 PATH 中,请安装 Git 并将其加入 PATH",
        ))),
    }
}

/// 一次 git 调用的原始结果。
pub(crate) struct Output {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// 跑 git;非零退出 → Err(Error::Git)。用于"必须成功"的命令。
pub(crate) fn run(workdir: &Path, args: &[&str]) -> Result<String, Error> {
    let out = run_checked(workdir, args)?;
    if out.success {
        Ok(out.stdout)
    } else {
        Err(Error::Git {
            args: args.iter().map(|s| s.to_string()).collect(),
            code: out.code,
            stderr: out.stderr,
        })
    }
}

/// 跑 git;非零退出不视为错误,原样返回(供整合等可能冲突的命令使用)。
pub(crate) fn run_checked(workdir: &Path, args: &[&str]) -> Result<Output, Error> {
    run_checked_env(workdir, args, &[])
}

/// 同 `run_checked`,但附加额外环境变量。供需固定 locale 等的命令使用
/// (如 `git clean -n` 的 "Would remove" 在 gettext 环境会被本地化,解析前须强制 C locale)。
pub(crate) fn run_checked_env(
    workdir: &Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<Output, Error> {
    // --no-optional-locks:禁止为可选优化获取锁(主要是 status 刷新 stat cache 时回写 index)。
    // GUI 后台高频 status 与前台 git 写操作会争抢 index.lock,且每次回写 index 都触发文件监视
    // 事件 → 刷新风暴;禁掉可选锁可显著减少这类抖动。必需的锁(commit/add 等)不受影响。
    let mut cmd = git_command();
    cmd.arg("--no-optional-locks")
        .args(args)
        .current_dir(workdir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let output = cmd.output()?;
    Ok(Output {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// 带 env 跑 git;非零退出 → Err。
pub(crate) fn run_env(
    workdir: &Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<String, Error> {
    let out = run_checked_env(workdir, args, env)?;
    if out.success {
        Ok(out.stdout)
    } else {
        Err(Error::Git {
            args: args.iter().map(|s| s.to_string()).collect(),
            code: out.code,
            stderr: out.stderr,
        })
    }
}

/// 跑 git 并把 `input` 写入其 stdin;非零退出 → Err。供 `git apply` 等需喂 patch 的命令使用。
pub(crate) fn run_with_stdin(workdir: &Path, args: &[&str], input: &str) -> Result<String, Error> {
    let mut child = git_command()
        .args(args)
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // 块作用域内写完即 drop stdin,git 才会收到 EOF。
    {
        let mut stdin = child.stdin.take().expect("stdin piped");
        stdin.write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Error::Git {
            args: args.iter().map(|s| s.to_string()).collect(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// 取消令牌:UI 持一份并在用户请求时 `cancel()`,执行层在读循环里轮询 `is_cancelled()`。
/// `clone` 出的副本共享同一标志位。
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// 请求取消;正在执行的长操作会在下一次轮询时中止。
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// 是否已被请求取消。
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 长操作的一次进度事件(从 git stderr 解析而来)。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Progress {
    /// 阶段名,如 "Receiving objects"、"Resolving deltas"。
    pub phase: String,
    /// 百分比 0-100;解析不出则 None(raw 仍可直接展示)。
    pub percent: Option<u8>,
    /// 原始进度行,UI 可不解析直接显示。
    pub raw: String,
}

/// 流式跑 git:逐段读 stderr 解析进度经 `on_progress` 上报,段间轮询 `cancel`,
/// 置位则 kill 子进程并返回 `Error::Cancelled`。stdout 另起线程抽干,避免双管道互锁。
///
/// 仅供 fetch / push 等长操作使用;即时命令仍走 `run` / `run_checked`。
pub(crate) fn run_streaming(
    workdir: &Path,
    args: &[&str],
    on_progress: &mut dyn FnMut(Progress),
    cancel: &CancelToken,
) -> Result<Output, Error> {
    let mut child = git_command()
        .args(args)
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // stdout 必须并行抽干:两管道都写满而无人读会让子进程卡死。
    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let stdout_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout_pipe.read_to_string(&mut buf);
        buf
    });

    // stderr 也单独起线程读:阻塞读 stderr 不能拖住主线程的取消轮询
    // (fetch 卡在联网阶段时 stderr 长时间无输出,留在主线程会让取消失灵)。
    // 进度以 \r 刷新、\n 收尾,按这两个分隔逐段切,解析后经 channel 回传
    // (on_progress 不能跨线程);线程结束时返回完整 stderr 文本供错误信息复用。
    let mut stderr_pipe = BufReader::new(child.stderr.take().expect("stderr piped"));
    let (tx, rx) = std::sync::mpsc::channel::<Progress>();
    let stderr_handle = std::thread::spawn(move || {
        let mut stderr_buf = String::new();
        let mut seg = Vec::<u8>::new();
        let mut one = [0u8; 1];
        loop {
            match stderr_pipe.read(&mut one) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let b = one[0];
                    if b == b'\r' || b == b'\n' {
                        flush_segment(&mut seg, &mut stderr_buf, &tx);
                        stderr_buf.push(b as char);
                    } else {
                        seg.push(b);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        // 末段可能没有尾分隔符。
        flush_segment(&mut seg, &mut stderr_buf, &tx);
        stderr_buf
    });

    // 主线程只轮询取消(置位即 kill,不等阻塞读)+ 把进度交给 on_progress。
    let mut cancelled = false;
    loop {
        if cancel.is_cancelled() {
            cancelled = true;
            let _ = child.kill();
            break;
        }
        while let Ok(p) = rx.try_recv() {
            on_progress(p);
        }
        match child.try_wait() {
            Ok(Some(_)) => break, // 子进程已退出
            Ok(None) => {}
            Err(_) => break, // 罕见,交给后面的 wait() 收尾
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // 取消路径:git 主进程已 kill,但它 fork 的 helper(git-remote-https 等)
    // 可能仍持有 stderr/stdout 写端,join 读线程会一直阻塞。故收尸主进程后立即返回,
    // 不 join(两个读线程随管道最终 EOF 自行结束),确保取消即时生效不卡死。
    if cancelled {
        let _ = child.wait();
        return Err(Error::Cancelled);
    }

    // 正常结束:git 已退出,管道会 EOF,join 安全。
    let status = child.wait()?;
    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr_buf = stderr_handle.join().unwrap_or_default();
    // 线程已结束(tx 已 drop),抽干 channel 里残留的进度。
    while let Ok(p) = rx.try_recv() {
        on_progress(p);
    }
    Ok(Output {
        success: status.success(),
        code: status.code(),
        stdout,
        stderr: stderr_buf,
    })
}

// 把累积的一段 stderr 解析为进度并经 channel 发出;原文一并追加进 stderr_buf 供错误信息复用。
fn flush_segment(
    seg: &mut Vec<u8>,
    stderr_buf: &mut String,
    tx: &std::sync::mpsc::Sender<Progress>,
) {
    if seg.is_empty() {
        return;
    }
    let line = String::from_utf8_lossy(seg).into_owned();
    if let Some(p) = parse_progress(&line) {
        let _ = tx.send(p);
    }
    stderr_buf.push_str(&line);
    seg.clear();
}

/// 解析一行 git 进度(如 "Receiving objects:  45% (450/1000)")。尽力而为:
/// 需带 ':' 且其后含 '%' 才视为进度,借此过滤 "fatal:" / "hint:" 等普通 stderr。
fn parse_progress(line: &str) -> Option<Progress> {
    let line = line.trim();
    let (phase, rest) = line.split_once(':')?;
    let phase = phase.trim();
    if phase.is_empty() || !rest.contains('%') {
        return None;
    }
    Some(Progress {
        phase: phase.to_string(),
        percent: percent_before_sign(rest),
        raw: line.to_string(),
    })
}

// 取第一个 '%' 之前紧邻的连续数字作百分比(git 进度该处均为 ASCII)。
fn percent_before_sign(s: &str) -> Option<u8> {
    let before = s.split('%').next()?;
    let tail: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    tail.chars().rev().collect::<String>().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_progress_extracts_phase_and_percent() {
        let p = parse_progress("Receiving objects:  45% (450/1000), 1.2 MiB | 600 KiB/s").unwrap();
        assert_eq!(p.phase, "Receiving objects");
        assert_eq!(p.percent, Some(45));

        let p = parse_progress("Resolving deltas: 100% (300/300), done.").unwrap();
        assert_eq!(p.phase, "Resolving deltas");
        assert_eq!(p.percent, Some(100));
    }

    #[test]
    fn parse_progress_keeps_phase_when_percent_unparseable() {
        // 带阶段和 '%' 但 '%' 前无数字 → percent None,raw 仍保留。
        let p = parse_progress("Counting objects: % weird").unwrap();
        assert_eq!(p.phase, "Counting objects");
        assert_eq!(p.percent, None);
        assert_eq!(p.raw, "Counting objects: % weird");
    }

    #[test]
    fn parse_progress_ignores_non_progress_stderr() {
        assert!(parse_progress("fatal: not a git repository").is_none());
        assert!(parse_progress("hint: use --force to override").is_none());
        assert!(parse_progress("just some text").is_none());
        assert!(parse_progress("").is_none());
    }

    #[test]
    fn cancel_token_shares_flag_across_clones() {
        let a = CancelToken::default();
        let b = a.clone();
        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled(), "clone 应共享同一标志位");
    }
}

//! 端到端:真实 git → GitSail 可执行文件(askpass 客户端模式)→ AskpassServer → 答案回到 git。

use gui_lib::askpass::{AskpassEvent, AskpassServer};
use std::io::Write;
use std::process::{Command, Output, Stdio};

// 无 credential helper 时 git 只能经 askpass 取凭据。
fn credential_fill(server: &AskpassServer) -> Output {
    let mut child = Command::new("git")
        .args(["-c", "credential.helper=", "credential", "fill"])
        .envs(server.git_env(env!("CARGO_BIN_EXE_gui")))
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"protocol=https\nhost=example.com\n\n")
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn git_gets_credentials_through_gitsail_askpass() {
    let server = AskpassServer::start(|srv, ev| {
        if let AskpassEvent::Request(req) = ev {
            let answer = if req.prompt.starts_with("Username") {
                "alice"
            } else {
                "s3cret"
            };
            srv.respond(req.id, Some(answer.into()));
        }
    })
    .unwrap();

    let out = credential_fill(&server);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("username=alice"), "stdout: {stdout}");
    assert!(stdout.contains("password=s3cret"), "stdout: {stdout}");
}

#[test]
fn cancelling_the_prompt_makes_git_fail() {
    let server = AskpassServer::start(|srv, ev| {
        if let AskpassEvent::Request(req) = ev {
            srv.respond(req.id, None);
        }
    })
    .unwrap();

    let out = credential_fill(&server);
    assert!(!out.status.success());
    // git 把 askpass 的 stderr 原样带进自己的报错,gitcore 据此展示为友好提示。
    let err = gitcore::Error::Git {
        args: vec!["fetch".into()],
        code: out.status.code(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    };
    assert_eq!(err.to_string(), "已取消身份验证");
}

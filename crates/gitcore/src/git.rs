use crate::error::Error;
use std::path::Path;
use std::process::Command;

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
    let output = Command::new("git")
        .args(args)
        .current_dir(workdir)
        .output()?;
    Ok(Output {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

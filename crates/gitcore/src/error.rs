use std::fmt;

/// askpass 程序在用户取消凭据输入时写到 stderr 的标记行。git 会把 askpass 的 stderr
/// 原样带进自己的报错,gitcore 据此把这次失败展示为"已取消身份验证",而不是
/// `unable to read askpass response …` 这类技术原文。由宿主的 askpass 实现负责输出。
pub const AUTH_CANCELLED_MARKER: &str = "gitcore: authentication cancelled by user";

/// gitcore 的统一错误类型。
#[derive(Debug)]
pub enum Error {
    /// git 子命令以非零退出码结束。
    Git {
        args: Vec<String>,
        code: Option<i32>,
        stderr: String,
    },
    /// 无法启动 git 进程,或读取其输出失败。
    Io(std::io::Error),
    /// git 输出不符合预期格式。
    Parse(String),
    /// 预检条件不满足(如无 upstream、已有合并进行中)。
    Precondition(String),
    /// 长操作被用户取消(置位 CancelToken 后中止)。
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Git { stderr, .. } if stderr.contains(AUTH_CANCELLED_MARKER) => {
                write!(f, "已取消身份验证")
            }
            Error::Git { args, code, stderr } => write!(
                f,
                "git {} 失败(退出码 {}): {}",
                args.join(" "),
                code.map(|c| c.to_string())
                    .unwrap_or_else(|| "信号中断".into()),
                stderr.trim()
            ),
            Error::Io(e) => write!(f, "无法执行 git: {e}"),
            Error::Parse(s) => write!(f, "解析 git 输出失败: {s}"),
            Error::Precondition(s) => write!(f, "预检未通过: {s}"),
            Error::Cancelled => write!(f, "操作已取消"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_err(stderr: &str) -> Error {
        Error::Git {
            args: vec!["fetch".into()],
            code: Some(128),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn auth_cancel_marker_shows_friendly_message() {
        let stderr = format!(
            "{AUTH_CANCELLED_MARKER}\nerror: unable to read askpass response from '/x/GitSail'\n\
             fatal: could not read Username for 'https://example.com': terminal prompts disabled\n"
        );
        assert_eq!(git_err(&stderr).to_string(), "已取消身份验证");
    }

    #[test]
    fn other_git_failures_keep_original_text() {
        let e = git_err("fatal: Authentication failed for 'https://example.com/'\n");
        assert_eq!(
            e.to_string(),
            "git fetch 失败(退出码 128): fatal: Authentication failed for 'https://example.com/'"
        );
    }
}

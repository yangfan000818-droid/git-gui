use std::fmt;

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
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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

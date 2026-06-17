use crate::{Error, Repo};

/// diff 查询选项。
#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
    /// 查看暂存区 vs HEAD(默认查看工作区 vs 暂存区)。
    pub cached: bool,
    /// 只查看指定文件的 diff。
    pub path: Option<String>,
}

/// 获取 diff 输出。
pub(crate) fn diff(repo: &Repo, opts: &DiffOptions) -> Result<String, Error> {
    let mut args = vec!["diff"];
    if opts.cached {
        args.push("--cached");
    }
    if let Some(ref p) = opts.path {
        args.push("--");
        args.push(p);
    }
    repo.git(&args)
}

/// 查看指定提交的完整内容(message + diff)。
pub(crate) fn show_commit(repo: &Repo, sha: &str) -> Result<String, Error> {
    repo.git(&["show", sha])
}

/// 获取指定提交的完整消息(多行)。
pub(crate) fn commit_message(repo: &Repo, sha: &str) -> Result<String, Error> {
    repo.git(&["log", "-1", "--pretty=format:%B", sha])
}

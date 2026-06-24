//! 从远程 URL clone 仓库。区别于其它操作:目标工作区尚不存在,故为独立函数(不挂在 Repo 上)。

use crate::git::{self, CancelToken, Progress};
use crate::Error;
use std::path::{Path, PathBuf};

/// 从 URL 推断目标子目录名:取末段去掉 `.git`(支持 https 与 scp 形式)。
fn repo_name_from_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next().unwrap_or("");
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() {
        "repo".to_string()
    } else {
        name.to_string()
    }
}

/// 从远程 `url` clone 到 `parent_dir` 下(子目录名由 URL 推断),返回 clone 出的仓库完整路径。
///
/// 进度经 `on_progress` 上报(`git clone --progress` 把进度写 stderr),`cancel` 置位则中止
/// (kill git 子进程;已下载的部分目录由调用方决定是否清理)。目标目录已存在时 git 报错返回。
pub fn clone_streaming(
    url: &str,
    parent_dir: &Path,
    on_progress: &mut dyn FnMut(Progress),
    cancel: &CancelToken,
) -> Result<PathBuf, Error> {
    let name = repo_name_from_url(url);
    let target = parent_dir.join(&name);
    // clone 前目标是否已存在:已存在则即便失败也不能删(可能是用户原有目录)。
    let preexisting = target.exists();

    match git::run_streaming(
        parent_dir,
        &["clone", "--progress", url, &name],
        on_progress,
        cancel,
    ) {
        Ok(out) if out.success => Ok(target),
        Ok(out) => {
            cleanup_partial(&target, preexisting);
            Err(Error::Git {
                args: vec!["clone".to_string(), url.to_string()],
                code: out.code,
                stderr: out.stderr,
            })
        }
        // 取消(kill git)或无法启动 git:同样清理本次新建的残留。
        Err(e) => {
            cleanup_partial(&target, preexisting);
            Err(e)
        }
    }
}

/// 仅清理"本次 clone 新建且现在残留"的目标目录(取消/失败时调)。
/// `preexisting` 为真(clone 前目标已存在)则不删,避免误删用户原有的同名目录。
fn cleanup_partial(target: &Path, preexisting: bool) {
    if !preexisting && target.exists() {
        let _ = std::fs::remove_dir_all(target);
    }
}

#[cfg(test)]
mod tests {
    use super::{cleanup_partial, repo_name_from_url};

    #[test]
    fn name_from_https_with_dot_git() {
        assert_eq!(repo_name_from_url("https://github.com/user/repo.git"), "repo");
    }

    #[test]
    fn name_from_https_without_dot_git() {
        assert_eq!(repo_name_from_url("https://github.com/user/repo"), "repo");
    }

    #[test]
    fn name_from_scp_form() {
        assert_eq!(repo_name_from_url("git@github.com:user/repo.git"), "repo");
    }

    #[test]
    fn name_from_trailing_slash() {
        assert_eq!(repo_name_from_url("https://github.com/user/repo/"), "repo");
    }

    #[test]
    fn name_empty_url_falls_back() {
        assert_eq!(repo_name_from_url(""), "repo");
    }

    #[test]
    fn cleanup_removes_only_newly_created() {
        use std::fs;
        let base = std::env::temp_dir().join(format!(
            "gitcore-clone-cleanup-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let newly = base.join("newly");
        let existing = base.join("existing");
        fs::create_dir_all(&newly).unwrap();
        fs::create_dir_all(&existing).unwrap();

        // 本次新建(preexisting=false)的残留 → 清理。
        cleanup_partial(&newly, false);
        assert!(!newly.exists(), "本次新建的残留应被清理");

        // clone 前已存在(preexisting=true) → 保留,避免误删用户目录。
        cleanup_partial(&existing, true);
        assert!(existing.exists(), "clone 前已存在的目录不应被删");

        let _ = fs::remove_dir_all(&base);
    }
}

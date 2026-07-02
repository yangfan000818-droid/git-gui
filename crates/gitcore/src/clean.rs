use crate::{Error, Repo};
use std::path::PathBuf;

/// 预览 `git clean` 将删除的未跟踪文件(dry-run,不实际删除)。
///
/// `directories=true` 时加 `-d`(含未跟踪目录)。**不加 `-x`**:.gitignore
/// 忽略的文件(构建产物等)不在清理范围内,安全优先。
///
/// 强制 `LC_ALL=C` + `core.quotepath=false`:`git clean -n` 的 "Would remove "
/// 前缀是 gettext 本地化的(gettext 环境会译成"将删除"等),且非 ASCII 路径默认被
/// C-style 八进制转义;二者都会破坏下方按行解析。固定 locale + 关转义后输出稳定。
pub(crate) fn clean_preview(repo: &Repo, directories: bool) -> Result<Vec<PathBuf>, Error> {
    let mut args = vec!["-c", "core.quotepath=false", "clean", "-n"];
    if directories {
        args.push("-d");
    }
    let out = repo.git_env(&args, &[("LC_ALL", "C")])?;
    // 每行形如 "Would remove <path>"。
    Ok(out
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Would remove "))
        .map(|p| PathBuf::from(p.trim()))
        .collect())
}

/// 强制清理未跟踪文件,返回删除的文件数。`directories=true` 时含未跟踪目录。
pub(crate) fn clean_force(repo: &Repo, directories: bool) -> Result<usize, Error> {
    let mut args = vec!["-c", "core.quotepath=false", "clean", "-f"];
    if directories {
        args.push("-d");
    }
    let out = repo.git_env(&args, &[("LC_ALL", "C")])?;
    // 每行形如 "Removing <path>"。
    Ok(out
        .lines()
        .filter(|l| l.trim().starts_with("Removing "))
        .count())
}

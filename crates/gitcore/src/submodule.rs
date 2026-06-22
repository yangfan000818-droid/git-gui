use crate::{Error, Repo};
use std::path::{Path, PathBuf};

/// 子仓库信息。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Submodule {
    /// 名称（如 "vendor/dep"）。
    pub name: String,
    /// 路径（相对于主仓库根目录）。
    pub path: PathBuf,
    /// 当前状态。
    pub status: SubmoduleStatus,
}

/// 子仓库状态。
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum SubmoduleStatus {
    /// 干净，无改动。
    Clean,
    /// 有未提交改动。
    Dirty,
    /// HEAD detached。
    Detached,
    /// 未初始化（`.git` 不存在）。
    Uninitialized,
}

/// 列出所有子仓库。
pub(crate) fn list_submodules(repo: &Repo) -> Result<Vec<Submodule>, Error> {
    // 检查 .gitmodules 文件是否存在
    let gitmodules_path = repo.workdir().join(".gitmodules");
    if !gitmodules_path.exists() {
        return Ok(Vec::new());
    }

    // 使用 git submodule status 获取列表
    let output = repo.git(&["submodule", "status"])?;
    let mut submodules = Vec::new();

    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }

        // git submodule status 格式:
        // " <sha> <path> (<branch>)"  → 干净
        // "+<sha> <path> (<branch>)"  → 有新提交
        // "-<sha> <path>"             → 未初始化
        // "U<sha> <path>"             → 冲突
        let (status_char, rest) = if let Some(first) = line.chars().next() {
            (first, &line[1..])
        } else {
            continue;
        };

        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let path = PathBuf::from(parts[1]);
        let name = parts[1].to_string();

        // 判断状态
        let status = if status_char == '-' {
            SubmoduleStatus::Uninitialized
        } else {
            // 检查子仓库内部状态
            check_submodule_status(repo, &path)?
        };

        submodules.push(Submodule { name, path, status });
    }

    Ok(submodules)
}

/// 检查子仓库内部状态（dirty / detached / clean）。
fn check_submodule_status(repo: &Repo, submodule_path: &PathBuf) -> Result<SubmoduleStatus, Error> {
    let full_path = repo.workdir().join(submodule_path);

    // 检查是否已初始化
    if !full_path.join(".git").exists() {
        return Ok(SubmoduleStatus::Uninitialized);
    }

    // 打开子仓库
    let sub_repo = Repo::open(&full_path)?;

    // 检查 dirty
    let status = sub_repo.status()?;
    if status.dirty {
        return Ok(SubmoduleStatus::Dirty);
    }

    // 检查 detached HEAD
    if status.branch.is_none() {
        return Ok(SubmoduleStatus::Detached);
    }

    Ok(SubmoduleStatus::Clean)
}

/// 初始化并更新子仓库到父仓库记录的提交(git submodule update --init)。
pub(crate) fn update_submodule(repo: &Repo, path: &Path) -> Result<(), Error> {
    let p = path
        .to_str()
        .ok_or_else(|| Error::Parse("子仓库路径含非 UTF-8 字符".into()))?;
    repo.git(&["submodule", "update", "--init", "--", p])?;
    Ok(())
}

/// 同步子仓库的 URL 配置(git submodule sync)。
pub(crate) fn sync_submodule(repo: &Repo, path: &Path) -> Result<(), Error> {
    let p = path
        .to_str()
        .ok_or_else(|| Error::Parse("子仓库路径含非 UTF-8 字符".into()))?;
    repo.git(&["submodule", "sync", "--", p])?;
    Ok(())
}

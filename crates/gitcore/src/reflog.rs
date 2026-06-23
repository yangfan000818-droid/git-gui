//! reflog 查看(对标 WebStorm/IntelliJ 的 reflog)——HEAD 移动历史,
//! 是 reset / rebase 等改写操作的"安全网":搞砸了能从这里找回某个旧状态。

use crate::{Error, Repo};

/// 一条 HEAD reflog 记录。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ReflogEntry {
    /// 选择子,如 `HEAD@{0}`。
    pub selector: String,
    /// 短 SHA。
    pub sha: String,
    /// 完整 SHA(供"重置到此"定位)。
    pub full_sha: String,
    /// reflog 说明(`%gs`),如 `commit: 修复 X`、`rebase (finish): ...`、`reset: moving to ...`。
    pub action: String,
}

/// 取 HEAD reflog 的前 `max_count` 条(最新在前)。
pub(crate) fn reflog(repo: &Repo, max_count: usize) -> Result<Vec<ReflogEntry>, Error> {
    let n = max_count.to_string();
    let out = repo.git(&[
        "reflog",
        "show",
        "--format=%H%x00%h%x00%gd%x00%gs",
        "-n",
        &n,
    ])?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let p: Vec<&str> = line.split('\0').collect();
            (p.len() == 4).then(|| ReflogEntry {
                full_sha: p[0].to_string(),
                sha: p[1].to_string(),
                selector: p[2].to_string(),
                action: p[3].to_string(),
            })
        })
        .collect())
}

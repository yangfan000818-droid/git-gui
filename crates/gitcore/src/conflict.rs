use crate::{Error, Repo};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 一个冲突文件的类型,决定用哪种解决 UI。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum ConflictKind {
    /// 两边都改了内容(stage 1/2/3 齐全,文本) → 三栏逐块解决。
    BothModified,
    /// 本地改、对方删(stage 1+2,无 3) → 保留改动 / 接受删除。
    ModifyDelete,
    /// 对方改、本地删(stage 1+3,无 2) → 保留对方 / 接受删除。
    DeleteModify,
    /// 两边都新增同一路径(stage 2+3,无 base) → 三栏(base 段空)。
    AddAdd,
    /// 二进制内容冲突(任一 stage blob 含 NUL) → 取 ours / theirs 整份。
    Binary,
}

/// 一个冲突文件及其类型。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ConflictFile {
    pub path: PathBuf,
    pub kind: ConflictKind,
}

/// 一个冲突文件的三个版本(三路合并的输入)。
#[derive(Debug, Clone)]
pub struct ThreeVersions {
    /// 共同祖先(stage 1);新增-新增冲突时为 None。
    pub base: Option<String>,
    /// 当前分支版本(stage 2,ours)。
    pub ours: Option<String>,
    /// 合入分支版本(stage 3,theirs)。
    pub theirs: Option<String>,
}

/// 列出当前所有冲突文件。
pub fn conflicted_files(repo: &Repo) -> Result<Vec<PathBuf>, Error> {
    let out = repo.git(&["diff", "--name-only", "--diff-filter=U"])?;
    Ok(out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect())
}

/// 取某个冲突文件的三个版本原文;某阶段不存在则为 None。
///
/// 直接从暂存区的三个阶段取,而非解析文件里的 `<<<<<<<` 标记——
/// 这是三栏合并 UI 的数据来源。
pub fn three_versions(repo: &Repo, path: &Path) -> Result<ThreeVersions, Error> {
    let p = path.to_string_lossy();
    Ok(ThreeVersions {
        base: show_stage(repo, 1, &p)?,
        ours: show_stage(repo, 2, &p)?,
        theirs: show_stage(repo, 3, &p)?,
    })
}

fn show_stage(repo: &Repo, stage: u8, path: &str) -> Result<Option<String>, Error> {
    let out = repo.git_checked(&["show", &format!(":{stage}:{path}")])?;
    Ok(out.success.then_some(out.stdout))
}

/// 列出冲突文件并按类型分类(决定用哪种解决 UI)。
///
/// 用 `ls-files -u` 的 stage 集合判别类型,比解析文件标记可靠——modify/delete、
/// 二进制等没有 `<<<<<<<` 标记,只看标记会误判成"无冲突"。
pub fn classify_conflicts(repo: &Repo) -> Result<Vec<ConflictFile>, Error> {
    let out = repo.git(&["ls-files", "-u", "-z"])?;
    // path -> [stage1, stage2, stage3] 是否出现(1=base,2=ours,3=theirs)。
    let mut stages: BTreeMap<String, [bool; 3]> = BTreeMap::new();
    for entry in out.split('\0').filter(|e| !e.is_empty()) {
        // 每条形如 "<mode> <sha> <stage>\t<path>"。
        let Some((meta, path)) = entry.split_once('\t') else {
            continue;
        };
        let stage = meta
            .split_whitespace()
            .nth(2)
            .and_then(|s| s.parse::<usize>().ok());
        if let Some(st) = stage.filter(|s| (1..=3).contains(s)) {
            stages.entry(path.to_string()).or_default()[st - 1] = true;
        }
    }

    let mut files = Vec::new();
    for (path, [base, ours, theirs]) in stages {
        let kind = match (base, ours, theirs) {
            // 两边都在(both-modified 或 add/add):二进制单独成类,文本看有无 base 区分。
            (_, true, true) => {
                if is_binary_conflict(repo, &path)? {
                    ConflictKind::Binary
                } else if base {
                    ConflictKind::BothModified
                } else {
                    ConflictKind::AddAdd
                }
            }
            (true, true, false) => ConflictKind::ModifyDelete,
            (true, false, true) => ConflictKind::DeleteModify,
            // 罕见组合兜底走三栏(可能空块,但不至于漏报)。
            _ => ConflictKind::BothModified,
        };
        files.push(ConflictFile {
            path: PathBuf::from(path),
            kind,
        });
    }
    Ok(files)
}

/// 任一可用 stage blob 含 NUL 字节即判为二进制(git 同款启发式;
/// `git show` 输出经 from_utf8_lossy 后 NUL 仍保留)。
fn is_binary_conflict(repo: &Repo, path: &str) -> Result<bool, Error> {
    for stage in [2u8, 3, 1] {
        if let Some(blob) = show_stage(repo, stage, path)? {
            return Ok(blob.contains('\0'));
        }
    }
    Ok(false)
}

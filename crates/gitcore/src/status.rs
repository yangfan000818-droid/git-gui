use crate::{Error, Repo, Submodule};
use std::path::PathBuf;

/// 工作区某一刻的状态快照。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RepoStatus {
    /// 当前分支;detached HEAD 为 None。
    pub branch: Option<String>,
    /// upstream(如 origin/main);未设置为 None。
    pub upstream: Option<String>,
    /// 落后 upstream 的提交数。
    pub behind: u32,
    /// 领先 upstream 的提交数。
    pub ahead: u32,
    /// 是否有未提交改动(含 untracked)。
    pub dirty: bool,
    /// 当前冲突文件。
    pub conflicted: Vec<PathBuf>,
    /// 文件级状态(已暂存/已修改/未跟踪等)。
    pub files: Vec<FileStatus>,
    /// 子仓库列表。
    pub submodules: Vec<Submodule>,
}

/// 单个文件的状态。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct FileStatus {
    pub path: PathBuf,
    pub state: FileState,
}

/// 文件在工作区和暂存区的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum FileState {
    /// 已暂存(待提交)。
    Staged,
    /// 已修改未暂存。
    Modified,
    /// 未跟踪。
    Untracked,
    /// 同一文件既有暂存又有新改动。
    StagedAndModified,
}

pub(crate) fn status(repo: &Repo) -> Result<RepoStatus, Error> {
    let branch_raw = repo.git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch_raw = branch_raw.trim();
    let branch = (branch_raw != "HEAD").then(|| branch_raw.to_string());

    // 没有 upstream 时该命令非零退出,用 git_checked 容错。
    let up = repo.git_checked(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])?;
    let upstream = up.success.then(|| up.stdout.trim().to_string());

    let (behind, ahead) = if upstream.is_some() {
        parse_left_right(&repo.git(&["rev-list", "--left-right", "--count", "@{u}...HEAD"])?)?
    } else {
        (0, 0)
    };

    // quotepath=false:非 ASCII 路径不转义,保持原始 UTF-8,供前端按 path 懒加载 diff。
    // untracked-files=all:展开未跟踪目录到文件级,否则 git 默认把整个未跟踪目录折叠成
    // 一个 "dir/" 条目(尾斜杠),前端按 path 取 basename 会得到空名、且无法 diff/单独暂存。
    let porcelain = repo.git(&[
        "-c",
        "core.quotepath=false",
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
    ])?;
    let files = parse_porcelain(&porcelain);
    let dirty = !files.is_empty();
    // 冲突文件直接从已取到的 porcelain 解析(等价 `git diff --diff-filter=U`),
    // 省掉热路径上每仓一次 git 进程——status 在文件监视刷新里高频调用,Windows 上
    // 进程创建昂贵,这一省对刷新风暴下的卡顿有实质改善。
    let conflicted = parse_conflicted(&porcelain);
    let submodules = repo.submodules().unwrap_or_default();

    Ok(RepoStatus {
        branch,
        upstream,
        behind,
        ahead,
        dirty,
        conflicted,
        files,
        submodules,
    })
}

// 解析 "behind<TAB>ahead"(git rev-list --left-right --count @{u}...HEAD)。
fn parse_left_right(s: &str) -> Result<(u32, u32), Error> {
    let mut it = s.split_whitespace();
    match (
        it.next().and_then(|x| x.parse().ok()),
        it.next().and_then(|x| x.parse().ok()),
    ) {
        (Some(b), Some(a)) => Ok((b, a)),
        _ => Err(Error::Parse(format!("rev-list --count 输出异常: {s:?}"))),
    }
}

// 解析 git status --porcelain=v1 输出,每行格式: XY path
// X = 暂存区状态, Y = 工作区状态
// 例: "M " = 已暂存, " M" = 已修改, "MM" = 既暂存又修改, "??" = 未跟踪
fn parse_porcelain(output: &str) -> Vec<FileStatus> {
    output
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let x = line.chars().next()?;
            let y = line.chars().nth(1)?;
            // rename/copy 的 porcelain 是 "old -> new",取 new 作路径(与 git diff 一致)。
            let raw = line[3..].trim();
            let path = raw.rsplit(" -> ").next().unwrap_or(raw).to_string();

            let state = match (x, y) {
                ('?', '?') => FileState::Untracked,
                (' ', 'M') | (' ', 'D') => FileState::Modified,
                ('M', ' ') | ('A', ' ') | ('D', ' ') | ('R', ' ') | ('C', ' ') => FileState::Staged,
                ('M', 'M') | ('A', 'M') => FileState::StagedAndModified,
                _ => return None, // 忽略其他状态(如冲突标记 UU 等,已在 conflicted 字段)
            };

            Some(FileStatus {
                path: path.into(),
                state,
            })
        })
        .collect()
}

// 从同一份 porcelain 输出解析未合并(冲突)文件,等价于 `git diff --name-only --diff-filter=U`。
// 未合并状态码(XY): DD / AU / UD / UA / DU / AA / UU —— 任一侧为 U,或 both-added(AA)/
// both-deleted(DD)。冲突路径不会是 rename,raw 即路径。
fn parse_conflicted(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let x = line.chars().next()?;
            let y = line.chars().nth(1)?;
            let unmerged = matches!(
                (x, y),
                ('D', 'D')
                    | ('A', 'U')
                    | ('U', 'D')
                    | ('U', 'A')
                    | ('D', 'U')
                    | ('A', 'A')
                    | ('U', 'U')
            );
            unmerged.then(|| PathBuf::from(line[3..].trim()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_conflicted_picks_unmerged_only() {
        // 含全部 7 种未合并码 + 普通暂存/修改/未跟踪行,只应取未合并文件。
        let porcelain = "\
UU both-modified.txt
AA both-added.txt
DD both-deleted.txt
AU added-by-us.txt
UA added-by-them.txt
DU deleted-by-us.txt
UD deleted-by-them.txt
M  staged.txt
 M modified.txt
?? untracked.txt
MM staged-and-modified.txt";
        let got = parse_conflicted(porcelain);
        let want: Vec<PathBuf> = [
            "both-modified.txt",
            "both-added.txt",
            "both-deleted.txt",
            "added-by-us.txt",
            "added-by-them.txt",
            "deleted-by-us.txt",
            "deleted-by-them.txt",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn parse_conflicted_empty_when_no_conflicts() {
        assert!(parse_conflicted("M  a.txt\n M b.txt\n?? c.txt").is_empty());
        assert!(parse_conflicted("").is_empty());
    }
}

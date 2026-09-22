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
    // 一条 `git status --porcelain=v2 --branch` 同时拿到分支/upstream/ahead-behind/
    // 文件状态,替代原先 rev-parse×2 + rev-list + status 四次 fork。status 在文件监视
    // 刷新里高频调用,Windows 上进程创建昂贵,合并进程对刷新风暴下的卡顿改善显著。
    // -z:记录以 NUL 分隔且路径逐字输出。quotepath=false 只挡住非 ASCII 的转义,
    // 含 " \ TAB 换行的路径照样会被 C 风格加引号(实测 `"back\\slash.txt"`),
    // 且重命名记录的新旧路径在非 -z 下用 TAB 拼在一起,路径含 TAB 就解析错。
    // quotepath=false 保留作兜底(-z 万一未生效时仍不转义非 ASCII)。
    // untracked-files=all:展开未跟踪目录到文件级,否则 git 默认把整个未跟踪目录折叠成
    // 一个 "dir/" 条目,前端按 path 取 basename 会得到空名、且无法 diff/单独暂存。
    let porcelain = repo.git(&[
        "-c",
        "core.quotepath=false",
        "status",
        "--porcelain=v2",
        "--branch",
        "--untracked-files=all",
        "-z",
    ])?;
    let parsed = parse_v2(&porcelain);

    let submodules = repo.submodules().unwrap_or_default();

    Ok(RepoStatus {
        branch: parsed.branch,
        upstream: parsed.upstream,
        behind: parsed.behind,
        ahead: parsed.ahead,
        dirty: !parsed.files.is_empty(),
        conflicted: parsed.conflicted,
        files: parsed.files,
        submodules,
    })
}

/// `parse_v2` 的解析产物:branch/upstream/ahead/behind 来自 `# branch.*` header 行,
/// files/conflicted 来自条目行。
struct ParsedV2 {
    branch: Option<String>,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    files: Vec<FileStatus>,
    conflicted: Vec<PathBuf>,
}

/// 解析 `git status --porcelain=v2 --branch -z` 输出。
///
/// 记录以 NUL 分隔(`-z`),header 记录以 `# branch.*` 开头;条目首字符标识类型:
/// `1`=普通改动,`2`=重命名/复制,`u`=未合并(冲突),`?`=未跟踪,`!`=忽略。
/// porcelain v2 的 XY 状态码用 `.` 表示"无变化"(v1 用空格,二者不同)。
fn parse_v2(output: &str) -> ParsedV2 {
    let mut branch = None;
    let mut upstream = None;
    let mut ahead = 0;
    let mut behind = 0;
    let mut files = Vec::new();
    let mut conflicted = Vec::new();

    let mut records = output.split('\0');
    while let Some(line) = records.next() {
        if line.is_empty() {
            continue; // 末尾 NUL 切出的空串
        }
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            // detached HEAD 时 git 输出字面量 "(detached)"。
            branch = (rest != "(detached)").then(|| rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            upstream = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // 形如 "+3 -2";无 upstream 时该行不出现,ahead/behind 保持 0。
            let (a, b) = parse_ab(rest);
            ahead = a;
            behind = b;
        } else if let Some(rest) = line.strip_prefix("1 ") {
            // 普通改动: <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>,path 为第 7 字段(0 基)。
            if let Some((x, y, path)) = split_xy_path(rest, 7) {
                if let Some(state) = xy_to_state(x, y) {
                    files.push(FileStatus {
                        path: path.into(),
                        state,
                    });
                }
            }
        } else if let Some(rest) = line.strip_prefix("2 ") {
            // 重命名/复制: 比普通改动多一个 <Xscore> 字段,新路径为第 8 字段;
            // -z 下源路径是紧随其后的独立 NUL 字段,必须消费掉;不消费的话,
            // 源路径本身长得像一条记录时(如文件名以 "? " 开头)会被当成条目解析。
            records.next();
            if let Some((_, y, path)) = split_xy_path(rest, 8) {
                // rename/copy 总是已暂存;其后工作区又改动(Y=M)则同时未暂存。
                let state = if y == 'M' {
                    FileState::StagedAndModified
                } else {
                    FileState::Staged
                };
                files.push(FileStatus {
                    path: path.into(),
                    state,
                });
            }
        } else if let Some(rest) = line.strip_prefix("u ") {
            // 未合并(冲突): <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>,path 为第 9 字段。
            if let Some((_, _, path)) = split_xy_path(rest, 9) {
                conflicted.push(PathBuf::from(path));
            }
        } else if let Some(rest) = line.strip_prefix("? ") {
            files.push(FileStatus {
                path: PathBuf::from(rest),
                state: FileState::Untracked,
            });
        }
        // "! "(忽略)与其余 header 行(branch.oid 等):跳过。
    }

    ParsedV2 {
        branch,
        upstream,
        ahead,
        behind,
        files,
        conflicted,
    }
}

// 解析 "# branch.ab" 的计数部分(如 "+3 -2")→ (ahead, behind)。
fn parse_ab(s: &str) -> (u32, u32) {
    let mut ahead = 0;
    let mut behind = 0;
    for tok in s.split_whitespace() {
        if let Some(n) = tok.strip_prefix('+') {
            ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = tok.strip_prefix('-') {
            behind = n.parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

// 从 porcelain v2 条目行(已去掉首字符与其后空格)取 XY 状态码与 path。
// path_idx:path 所在字段序号(0 基,空格分隔);path 可能含空格,取该字段到行尾。
// path 字段为空则返回 None(异常行容错)。
fn split_xy_path(rest: &str, path_idx: usize) -> Option<(char, char, String)> {
    let mut parts = rest.splitn(path_idx + 1, ' ');
    let xy = parts.next()?;
    let mut xy_chars = xy.chars();
    let x = xy_chars.next()?;
    let y = xy_chars.next()?;
    // 跳过 XY 与 path 之间的中间字段(sub/mode/hash 等)。
    for _ in 1..path_idx {
        parts.next()?;
    }
    let path = parts.next()?.to_string();
    if path.is_empty() {
        return None;
    }
    Some((x, y, path))
}

// porcelain v2 XY 状态码 → FileState(X=暂存区,Y=工作区,'.'=无变化)。
// 未覆盖的组合返回 None(冲突走 `u` 行,不经此处)。
fn xy_to_state(x: char, y: char) -> Option<FileState> {
    match (x, y) {
        ('.', 'M') | ('.', 'D') => Some(FileState::Modified),
        ('M', '.') | ('A', '.') | ('D', '.') | ('R', '.') | ('C', '.') => Some(FileState::Staged),
        ('M', 'M') | ('A', 'M') => Some(FileState::StagedAndModified),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // 典型 porcelain v2 --branch -z 输出:记录以 NUL 分隔,
    // 重命名记录后紧跟一条独立的源路径记录。
    const SAMPLE: &str = concat!(
        "# branch.oid 1111111111111111111111111111111111111111\0",
        "# branch.head main\0",
        "# branch.upstream origin/main\0",
        "# branch.ab +2 -3\0",
        "1 M. N... 100644 100644 100644 aaa bbb staged.txt\0",
        "1 .M N... 100644 100644 100644 aaa bbb modified.txt\0",
        "1 MM N... 100644 100644 100644 aaa bbb both.txt\0",
        "1 .M N... 100644 100644 100644 aaa bbb with space.txt\0",
        // -z 下路径逐字输出:引号 / 反斜杠 / TAB 都不再被 C 风格转义。
        "1 .M N... 100644 100644 100644 aaa bbb has\"quote.txt\0",
        "1 .M N... 100644 100644 100644 aaa bbb tab\there.txt\0",
        "2 R. N... 100644 100644 100644 aaa bbb R100 new name.txt\0",
        "old name.txt\0",
        "u UU N... 100644 100644 100644 100644 aaa bbb ccc conflict.txt\0",
        "? untracked.txt\0",
        "! ignored.txt\0",
    );

    #[test]
    fn parse_v2_header_branch_and_ab() {
        let p = parse_v2(SAMPLE);
        assert_eq!(p.branch.as_deref(), Some("main"));
        assert_eq!(p.upstream.as_deref(), Some("origin/main"));
        assert_eq!(p.ahead, 2);
        assert_eq!(p.behind, 3);
    }

    #[test]
    fn parse_v2_files_and_states() {
        let p = parse_v2(SAMPLE);
        let find = |name: &str| {
            p.files
                .iter()
                .find(|f| f.path.as_path() == Path::new(name))
                .map(|f| f.state)
        };
        assert_eq!(find("staged.txt"), Some(FileState::Staged));
        assert_eq!(find("modified.txt"), Some(FileState::Modified));
        assert_eq!(find("both.txt"), Some(FileState::StagedAndModified));
        // path 含空格:取该字段到行尾。
        assert_eq!(find("with space.txt"), Some(FileState::Modified));
        // -z 下引号 / TAB 逐字保留,不再是 `"has\"quote.txt"` 这种转义串。
        assert_eq!(find("has\"quote.txt"), Some(FileState::Modified));
        assert_eq!(find("tab\there.txt"), Some(FileState::Modified));
        // rename 取新路径,源路径不作为独立文件。
        assert_eq!(find("new name.txt"), Some(FileState::Staged));
        assert!(find("old name.txt").is_none());
        assert_eq!(find("untracked.txt"), Some(FileState::Untracked));
        // 冲突与忽略文件不进 files。
        assert!(find("conflict.txt").is_none());
        assert!(find("ignored.txt").is_none());
    }

    #[test]
    fn parse_v2_conflicted_only_unmerged() {
        let p = parse_v2(SAMPLE);
        assert_eq!(p.conflicted, vec![PathBuf::from("conflict.txt")]);
    }

    // 回归:-z 下重命名的源路径是独立记录,必须被消费掉。源路径本身长得像一条
    // 记录时(这里是以 "? " 开头的文件名),不消费就会被当成一个未跟踪文件。
    #[test]
    fn parse_v2_rename_orig_path_is_consumed() {
        let out = concat!(
            "2 R. N... 100644 100644 100644 aaa bbb R100 renamed.txt\0",
            "? weird name.txt\0",
            "1 .M N... 100644 100644 100644 aaa bbb after.txt\0",
        );
        let p = parse_v2(out);
        let names: Vec<&Path> = p.files.iter().map(|f| f.path.as_path()).collect();
        assert_eq!(
            names,
            vec![Path::new("renamed.txt"), Path::new("after.txt")],
            "源路径不得被解析成未跟踪文件,其后的记录仍要正常解析"
        );
    }

    #[test]
    fn parse_v2_detached_head() {
        let p = parse_v2("# branch.oid abc\0# branch.head (detached)\0");
        assert_eq!(p.branch, None);
    }

    #[test]
    fn parse_v2_no_upstream_zero_ab() {
        // 无 upstream 时 git 不输出 branch.upstream / branch.ab 行。
        let out = concat!(
            "# branch.oid abc\0",
            "# branch.head main\0",
            "1 .M N... 100644 100644 100644 aaa bbb f.txt\0",
        );
        let p = parse_v2(out);
        assert_eq!(p.branch.as_deref(), Some("main"));
        assert_eq!(p.upstream, None);
        assert_eq!(p.ahead, 0);
        assert_eq!(p.behind, 0);
    }

    #[test]
    fn parse_ab_extracts_ahead_behind() {
        assert_eq!(parse_ab("+5 -0"), (5, 0));
        assert_eq!(parse_ab("+0 -7"), (0, 7));
        assert_eq!(parse_ab("+12 -34"), (12, 34));
    }
}

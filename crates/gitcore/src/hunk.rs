//! 结构化 diff 解析 + 逐 hunk 暂存/取消。
//!
//! 把 `git diff` 的 unified 输出解析为 文件 → hunk → 行 结构(供 UI 着色展示),
//! 并保留每块的原始 patch 文本。暂存某个 hunk = 截取"文件头 + 该 hunk"原样喂给
//! `git apply --cached`(取消则加 `-R`);基准始终是当前 index,故行号精确、无需重算。

use crate::{Error, Repo};

/// 一行 diff 的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// diff 中的一行(已去掉前导 ` `/`+`/`-` 标记)。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiffLine {
    pub kind: LineKind,
    pub content: String,
}

/// 一个改动块。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Hunk {
    /// `@@` 行里旧文件起始行号。
    pub old_start: u32,
    /// `@@` 行里新文件起始行号。
    pub new_start: u32,
    /// `@@ ... @@` 之后的区段标题(常是所属函数名),可能为空。
    pub heading: String,
    pub lines: Vec<DiffLine>,
    /// 原始文本(从 `@@` 行到该 hunk 结束),重建 patch 用。
    raw: String,
}

/// 一个文件的全部改动。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FileDiff {
    /// 展示用路径(新路径;删除时为旧路径)。
    pub path: String,
    /// 二进制文件(无法按行 diff)。
    pub binary: bool,
    pub hunks: Vec<Hunk>,
    /// 文件头原始文本(`diff --git` 到第一个 `@@` 之前),重建 patch 用。
    header_raw: String,
}

/// 解析 `git diff` 的 unified 输出为按文件分组的结构。
pub(crate) fn parse(diff_text: &str) -> Vec<FileDiff> {
    let lines: Vec<&str> = diff_text.split_inclusive('\n').collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("diff --git ") {
            let (file, next) = parse_file(&lines, i);
            files.push(file);
            i = next;
        } else {
            i += 1;
        }
    }
    files
}

fn parse_file(lines: &[&str], start: usize) -> (FileDiff, usize) {
    let mut i = start;
    let mut header = String::new();
    let mut old_path = String::new();
    let mut new_path = String::new();
    let mut binary = false;

    // 文件头:从 "diff --git" 到第一个 "@@" 或下一个 "diff --git"。
    while i < lines.len() {
        let l = lines[i];
        if i > start && l.starts_with("diff --git ") {
            break;
        }
        if l.starts_with("@@") {
            break;
        }
        if l.starts_with("Binary files ") {
            binary = true;
        } else if let Some(p) = l.strip_prefix("--- ") {
            old_path = clean_path(p);
        } else if let Some(p) = l.strip_prefix("+++ ") {
            new_path = clean_path(p);
        }
        header.push_str(l);
        i += 1;
    }

    let path = if !new_path.is_empty() && new_path != "/dev/null" {
        new_path
    } else if !old_path.is_empty() && old_path != "/dev/null" {
        old_path
    } else {
        parse_path_from_git_line(lines[start]).unwrap_or_default()
    };

    // hunks。
    let mut hunks = Vec::new();
    while i < lines.len() {
        let l = lines[i];
        if l.starts_with("diff --git ") {
            break;
        }
        if l.starts_with("@@") {
            let (hunk, next) = parse_hunk(lines, i);
            hunks.push(hunk);
            i = next;
        } else {
            i += 1;
        }
    }

    (
        FileDiff {
            path,
            binary,
            hunks,
            header_raw: header,
        },
        i,
    )
}

fn parse_hunk(lines: &[&str], start: usize) -> (Hunk, usize) {
    let header_line = lines[start];
    let (old_start, new_start, heading) = parse_hunk_header(header_line);
    let mut raw = String::from(header_line);
    let mut diff_lines = Vec::new();
    let mut i = start + 1;
    while i < lines.len() {
        let l = lines[i];
        if l.starts_with("@@") || l.starts_with("diff --git ") {
            break;
        }
        match l.chars().next() {
            Some('+') => diff_lines.push(DiffLine {
                kind: LineKind::Added,
                content: line_body(l),
            }),
            Some('-') => diff_lines.push(DiffLine {
                kind: LineKind::Removed,
                content: line_body(l),
            }),
            Some(' ') => diff_lines.push(DiffLine {
                kind: LineKind::Context,
                content: line_body(l),
            }),
            // "\ No newline at end of file":保留进 raw(patch 需要),不作展示行。
            Some('\\') => {}
            // 其他(空行等)视为 hunk 结束。
            _ => break,
        }
        raw.push_str(l);
        i += 1;
    }
    (
        Hunk {
            old_start,
            new_start,
            heading,
            lines: diff_lines,
            raw,
        },
        i,
    )
}

// "@@ -A,B +C,D @@ heading" → (A, C, heading)。
fn parse_hunk_header(l: &str) -> (u32, u32, String) {
    let l = l.trim_end();
    let mut old_start = 0;
    let mut new_start = 0;
    let mut heading = String::new();
    if let Some(rest) = l.strip_prefix("@@ ") {
        if let Some((ranges, tail)) = rest.split_once(" @@") {
            let mut it = ranges.split_whitespace();
            if let Some(old) = it.next() {
                old_start = first_number(old.trim_start_matches('-'));
            }
            if let Some(new) = it.next() {
                new_start = first_number(new.trim_start_matches('+'));
            }
            heading = tail.trim().to_string();
        }
    }
    (old_start, new_start, heading)
}

// 取 "A,B" 或 "A" 形式里的 A。
fn first_number(s: &str) -> u32 {
    s.split(',')
        .next()
        .and_then(|x| x.parse().ok())
        .unwrap_or(0)
}

// 去掉行尾换行与首个标记字符,得到纯内容。
fn line_body(l: &str) -> String {
    let s = l.strip_suffix('\n').unwrap_or(l);
    s.get(1..).unwrap_or("").to_string()
}

// 去掉 "a/"/"b/" 前缀和行尾换行;"/dev/null" 原样返回。
fn clean_path(s: &str) -> String {
    let s = unquote_c_style(s.trim_end());
    if s == "/dev/null" {
        return s;
    }
    s.strip_prefix("a/")
        .or_else(|| s.strip_prefix("b/"))
        .unwrap_or(&s)
        .to_string()
}

// 兜底:从 "diff --git a/X b/X" 提取路径(取 b/ 一侧)。
fn parse_path_from_git_line(l: &str) -> Option<String> {
    let rest = l.trim_end().strip_prefix("diff --git ")?;
    // 带引号形如 `"a/path" "b/path"`(rename 时可能只有一侧带引号)。
    if let Some(idx) = rest.rfind(" \"b/") {
        return Some(clean_path(&rest[idx + 1..]));
    }
    // 形如 "a/path b/path";取后半。
    let idx = rest.find(" b/")?;
    Some(rest[idx + 3..].to_string())
}

// 还原 git 的 C 风格引号路径(`"a/\344\270\255.txt"` → `a/中.txt`)。
// 路径含 `"` `\` 控制字符(core.quotepath=true 时还有非 ASCII)时,git 会把整个路径
// 加引号并转义;quotepath=false 挡不住前几类。未加引号的原样返回。
fn unquote_c_style(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) else {
        return s.to_string();
    };
    let b = inner.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        // git 对 >= 0x80 的字节与控制字符用三位八进制 `\ooo`(首位 0-3)。
        let octal = b.get(i + 1..i + 4).filter(|d| {
            (b'0'..=b'3').contains(&d[0]) && d[1..].iter().all(|c| (b'0'..=b'7').contains(c))
        });
        if let Some(d) = octal {
            out.push((d[0] - b'0') * 64 + (d[1] - b'0') * 8 + (d[2] - b'0'));
            i += 4;
            continue;
        }
        out.push(match b[i + 1] {
            b'a' => 0x07,
            b'b' => 0x08,
            b't' => b'\t',
            b'n' => b'\n',
            b'v' => 0x0b,
            b'f' => 0x0c,
            b'r' => b'\r',
            other => other, // `\"` 与 `\\`
        });
        i += 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 暂存某文件的某个 hunk(基于 unstaged diff)。
pub(crate) fn stage_hunk(repo: &Repo, file: &FileDiff, hunk: &Hunk) -> Result<(), Error> {
    apply_cached(repo, &patch_of(file, hunk), false)
}

/// 取消暂存某文件的某个 hunk(基于 staged diff)。
pub(crate) fn unstage_hunk(repo: &Repo, file: &FileDiff, hunk: &Hunk) -> Result<(), Error> {
    apply_cached(repo, &patch_of(file, hunk), true)
}

fn patch_of(file: &FileDiff, hunk: &Hunk) -> String {
    let mut p = file.header_raw.clone();
    p.push_str(&hunk.raw);
    if !p.ends_with('\n') {
        p.push('\n');
    }
    p
}

fn apply_cached(repo: &Repo, patch: &str, reverse: bool) -> Result<(), Error> {
    let mut args = vec!["apply", "--cached"];
    if reverse {
        args.push("-R");
    }
    repo.git_with_stdin(&args, patch)?;
    Ok(())
}

/// 解析某个 commit 的改动为结构化 diff(按文件;相对其第一个父,根提交则为全量新增)。
/// 复用 unified diff 解析器;merge 提交的 combined diff 不在此处特殊处理。
pub(crate) fn commit_files(repo: &Repo, sha: &str) -> Result<Vec<FileDiff>, Error> {
    let text = repo.git(&[
        "-c",
        "diff.noprefix=false",
        "-c",
        "diff.mnemonicprefix=false",
        "show",
        "--no-color",
        "--format=",
        sha,
    ])?;
    Ok(parse(&text))
}

/// 获取某提交中单个文件的 diff(0 或 1 个 FileDiff)。
/// 改名前匹配不到 → None;该提交未改动此文件 → None。
pub(crate) fn commit_file_diff(
    repo: &Repo,
    sha: &str,
    file_path: &std::path::Path,
) -> Result<Option<FileDiff>, Error> {
    let path_str = file_path.to_string_lossy().to_string();
    let text = repo.git(&[
        "-c",
        "diff.noprefix=false",
        "-c",
        "diff.mnemonicprefix=false",
        "show",
        "--no-color",
        "--format=",
        sha,
        "--",
        &path_str,
    ])?;
    Ok(parse(&text).into_iter().next())
}

/// 某个 ref(分支/标签/commit)与当前工作区的差异(已跟踪文件;不含未跟踪)。
/// `git diff <rev>`:rev 为旧、工作区为新 → 新增行 = 工作区相对该 ref 多出的内容。
pub(crate) fn diff_with_workdir(repo: &Repo, rev: &str) -> Result<Vec<FileDiff>, Error> {
    let text = repo.git(&[
        "-c",
        "diff.noprefix=false",
        "-c",
        "diff.mnemonicprefix=false",
        "diff",
        "--no-color",
        "--no-ext-diff",
        rev,
    ])?;
    Ok(parse(&text))
}

/// 把一个未跟踪文件构造成"全新增"的 FileDiff(`git diff` 不含未跟踪,需单独补)。
/// 生成的 patch 是标准 new-file 格式,`git apply --cached` 可直接当 `git add` 用。
pub(crate) fn untracked_file(repo: &Repo, path: &str) -> Option<FileDiff> {
    let content = std::fs::read(repo.workdir().join(path)).ok()?;
    let header_raw = format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n"
    );

    // 二进制(含 NUL 字节)或空文件:无按行 hunk。
    if content.contains(&0) {
        return Some(FileDiff {
            path: path.to_string(),
            binary: true,
            hunks: Vec::new(),
            header_raw,
        });
    }
    let text = String::from_utf8_lossy(&content);
    if text.is_empty() {
        return Some(FileDiff {
            path: path.to_string(),
            binary: false,
            hunks: Vec::new(),
            header_raw,
        });
    }

    let ends_with_newline = text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();
    let n = lines.len();
    let mut raw = format!("@@ -0,0 +1,{n} @@\n");
    let mut diff_lines = Vec::with_capacity(n);
    for line in &lines {
        raw.push('+');
        raw.push_str(line);
        raw.push('\n');
        diff_lines.push(DiffLine {
            kind: LineKind::Added,
            content: line.to_string(),
        });
    }
    if !ends_with_newline {
        raw.push_str("\\ No newline at end of file\n");
    }

    Some(FileDiff {
        path: path.to_string(),
        binary: false,
        hunks: vec![Hunk {
            old_start: 0,
            new_start: 1,
            heading: String::new(),
            lines: diff_lines,
            raw,
        }],
        header_raw,
    })
}

/// 暂存某文件某 hunk 中"选中的行"(`selected` 为 `hunk.lines` 的下标,仅 +/- 行有意义)。
pub(crate) fn stage_lines(
    repo: &Repo,
    file: &FileDiff,
    hunk: &Hunk,
    selected: &[usize],
) -> Result<(), Error> {
    if selected.is_empty() {
        return Ok(());
    }
    apply_cached(repo, &partial_patch(file, hunk, selected), false)
}

/// 取消暂存某文件某 hunk 中"选中的行"(`file`/`hunk` 取自 staged_diff)。
pub(crate) fn unstage_lines(
    repo: &Repo,
    file: &FileDiff,
    hunk: &Hunk,
    selected: &[usize],
) -> Result<(), Error> {
    if selected.is_empty() {
        return Ok(());
    }
    apply_cached(repo, &partial_patch(file, hunk, selected), true)
}

/// 构造只含选中行的 patch:
/// 未选中的新增行丢弃(不进 index),未选中的删除行转为上下文(该行仍在 index),据此重算行数。
fn partial_patch(file: &FileDiff, hunk: &Hunk, selected: &[usize]) -> String {
    let sel: std::collections::HashSet<usize> = selected.iter().copied().collect();
    let mut body = String::new();
    let mut old_count = 0u32;
    let mut new_count = 0u32;
    for (i, line) in hunk.lines.iter().enumerate() {
        match line.kind {
            LineKind::Context => {
                push_line(&mut body, ' ', &line.content);
                old_count += 1;
                new_count += 1;
            }
            LineKind::Added => {
                if sel.contains(&i) {
                    push_line(&mut body, '+', &line.content);
                    new_count += 1;
                }
                // 未选中的新增:丢弃。
            }
            LineKind::Removed => {
                if sel.contains(&i) {
                    push_line(&mut body, '-', &line.content);
                    old_count += 1;
                } else {
                    // 未选中的删除:该行仍在 index,作为上下文保留。
                    push_line(&mut body, ' ', &line.content);
                    old_count += 1;
                    new_count += 1;
                }
            }
        }
    }
    let header = format!(
        "@@ -{},{} +{},{} @@\n",
        hunk.old_start, old_count, hunk.new_start, new_count
    );
    let mut patch = file.header_raw.clone();
    patch.push_str(&header);
    patch.push_str(&body);
    patch
}

fn push_line(buf: &mut String, marker: char, content: &str) {
    buf.push(marker);
    buf.push_str(content);
    buf.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    // 回归:路径被 git 加引号转义时,必须还原成真实路径,而不是 `"b/\344..."` 字面量。
    #[test]
    fn parse_unquotes_c_style_paths() {
        let sample = concat!(
            "diff --git \"a/\\344\\270\\255.txt\" \"b/\\344\\270\\255.txt\"\n",
            "index e69de29..d95f3ad 100644\n",
            "--- \"a/\\344\\270\\255.txt\"\n",
            "+++ \"b/\\344\\270\\255.txt\"\n",
            "@@ -1 +1 @@\n",
            "-a\n",
            "+b\n",
            "diff --git \"a/has\\\"quote\\\\back\\ttab.txt\" \"b/has\\\"quote\\\\back\\ttab.txt\"\n",
            "new file mode 100644\n",
            "--- /dev/null\n",
            "+++ \"b/has\\\"quote\\\\back\\ttab.txt\"\n",
            "@@ -0,0 +1 @@\n",
            "+x\n",
            // 二进制文件没有 ---/+++ 行,只能从 diff --git 行取路径。
            "diff --git \"a/\\344\\270\\255.bin\" \"b/\\344\\270\\255.bin\"\n",
            "index e69de29..d95f3ad 100644\n",
            "Binary files \"a/\\344\\270\\255.bin\" and \"b/\\344\\270\\255.bin\" differ\n",
        );
        let paths: Vec<String> = parse(sample).into_iter().map(|f| f.path).collect();
        assert_eq!(
            paths,
            vec!["中.txt", "has\"quote\\back\ttab.txt", "中.bin"],
            "引号路径应还原为真实路径"
        );
    }

    #[test]
    fn unquote_c_style_leaves_plain_paths_untouched() {
        assert_eq!(unquote_c_style("a/plain.txt"), "a/plain.txt");
        assert_eq!(unquote_c_style("a/中文.txt"), "a/中文.txt");
        assert_eq!(unquote_c_style("/dev/null"), "/dev/null");
    }

    #[test]
    fn parses_single_file_single_hunk() {
        let sample = "\
diff --git a/foo.txt b/foo.txt
index e69de29..d95f3ad 100644
--- a/foo.txt
+++ b/foo.txt
@@ -1,2 +1,3 @@ fn main
 line1
-old
+new
+added
";
        let files = parse(sample);
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.path, "foo.txt");
        assert!(!f.binary);
        assert_eq!(f.hunks.len(), 1);

        let h = &f.hunks[0];
        assert_eq!(h.old_start, 1);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.heading, "fn main");
        assert_eq!(h.lines.len(), 4);
        assert_eq!(h.lines[0].kind, LineKind::Context);
        assert_eq!(h.lines[0].content, "line1");
        assert_eq!(h.lines[1].kind, LineKind::Removed);
        assert_eq!(h.lines[1].content, "old");
        assert_eq!(h.lines[2].kind, LineKind::Added);
        assert_eq!(h.lines[2].content, "new");
        assert_eq!(h.lines[3].content, "added");
    }

    #[test]
    fn parses_multiple_hunks_and_files() {
        let text = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-a
+b
@@ -10 +10 @@
-c
+d
diff --git a/b.txt b/b.txt
--- a/b.txt
+++ b/b.txt
@@ -1 +1 @@
-x
+y
";
        let files = parse(text);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "a.txt");
        assert_eq!(files[0].hunks.len(), 2);
        assert_eq!(files[1].path, "b.txt");
        assert_eq!(files[1].hunks.len(), 1);
    }

    #[test]
    fn detects_binary_file() {
        let text = "\
diff --git a/img.png b/img.png
index abc..def 100644
Binary files a/img.png and b/img.png differ
";
        let files = parse(text);
        assert_eq!(files.len(), 1);
        assert!(files[0].binary);
        assert!(files[0].hunks.is_empty());
    }

    #[test]
    fn parses_new_file_path_from_dev_null() {
        let text = "\
diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..abc
--- /dev/null
+++ b/new.txt
@@ -0,0 +1 @@
+hello
";
        let files = parse(text);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "new.txt");
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[0].hunks[0].lines[0].kind, LineKind::Added);
    }
}

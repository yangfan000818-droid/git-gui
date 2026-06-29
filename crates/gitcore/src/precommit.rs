//! 提交前检查:扫描暂存内容,检测常见提交事故(敏感信息 / 冲突标记 / 大文件 /
//! 调试残留 / TODO),供前端在提交前提示。纯 git 内容扫描,**零依赖**(不用 regex)。
//!
//! 检查对象 = 本次将要提交的**新增行**(staged diff 的 `+` 行),以减少误报;
//! 大文件 / `.env` 文件按整文件告警。

use std::path::Path;

use crate::{Error, FileDiff, LineKind, Repo};

/// 暂存文件大小的告警阈值:超过视为大文件(对标常见 .gitignore 忽略下限)。
const LARGE_FILE_BYTES: u64 = 1024 * 1024;

/// 一条警告的类别。默认 serde 序列化为变体名(前端按字符串匹配)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum WarningKind {
    /// 敏感信息泄露(`.env` 文件 / 含密钥模式的新增行)。
    SensitiveInfo,
    /// 冲突标记残留(`<<<<<<< ======= >>>>>>>` 没清干净)。
    ConflictMarker,
    /// 暂存的大文件(>1MB)。
    LargeFile,
    /// 调试残留(`console.log` / `debugger` / `print!` 等)。
    DebugResidue,
    /// `TODO` / `FIXME` 等待办标记。
    Todo,
}

/// 一条提交前警告。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PrecommitWarning {
    pub kind: WarningKind,
    pub file: String,
    /// 整文件警告(大文件 / `.env`)为 None;行级警告有新文件行号。
    pub line: Option<u32>,
    /// 可读说明(命中的关键词 / 文件大小等),不含实际密钥值。
    pub detail: String,
}

/// 检查结果。
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PrecommitReport {
    pub warnings: Vec<PrecommitWarning>,
}

/// 敏感信息 key 词(小写匹配)。词后跟 `=` / `:` 且有值即视为疑似密钥。
const SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "client_secret",
    "access_token",
    "auth_token",
    "aws_secret",
];

/// 调试残留关键词(子串匹配,小写)。
const DEBUG_MARKERS: &[&str] = &[
    "console.log",
    "console.debug",
    "console.warn",
    "console.info",
    "console.error",
    "debugger",
    "print!",
    "eprintln!",
    "println!",
    "dbg!",
];

/// TODO 类关键词:需作为独立 token 出现(避免误报含子串的普通词,如 "hacking")。
const TODO_MARKERS: &[&str] = &["todo", "fixme", "xxx", "hack"];

/// 对一行新增内容做行级检查,返回命中的(类别, detail)。纯 str 操作。
fn classify_added_line(content: &str) -> Vec<(WarningKind, String)> {
    let mut hits = Vec::new();
    let lower = content.to_ascii_lowercase();
    let trimmed = content.trim();

    // 冲突标记(整行精确匹配)。
    if trimmed.starts_with("<<<<<<< ") || trimmed == "=======" || trimmed.starts_with(">>>>>>> ") {
        hits.push((WarningKind::ConflictMarker, "冲突标记残留".to_string()));
    }

    // 敏感信息:key 词 + 后随 `=` / `:` 且有值。
    for key in SECRET_KEYS {
        if has_secret(content, key) {
            hits.push((WarningKind::SensitiveInfo, format!("疑似密钥:{key}")));
        }
    }

    // 调试残留(子串)。
    for marker in DEBUG_MARKERS {
        if lower.contains(marker) {
            hits.push((WarningKind::DebugResidue, format!("含 {marker}")));
        }
    }

    // TODO / FIXME(词边界,避免误报)。
    for marker in TODO_MARKERS {
        if contains_word(content, marker) {
            hits.push((WarningKind::Todo, format!("含 {marker}")));
        }
    }

    hits
}

/// `b` 是否为"词字符"(字母 / 数字 / `_` / `-`),用于判定词边界。
fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// 在 `content`(自动小写)中查找 `key`,命中条件:key 前是词边界,且 key 后(跳过空格)
/// 紧跟 `=` 或 `:`,再去掉首尾引号后仍有内容。不回显实际值。
fn has_secret(content: &str, key: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let kb = key.len();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(key) {
        let abs = from + rel;
        let prev_ok = abs == 0 || !is_word_char(bytes[abs - 1]);
        let after = lower[abs + kb..].trim_start();
        let has_assign = after.starts_with('=') || after.starts_with(':');
        // 去掉首尾引号后判空:`password = ""` 不算,`password = "x"` 算。
        let val = after
            .get(1..)
            .map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`'))
            .unwrap_or("");
        if prev_ok && has_assign && !val.is_empty() {
            return true;
        }
        from = abs + kb;
    }
    false
}

/// 在 `content`(自动小写)中按词边界查找 `needle`(前后均非词字符)。
fn contains_word(content: &str, needle: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let nb = needle.len();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(needle) {
        let abs = from + rel;
        let prev_ok = abs == 0 || !is_word_char(bytes[abs - 1]);
        let next = abs + nb;
        let next_ok = next >= bytes.len() || !is_word_char(bytes[next]);
        if prev_ok && next_ok {
            return true;
        }
        from = abs + 1;
    }
    false
}

/// 路径是否为 `.env` 系列环境变量文件(`.env` / `.env.local` / `.env.production` 等)。
fn is_env_file(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    name == ".env" || name.starts_with(".env.")
}

/// 对单个仓库跑提交前检查。
pub(crate) fn check(repo: &Repo) -> Result<PrecommitReport, Error> {
    let files: Vec<FileDiff> = repo.staged_diff()?;
    let workdir = repo.workdir();
    let mut warnings = Vec::new();

    for f in &files {
        // `.env` 文件:整文件告警(通常含密钥不应入库)。
        if is_env_file(&f.path) {
            warnings.push(PrecommitWarning {
                kind: WarningKind::SensitiveInfo,
                file: f.path.clone(),
                line: None,
                detail: "环境变量文件,通常含密钥不应入库".to_string(),
            });
        }

        // 大文件(工作区文件大小 > 阈值;binary 产物同样查;纯删除无工作区文件则跳过)。
        if let Ok(meta) = std::fs::metadata(workdir.join(&f.path)) {
            if meta.len() > LARGE_FILE_BYTES {
                warnings.push(PrecommitWarning {
                    kind: WarningKind::LargeFile,
                    file: f.path.clone(),
                    line: None,
                    detail: format!("{} KB(>1MB)", meta.len() / 1024),
                });
            }
        }

        // 行级扫描:只查新增行(新增内容),维护新文件行号。
        for hunk in &f.hunks {
            let mut new_line = hunk.new_start;
            for line in &hunk.lines {
                match line.kind {
                    LineKind::Added => {
                        for (kind, detail) in classify_added_line(&line.content) {
                            warnings.push(PrecommitWarning {
                                kind,
                                file: f.path.clone(),
                                line: Some(new_line),
                                detail,
                            });
                        }
                        new_line = new_line.saturating_add(1);
                    }
                    LineKind::Context => {
                        new_line = new_line.saturating_add(1);
                    }
                    LineKind::Removed => {}
                }
            }
        }
    }

    Ok(PrecommitReport { warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── has_secret ──
    #[test]
    fn secret_password_assignment() {
        assert!(has_secret("const password = \"abc\"", "password"));
        assert!(has_secret("  PASSWORD: xyz", "password")); // 小写化后命中
    }
    #[test]
    fn secret_needs_assign_and_value() {
        // 只有 key 没赋值 → 不算
        assert!(!has_secret("const password = \"\"", "password"));
        // key 后跟别的词 → 不算
        assert!(!has_secret("password policy", "password"));
    }
    #[test]
    fn secret_word_boundary() {
        // "mypassword" 是另一词,不算 "password" 命中
        assert!(!has_secret("const mypassword = \"x\"", "password"));
        assert!(has_secret("api_key=sk-xxx", "api_key"));
    }

    // ── contains_word ──
    #[test]
    fn todo_word_boundary() {
        assert!(contains_word("// TODO: fix later", "todo"));
        assert!(contains_word("# FIXME here", "fixme"));
        assert!(!contains_word("// hacking around", "hack"));
        assert!(!contains_word("syntax error", "xxx"));
    }

    // ── is_env_file ──
    #[test]
    fn env_file_detection() {
        assert!(is_env_file(".env"));
        assert!(is_env_file(".env.local"));
        assert!(is_env_file(".env.production"));
        assert!(is_env_file("config/.env"));
        assert!(!is_env_file("environment.ts"));
        assert!(!is_env_file("env.js"));
        assert!(!is_env_file("src/main.rs"));
    }

    // ── classify_added_line ──
    #[test]
    fn classify_conflict_markers() {
        let kinds = |c: &str| {
            classify_added_line(c)
                .into_iter()
                .map(|(k, _)| k)
                .collect::<Vec<_>>()
        };
        assert!(kinds("<<<<<<< HEAD").contains(&WarningKind::ConflictMarker));
        assert!(kinds("=======").contains(&WarningKind::ConflictMarker));
        assert!(kinds(">>>>>>> feature/x").contains(&WarningKind::ConflictMarker));
    }
    #[test]
    fn classify_debug_residue() {
        assert!(classify_added_line("console.log('hi')")
            .iter()
            .any(|(k, _)| *k == WarningKind::DebugResidue));
        assert!(classify_added_line("  debugger;")
            .iter()
            .any(|(k, _)| *k == WarningKind::DebugResidue));
        assert!(classify_added_line("println!(\"{:?}\", x)")
            .iter()
            .any(|(k, _)| *k == WarningKind::DebugResidue));
    }
    #[test]
    fn classify_todo() {
        assert!(classify_added_line("// TODO: refactor")
            .iter()
            .any(|(k, _)| *k == WarningKind::Todo));
        assert!(classify_added_line("// FIXME: bug")
            .iter()
            .any(|(k, _)| *k == WarningKind::Todo));
    }
    #[test]
    fn classify_clean_line_no_hits() {
        assert!(classify_added_line("fn add(a: i32, b: i32) -> i32 { a + b }").is_empty());
        assert!(classify_added_line("const greeting = \"hello\";").is_empty());
    }
    #[test]
    fn classify_sensitive_in_code() {
        assert!(classify_added_line("const token = \"ghp_xxx\";")
            .iter()
            .any(|(k, _)| *k == WarningKind::SensitiveInfo));
        // detail 不含实际值
        let hit = classify_added_line("password = \"secret123\"");
        let s = hit
            .iter()
            .find(|(k, _)| *k == WarningKind::SensitiveInfo)
            .unwrap();
        assert!(!s.1.contains("secret123"));
    }
}

//! AI 提交助手:基于暂存 diff 生成 Conventional Commits 提交信息(OpenAI 兼容协议)。

/// 提交信息语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Zh,
    En,
}

/// 构造 (system, user) 提示词:要求按 Conventional Commits 规范、指定语言,只返回信息本身。
pub fn build_prompt(diff: &str, language: Language) -> (String, String) {
    let lang = match language {
        Language::Zh => "中文",
        Language::En => "英文",
    };
    let system = format!(
        "你是一个提交信息生成助手。根据给定的 git 暂存区 diff,生成一条符合 Conventional Commits 规范的提交信息。格式 `type(scope): 描述`;type 从 feat/fix/chore/docs/style/refactor/perf/test/build/ci/release 中选最贴切的;scope 可选、简短。只返回提交信息本身,不要解释、不要代码块标记、不要引号。语言:{lang}。"
    );
    let user = format!("暂存区 diff:\n\n{diff}\n\n生成提交信息。");
    (system, user)
}

#[cfg(test)]
mod tests {
    use super::truncate_diff;

    use super::{build_prompt, Language};

    #[test]
    fn prompt_zh_contains_conventional_rule() {
        let (sys, user) = build_prompt("diff", Language::Zh);
        assert!(sys.contains("Conventional Commits"));
        assert!(sys.contains("中文"));
        assert!(user.contains("diff"));
    }

    #[test]
    fn prompt_en_uses_english() {
        let (sys, _) = build_prompt("diff", Language::En);
        assert!(sys.contains("英文"));
    }

    #[test]
    fn short_diff_not_truncated() {
        let diff = "diff --git a/x b/x\n+hello";
        assert_eq!(truncate_diff(diff, 1000), diff);
    }

    #[test]
    fn long_diff_truncated_with_note() {
        let diff: String = "あ".repeat(50); // 多字节字符,验证按字符而非字节计数
        let out = truncate_diff(&diff, 10);
        assert!(out.starts_with(&"あ".repeat(10)));
        assert!(out.contains("diff 已截断"));
    }
}

/// 超长 diff 按字符数截断并注明,避免超出模型上下文。
pub fn truncate_diff(diff: &str, max_chars: usize) -> String {
    if diff.chars().count() <= max_chars {
        return diff.to_string();
    }
    let head: String = diff.chars().take(max_chars).collect();
    format!("{head}\n\n…（diff 已截断,仅展示前 {max_chars} 字符）")
}

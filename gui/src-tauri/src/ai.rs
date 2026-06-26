//! AI 提交助手:基于暂存 diff 生成 Conventional Commits 提交信息(OpenAI 兼容协议)。

#[cfg(test)]
mod tests {
    use super::truncate_diff;

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

//! AI 提交助手:基于暂存 diff 生成 Conventional Commits 提交信息(OpenAI 兼容协议)。

use serde_json::json;

use crate::AppSettings;

/// 提交信息语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Zh,
    En,
}

/// 构造 (system, user) 提示词:引导按 Conventional Commits 规范、提炼改动意图,只返回信息本身。
pub fn build_prompt(diff: &str, language: Language) -> (String, String) {
    let lang = match language {
        Language::Zh => "中文",
        Language::En => "英文",
    };
    let system = format!(
        "你是专业的 git commit message 作者。根据给定的 git 暂存区 diff,生成一条符合 Conventional Commits 规范的提交信息。

要求:
- 重点说明改动的意图(为什么这样改),而非逐条罗列改了哪些文件;diff 本身已说明「改了什么」,你要提炼「为什么改」。
- 格式 `type(scope): 描述`,例如 `feat(gui): 接入 AI 生成提交信息`。
- type 必须小写,从下列选最贴切的:feat=新功能;fix=修复缺陷;docs=文档;style=代码格式/空白(不影响逻辑);refactor=重构(不改功能也不修缺陷);perf=性能优化;test=测试;build=构建系统或依赖;ci=CI 配置;chore=其他杂项(不涉及源码/测试);revert=回滚。
- scope 可选且简短:从文件路径或模块名推断(如 gui、gitcore);改动跨多个无关模块时省略 scope。
- 描述用简洁的动宾结构,不加句号,整行不超过 72 个字符。

严格输出:
- 只返回一条 commit message,不要任何解释、前言或候选选项。
- 不要代码块标记(如 ```),不要用引号包裹,不要给多个候选。
- 若 diff 仅是格式或空白调整,用 style 类型并给极简描述。

输出语言:{lang}。"
    );
    let user = format!("暂存区 diff:\n\n{diff}\n\n生成提交信息。");
    (system, user)
}

/// 从 OpenAI 兼容响应 JSON 提取 `choices[0].message.content`,trim 并拒绝空结果。
pub fn parse_chat_completion(body: &serde_json::Value) -> Result<String, String> {
    body.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "AI 返回为空或格式不符".into())
}

#[cfg(test)]
mod tests {
    use super::truncate_diff;

    use super::parse_chat_completion;
    use super::{build_prompt, AiConfig, Language, MIN_MAX_DIFF_CHARS};
    use serde_json::json;

    #[test]
    fn parses_content_from_response() {
        let body = json!({
            "choices": [{ "message": { "content": "  feat(gui): 优化暂存交互  " } }]
        });
        assert_eq!(
            parse_chat_completion(&body).unwrap(),
            "feat(gui): 优化暂存交互"
        );
    }

    #[test]
    fn empty_content_is_error() {
        let body = json!({ "choices": [{ "message": { "content": "   " } }] });
        assert!(parse_chat_completion(&body).is_err());
    }

    #[test]
    fn malformed_response_is_error() {
        let body = json!({ "error": "x" });
        assert!(parse_chat_completion(&body).is_err());
    }

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
    fn prompt_guides_why_and_strict_output() {
        let (sys, _) = build_prompt("diff", Language::Zh);
        assert!(sys.contains("为什么")); // 引导 why 而非 what
        assert!(sys.contains("feat=新功能")); // type 带中文描述(模型选得更准)
        assert!(sys.contains("72")); // 描述长度约束
        assert!(sys.contains("style")); // 格式化 diff 的处理规则
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

    #[test]
    fn missing_ai_fields_use_defaults() {
        // 模拟旧版 settings.json(无任何 ai_* 字段)。
        let json = r#"{"update_strategy":"Merge","ignore_whitespace":true}"#;
        let s: crate::AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.ai_enabled, false);
        assert_eq!(s.ai_base_url, "https://api.openai.com/v1");
        assert_eq!(s.ai_model, "gpt-4o-mini");
        assert_eq!(s.ai_language, "zh");
        assert_eq!(s.ai_max_diff_chars, 30000);
        assert_eq!(s.ai_api_key, "");
    }

    #[test]
    fn from_settings_clamps_small_max_diff_chars() {
        // 用户误填 0 会被 clamp 到下界,避免 diff 全被截断。
        let mut s = crate::AppSettings::default();
        s.ai_max_diff_chars = 0;
        let cfg = AiConfig::from_settings(&s);
        assert_eq!(cfg.max_diff_chars, MIN_MAX_DIFF_CHARS);

        // 正常大值不受影响。
        s.ai_max_diff_chars = 30000;
        let cfg = AiConfig::from_settings(&s);
        assert_eq!(cfg.max_diff_chars, 30000);
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

/// AI 调用配置(从 AppSettings 提取,解耦 ai 模块与完整设置结构)。
pub struct AiConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub language: Language,
    pub max_diff_chars: usize,
}

/// diff 截断字符数下界:防止用户误填 0/小值导致 diff 全被截断、生成质量不可控。
const MIN_MAX_DIFF_CHARS: usize = 1000;

impl AiConfig {
    pub fn from_settings(s: &AppSettings) -> Self {
        Self {
            base_url: s.ai_base_url.clone(),
            api_key: s.ai_api_key.clone(),
            model: s.ai_model.clone(),
            language: if s.ai_language == "en" {
                Language::En
            } else {
                Language::Zh
            },
            max_diff_chars: s.ai_max_diff_chars.max(MIN_MAX_DIFF_CHARS),
        }
    }
}

/// 调用 OpenAI 兼容 `{base_url}/chat/completions`,返回生成的提交信息。
pub async fn chat_complete(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败:{e}"))?;
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let body = json!({
        "model": cfg.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "temperature": 0.4,
    });
    let resp = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("网络错误:{e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return if status.as_u16() == 401 {
            Err("API Key 无效或未授权(401)".into())
        } else {
            Err(format!("AI 服务异常:{status} {text}"))
        };
    }
    let value: serde_json::Value = resp.json().await.map_err(|e| format!("解析响应失败:{e}"))?;
    parse_chat_completion(&value)
}

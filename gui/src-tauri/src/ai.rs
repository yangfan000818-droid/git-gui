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
/// generate_body=true 时允许标题后附简短正文(解释 why)。
pub fn build_prompt(diff: &str, language: Language, generate_body: bool) -> (String, String) {
    let lang = match language {
        Language::Zh => "中文",
        Language::En => "英文",
    };
    // 标题后的正文规则 + 输出形态,按是否生成正文切换。
    let (body_rule, output_form) = if generate_body {
        (
            "\n- 标题行后空一行,可附 1~3 行正文简要说明「为什么改」(不要与标题重复;改动很简单时可只写标题,不强制正文)。",
            "标题,必要时附空行 + 简短正文",
        )
    } else {
        ("", "仅一行标题")
    };
    let system = format!(
        "你是专业的 git commit message 作者。根据给定的 git 暂存区 diff,生成一条符合 Conventional Commits 规范的提交信息。

要求:
- 重点说明改动的意图(为什么这样改),而非逐条罗列改了哪些文件;diff 本身已说明「改了什么」,你要提炼「为什么改」。
- 格式 `type(scope): 描述`,例如 `feat(gui): 接入 AI 生成提交信息`。
- type 必须小写,从下列选最贴切的:feat=新功能;fix=修复缺陷;docs=文档;style=代码格式/空白(不影响逻辑);refactor=重构(不改功能也不修缺陷);perf=性能优化;test=测试;build=构建系统或依赖;ci=CI 配置;chore=其他杂项(不涉及源码/测试);revert=回滚。
- scope 可选且简短:从文件路径或模块名推断(如 gui、gitcore);改动跨多个无关模块时省略 scope。
- 描述用简洁的动宾结构,不加句号,整行不超过 72 个字符。{body_rule}

严格输出:
- 只返回一条 commit message({output_form}),不要任何解释、前言或候选选项。
- 不要代码块标记(如 ```),不要用引号包裹,不要给多个候选。
- 若 diff 仅是格式或空白调整,用 style 类型并给极简描述。

输出语言:{lang}。"
    );
    let user = format!("暂存区 diff:\n\n{diff}\n\n生成提交信息。");
    (system, user)
}

/// 从 OpenAI 兼容响应 JSON 提取 `choices[0].message.content`,清洗后返回。
/// keep_body=true 时保留「标题 + 空行 + 正文」,否则只取标题首行。
pub fn parse_chat_completion(body: &serde_json::Value, keep_body: bool) -> Result<String, String> {
    let raw = body
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "AI 返回为空或格式不符".to_string())?;
    let cleaned = sanitize_message(raw, keep_body);
    if cleaned.is_empty() {
        return Err("AI 返回为空或格式不符".into());
    }
    Ok(cleaned)
}

/// 清洗模型返回:剥离 reasoning 标签 → 去代码块围栏 → 去包裹引号。
/// keep_body=false 时再只取首行(只要标题);keep_body=true 保留「标题 + 空行 + 正文」。
/// prompt 约束不足以杜绝 LLM 画蛇添足(<think>、代码块、引号、多行解释),这里兜底。
fn sanitize_message(raw: &str, keep_body: bool) -> String {
    let stripped = strip_think_blocks(raw.trim());
    let fenced = strip_code_fence(&stripped);
    if keep_body {
        return strip_wrapping_quotes(fenced.trim()).to_string();
    }
    let line = fenced.lines().next().unwrap_or("").trim();
    strip_wrapping_quotes(line).to_string()
}

/// 去除 `<think>...</think>` 块(reasoning model 如 DeepSeek-R1 可能输出思考过程)。
fn strip_think_blocks(mut s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    while let Some(start) = s.find("<think") {
        out.push_str(&s[..start]);
        match s[start..].find("</think>") {
            Some(close) => s = &s[start + close + "</think>".len()..],
            None => {
                // 未闭合:丢弃 <think 之后的内容(尚在思考,无有效输出)。
                s = "";
                break;
            }
        }
    }
    out.push_str(s);
    out
}

/// 去掉 ``` 代码块围栏(模型偶尔用 ``` 包裹整条 message),返回围栏内内容。
fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    let rest = match s.strip_prefix("```") {
        Some(r) => r,
        None => return s,
    };
    let inner = rest.strip_suffix("```").unwrap_or(rest);
    // 跳过开头的语言标记行(如 ```rust / ```ts)。
    if let Some(nl) = inner.find('\n') {
        let tag = &inner[..nl];
        if !tag.is_empty()
            && tag.len() <= 16
            && !tag.contains(' ')
            && tag
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '+')
        {
            return inner[nl + 1..].trim_start_matches('\n');
        }
    }
    inner.trim()
}

/// 去掉首尾成对的单字符包裹:反引号、单引号、双引号。
fn strip_wrapping_quotes(s: &str) -> &str {
    let s = s.trim();
    for q in ['`', '\'', '"'] {
        if let Some(rest) = s.strip_prefix(q) {
            if let Some(inner) = rest.strip_suffix(q) {
                return inner.trim();
            }
        }
    }
    s
}

/// 超长 diff 按字符数截断并注明,避免超出模型上下文。
pub fn truncate_diff(diff: &str, max_chars: usize) -> String {
    if diff.chars().count() <= max_chars {
        return diff.to_string();
    }
    let head: String = diff.chars().take(max_chars).collect();
    format!("{head}\n\n…（diff 已截断,仅展示前 {max_chars} 字符）")
}

/// 把超长 diff 按文件块(`diff --git` 边界)切成多段,每段 ≤ max_chars。
/// 单个文件块自身超 max_chars 时,该块独占一段并截断标注(极端兜底)。
/// 不会在单个文件块中间切断,保持每段语义完整。
#[allow(dead_code)] // Task 6 map-reduce 编排将消费此函数
pub fn split_diff(diff: &str, max_chars: usize) -> Vec<String> {
    if diff.trim().is_empty() {
        return Vec::new();
    }
    // 1. 按文件块切分:每块以 "diff --git" 开头的行作为边界。
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("diff --git") && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    // 2. 贪心装箱:每段 ≤ max_chars;单块超长则独占 + 截断。
    let mut chunks: Vec<String> = Vec::new();
    let mut acc = String::new();
    for block in blocks {
        let block_chars = block.chars().count();
        if block_chars > max_chars {
            if !acc.is_empty() {
                chunks.push(std::mem::take(&mut acc));
            }
            chunks.push(truncate_diff(&block, max_chars));
            continue;
        }
        if acc.chars().count() + block_chars > max_chars {
            chunks.push(std::mem::take(&mut acc));
        }
        acc.push_str(&block);
    }
    if !acc.is_empty() {
        chunks.push(acc);
    }
    chunks
}

/// diff 截断字符数下界:防止用户误填 0/小值导致 diff 全被截断、生成质量不可控。
const MIN_MAX_DIFF_CHARS: usize = 1000;

/// AI 调用配置(从 AppSettings 提取,解耦 ai 模块与完整设置结构)。
#[derive(Clone)]
pub struct AiConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub language: Language,
    pub max_diff_chars: usize,
    pub generate_body: bool,
}

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
            generate_body: s.ai_generate_body,
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
    parse_chat_completion(&value, cfg.generate_body)
}

#[cfg(test)]
mod tests {
    use super::split_diff;
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
            parse_chat_completion(&body, false).unwrap(),
            "feat(gui): 优化暂存交互"
        );
    }

    #[test]
    fn empty_content_is_error() {
        let body = json!({ "choices": [{ "message": { "content": "   " } }] });
        assert!(parse_chat_completion(&body, false).is_err());
    }

    #[test]
    fn malformed_response_is_error() {
        let body = json!({ "error": "x" });
        assert!(parse_chat_completion(&body, false).is_err());
    }

    #[test]
    fn sanitize_strips_think_block() {
        let body = json!({
            "choices": [{ "message": { "content": "<think>分析改动…</think>feat(gui): 优化暂存交互" } }]
        });
        assert_eq!(
            parse_chat_completion(&body, false).unwrap(),
            "feat(gui): 优化暂存交互"
        );
    }

    #[test]
    fn sanitize_takes_first_line_only() {
        let body = json!({
            "choices": [{ "message": { "content": "feat(gui): 优化暂存交互\n\n这是解释,不应进入提交信息。" } }]
        });
        assert_eq!(
            parse_chat_completion(&body, false).unwrap(),
            "feat(gui): 优化暂存交互"
        );
    }

    #[test]
    fn sanitize_strips_wrapping_quotes() {
        for c in ["`feat: x`", "\"feat: x\"", "'feat: x'"] {
            let body = json!({ "choices": [{ "message": { "content": c } }] });
            assert_eq!(
                parse_chat_completion(&body, false).unwrap(),
                "feat: x",
                "case: {c}"
            );
        }
    }

    #[test]
    fn sanitize_strips_code_fence_with_lang_tag() {
        let body = json!({
            "choices": [{ "message": { "content": "```rust\nfeat(gui): 优化暂存交互\n```" } }]
        });
        assert_eq!(
            parse_chat_completion(&body, false).unwrap(),
            "feat(gui): 优化暂存交互"
        );
    }

    #[test]
    fn prompt_zh_contains_conventional_rule() {
        let (sys, user) = build_prompt("diff", Language::Zh, false);
        assert!(sys.contains("Conventional Commits"));
        assert!(sys.contains("中文"));
        assert!(user.contains("diff"));
    }

    #[test]
    fn prompt_en_uses_english() {
        let (sys, _) = build_prompt("diff", Language::En, false);
        assert!(sys.contains("英文"));
    }

    #[test]
    fn prompt_guides_why_and_strict_output() {
        let (sys, _) = build_prompt("diff", Language::Zh, false);
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
        assert!(!s.ai_enabled);
        assert_eq!(s.ai_base_url, "https://api.openai.com/v1");
        assert_eq!(s.ai_model, "gpt-4o-mini");
        assert_eq!(s.ai_language, "zh");
        assert_eq!(s.ai_max_diff_chars, 30000);
        assert_eq!(s.ai_api_key, "");
        assert!(!s.ai_generate_body);
    }

    #[test]
    fn from_settings_clamps_small_max_diff_chars() {
        // 用户误填 0 会被 clamp 到下界,避免 diff 全被截断。
        let mut s = crate::AppSettings {
            ai_max_diff_chars: 0,
            ..Default::default()
        };
        let cfg = AiConfig::from_settings(&s);
        assert_eq!(cfg.max_diff_chars, MIN_MAX_DIFF_CHARS);

        // 正常大值不受影响。
        s.ai_max_diff_chars = 30000;
        let cfg = AiConfig::from_settings(&s);
        assert_eq!(cfg.max_diff_chars, 30000);
    }

    #[test]
    fn build_prompt_with_body_adds_body_rule() {
        let (sys, _) = build_prompt("diff", Language::Zh, true);
        assert!(sys.contains("正文")); // generate_body=true 时允许正文
    }

    #[test]
    fn parse_keeps_body_when_requested() {
        let body = json!({
            "choices": [{ "message": { "content": "feat(gui): 接入 AI\n\n解释为什么改。" } }]
        });
        // keep_body=true:保留「标题 + 空行 + 正文」。
        assert_eq!(
            parse_chat_completion(&body, true).unwrap(),
            "feat(gui): 接入 AI\n\n解释为什么改。"
        );
        // keep_body=false:只取标题首行。
        assert_eq!(
            parse_chat_completion(&body, false).unwrap(),
            "feat(gui): 接入 AI"
        );
    }

    #[test]
    fn split_diff_empty_returns_empty() {
        assert!(split_diff("", 1000).is_empty());
        assert!(split_diff("   \n  ", 1000).is_empty());
    }

    #[test]
    fn split_diff_single_file_under_limit_one_chunk() {
        let diff = "diff --git a/x b/x\nindex 1..2\n+hello";
        assert_eq!(split_diff(diff, 1000), vec![diff.to_string()]);
    }

    #[test]
    fn split_diff_packs_multiple_files_into_one_chunk_when_under_limit() {
        let block_a = "diff --git a/a b/a\n+a\n";
        let block_b = "diff --git a/b b/b\n+b\n";
        let diff = format!("{block_a}{block_b}");
        assert_eq!(split_diff(&diff, 1000), vec![diff.to_string()]);
    }

    #[test]
    fn split_diff_splits_when_exceeding_limit_at_file_boundary() {
        // 每块 22 字符(≤ limit=40),两块合计 44 > limit → 在文件边界切两段,不在单文件中间断。
        let block_a = "diff --git a/a b/a\n+a\n";
        let block_b = "diff --git a/b b/b\n+b\n";
        let diff = format!("{block_a}{block_b}");
        let chunks = split_diff(&diff, 40);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], block_a); // a 块完整保留在第一段
        assert_eq!(chunks[1], block_b);
    }

    #[test]
    fn split_diff_oversized_single_file_truncated_alone() {
        // 单个文件块自身超 limit(74 > 20):独占一段并被截断标注。
        let big: String = format!("diff --git a/big b/big\n{}\n", "x".repeat(50));
        let chunks = split_diff(&big, 20);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("diff 已截断"));
    }
}

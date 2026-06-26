# AI 提交信息:超长 diff 渐进式 map-reduce 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 AI 生成提交信息在 diff 超长时,从"砍前 N 字符丢信息"升级为"按文件分批 map-reduce 覆盖全部改动",失败回退单次截断,小 diff 零变化。

**Architecture:** diff ≤ 阈值走现状单次;> 阈值按 `diff --git` 文件边界切多批,并发(≤3) map 出要点短语,一次 reduce 合成 commit。请求函数注入做可测性,任一环节失败回退现状。

**Tech Stack:** Rust + Tauri 2 + reqwest 0.12 + 新增 `futures-util` 0.3(async 并发)。代码在 `gui/src-tauri/src/ai.rs`、`gui/src-tauri/src/lib.rs`、`gui/src/lib/Settings.svelte`。

## Global Constraints

- 测试运行(workspace 根目录 `/Users/yfan/work/git-gui`):`cargo test -p gui ai::` 跑 ai 模块单测;`cargo test -p gui` 跑 gui 全部。
- CI 门禁:`cargo clippy -p gui --all-targets` 必须零警告(项目历史 commit `687d3be`)。
- 格式化只动 gui 包:`cargo fmt -p gui`(禁止全仓 fmt 制造无关 diff)。
- 提交信息用 Conventional Commits 中文风格:`feat(gui): ...` / `refactor(gui): ...` / `test(gui): ...`。
- 当前在 `main` 分支;执行前应先开特性分支(见执行交接)。
- 现有 `chat_complete`、`truncate_diff`、`build_prompt`、`strip_*` 清洗函数的行为对调用方保持不变。

## File Structure

- `gui/src-tauri/Cargo.toml` — 加 `futures-util` 依赖。
- `gui/src-tauri/src/ai.rs` — 新增 `split_diff` / `build_map_prompt` / `sanitize_notes` / `build_reduce_prompt` / `extract_content` / `chat_complete_raw` / `generate_map_reduce` + `RequestFn` 类型 + 对应单测;给 `AiConfig` 加 `Clone`。
- `gui/src-tauri/src/lib.rs` — 改 `ai_generate_commit_message`(`lib.rs:594`):单次/分批分支 + 失败回退。
- `gui/src/lib/Settings.svelte` — `Settings.svelte:525` 文案补一句说明。

---

## Task 1: 基础设施 — `futures-util` 依赖 + `AiConfig: Clone`

**Files:**
- Modify: `gui/src-tauri/Cargo.toml`
- Modify: `gui/src-tauri/src/ai.rs:148`(`AiConfig` 定义)

**Interfaces:**
- Produces: `AiConfig` 实现 `Clone`(后续 `lib.rs` 构造请求闭包要 clone cfg);`futures-util` 可用。

- [ ] **Step 1: 加依赖**

在 `gui/src-tauri/Cargo.toml` 的 `[dependencies]` 末尾(reqwest 那行之后)加:

```toml
# AI 分批生成:stream buffer_unordered 做温和并发 map。
futures-util = "0.3"
```

- [ ] **Step 2: 给 AiConfig 加 Clone**

`gui/src-tauri/src/ai.rs:148`,把:

```rust
pub struct AiConfig {
```

改为:

```rust
#[derive(Clone)]
pub struct AiConfig {
```

- [ ] **Step 3: 验证编译 + 依赖拉取**

Run: `cargo build -p gui`
Expected: 编译通过(`futures-util` 被拉入;`AiConfig` 字段全是 `String`/`usize`/`Language`(已 `Copy`),derive Clone 合法)。

- [ ] **Step 4: clippy 不退步**

Run: `cargo clippy -p gui --all-targets 2>&1 | tail -5`
Expected: 无新警告。

- [ ] **Step 5: Commit**

```bash
git add gui/src-tauri/Cargo.toml gui/src-tauri/src/ai.rs
git commit -m "chore(gui): AI 模块加 futures-util 依赖与 AiConfig: Clone(分批生成前置)"
```

---

## Task 2: `split_diff` — 按文件边界切分

**Files:**
- Modify: `gui/src-tauri/src/ai.rs`(加函数 + 测试)

**Interfaces:**
- Consumes: `truncate_diff(diff, max_chars)`(已存在,`ai.rs:137`)
- Produces: `pub fn split_diff(diff: &str, max_chars: usize) -> Vec<String>`

- [ ] **Step 1: 写失败测试**

在 `ai.rs` 的 `mod tests` 内(`fn parse_keeps_body_when_requested` 之后)加:

```rust
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p gui ai::tests::split_diff 2>&1 | tail -15`
Expected: 编译失败,`split_diff` 未定义。

- [ ] **Step 3: 实现 `split_diff`**

在 `ai.rs` 的 `truncate_diff` 函数之后(`MIN_MAX_DIFF_CHARS` 常量之前)加:

```rust
/// 把超长 diff 按文件块(`diff --git` 边界)切成多段,每段 ≤ max_chars。
/// 单个文件块自身超 max_chars 时,该块独占一段并截断标注(极端兜底)。
/// 不会在单个文件块中间切断,保持每段语义完整。
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
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p gui ai::tests::split_diff 2>&1 | tail -10`
Expected: 5 个 split_diff 测试全 PASS。

- [ ] **Step 5: Commit**

```bash
git add gui/src-tauri/src/ai.rs
git commit -m "feat(gui): split_diff 按文件边界切分超长 diff(map-reduce 前置)"
```

---

## Task 3: map 阶段 — `build_map_prompt` + `sanitize_notes`

**Files:**
- Modify: `gui/src-tauri/src/ai.rs`

**Interfaces:**
- Consumes: `Language`(`ai.rs:8`)、`strip_think_blocks`/`strip_code_fence`/`strip_wrapping_quotes`(`ai.rs:82/100/124`)
- Produces: `pub fn build_map_prompt(chunk: &str, language: Language) -> (String, String)`、`fn sanitize_notes(raw: &str) -> String`

- [ ] **Step 1: 写失败测试**

在 `mod tests` 内加:

```rust
#[test]
fn build_map_prompt_asks_for_notes_not_commit() {
    let (sys, user) = build_map_prompt("diff", Language::Zh);
    assert!(sys.contains("要点")); // 要点而非 commit
    assert!(!sys.contains("Conventional Commits")); // 不要求 commit 格式
    assert!(sys.contains("中文"));
    assert!(user.contains("diff"));
}

#[test]
fn sanitize_notes_keeps_multiple_lines() {
    let raw = "改了 A 模块\n改了 B 模块";
    assert_eq!(sanitize_notes(raw), "改了 A 模块\n改了 B 模块");
}

#[test]
fn sanitize_notes_strips_think_and_fence_but_keeps_lines() {
    let raw = "<think>分析</think>\n```\n要点一\n要点二\n```";
    let out = sanitize_notes(raw);
    assert_eq!(out, "要点一\n要点二");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p gui ai::tests::build_map_prompt ai::tests::sanitize_notes 2>&1 | tail -10`
Expected: 编译失败,函数未定义。

- [ ] **Step 3: 实现两个函数**

在 `build_prompt` 函数之后(`parse_chat_completion` 之前)加:

```rust
/// 构造 map 阶段提示词:让 AI 提炼这批 diff 的改动要点(≤3 条短语),非 commit 格式。
/// system 不依赖 chunk;user 含 chunk。
pub fn build_map_prompt(chunk: &str, language: Language) -> (String, String) {
    let lang = match language {
        Language::Zh => "中文",
        Language::En => "英文",
    };
    let system = format!(
        "你是代码审查助手。根据给定的 git diff 片段,提炼这批改动的要点。

要求:
- 输出不超过 3 条要点,每条一行,描述「改了什么 + 为什么这样改」。
- 只输出要点本身,不要 commit message 格式(不要 type(scope): 前缀),不要解释、前言或编号。
- 若片段仅为格式/空白调整,用一句话说明。

输出语言:{lang}。"
    );
    let user = format!("diff 片段:\n\n{chunk}\n\n提炼要点。");
    (system, user)
}

/// map 阶段清洗:剥离 think 块 → 去围栏 → 去包裹引号,但保留多行(要点可多行)。
fn sanitize_notes(raw: &str) -> String {
    let stripped = strip_think_blocks(raw.trim());
    let fenced = strip_code_fence(&stripped);
    strip_wrapping_quotes(fenced.trim()).trim().to_string()
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p gui ai::tests::build_map_prompt ai::tests::sanitize_notes 2>&1 | tail -10`
Expected: 3 个测试全 PASS。

- [ ] **Step 5: Commit**

```bash
git add gui/src-tauri/src/ai.rs
git commit -m "feat(gui): build_map_prompt 与 sanitize_notes(map 阶段要点提取)"
```

---

## Task 4: reduce 阶段 — `build_reduce_prompt`

**Files:**
- Modify: `gui/src-tauri/src/ai.rs`

**Interfaces:**
- Consumes: `build_prompt(diff, language, generate_body)`(`ai.rs:16`,复用其 system 的 conventional 规则)
- Produces: `pub fn build_reduce_prompt(notes: &[String], language: Language, generate_body: bool) -> (String, String)`

- [ ] **Step 1: 写失败测试**

在 `mod tests` 内加:

```rust
#[test]
fn build_reduce_prompt_reuses_conventional_rules_and_lists_notes() {
    let notes = vec!["改了 A".to_string(), "改了 B".to_string()];
    let (sys, user) = build_reduce_prompt(&notes, Language::Zh, false);
    // 复用 build_prompt 的 conventional 规则。
    assert!(sys.contains("Conventional Commits"));
    assert!(sys.contains("feat=新功能"));
    // user 含全部要点。
    assert!(user.contains("改了 A"));
    assert!(user.contains("改了 B"));
    assert!(user.contains("覆盖全部改动"));
}

#[test]
fn build_reduce_prompt_body_flag_propagates() {
    let notes = vec!["改了 A".to_string()];
    let (sys_with, _) = build_reduce_prompt(&notes, Language::Zh, true);
    let (sys_without, _) = build_reduce_prompt(&notes, Language::Zh, false);
    assert!(sys_with.contains("正文"));
    assert!(!sys_without.contains("正文"));
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p gui ai::tests::build_reduce_prompt 2>&1 | tail -10`
Expected: 编译失败,函数未定义。

- [ ] **Step 3: 实现 `build_reduce_prompt`**

在 `build_map_prompt` 之后加:

```rust
/// 构造 reduce 阶段提示词:把分批要点合成一条覆盖全部改动的 conventional commit。
/// system 复用 build_prompt 的规范规则;user 换成"要点列表 → 合成一条 commit"。
pub fn build_reduce_prompt(
    notes: &[String],
    language: Language,
    generate_body: bool,
) -> (String, String) {
    // system 与 build_prompt 完全一致(含 conventional 规则与 body 规则)。
    let (system, _) = build_prompt("", language, generate_body);
    let notes_text = notes
        .iter()
        .map(|n| format!("- {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let user = format!(
        "以下是某次提交按文件分组提取的改动要点:\n\n{notes_text}\n\n请综合全部要点,生成一条覆盖所有改动的 commit message。"
    );
    (system, user)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p gui ai::tests::build_reduce_prompt 2>&1 | tail -10`
Expected: 2 个测试全 PASS。

- [ ] **Step 5: Commit**

```bash
git add gui/src-tauri/src/ai.rs
git commit -m "feat(gui): build_reduce_prompt 合成分批要点为单条 commit(reduce 阶段)"
```

---

## Task 5: 拆分 `chat_complete_raw` + `extract_content`(重构,行为不变)

**Files:**
- Modify: `gui/src-tauri/src/ai.rs:53`(`parse_chat_completion`)、`ai.rs:176`(`chat_complete`)

**Interfaces:**
- Produces: `fn extract_content(body: &serde_json::Value) -> Result<String, String>`(从 JSON 取 raw content)、`pub async fn chat_complete_raw(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String>`(HTTP + extract,返回未清洗 raw content)
- `chat_complete` 改为 `chat_complete_raw` + `sanitize_message(.., generate_body)`,**对外签名与返回不变**。

- [ ] **Step 1: 先确认现状测试全绿(重构基线)**

Run: `cargo test -p gui ai:: 2>&1 | tail -5`
Expected: 全部 PASS(重构前基线)。

- [ ] **Step 2: 抽出 `extract_content`**

把 `parse_chat_completion`(`ai.rs:53`):

```rust
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
```

改为(抽出 extract_content,parse_chat_completion 行为不变):

```rust
/// 从 OpenAI 兼容响应 JSON 提取 `choices[0].message.content` 原文(未清洗)。
fn extract_content(body: &serde_json::Value) -> Result<String, String> {
    body.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "AI 返回为空或格式不符".to_string())
}

/// 从响应 JSON 取 raw content 并清洗;keep_body=false 只取标题首行。
pub fn parse_chat_completion(body: &serde_json::Value, keep_body: bool) -> Result<String, String> {
    let raw = extract_content(body)?;
    let cleaned = sanitize_message(&raw, keep_body);
    if cleaned.is_empty() {
        return Err("AI 返回为空或格式不符".into());
    }
    Ok(cleaned)
}
```

- [ ] **Step 3: 拆出 `chat_complete_raw`,改 `chat_complete`**

把 `chat_complete`(`ai.rs:176`)的结尾两行:

```rust
    let value: serde_json::Value = resp.json().await.map_err(|e| format!("解析响应失败:{e}"))?;
    parse_chat_completion(&value, cfg.generate_body)
}
```

改为(拆出 raw,chat_complete 包一层 sanitize):

```rust
    let value: serde_json::Value = resp.json().await.map_err(|e| format!("解析响应失败:{e}"))?;
    let raw = extract_content(&value)?;
    let cleaned = sanitize_message(&raw, cfg.generate_body);
    if cleaned.is_empty() {
        return Err("AI 返回为空或格式不符".into());
    }
    Ok(cleaned)
}

/// 发请求并返回**未清洗的 raw content**(供 map/reduce 各自清洗)。HTTP/鉴权/错误处理与 chat_complete 一致。
pub async fn chat_complete_raw(
    cfg: &AiConfig,
    system: &str,
    user: &str,
) -> Result<String, String> {
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
    extract_content(&value)
}
```

注:`chat_complete` 现仍保留独立的 client 构造(原样),仅结尾改清洗;`chat_complete_raw` 是它的"返回 raw"变体。两者 HTTP 逻辑重复属可接受范围(避免本轮再抽 client 层扩大改动;后续可统一)。

- [ ] **Step 4: 跑全部 ai 测试确认行为不变**

Run: `cargo test -p gui ai:: 2>&1 | tail -5`
Expected: 全部 PASS(现有 parse/chat 测试仍绿,证明重构无行为变化)。

- [ ] **Step 5: clippy + fmt**

Run: `cargo clippy -p gui --all-targets 2>&1 | tail -5` → 无新警告。
Run: `cargo fmt -p gui`

- [ ] **Step 6: Commit**

```bash
git add gui/src-tauri/src/ai.rs
git commit -m "refactor(gui): 拆出 chat_complete_raw 与 extract_content(map-reduce 复用,行为不变)"
```

---

## Task 6: `generate_map_reduce` 编排 + 注入测试

**Files:**
- Modify: `gui/src-tauri/src/ai.rs`

**Interfaces:**
- Consumes: `build_map_prompt`、`build_reduce_prompt`、`sanitize_notes`、`sanitize_message`、`AiConfig`(Clone)
- Produces: `pub type RequestFn`、`pub async fn generate_map_reduce(cfg: &AiConfig, chunks: &[String], request: RequestFn) -> Result<String, String>`

- [ ] **Step 1: 写失败测试(成功路径 + 失败传播)**

在 `mod tests` 内加。需要 `use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};` 与 `use futures::future::BoxFuture;` 与 `use super::*;`(测试模块顶部已有 `use super::*` 部分导入,补齐)。

先在 `mod tests` 顶部补导入(若缺):

```rust
use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};
use futures::future::BoxFuture;
```

再加测试:

```rust
// 构造 mock request:按调用顺序返回预设 raw content;某次返回 Err 模拟失败。
fn mock_request(responses: Vec<Result<&str, &str>>) -> RequestFn {
    let counter = Arc::new(AtomicUsize::new(0));
    let responses: Vec<Result<String, String>> = responses
        .into_iter()
        .map(|r| match r {
            Ok(s) => Ok(s.to_string()),
            Err(s) => Err(s.to_string()),
        })
        .collect();
    let responses = Arc::new(responses);
    Arc::new(move |_sys: String, _user: String| {
        let i = counter.fetch_add(1, Ordering::SeqCst);
        let resp = responses.get(i).cloned().unwrap_or(Ok(String::new()));
        Box::pin(async move { resp }) as BoxFuture<'static, Result<String, String>>
    })
}

#[test]
async fn map_reduce_success_combines_notes_into_commit() {
    let cfg = AiConfig {
        base_url: String::new(),
        api_key: String::new(),
        model: String::new(),
        language: Language::Zh,
        max_diff_chars: 1000,
        generate_body: false,
    };
    let chunks = vec!["chunk-a".to_string(), "chunk-b".to_string()];
    // 调用顺序:map 批1 → map 批2 → reduce。
    let request = mock_request(vec![
        Ok("要点一"),       // map 批1
        Ok("要点二"),       // map 批2
        Ok("feat(gui): 合成提交"), // reduce
    ]);
    let msg = generate_map_reduce(&cfg, &chunks, request).await.unwrap();
    assert_eq!(msg, "feat(gui): 合成提交");
}

#[test]
async fn map_reduce_map_failure_propagates_err() {
    let cfg = AiConfig {
        base_url: String::new(),
        api_key: String::new(),
        model: String::new(),
        language: Language::Zh,
        max_diff_chars: 1000,
        generate_body: false,
    };
    let chunks = vec!["chunk-a".to_string(), "chunk-b".to_string()];
    // 第二批 map 失败 → 整体 Err(reduce 不应被调用)。
    let request = mock_request(vec![
        Ok("要点一"),
        Err("网络错误"),
    ]);
    let result = generate_map_reduce(&cfg, &chunks, request).await;
    assert!(result.is_err());
}

#[test]
async fn map_reduce_empty_chunks_is_err() {
    let cfg = AiConfig {
        base_url: String::new(),
        api_key: String::new(),
        model: String::new(),
        language: Language::Zh,
        max_diff_chars: 1000,
        generate_body: false,
    };
    let request = mock_request(vec![]);
    let result = generate_map_reduce(&cfg, &[], request).await;
    assert!(result.is_err());
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p gui ai::tests::map_reduce 2>&1 | tail -15`
Expected: 编译失败,`RequestFn` / `generate_map_reduce` 未定义。

- [ ] **Step 3: 实现类型 + 编排函数**

在 `ai.rs` 顶部 `use` 区,把 `use serde_json::json;` 那行附近补:

```rust
use std::sync::Arc;
use futures::future::BoxFuture;
use futures::stream::{self, StreamExt};
```

在 `chat_complete_raw` 之后(`#[cfg(test)]` 之前)加类型与编排函数:

```rust
/// 注入的请求函数:接收 (system, user),返回未清洗 raw content 的 future。
/// 用 Arc<dyn Fn> + 'static BoxFuture,便于生产闭包与测试 mock 各自构造。
pub type RequestFn =
    Arc<dyn Fn(String, String) -> BoxFuture<'static, Result<String, String>> + Send + Sync>;

/// 分批 map-reduce:并发(≤3)对每批 diff 生成要点,再一次 reduce 合成 commit。
/// 任一批 map 或 reduce 失败 → 向上传播 Err(由调用方决定是否回退)。
pub async fn generate_map_reduce(
    cfg: &AiConfig,
    chunks: &[String],
    request: RequestFn,
) -> Result<String, String> {
    if chunks.is_empty() {
        return Err("diff 为空,无法分批生成".into());
    }
    // map:system 对所有批相同(不依赖 chunk),只算一次。
    let map_sys = Arc::new(build_map_prompt("", cfg.language).0);
    let request_map = Arc::clone(&request);
    let language = cfg.language;
    let notes: Vec<String> = stream::iter(chunks.to_vec())
        .map(|chunk| {
            let request = Arc::clone(&request_map);
            let sys = Arc::clone(&map_sys);
            async move {
                let user = build_map_prompt(&chunk, language).1;
                let raw = request((*sys).clone(), user).await?;
                Ok::<String, String>(sanitize_notes(&raw))
            }
        })
        .buffer_unordered(3)
        .collect::<Result<Vec<_>, _>>()
        .await?;
    // reduce:要点 → 一条 commit。
    let (red_sys, red_user) = build_reduce_prompt(&notes, cfg.language, cfg.generate_body);
    let raw = request(red_sys, red_user).await?;
    let msg = sanitize_message(&raw, cfg.generate_body);
    if msg.trim().is_empty() {
        return Err("AI 返回为空或格式不符".into());
    }
    Ok(msg)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p gui ai::tests::map_reduce 2>&1 | tail -15`
Expected: 3 个 map_reduce 测试全 PASS。

- [ ] **Step 5: clippy + fmt**

Run: `cargo clippy -p gui --all-targets 2>&1 | tail -5` → 无新警告(buffer_unordered 闭包需 `Send`;`AiConfig`/`String` 皆 Send,通过)。
Run: `cargo fmt -p gui`

- [ ] **Step 6: Commit**

```bash
git add gui/src-tauri/src/ai.rs
git commit -m "feat(gui): generate_map_reduce 并发 map-reduce 编排(请求注入可测)"
```

---

## Task 7: 接入 `ai_generate_commit_message`(单次/分批分支 + 失败回退)

**Files:**
- Modify: `gui/src-tauri/src/lib.rs:594`(`ai_generate_commit_message`)

**Interfaces:**
- Consumes: `ai::split_diff`、`ai::generate_map_reduce`、`ai::RequestFn`、`ai::chat_complete_raw`、`ai::build_prompt`、`ai::chat_complete`、`ai::truncate_diff`、`AiConfig`(Clone)
- 调用方(`lib.rs:1485` 的 command 注册)签名不变,前端无感。

- [ ] **Step 1: 确认现状函数全文(改动基线)**

Run: `sed -n '594,618p' gui/src-tauri/src/lib.rs`(仅查看,不修改)
确认现有 `ai_generate_commit_message` 与本计划"Global Constraints"引用一致。

- [ ] **Step 2: 改造函数体**

把 `lib.rs:594` 起的 `ai_generate_commit_message` 整体替换为:

```rust
async fn ai_generate_commit_message(app: AppHandle, path: String) -> Result<String, String> {
    let settings = AppSettings::load(&app);
    if !settings.ai_enabled || settings.ai_api_key.trim().is_empty() {
        return Err("未配置 AI 提交助手,请在设置中填写 API Key 并启用".into());
    }
    let cfg = ai::AiConfig::from_settings(&settings);
    let max_chars = cfg.max_diff_chars;
    // 同步部分:取 staged diff(git 调用放 spawn_blocking,不阻塞 webview)。
    let diff_text =
        tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
            let repo = Repo::open(&path).map_err(|e| e.to_string())?;
            repo.staged_diff_text().map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
    if diff_text.trim().is_empty() {
        return Err("没有暂存的改动".into());
    }
    // 单次路径(小 diff):与现状一致。
    if diff_text.chars().count() <= max_chars {
        let (system, user) = ai::build_prompt(&diff_text, cfg.language, cfg.generate_body);
        return ai::chat_complete(&cfg, &system, &user).await;
    }
    // 分批路径:split → map-reduce;任一失败回退单次截断(永不比现状差)。
    let chunks = ai::split_diff(&diff_text, max_chars);
    let cfg_for_request = cfg.clone();
    let request: ai::RequestFn = Arc::new(move |system: String, user: String| {
        let cfg = cfg_for_request.clone();
        Box::pin(async move { ai::chat_complete_raw(&cfg, &system, &user).await })
    });
    match ai::generate_map_reduce(&cfg, &chunks, request).await {
        Ok(msg) => Ok(msg),
        Err(_) => {
            let truncated = ai::truncate_diff(&diff_text, max_chars);
            let (system, user) = ai::build_prompt(&truncated, cfg.language, cfg.generate_body);
            ai::chat_complete(&cfg, &system, &user).await
        }
    }
}
```

注:`Arc` 需在 `lib.rs` 可见 —— `ai::RequestFn` 已是 `Arc<...>` 别名,`lib.rs` 用 `ai::RequestFn` 即隐式引入 `Arc`(经由 `ai` 模块 re-export 路径)。若编译报 `Arc` 未导入,在 `lib.rs` 顶部 `use` 区加 `use std::sync::Arc;`。

- [ ] **Step 3: 编译 + 全量 ai 测试**

Run: `cargo build -p gui 2>&1 | tail -10`
Expected: 编译通过。
Run: `cargo test -p gui ai:: 2>&1 | tail -5`
Expected: 全 PASS。

- [ ] **Step 4: clippy + fmt**

Run: `cargo clippy -p gui --all-targets 2>&1 | tail -5` → 无新警告。
Run: `cargo fmt -p gui`

- [ ] **Step 5: Commit**

```bash
git add gui/src-tauri/src/lib.rs
git commit -m "feat(gui): AI 提交信息超长 diff 自动分批 map-reduce,失败回退单次截断"
```

---

## Task 8: Settings 文案微调

**Files:**
- Modify: `gui/src/lib/Settings.svelte:523-533`

**Interfaces:** 无代码接口;纯文案。

- [ ] **Step 1: 改文案**

把 `Settings.svelte:523-533` 的:

```svelte
          <label class="st-radio" style="display:block;padding:6px 0">
            <span>
              <b>diff 截断字符数</b>
              <input
                class="ai-input"
                type="number"
                min="1000"
                bind:value={aiMaxDiffChars}
              />
            </span>
          </label>
```

改为(补一行说明):

```svelte
          <label class="st-radio" style="display:block;padding:6px 0">
            <span>
              <b>diff 截断字符数</b>
              <input
                class="ai-input"
                type="number"
                min="1000"
                bind:value={aiMaxDiffChars}
              />
              <em style="display:block;opacity:.7;font-size:.85em"
                >超过此值时自动分批请求,确保覆盖全部改动</em
              >
            </span>
          </label>
```

- [ ] **Step 2: 前端检查(若有 lint/build)**

Run: `cd gui && npm run check 2>&1 | tail -10`(若脚本不存在则跳过,说明并跳过)
Expected: 无 svelte 检查错误。

- [ ] **Step 3: Commit**

```bash
git add gui/src/lib/Settings.svelte
git commit -m "docs(gui): diff 截断字符数补充分批说明文案"
```

---

## Task 9: 收尾 — clippy 全门禁 + 全量测试 + 手测路径

**Files:** 无(验证 only)。

- [ ] **Step 1: CI 门禁 clippy**

Run: `cargo clippy --workspace --all-targets 2>&1 | tail -10`
Expected: 零警告(含 gitcore,确认未连带退步)。

- [ ] **Step 2: 全量测试**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: 全 PASS。

- [ ] **Step 3: 格式化检查无残留**

Run: `cargo fmt -p gui -- --check 2>&1 | tail -5`
Expected: 无输出(已格式化)。

- [ ] **Step 4: 手测两条路径(需真实 API Key,可选)**

启动 app,准备:① 小改动(< 30000 字符)→ 确认单次生成如常;② 超大改动(可临时 stage 一个大文件或多个文件)→ 确认分批生成、覆盖更多文件、失败时回退仍有结果。若无可用的 API Key,在总结中标注"网络路径未手测,离线单测已覆盖编排逻辑"。

---

## Self-Review(计划自审)

**1. Spec 覆盖:**
- §3.1 总体流程 → Task 7(分支)。
- §3.2 沿用阈值 → Task 7 用 `max_chars` 判断(无新设置项)。
- §3.3 失败回退 → Task 7 `match ... Err(_) => 单次截断`。
- §4 `split_diff` → Task 2;`build_map_prompt`/`sanitize_notes` → Task 3;`build_reduce_prompt` → Task 4;`chat_complete_raw`/`extract_content` → Task 5;`generate_map_reduce`/`RequestFn` → Task 6。
- §4.2 `futures-util` + `buffer_unordered(3)` → Task 1 依赖 + Task 6 实现。
- §4.3 请求注入 + parse 分层 → Task 5(raw)+ Task 6(注入 + map/reduce 分流清洗)。
- §5 文案 → Task 8。
- §6 测试策略 → 各 Task 的 TDD 测试 + Task 9 收尾。
- 全覆盖,无遗漏。

**2. 占位扫描:** 无 TBD/TODO/"add error handling" 等占位;每步含完整代码或精确命令。

**3. 类型一致性:**
- `RequestFn` 在 Task 6 定义为 `Arc<dyn Fn(String, String) -> BoxFuture<'static, Result<String, String>> + Send + Sync>`,Task 7 生产闭包与 Task 6 mock(`mock_request`)返回类型一致(均 `Box::pin(async move { resp }) as BoxFuture<'static, Result<String, String>>`)。
- `split_diff(&str, usize) -> Vec<String>`(Task 2)与 Task 7 调用 `ai::split_diff(&diff_text, max_chars)` 一致。
- `generate_map_reduce(&AiConfig, &[String], RequestFn)`(Task 6)与 Task 7 调用一致。
- `chat_complete_raw(&AiConfig, &str, &str) -> Result<String, String>`(Task 5)与 Task 7 闭包内 `ai::chat_complete_raw(&cfg, &system, &user)` 一致。
- `AiConfig` 字段在 Task 6 测试构造与 `ai.rs:148` 定义一致(base_url/api_key/model/language/max_diff_chars/generate_body)。
- 一致,无类型漂移。

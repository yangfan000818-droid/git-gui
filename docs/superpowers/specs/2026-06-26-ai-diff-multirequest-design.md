# AI 提交信息:超长 diff 渐进式 map-reduce

- 日期:2026-06-26
- 状态:设计已确认,待实现
- 关联代码:`gui/src-tauri/src/ai.rs`、`gui/src-tauri/src/lib.rs`、`gui/src/lib/Settings.svelte`

## 1. 背景与问题

当前 AI 生成提交信息是**纯单次请求**(`ai_generate_commit_message`,`lib.rs:594`):

```
取 staged diff → truncate_diff 砍到前 N 字符 → 单次 chat_complete → 清洗
```

`truncate_diff`(`ai.rs:137`)是"取前 N 字符"的粗暴截断。当 diff 超过 `ai_max_diff_chars`(默认 30000)时,**排在后面的文件改动 AI 完全看不到**,生成的 commit message 只覆盖部分文件,信息不全。

(注:前端已有的"批量生成"是按多个仓库目录循环调同一个 command,不是把一条 commit 的 diff 拆开重发,与本设计无关。)

## 2. 目标

- 超长 diff 时让 AI **覆盖全部文件改动**,生成完整的 commit message。
- **默认仍走单次请求**,只有 diff 真正超过阈值时才分批——小 diff 零额外成本、零延迟。
- 失败时**永不比现状更差**:分批任何环节出错,回退到现状的"单次截断"。

非目标(本期不做,避免 scope creep):
- 分批进度条事件
- 按模型上下文窗口动态算阈值
- 并发数可配置

## 3. 方案:渐进式 map-reduce

### 3.1 总体流程

改造 `ai_generate_commit_message`(`lib.rs:594`):

```
取 staged diff_text
├─ chars ≤ ai_max_diff_chars → 单次路径(现状逻辑,零改动,小 diff 常态)
└─ chars > 阈值 → 分批路径:
     1. split_diff(diff, 阈值) → Vec<chunk>     [按文件边界切,纯 CPU]
     2. 并发 map(≤3):每 chunk → 改动要点短语
     3. reduce:所有要点 → 一条 conventional commit
     任一步失败 → 回退「单次截断」(现状逻辑)
```

`lib.rs:594` 现有结构(`spawn_blocking` 内取 diff+截断+构造 prompt,外面 `await` 一次请求)需微调:`spawn_blocking` 只负责"取 diff + split"(纯 CPU);chunks 返回后在 async 上下文里并发发请求。原因:多个 `chat_complete` 是 async 网络调用,必须在 async 上下文并发,不能塞进 `spawn_blocking`。

### 3.2 触发阈值

**沿用现有 `ai_max_diff_chars`**(默认 30000,下界 1000)作为"单批上限"。总长超过它才切分;不超过则单次。不新增设置项,用户已有配置继续生效。

### 3.3 失败降级

- map 任一批失败 / reduce 失败 → `generate_map_reduce` 返回 `Err`。
- `ai_generate_commit_message` 捕获 `Err` → 走现状「单次截断」路径(`truncate_diff` + 单次请求)。
- 结果:用户**永远能拿到一条 commit message**;分批失败时退化为现状体验,不会比现在更差。

## 4. 组件设计(`ai.rs`)

| 函数 | 职责 | 可测性 |
|---|---|---|
| `split_diff(diff, max) -> Vec<String>` | 按 `diff --git` 文件边界贪心装箱,每块 ≤ max;单文件块自身 > max 时该块单独截断并标注 | 纯函数,核心测试目标 |
| `build_map_prompt(chunk, lang) -> (sys, user)` | 让 AI 输出**这批改动要点 ≤3 条短语**(非 commit 格式),严格无解释 | 纯函数 |
| `parse_map_output(body) -> Result<String>` | 提取要点文本(复用 `strip_think_blocks`/`strip_code_fence`/`strip_wrapping_quotes`,但保留多行) | 纯函数 |
| `build_reduce_prompt(notes, lang, generate_body) -> (sys, user)` | 复用现有 conventional commits 规则,user 部分换成"要点列表 → 合成一条 commit" | 纯函数 |
| `chat_complete_raw(cfg, system, user) -> Result<String>` | 发请求,返回**未清洗的 raw content**(从 `chat_complete` 拆出 HTTP + 取 content 部分) | 纯网络,不单测 |
| `generate_map_reduce(cfg, chunks, request_fn) -> Result<String>` | 编排:并发 map(≤3) → reduce;`request_fn` 返回 raw content,map/reduce 各用对应 parse;任一失败向上传播 `Err` | 注入 `request_fn` 可测 |

`chat_complete` 拆为 `chat_complete_raw` + `parse_chat_completion(.., generate_body)`,**对外行为不变**(纯重构,现有调用点 `lib.rs:617` 无感)。`truncate_diff`、`build_prompt`、`strip_*` 系列清洗函数不改。

### 4.1 `split_diff` 切分算法

- 按 `diff --git a/ b/` 文件块边界切分(不把单个文件的 diff 切成两半,保持语义完整)。
- 贪心装箱:当前块 + 下一个文件块 ≤ `max` 则塞入,否则开新块。
- 单个文件块自身 > `max` 的极端情况:该块单独走 `truncate_diff` 截断并标注,独占一批。
- 边界:空 diff 返回空 Vec;diff 刚好等于阈值不进分批路径(由 §3.1 的 `chars ≤ 阈值` 分支拦掉)。

### 4.2 并发实现

采用 `futures-util` 的 `stream::iter(chunks).map(..).buffer_unordered(3).collect()`:
- 新增依赖 `futures-util`(轻量,async 项目标配)。
- map 阶段最多 3 路并发,控速率防 API 限流;reduce 等 map 全部完成后再发单次。

> 取舍:批数通常 ≤5(30000 字符/批),并发收益有限但用户已确认要并发。备选串行更简单,但慢;`tokio::Semaphore` 可免新依赖但写法啰嗦且依赖隐式传递。最终选 `futures-util`。

### 4.3 可测性:请求函数注入 + parse 分层

`generate_map_reduce` 接受 `request_fn: impl Fn(&str, &str) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>`,返回**未清洗的 raw content**(生产传 `|s, u| chat_complete_raw(&cfg, s, u)`,测试传 mock 返回预设 content)。

parse 按阶段分流,在编排内部完成:
- map 阶段:每批 raw content → `parse_map_output`(保留多行要点)。
- reduce 阶段:raw content → `parse_chat_completion(.., generate_body)`(复用现有清洗,取标题 / 标题+正文)。

这样 map / reduce 的 parse 逻辑解耦(要点多行 vs commit 单行),编排逻辑(并发、失败传播)可离线测试,不必真发网络请求。

## 5. UI / 文案

不新增设置项。`Settings.svelte:525` 的 **"diff 截断字符数"** 在分批模式下语义变成"单批上限",补一句说明避免误解:

> diff 截断字符数 — 超过此值时自动分批请求,确保覆盖全部改动

仅文案/tooltip 微调,非功能改动。

## 6. 测试策略

- `split_diff`:按文件边界切、跨文件不切断、单文件超长保底截断、空 diff、刚好等于阈值。
- `build_map_prompt` / `build_reduce_prompt`:断言关键引导词(要点 / conventional / body 规则)。
- `parse_map_output`:多行保留、think 块 / 围栏 / 引号清洗。
- 编排 + 回退(经 `request_fn` 注入 mock):
  - 小 diff → 单次路径(不进 `generate_map_reduce`)。
  - 大 diff → 分批 map+reduce 成功。
  - 分批中某 map 失败 / reduce 失败 → `generate_map_reduce` 返回 `Err`,`ai_generate_commit_message` 回退单次截断。

## 7. 影响面

- **改**:`gui/src-tauri/src/ai.rs`(§4 组件 + 编排)、`gui/src-tauri/src/lib.rs:594`(流程分支 + `spawn_blocking` 边界调整)、`gui/src/lib/Settings.svelte`(一句文案)。
- **新增依赖**:`gui/src-tauri/Cargo.toml` 加 `futures-util`。
- **不改**:`chat_complete`、`truncate_diff`、现有清洗函数、前端调用方式(command 签名不变)。

## 8. 决策记录

| 决策点 | 选择 | 理由 |
|---|---|---|
| 触发阈值 | 沿用 `ai_max_diff_chars` | 不新增设置项,复用现有配置 |
| 失败降级 | 回退单次截断 | 永不比现状差,保证总有结果 |
| map 并发 | `buffer_unordered(3)` + `futures-util` | 温和并发防限流;代码最清晰 |
| 可测性 | 请求函数注入 | 编排逻辑可离线测试,不依赖网络 |
| body 生成 | reduce 阶段支持 `generate_body`;map 阶段只要要点 | map 产短语,reduce 产最终 commit(可含 body) |

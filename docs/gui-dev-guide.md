# git-gui GUI 开发指南

面向接手 GUI 功能开发的同学。骨架(walking skeleton)已搭好并跑通,本文讲**在它之上怎么继续加功能**,以及**必须守住的约束**。

---

## 1. 架构总览

三层,职责单一,从下往上:

```
crates/gitcore     UI 无关的 git 编排核心库(spawn git CLI,零依赖)。所有 git 逻辑都在这。
   ↑ 直接函数调用(同进程,无 IPC)
gui/src-tauri      Tauri 后端(原生 Rust)。把 gitcore 能力包成 #[tauri::command] 暴露给前端。
   ↕ invoke(命令) / emit(事件)  —— 进程内 IPC,数据走 serde JSON
gui/src            SvelteKit 前端(WebView)。画界面,通过 invoke 调后端。
```

一句话:**git 逻辑只写在 gitcore,后端只做"包一层 + 过桥",前端只管渲染和交互。** 不要在后端写 git 逻辑,不要在前端拼 git 命令。

---

## 2. 目录结构

```
git-gui/
  Cargo.toml              workspace(members 含 gui/src-tauri)
  crates/
    gitcore/              核心库;GUI 用时开 serde feature(默认仍零依赖)
    tui/                  终端前端(已完成,仅作交互验证,GUI 不依赖它)
  gui/
    .npmrc                临时:私有源挂了时走官方源(见 §3)
    package.json          前端工具链 + Tauri CLI(devDependency)
    src/
      routes/+page.svelte 前端入口(目前是 status 演示页)
      routes/+layout.ts   ssr=false,纯客户端 SPA
    src-tauri/
      src/lib.rs          命令定义 + Tauri Builder(run())
      src/main.rs         入口(调 gui_lib::run())
      Cargo.toml          后端依赖(tauri + gitcore[features=serde])
      tauri.conf.json     窗口/打包/前端产物路径配置
      capabilities/       权限(命令白名单等)
  docs/                   本文档所在
```

---

## 3. 跑起来

**前置**:Node(已装,nvm)、Rust(brew stable)、git。

**npm 源(重要)**:内网私有源 `192.168.1.49:9091` 当前不可用,`gui/.npmrc` 临时指向官方源。私有源恢复后,删掉 `gui/.npmrc` 即可回到全局配置。**`gui/.npmrc` 要不要提交,取决于团队是否统一走官方源——默认建议先别提交(它会强制覆盖个人/CI 的源)。**

```bash
# 安装前端依赖(首次)
npm --prefix gui install

# 开发模式:起 vite + 编译后端 + 开窗口(热重载)
cd gui && npm run tauri dev

# 只验证编译(不开窗口)
npm --prefix gui run build      # 前端 → gui/build/
cargo build -p gui              # 后端(需 gui/build/ 已存在)
```

> Tauri 的 `generate_context!` 在编译期读 `gui/build/`,所以**编译后端前必须先 build 前端**。

---

## 4. 范例:status 是怎么从 git 走到界面的

已实现的 `repo_status` 是标准范例,五步贯通:

1. **gitcore** 提供 `Repo::open(path)?.status()? -> RepoStatus`(`crates/gitcore/src/status.rs`)。
2. **可序列化**:`RepoStatus`/`FileStatus`/`FileState` 等加了 `#[cfg_attr(feature = "serde", derive(serde::Serialize))]`。
3. **命令**(`gui/src-tauri/src/lib.rs`):
   ```rust
   #[tauri::command]
   fn repo_status(path: String) -> Result<RepoStatus, String> {
       let repo = Repo::open(&path).map_err(|e| e.to_string())?;
       repo.status().map_err(|e| e.to_string())
   }
   ```
   注册进 `tauri::generate_handler![repo_status]`。
4. **前端**(`+page.svelte`)定义对应 TS 接口,`await invoke<RepoStatus>("repo_status", { path })`。
5. 渲染。serde 默认:**enum 单元变体 → 字符串**(`FileState::Staged` → `"Staged"`),**`PathBuf` → 字符串**。

---

## 5. 加一个新功能的标准流程(照着做)

以"做 Changes 视图的 stage 文件"为例,checklist:

1. **gitcore 有现成 API 吗?** 先查 `crates/gitcore/src/lib.rs` 的 `impl Repo`。多数已有(`stage`/`unstage`/`commit`/`unstaged_diff`/`log_graph`/…)。**缺了才在 gitcore 加**,且保持 UI 无关(只 spawn git + 返回数据结构,不碰 UI 概念)。
2. **跨 IPC 的新返回类型**,在 gitcore 里给它加 `#[cfg_attr(feature = "serde", derive(serde::Serialize))]`(只加 Serialize,前端只读)。
3. **后端加命令**:`gui/src-tauri/src/lib.rs` 写 `#[tauri::command] fn xxx(...) -> Result<T, String>`,`.map_err(|e| e.to_string())`。
4. **注册**进 `generate_handler![...]`。
5. **前端**:TS 接口 + `invoke` + 渲染。命令参数 Rust `snake_case` ↔ JS `camelCase` 由 Tauri 自动转。

---

## 6. 必须遵守的约束

### 6.1 gitcore 是同步阻塞的 → 线程模型

每个 gitcore 调用都会 spawn git 子进程并**阻塞当前线程**。绝不能在 UI 线程跑。

- **短操作**(status/diff/log/stage/commit):直接写 `fn`(同步)命令即可。Tauri 在独立线程执行同步命令,**不会卡 UI**。
- **长操作**(update/fetch/push):用 `async` 命令 + `spawn_blocking` 跑 gitcore,进度用 `emit` 推事件,取消用 `CancelToken`。模式示意(update 落地时参照):

  ```rust
  // gitcore 已提供:execute_update_streaming(opts, &mut on_progress, &cancel)
  // CancelToken = Arc<AtomicBool>,clone 共享;Progress { phase, percent, raw }
  #[tauri::command]
  async fn update_repo(app: AppHandle, path: String, op_id: String,
                       state: State<'_, CancelRegistry>) -> Result<UpdateOutcome, String> {
      let cancel = CancelToken::default();
      state.insert(op_id.clone(), cancel.clone());          // 存入注册表供取消
      let res = tauri::async_runtime::spawn_blocking(move || {
          let repo = Repo::open(&path)?;
          let mut on_progress = |p: Progress| { let _ = app.emit("update-progress", p); };
          repo.execute_update_streaming(&UpdateOptions::default(), &mut on_progress, &cancel)
      }).await.unwrap();
      state.remove(&op_id);
      res.map_err(|e| e.to_string())
  }

  // 取消命令:前端点取消 → 查注册表 → token.cancel(),后台下次轮询即中止
  #[tauri::command]
  fn cancel_op(op_id: String, state: State<'_, CancelRegistry>) {
      if let Some(t) = state.get(&op_id) { t.cancel(); }
  }
  ```
  前端 `listen("update-progress", …)` 更新进度条;`UpdateOutcome` 是 tagged enum,前端 match variant 决定 UI(成功/进冲突视图/警告)。**进度的 `Progress` 和 `UpdateOutcome` 也要加 serde derive。**

### 6.2 serde 是 feature-gated

gitcore **默认零依赖**;GUI 通过 `gitcore = { features = ["serde"] }` 开启。加任何跨 IPC 的新类型,记得加 `#[cfg_attr(feature = "serde", derive(...))]`,**不要**无条件 `derive`(会破坏 gitcore 的零依赖)。

derive 哪些按命令的**数据方向**定,别无脑全加:只读展示给前端的类型(如 `RepoStatus`)加 `Serialize`;**需要从前端传回后端**的类型(如 `stage_hunk`/`stage_lines` 入参的 `FileDiff`/`Hunk`)再加 `Deserialize`;只在后端构造、不跨 IPC 的(如 `CommitOptions`,命令只收 `message` 自己组装)两个都不加。

### 6.3 跨平台(Windows)硬规约 —— 现在就守,否则后期重写

目标后续兼容 Windows。以下两条**从现在每次改动就遵守**:

1. **文本解析剥 `\r`**:Windows + autocrlf 下 git 输出带 `\r\n`。任何按 `\n` 切行的解析,行尾统一 `trim_end_matches('\r')`。涉及 `hunk.rs`/`resolve.rs` 等。
2. **不硬编码路径分隔符**:别在前后端拼 `/` 或 `\`,路径交给 git 和 `PathBuf`。

以下适配 Windows 时再做(集中、有界):

3. **spawn git 加 no-window flag**:`git.rs` 所有 `Command::new("git")` 加 `#[cfg(windows)]` 的 `CREATE_NO_WINDOW`,否则每次 spawn 闪黑框。
4. **git 存在性检测**:两平台都做"git 不在 PATH"的友好提示(Windows 不自带 git)。

构建分发:macOS 上无法直接编 Windows 包,跨平台用 CI(GitHub Actions 各平台 runner)各编各的。签名/公证(macOS notarization、Windows 签名)后置。

---

## 7. MVP 路线 + gitcore API 映射

按此顺序长肉(每块都已有 gitcore API 支撑):

| 视图 | 用到的 gitcore API | 缺口 |
|---|---|---|
| **Changes** | `unstaged_diff`/`staged_diff`(文件→hunk→行)、`stage`/`unstage`/`stage_hunk`/`stage_lines`、`commit` | 行内字符级 diff(前端用 diff 库算) |
| **History** | `log_graph` → `GraphRow{ graph, entry }`、`commit_files`、`commit_message` | **log 图目前是 ASCII 串**,MVP 先等宽渲染;产品级曲线图需 gitcore 补结构化拓扑 |
| **Update(安全网)** | `plan_update`、`execute_update_streaming`、`read_conflict`/`resolve_file`/`continue_update`/`abort_update`、`CancelToken`/`Progress` | 无(链路完整) |
| **Conflicts** | `read_conflict` → `Vec<Segment>`、`ConflictHunk::magic()`/`refine()`(魔法棒) | 无(已就绪) |

---

## 8. 已知缺口 / TODO

- [ ] **gitcore 结构化 log 拓扑**:画彩色曲线节点图需要 lane/node/parent-edge,而非现在的 ASCII 前缀串。可见性护城河的关键。
- [ ] **Windows spawn no-window flag** + git 存在性检测(§6.3)。
- [ ] **文件系统 watch**:外部改动(命令行 git/IDE)自动刷新,后端用 `notify` debounce 后 emit `repo-changed`。MVP 先手动刷新。
- [ ] **代码签名 / 公证 + 跨平台 CI**。
- [ ] **行内字符级 diff**(WebStorm 招牌,前端算)。

---

## 附:已验证状态(骨架)

- gitcore + serde feature 编译通过
- 前端 SvelteKit + TS 类型检查通过
- Tauri 后端链接 gitcore、`repo_status` 命令编译通过
- 端到端运行(开窗口看 status):`cd gui && npm run tauri dev`

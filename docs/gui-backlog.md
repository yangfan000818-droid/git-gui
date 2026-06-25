# git-gui GUI 后续派发清单

GUI MVP 四视图(Changes / Update / Conflicts / History)已合入 main。本文是 MVP 之后的后续工作,
每个任务卡**自包含、可直接派发给新会话**。

> **状态(2026-06-25)**:第一批 1–7 ✅ / 第二批 8–12 ✅ / 第三批 13–14 ✅ / 第四批 15–21 ✅ / 第五批 22–26 ✅ / 第六批 27–30 ✅。**第七批 31–39(分支体验 + 仓库运维补全)待派发**。

**派发方式**:把下面的【通用约束】+ 对应【任务卡】一起贴给新会话即可。做完按【工作流】交回审核。

---

## 通用约束(所有任务都遵守)

- **架构**:git 逻辑只写在 `crates/gitcore`,后端(`gui/src-tauri`)只包一层 `#[tauri::command]` 过桥,
  前端(`gui/src`)只渲染 + invoke。不在后端写 git 逻辑、不在前端拼 git 命令。详见 [gui-dev-guide.md](gui-dev-guide.md)。
- **serde 按数据方向加**:跨 IPC 的类型,只展示给前端→只加 `Serialize`;前端回传后端→加 `Deserialize`;
  只在后端构造、不跨 IPC→都不加。**别无脑全加**(历史上反复踩:CommitOptions/Segment/Choice 多加了被收紧)。
  用 `#[cfg_attr(feature = "serde", derive(...))]`,保持 gitcore 默认零依赖。
- **a11y 走 root-cause,别用 svelte-ignore**:svelte 5.56 对 `a11y_no_noninteractive_*` 两条规则的
  svelte-ignore **压不住**。可点击元素用 `<div role="button">` + `tabindex="0"` + `onkeydown`(Enter/Space 激活),
  动态 role 会触发告警→改用静态 role(参照 Changes 的 diff 行、Conflicts 的三栏)。
- **线程模型**:所有涉及 `Repo::open` 的 command 必须 `async fn` + `tauri::async_runtime::spawn_blocking`
  (2026-06-24 已全量转换,64 async / 10 sync 仅配置类)。**新加的 command 凡有 Repo::open → async+spawn_blocking**。
  长网络操作(fetch/push/update)额外 `emit` 进度事件 + `CancelToken` 取消(参照 `execute_update`)。
- **弹窗别用原生 confirm/alert**:Tauri WKWebView 不弹原生 `window.confirm/alert`(静默失效,且 confirm 返回 truthy)。
  改用 `@tauri-apps/plugin-dialog` 的 `ask()`/`message()`(异步,走原生 Rust 对话框)。破坏性操作带 `kind: "warning"`,普通确认不带。
  详见 memory `tauri-dialog-native-silently-fails`。
- **Windows 硬规约**:文本解析按行 `trim_end_matches('\r')`;不硬编码路径分隔符(交给 `PathBuf`/git)。
- **严守任务范围**:只做本卡范围。要扩范围(顺手改别的)**先停下说明**,别 scope creep
  (历史教训:做 topology 时顺带改了 plan 流式化,因超范围 + 改了既有行为被整体回退)。
- **改 gitcore 公共类型必跑全 workspace**:`cargo test -p gitcore` + `cargo build -p tui`,
  只验 gui 会漏掉 tui/测试的编译挂(历史教训:PendingConflicts tuple→struct 漏了)。

## 完成标准(每个任务的基线)

- `cargo clippy -p gitcore --features serde -- -D warnings` 0;`cargo clippy -p gui --all-targets -- -D warnings` 0
- `npm --prefix gui run check`(svelte-check)**0 错误 0 警告**(含 a11y)
- `cargo fmt --all --check`(**注意 cwd 别停在 gui 子目录**,否则 "Failed to find targets");前端 `prettier`(`gui/.prettierrc`)
- 动了 gitcore:`cargo test -p gitcore` 全绿
- **工作流**:实现 → 分步 `cd gui && npm run tauri dev` 运行时自验 → 交主会话审核(未验证项如实标注,别宣称"应该没问题")

---

## 任务卡

### 1. 前端 SVG 彩色曲线图 〔P0 · 依赖 `log_topology`(已合并 #22)〕

**可见性护城河的招牌。** 用 gitcore 已就绪的拓扑数据画 SVG 彩色提交图,替换 History 当前的等宽 ASCII 图。

**已就绪的 API**(main 已含):

```
repo.log_topology(&LogOptions) -> Vec<GraphCommit>
GraphCommit { entry: LogEntry, parents: Vec<String>, lane: usize, edges: Vec<GraphEdge> }
GraphEdge { from_lane: usize, to_lane: usize }   // 本行 from_lane 连到下一行 to_lane
```

语义:每个 commit 一行(row),node 画在它的 `lane` 列;`edges` 是本行各活跃 lane 到下一行的连线。

**先读**:`crates/gitcore/src/topology.rs`(吃透 lane/edge 语义)、`gui/src/lib/HistoryView.svelte`(要换左侧图、复用详情面板)、`gui/src-tauri/src/lib.rs` 的 `repo_log_graph`(命令参照)。

**做**:

1. 后端命令 `repo_log_topology(path, max_count, branch) -> Vec<GraphCommit>`(sync,照 `repo_log_graph`)
2. 前端把 HistoryView 左侧换 SVG:行高固定(如 24px);node = 圆点画在 `(lane * laneWidth, row 中心)`;
   每条 edge 从 `(from_lane, 本行)` 到 `(to_lane, 下一行)`——直传(from==to)竖线、分叉/合并(from!=to)三次贝塞尔曲线;
   lane 索引映射颜色循环(8 色轮转);node 右侧同行渲染 sha/author/date/message
3. 点击 node/行选中 → 复用现有详情面板(message + commit_files diff)
4. 加载更多(max_count 重取)、复制 SHA 沿用现有
5. 可拆 `LogGraph.svelte` 子组件,HistoryView 组合

**验收核心**:**分叉/合并的曲线连得对**(用有分支历史的仓库验,可造几个 merge 或开 git-gui 自身)。

---

### 2. 抽 DiffView 共享组件 〔P1 · 技术债,无依赖〕

**现状**:structured diff 的渲染(`hunkLines` + 文件→hunk→行 markup + 增删着色 CSS)在 `+page.svelte`(Changes)
和 `HistoryView.svelte`(History)里**各复制了一份**。后续 Conflicts/曲线图详情也会用到。

**做**:抽 `gui/src/lib/DiffView.svelte`,props 传 `files: FileDiff[]`;Changes / History 改为复用它。

**关键**:Changes 的 diff 有 **stage / hunk / 行选择交互**,History 是**只读**。DiffView 要支持两态
(props 控制:只读 vs 可交互回调),别为了复用把只读处也塞进交互逻辑、也别把交互处砍成只读。
建议 props 形如 `{ files, interactive?: boolean, onStageHunk?, onStageLines?, ... }`。

**验收**:Changes / History 行为**完全不变**,diff 渲染收敛到一处。

---

### 3. 文件 watch 自动刷新 〔P1 · 体验〕

**目标**:外部改动(命令行 git / IDE 改文件 / 别处提交)后,GUI 当前视图自动刷新,不用手动点。

**做**:后端监视当前打开仓库的工作区 + `.git` 目录,debounce(如 300ms)后 `emit("repo-changed")`;
前端 `listen` 后重新 load 当前视图(status/diff/log)。

**依赖/选型**:先确认 Tauri 2 有没有内置 fs watch 能力(fs 插件);没有再加 `notify` crate
(加在 `gui/src-tauri`,**不是 gitcore**——gitcore 保持零依赖)。**新依赖要在动手时 flag 一下**。

**注意**:`.git/index` 等内部变化也要监视(stage 状态会变);debounce 防抖动;只 watch 当前仓库,切仓库时重置 watcher。

**验收**:命令行 `git add`/改文件后,GUI 在 ~300ms 内自动更新。

---

### 4. 行内字符级 diff 〔P2 · WebStorm 招牌〕

**目标**:diff 的"修改行"显示**行内字符级**高亮(具体改了哪几个字符),不只整行红绿。

**做**:前端在 structured diff 基础上,对**配对的**删除行 + 新增行算字符级 diff,高亮变化字符段。
前端可加 diff 库(如 `diff-match-patch`,**新依赖 flag**)或自写 LCS。

**注意**:只对"成对改动行"(一删一增配对)做,纯增/纯删行不需要;注意长行/大 diff 的性能。
若【任务 2】已抽 DiffView,在 DiffView 里统一加。

**验收**:修改行内能看到具体改了哪几个字符段。

---

### 5. Windows 适配 〔P2 · 跨平台,发布前〕

**目标**:Windows 上可用。文本剥 `\r`、不硬编码分隔符已一路遵守,剩集中适配两条:

**做**:

1. `crates/gitcore/src/git.rs` 所有 `Command::new("git")` 加 `#[cfg(windows)]` 的 `CREATE_NO_WINDOW`
   flag(`std::os::windows::process::CommandExt`),否则每次 spawn 闪黑框
2. **git 存在性检测**:两平台都做"git 不在 PATH"的友好提示(Windows 不自带 git)

**注意**:macOS 上无法直接测 Windows,`#[cfg(windows)]` 代码靠编译过 + CI/真机验。

**验收**:cfg(windows) 代码编译通过;Windows 真机/CI 上无黑框闪烁、缺 git 有友好提示。

---

### 6. 跨平台 CI + 签名/公证 〔P3 · 发布工程〕

**目标**:自动构建三平台安装包 + 签名。

**做**:GitHub Actions 用各平台 runner(macOS / Windows / Linux)各自编 tauri 包(可用 `tauri-action`);
macOS notarization、Windows 代码签名。

**依赖**:签名证书(macOS Developer ID、Windows 代码签名证书)需 ⛵️ 提供。
**签名/公证后置**——先让 CI 出未签名包跑通构建,再接签名。

**验收**:CI 三平台构建绿,产出可安装包。

---

### 7. plan 流式化 〔P3 · 可选,曾被回退〕

**背景**:做 topology 时新会话顺带做过这个(让"检查更新"的 fetch 也有进度/取消),因 scope creep +
改了 plan fetch 子模块的既有行为 + 前端未验证,被整体回退。若确实想要,单独正式做。

**目标**:让 `plan_update`(检查更新)的 fetch 阶段和 `execute_update` 对称——有进度条 + 可取消。

**做**:gitcore `plan_update_streaming`(fetch 带 progress/cancel)+ 后端 `plan_update` 改 async 命令
(参照 `execute_update` + CancelRegistry)+ 前端 `doPlan` 进度/取消 UI + 重入防护。

**派发前先定**:plan 阶段的 fetch **要不要 recurse submodules**?原 plan 是 `git fetch --prune`(不 recurse),
检查更新若 recurse 子模块会变慢——这是产品取舍,定了再派。

**验收**:检查更新有进度/取消,fetch 行为符合上面的决策。

---

## 第二批:对标 WebStorm Git(本次新增)

> 来源:WebStorm/IntelliJ Git 功能调研对比。第一批补齐了"可见性 + 基础操作",
> 第二批补齐**历史操作**(amend/cherry-pick/revert)与**调查能力**(log 搜索/文件历史/blame),
> 这两块是日常 IDE Git 用得最多、当前 GUI 缺口最明显的部分。

### 8. Amend 上次提交 〔P1 · 高频,实现简单〕

**WebStorm 高频操作。** 把当前暂存改动并入上一次提交,或只改上次提交的 message。

**先读**:`crates/gitcore/src/commit.rs`(CommitOptions/commit)、`gui/src-tauri/src/lib.rs` 的 `repo_commit`、`gui/src/routes/+page.svelte` 的提交区。

**做**:

1. gitcore `CommitOptions` 加 `amend: bool`(默认 false,**不跨 IPC、不加 serde**——后端命令直接传参);
   `commit()` 中 amend=true 时用 `commit --amend -m <msg>`,并**跳过"暂存区为空"检查**(amend 允许只改 message)
2. 后端 `repo_commit(path, message, amend: bool)` 透传
3. 前端提交区(`gui/src/routes/+page.svelte`)加"修改上次提交(amend)"勾选;
   勾选时调 `commit_message(mainPath, "HEAD")` 预填 message 输入框,按钮文案改"修改主仓库上次提交"

**多仓库语义(已定)**:提交区是**多仓库统一提交**(`doCommit` 对每个有暂存改动的仓库循环 `repo_commit`,同一条 message)。
amend **仅作用于主仓库**——勾选后切单仓库模式:只对主仓 `repo_commit(mainPath, msg, amend:true)`,
**子仓库此次不参与**(给一行提示:"amend 仅改主仓库上次提交;子仓库改动请取消勾选后单独提交")。
理由:amend 几乎总是针对单个特定仓库,批量改写多仓库历史是伪需求且危险;主仓库是默认焦点。
子仓库各自 amend 是边缘需求,留后续。

**注意**:amend 已 push 的提交需 force push(本清单未做)。勾选时给提示文案,别静默改写已推送历史。

**验收核心**:暂存改动 + 勾 amend 提交后,`git log` 顶部提交被替换而非新增;只改 message 也能成功。

---

### 9. Cherry-pick / Revert 提交 〔P1 · 高频,实现简单〕

**从历史里挑/反转单个提交。** History 视图选中提交 → cherry-pick 到当前分支,或 revert 生成反转提交。

**先读**:`crates/gitcore/src/commit.rs`(命令执行模式 `repo.git`)、`gui/src/lib/HistoryView.svelte`(提交行渲染、选中态)、`gui/src/lib/ConflictView.svelte` + `lib.rs` 的 `resume_conflicts`/`PendingConflicts`(冲突接入)。

**做**(已定接入方案):

1. `Integration` 枚举(`update.rs`)从 Merge/Rebase **扩展为四变体**,加 `CherryPick`、`Revert`;
   `in_progress()` 加检测 `.git/CHERRY_PICK_HEAD` → CherryPick、`.git/REVERT_HEAD` → Revert
2. **四个 `match in_progress` 点都加分支**(编译器 exhaustive 强制,不会漏):
   `continue_update`(`-c core.editor=true -c rerere.enabled=true cherry-pick/revert --continue`)、
   `abort_update`(`cherry-pick/revert --abort`)、`recover_or_strand`(同 abort)、`preflight`(文案加"拣选"/"回退")
3. gitcore 加 `cherry_pick(&self, sha)` / `revert(&self, sha)`:先 `preflight`(复用,防重入)→
   `git cherry-pick <sha>` / `git revert --no-edit <sha>`;干净返回 `UpdateOutcome::Resolved`,
   冲突(有 `conflicted_files`)返回 `UpdateOutcome::Conflicted { files, autostash: None }`(**复用 UpdateOutcome**)
4. 后端 `repo_cherry_pick` / `repo_revert` 返回 UpdateOutcome(前端已有 `outcomeVariant`/`outcomeData` 解析)
5. 前端 HistoryView 提交行右键/详情按钮触发;冲突进**现有 ConflictView**;
   解决后调**现有 `continue_update_cmd`**、放弃调**现有 `abort_update_cmd`**(autostash 传 None)——前端零分发改动

**关键设计决策(已定)**:

- **复用 Update 整套冲突基础设施**:Integration 四变体后,continue/abort 内部按 `in_progress` 自动分发到对的 git 命令,
  前端调同一个 continue/abort 即可,**无需知道当前是哪种操作**;ConflictView 零改动复用。
- **不 autostash**:cherry-pick/revert 是用户主动挑单提交,工作区脏时让 git 自然报错(提示先提交/stash),
  不套 pull 的 autostash 安全网(避免改动纠缠)。故 Conflicted 的 autostash 恒 None。
- **recurse_submodules 传 false**:cherry-pick/revert 不联动子模块同步。

**注意**:改了 `Integration` 公共枚举 → 必跑全 workspace(`cargo test -p gitcore` + `cargo build -p tui`);
exhaustive match 会强制 TUI 侧也处理新变体,别漏。

**验收核心**:cherry-pick 干净提交落到当前分支;revert 生成反转提交;制造冲突进 ConflictView 三栏解决,
continue 完成、abort 干净回退;工作区脏时 cherry-pick 友好报错、不偷偷 stash。

---

### 10. Log 筛选 / 搜索 〔P1 · History 核心体验〕

**WebStorm Log 招牌。** 按作者、提交消息关键词过滤历史,快速定位提交。

**已就绪**:`LogOptions { max_count, branch }`(`log.rs`),`log()`/`log_graph()`/`log_topology()` 三处消费它。

**先读**:`crates/gitcore/src/log.rs`、`crates/gitcore/src/topology.rs`(log_topology 在这)、`gui/src/lib/HistoryView.svelte`。

**做**:

1. `LogOptions` 加 `author: Option<String>`、`grep: Option<String>`;映射到 `git log --author=<> --grep=<> --regexp-ignore-case`
2. **三处都要透传**:`log`、`log_graph`、`log_topology`(topology.rs 单独改,别漏)
3. 后端 `repo_log_topology` 等命令加 author/grep 参数
4. 前端 HistoryView 顶部加筛选行(作者输入 + 消息关键词),debounce 后重新加载

**注意**:改了 `LogOptions` 公共类型 → 跑全 workspace(`cargo test -p gitcore` + `cargo build -p tui`),TUI 也用 LogOptions。

**⚠️ 关键坑(实现时踩到,2026-06-21)**:`--author`/`--grep` 是把**过滤后的残缺提交子集**喂给 `log_topology`,
parent 链断裂导致 topology lane 爆炸(实测某真实多作者仓库 author 筛选下 maxLane 4→35、svgWidth 80→576px),
不仅撑爆布局把提交文字列挤出可视区(`.graph-svg{flex-shrink:0}`+`.log-scroll{overflow-x:hidden}`),那张图本身也无意义。
**修法**:筛选激活时前端不画拓扑图,退化为线性列表(`filtering = authorFilter||grepFilter` 控制 `{#if !filtering}` 包住 SVG)

- `.info-rows{min-width:0}` 纵深防御。**通用教训:凡"在拓扑图上做提交过滤"都有此冲突——图需完整 parent 链,过滤从根上破坏它**。

**验收核心**:输入作者/关键词后 History 只显示匹配提交(**线性列表,不画图**);清空恢复曲线图;
**务必用多作者多 merge 的真实大仓库验**(单作者小仓库不暴露 lane 爆炸)。

**状态**:已实现并修复上述坑(2026-06-21,工作树未提交)。

---

### 11. 文件历史 〔P2 · 调查单文件演变〕

**追溯单个文件的提交历史。** 从 Changes 文件或 diff 里"查看此文件历史",看它历次改动。

**先读**:`crates/gitcore/src/log.rs`(LogEntry/log 模式)、`gui/src/lib/DiffView.svelte`(文件标题,加入口)、`gui/src/lib/HistoryView.svelte`(复用提交列表渲染)。

**做**:

1. gitcore 加 `file_history(&self, path: &Path, opts: &LogOptions) -> Vec<LogEntry>`——`git log --follow -- <path>`(--follow 跟重命名),复用 LogEntry 解析
2. 后端 `repo_file_history(path, file_path, max_count)`
3. 前端入口:Changes 的 DiffView 文件标题旁"历史"按钮(DiffView 加可选 `onFileHistory` prop)→
   弹出该文件提交列表(新组件 `FileHistory`,弹窗/抽屉形态);点提交看该文件在那次提交的 diff
4. **diff 取法(已定:精准,非前端过滤)**:gitcore 加 `commit_file_diff(sha, file_path) -> Option<FileDiff>`
   ——照 `commit_files`(hunk.rs:262)的 `show --no-color --format=` 命令在 `<sha>` 后加 `-- <path>`,
   `parse(&text).into_iter().next()`(改名前匹配不到 → None)+ 后端 `repo_commit_file_diff`;
   FileHistory 点提交时 invoke 它 → DiffView 渲染(None 显示"该提交未改动此文件")
5. 复用 HistoryView 的行渲染样式,别另起一套

**验收核心**:对有多次改动的文件能列出提交历史并逐次看 diff;重命名过的文件用 --follow 能追到改名前。

---

### 12. Git blame 行注解 〔P2 · WebStorm 招牌,UI 较重〕

**逐行追溯作者。** 文件每行显示是谁、哪次提交引入的,点击跳到该提交。

**先读**:`crates/gitcore/src/git.rs`(命令执行)、`gui/src-tauri/src/lib.rs` 的 `read_repo_file`(已有读文件)、`gui/src/lib/HistoryView.svelte`(提交详情,blame 点击跳转复用)。

**做**:

1. gitcore 加 `blame(&self, path: &Path) -> Vec<BlameLine>`,`BlameLine { sha, full_sha, author, date, line_no, content }`;
   解析 `git blame --porcelain <path>`(porcelain 格式稳定,含 author/author-time/行内容)
2. `BlameLine` **只加 `Serialize`**(跨 IPC 给前端,按数据方向)
3. 后端 `repo_blame(path, file_path)`
4. 前端:文件内容逐行左侧显示 blame 列(短 sha + 作者 + 相对时间),点 sha 跳到该提交详情;
   同 sha 连续行合并不重复显示(参照 WebStorm)

**前置考量(派发前先定)**:GUI 现在**没有"打开任意文件逐行看内容"的视图**(只有 diff 视图)。
blame 需要这个载体——要么先做一个简单文件查看器,要么把 blame 入口挂在文件历史/diff 里。定了载体再派。

**注意**:大文件逐行渲染性能;`--porcelain` 解析按行 `trim_end_matches('\r')`(Windows)。

**验收核心**:打开文件能看到每行作者+提交+时间;点 sha 能跳到提交;改名/移动行的归属合理。

---

## 第三批:第二批收尾小项

> 第二批(8-12)已全部合入 main。以下两项是实现卡 11/12 时主动简化、留作后续的小项,自包含可直接做。

### 13. blame 点 sha 跳转提交详情 〔P3 · 卡 12 收尾〕

**现状**:`BlameView`(已合入 830bec2)逐行显示作者/提交,点 sha **暂不跳转**,只 hover `title` 显示完整 sha。

**先读**:`gui/src/lib/BlameView.svelte`(blame 行渲染)、`gui/src/lib/HistoryView.svelte`(提交选中/详情)、`gui/src/routes/+page.svelte`(tab 切换 + 弹窗协调)。

**做(设计点先定,二选一)**:点某行 sha → 看该提交详情。

- **A 切 History 定位**:关 blame 弹窗 → `tab="history"` → HistoryView 选中该 sha 的提交(需 HistoryView 暴露"按 sha 选中",且该 sha 在已加载 log 范围内,否则要加载)。体验完整但跨组件协调多。
- **B 弹内联详情**:BlameView 内点 sha 弹小面板,复用 `repo_commit_message` + `repo_commit_files` 显示该提交 message + 改动文件。自包含、改动小,但与 History 详情重复一套渲染。

**建议**:先做 **B**(自包含、不碰 tab 协调),A 留更后。

**验收**:blame 行点 sha 能看到该提交的 message + 改动文件。

---

### 14. 文件历史接入 History 视图 〔P3 · 卡 11 收尾〕

**现状**:文件历史(`FileHistory` 弹窗)只在 **Changes** 的 DiffView 接了 `onFileHistory`(已合入 083891c);**History 视图**看提交详情时的只读 DiffView(`commitDiffs`)没接,看历史提交里某文件时无法直接查它的完整历史。

**先读**:`gui/src/lib/HistoryView.svelte`(DiffView 调用处 `<DiffView files={commitDiffs} />`)、`gui/src/lib/FileHistory.svelte`、`gui/src/routes/+page.svelte`(FileHistory 弹窗当前挂在 page)。

**做**:

1. HistoryView 的 DiffView 也传 `onFileHistory`
2. **协调点**:FileHistory 弹窗现挂在 +page.svelte,而 HistoryView 是独立组件——要么 HistoryView 把事件回调到 page(由 page 弹 FileHistory,复用一份),要么 HistoryView 自渲染一个 FileHistory 实例。**建议前者**(避免两份弹窗)。

**验收**:History 视图看某提交详情时,对其中一个文件能"查看文件历史"。

---

## 第四批:分支管理(本次新增)

> 来源:多仓库分支展示/切换需求 + WebStorm Branches 功能调研(2026-06-22)。
> **关键现状(已核实)**:gitcore 分支能力**全现成**——`branches()`/`switch_branch()`(脏则报错)/
> `create_branch()`/`delete_branch()`(安全模式,拒删当前/未合并),见 `branch.rs` + `lib.rs:424-440`。
> 缺口只在 **IPC + UI** 两层:`BranchInfo` 没加 serde、后端**零 branch 命令**、前端只显示**主仓**分支(只读),
> 子仓 `ss.branch` 已加载却被丢弃(`+page.svelte:198` 只取了 `ss.files`)。

### 15. 每个仓库当前分支展示 〔P1 · 纯前端,小〕

**痛点直击:看不到各仓库在哪个分支。** 主仓 + 每个子仓在 Changes 左侧分组旁显示当前分支 + ahead/behind。

**先读**:`gui/src/routes/+page.svelte` 的 `RepoView`(56-64)、`buildRepoView`(147-175)、`reload`(177-214)、左侧仓库分组渲染(`{#each repos}` 737 起)、顶部分支显示(681)。

**做**:

1. `RepoView` 加 `branch: string | null`、`ahead: number`、`behind: number`
2. `buildRepoView` 增这三个参数;`reload` 里 main 传 `s.branch/s.ahead/s.behind`,subs 传 `ss.branch/ss.ahead/ss.behind`
   ——**数据已经加载**(195 行的 `ss`),现在 198 行只取了 `ss.files` 把 branch 丢了,接住即可
3. 左侧各仓库分组标题行显示分支名 + `↑{ahead} ↓{behind}` 徽章(为 0 时不显示)

**注意**:detached HEAD 时 `branch=null`,显示 "(detached)";子仓 `Uninitialized` 无分支数据,不显示。

**验收**:主仓 + 每个已初始化子仓在分组旁各自显示当前分支与 ahead/behind;detached/未初始化不报错。

---

### 16. 分支切换(checkout) 〔P1 · 补 IPC + UI〕

**痛点直击:不能切换分支。** 点仓库分支名 → 弹本地分支列表 → 选中切换。

**先读**:`crates/gitcore/src/branch.rs`(`BranchInfo`/`list_branches`/`switch_branch`)、`crates/gitcore/src/lib.rs:424-440`(已暴露的 Repo 方法)、`gui/src-tauri/src/lib.rs`(命令注册模式,参照 `repo_status`)、卡 15 的分支展示 UI。

**做**:

1. `BranchInfo` 加 `#[cfg_attr(feature = "serde", derive(serde::Serialize))]`——**只读给前端,按方向只加 Serialize**(别加 Deserialize)
2. 后端两个 **sync** 命令(短操作,Tauri 独立线程):`repo_branches(path) -> Vec<BranchInfo>`、`repo_switch_branch(path, name)`
3. 前端:卡 15 的分支名做成可点 → 弹分支列表面板(新组件 `BranchPicker.svelte` 或内联下拉):
   列 `repo_branches` 结果(当前分支高亮 + ✓,显示 upstream + ↑↓),点非当前分支 → `repo_switch_branch(repo.path, name)` → `reload()`
4. **错误处理**:`switch_branch` 工作区脏会返回 `Error::Precondition("工作区有未提交改动...")` → 前端提示(**别静默吞**),引导先提交/暂存

**多仓库语义(已定)**:本卡只做**单仓切换**——每个仓库各自点自己的分支名、调各自 `repo.path` 的 `repo_switch_branch`。多仓库统一切换留卡 18。

**注意**:`BranchInfo` 加 serde 是 feature-gated,不影响 TUI;仍按规约跑 `cargo test -p gitcore` + `cargo build -p tui`。前端 `BranchInfo` interface 字段命名(`is_current`/`ahead`/`behind`)要与 Rust 对齐。

**验收核心**:点某仓分支名弹出本地分支列表,选另一分支能切过去、UI 刷新;脏工作区切换给友好报错不静默;detached 状态能正常列分支。

---

### 17. 新建 / 删除分支 〔P2 · gitcore 已有,接 UI〕

**先读**:`crates/gitcore/src/branch.rs`(`create_branch`/`delete_branch`)、卡 16 的分支列表面板。

**做**:

1. 后端 sync 命令 `repo_create_branch(path, name)`、`repo_delete_branch(path, name)`
2. 分支列表面板加"+ 新建分支"输入框;每个**非当前**分支项加删除按钮
3. **设计点(先定)**:新建后是否自动切换?WebStorm "New Branch" 默认**建 + 切**,而 gitcore `create_branch` 只建不切。
   建议:新建后接 `switch_branch` 一起切(贴合 WebStorm),或面板给"新建并切换 / 仅新建"两个选项

**注意**:`delete_branch` 是安全模式(拒删当前分支、拒删未合并 `-d`)→ 报错要前端提示;不提供 `-D` 强删(避免误删未合并工作),需要时再单列。

**验收**:能新建分支(按定好的策略切或不切)、删除已合并分支;删当前/未合并分支有友好提示。

---

### 18. 多仓库统一切换 + 分支进阶 〔P2/P3 · WebStorm 招牌 + 后置重项〕

**P2 — 多仓库统一切换(WebStorm「Common Branches」招牌)**:
所有仓库都存在某同名分支时,提供"一次切全部"(参照现有**统一提交框**的多仓循环模式):
列出各仓共有的分支 → 选一个 → 逐仓 `repo_switch_branch` → 汇总成功/失败(某仓脏则跳过并提示)。

**P3 — 后置重项(需要时再细化展开成单卡)**:

- ~~**脏工作区 smart checkout(autostash)**:切换前自动 stash → 切 → pop;可复用 `update.rs` 的 autostash 设施~~ ✅ 已合入 (8d69747,switch_branch_autostash + SwitchOutcome;BranchPicker 脏被挡时弹确认走 smart checkout,贴回冲突 alert 提示;3 集成测试)
  (设计:`switch_branch` 本身不变保守,smart checkout 为脏被挡时的可选回退;贴回冲突不进合并 continue/abort,改动安全留工作区+stash。子仓"全部更新"脏处理见 **卡20**)
- ~~**rename 分支**:gitcore 需新增 `rename_branch`(`git branch -m`)~~ ✅ 已合入 (29cc249,BranchPicker ✎ 内联重命名)
- **Merge into current / Rebase current onto**:可接 `update.rs` 已有的合并/rebase + 冲突(ConflictView)设施 ~~← 已合入 (87428a3,BranchPicker「合并」「变基」按钮 +page ConflictView 弹层)~~
- ~~**Compare with current**:看两分支间差异提交列表,UI 较重~~ ✅ 已合入 (f9dbfcd,BranchPicker 本地/远程行「比较」按钮 → +page 双向独有提交弹层;gitcore compare_commits)
- ~~**New Branch from Selected**(WebStorm 调研补):从**指定分支**新建~~ ✅ 已合入 (578096f,create_branch 加起点参数 + 新建区「起点」下拉)
- ~~**Show Diff with Working Tree**(WebStorm 调研补):选定分支与工作区差异;本地+远程分支均支持~~ ✅ 已合入 (f2846f4,BranchPicker 本地/远程行「差异」按钮 → +page DiffView 只读弹层;gitcore diff_with_workdir)

**验收**:多仓同名分支一次切换、失败仓有提示;进阶项各自验收。

---

### 19. 远程分支查看 + 检出 〔P2 · 对标 WebStorm「Remote Branches」,填补当前最大缺口〕

**痛点直击:看不到 `origin/*` 远程分支,也无法从远程分支拉一个本地跟踪分支。**
当前 `BranchPicker` 只列本地分支(`list_branches` 只扫 `refs/heads/`);WebStorm 的 Branches 弹窗有独立
「Remote Branches」区,可直接 checkout 远程分支为本地跟踪分支——日常协作高频,当前完全缺失。

**WebStorm 对标(2026-06-22 调研)**:

- **Remote Branches** 区列出最近一次 fetch 后的所有远程分支,按 `/` 前缀分组
- 选远程分支 → **Checkout**:建一个跟踪它的本地分支(`git checkout -b <local> --track origin/<x>`)
- **Checkout and Update**:checkout + 同步到远程最新(本卡先不做 Update 部分,留进阶)
- 顶部 **Fetch** 按钮:刷新远程分支列表 + 显示 incoming 指示
- (多根项目「Execute branch operations on all roots」我方已有 CommonBranchPicker 对应)

**先读**:`crates/gitcore/src/branch.rs`(`list_branches`/`BranchInfo`,只扫 refs/heads/)、
`crates/gitcore/src/lib.rs:185`(已有 fetch 流,在 update streaming 内)、
`gui/src/lib/BranchPicker.svelte`(本地分支弹窗,加远程分组)。

**做**:

1. gitcore:`BranchInfo` 加 `is_remote: bool`;新增 `list_remote_branches()` 扫 `refs/remotes/`
   (`for-each-ref refs/remotes/`),**过滤掉 `origin/HEAD`**(符号引用,非真分支)。远程项无 ahead/behind/upstream(填 None/0)
2. gitcore:`checkout_remote(remote_branch: &str)`——先脏检查(复用 `switch_branch` 的脏拒绝逻辑)→
   `git checkout -b <local> --track <remote>`;本地名 = 远程名去掉 `origin/` 前缀;**本地已存在同名则报错**(不覆盖,引导用户直接切过去)
3. gitcore:独立 `fetch_prune()`(给「刷新远程」用)。**先确认** `lib.rs:185` 的 fetch 能否单独调用,不能则从 update streaming 里抽一个可复用的 fetch
4. 后端命令:`repo_remote_branches(path)`(sync)、`repo_checkout_remote(path, remote_branch)`(sync)、
   `repo_fetch(path)`——fetch 是**网络操作** → async + spawn_blocking + 进度(参照 `execute_update`);或先做最简同步版并 **flag 取舍**
5. 前端 `BranchPicker`:分「本地 / 远程」两组;远程组每项点击 = `repo_checkout_remote` → `reload()`;
   顶部加 **Fetch** 按钮刷新远程列表;`is_remote` 项**不显示删除按钮**(删远程分支是另一回事,不在本卡)

**注意**:

- 改了 `BranchInfo` 公共类型 → 跑全 workspace(`cargo test -p gitcore` + `cargo build -p tui`);serde 仍 feature-gated
- 检出失败两种常见:本地同名已存在、工作区脏 → 都前端友好提示**不静默吞**
- fetch 慢/可能失败(无网络/认证)——按长操作处理或至少 loading + 错误提示

**验收核心**:Fetch 后 BranchPicker 远程组列出 `origin/*`;选一个远程分支能检出为本地跟踪分支并切过去;
本地同名已存在/工作区脏给友好报错;`origin/HEAD` 不出现在列表。

---

### 20. 全部更新:子仓改为「在当前分支 pull」(对标 WebStorm,修两个真实问题) 〔P1 · 行为修正,中等设计变更〕

**两个真实用户问题(2026-06-22 反馈),同源**:

1. **更新后子仓变成 detached HEAD,丢掉原来选中的分支** —— WebStorm 更新后仍停在原分支
2. **子仓脏工作区时无 autostash 安全网** —— 仅 git 报错(英文原文),不像主仓那样自动 stash,也无"请先提交/暂存"友好提示

**根因(已核实)**:子仓是真 git submodule;全部更新对子仓走 `git submodule update --remote --init`
(`submodule.rs:120`)——这是 **checkout 模式**,把子模块检出到远程跟踪分支最新提交的**具体 SHA(detached HEAD)**,
不在任何分支上。这是 `git submodule update` 的设计行为,但与用户(及 WebStorm)预期不符。

**WebStorm 对标**:把每个 submodule 当**独立仓库 root**,「Update Project」在各仓库**当前所在分支**上做
update(fetch + merge/rebase upstream),整合后**仍停在原分支**;不用 `submodule update --remote` 的 detached 检出。

**做(已定方向)**:全部更新里子仓处理从 `submodule update --remote` 改为**像主仓一样在子仓当前分支上 pull**——
把每个子仓当独立 repo 复用现有 `execute_update`(fetch + 整合 upstream)。一举两得:

- ✅ 更新后**留在原分支**(修问题 1)
- ✅ `execute_update` 自带 **autostash 安全网**(修问题 2)
- ✅ 复用现成冲突处理(子仓冲突也能进 ConflictView)

**先读**:`crates/gitcore/src/submodule.rs:115-122`(`update_submodule_remote`,要替换的路径)、
`crates/gitcore/src/update.rs`(`execute_update`,子仓复用它)、
`gui/src/lib/UpdateView.svelte:241-265`(`proceedToSubmodules`,前端逐仓循环 + outcome 汇总)、
`crates/gitcore/src/status.rs:49-51`(子仓当前分支判定)、`submodule.rs:80-106`(detached 检测)。

**必须处理的边界(没有"当前分支"可 pull 的子仓)**:
| 子仓状态 | 处理 |
|---|---|
| 在分支 + 有 upstream | 在当前分支 pull(主路径) |
| **detached HEAD** | 跳过 + 提示(无分支可留),或保留旧 `submodule update` 行为(**派发前定**) |
| 当前分支**无 upstream** | 跳过 + 提示"无上游分支",不报红错 |

**注意**:

- 这是**行为变更**——改了"全部更新"对所有子仓的语义,前端 outcome 子仓汇总要能展示「已 pull / 跳过(detached/无上游)/ 冲突」多态
- 子仓 pull 复用 `execute_update`,即继承其 strategy(merge/rebase)/autostash/冲突全套;前端要让子仓冲突也能进 ConflictView(当前只有主仓走冲突解决,子仓只是 try/catch 记错)——**这块是主要工作量,派发前评估**
- 改了子仓更新语义,跑全 workspace 验证(`cargo test -p gitcore` + `cargo build -p tui`);TUI 若也有"全部更新"路径需同步

**验收核心**:子仓在分支上 + 全部更新后**仍在原分支**(不再 detached);子仓脏时自动 stash→pull→pop(不再裸报错);
detached/无上游子仓被跳过并友好提示;子仓 pull 冲突能进 ConflictView 解决。

**状态**:已实现并合入 (0d44de6)。两处对标 WebStorm 的差距(子仓冲突进 ConflictView、detached 处理)拆到 **卡21** 收尾。

---

### 21. 全部更新:对标 WebStorm 的两处子仓收尾 〔卡20 后续 · WebStorm Update Project 调研 2026-06-22〕

**背景**:卡20 已让子仓"更新后留在原分支"、脏子仓 autostash,与 WebStorm「Update Project」对齐。
对标后仍剩两处差距,本卡收尾。

**对标 WebStorm(官方文档 + 实测)**:
| 维度 | WebStorm | git-gui(卡20 后) | 差距 |
|---|---|---|---|
| 子仓在分支上更新后留分支 | ✓ | ✓ | 已对齐 |
| 脏树 autostash | ✓(stash/shelve) | ✓(stash) | 已对齐 |
| 无上游子仓 | 跳过 | 跳过 + 提示 | 已对齐 |
| **子仓更新冲突** | 逐 root 进三栏合并工具解决 | 回退 + 提示,不进 UI | **缺口 A** |
| **detached 子仓** | `git submodule update` 同步到父仓记录 commit | 完全跳过 | **缺口 B** |
| 更新策略 Reset(丢本地对齐远程) | 有(第三种) | 无 | 不做(见末) |

**A〔P2 · 子仓冲突进 ConflictView〕**(卡20 的主要 descope):
子仓 pull 冲突时像主仓一样进 ConflictView 三栏解决,而非现在的回退 + 提示。

- **关键改造**:UpdateView 状态机要支持"子仓循环中途遇冲突 → 暂停 → 进 ConflictView → 解决/放弃 → 恢复剩余子仓"。
  现有 `proceedToSubmodules` 是一把梭循环,冲突即中断,需把"剩余子仓队列"存进状态、解决后从断点续跑。
  ConflictView / continue / abort 都已 path 参数化,对子仓绝对路径可**直接复用,无需新组件**。
- **先读**:`gui/src/lib/UpdateView.svelte`(`proceedToSubmodules` + `conflict_resolution` phase)、
  `crates/gitcore/src/update.rs`(`update_submodule_on_branch`,现在冲突走 abort,要改成把冲突交还前端)。
- **做**:gitcore `update_submodule_on_branch` 冲突时**不再 abort**,返回带子仓 path + 冲突文件的 Conflicted;
  UpdateView 加"子仓冲突解决"子状态,复用 ConflictView(path=子仓绝对路径),解决调 continue、放弃调 abort,再 resume 剩余子仓队列。
- **验收**:制造子仓 pull 冲突 → 进 ConflictView 三栏 → 解决后该子仓完成并继续更新剩余子仓;放弃则该子仓回退、剩余继续。

**B〔P3 · detached 子仓同步到记录点〕**(跟 WebStorm,更合理):
当前 detached 子仓一律跳过;WebStorm 对 detached 子仓执行 `git submodule update`(checkout 到父仓 gitlink 记录的 commit)。
detached 本无"要保留的分支位置",同步到记录点更有用。

- **做**:`update_submodule_on_branch` 的 `SkippedDetached` 分支改为调用现有 `update_submodule`
  (`git submodule update --init -- <path>`,即同步到记录 commit);`SubmoduleUpdate` 加变体如 `SyncedToRecorded`
  (前端展示"已同步到记录提交")。注:非 `--remote` 的 `update_submodule` 函数仍在,直接复用。
- **边界**:detached 且工作区脏时 `submodule update` 会失败 → 仍按失败提示(不强切)。
- **验收**:detached 子仓全部更新后被同步到父仓记录 commit(仍 detached 但指向记录点),脏则友好失败。

**不做 — Reset 更新策略**:WebStorm 第三种「Reset to the Remote Branch」丢弃本地提交硬对齐远程,
破坏性强,与本项目"保守、不丢改动"取向冲突,**不补**;确有需要再单列。

**验收**:A/B 各自验收;对标 WebStorm 的"全部更新"子仓行为差距清零(Reset 除外,有意不做)。

**状态**:A(506f0b7)+ B(c2dc9c8)已合入。对标 WebStorm 的子仓行为差距已清零(Reset 有意不做)。
**验证**:✅ 已完整闭环。gitcore 后端逻辑端到端自动验证(真实 submodule 一次性测试)+ GUI 真机验证均通过:
留分支/detached 同步记录点/无上游跳过(卡20、21-B)、冲突暂停→进 ConflictView 解决→续跑剩余子仓(卡21-A)全部正确。
手动 QA 脚本留作回归:`scripts/qa-submodule-update.sh`。

---

## 优先级小结(建议,可调)

**第一批(MVP 后续)已全部落地** ✅:1 SVG 曲线图 / 2 DiffView 共享组件 / 3 文件 watch / 4 行内字符级 diff / 5 Windows 适配 / 6 跨平台 CI / 7 plan 流式化。

**第二批(对标 WebStorm,本次新增)进度**:

| 优先级 | 任务                    | 价值                  | 状态                |
| ------ | ----------------------- | --------------------- | ------------------- |
| **P1** | 8. Amend 上次提交       | 高频,实现简单         | ✅ 已合入 (aa5c503) |
| **P1** | 9. Cherry-pick / Revert | 高频,接现有冲突流程   | ✅ 已合入 (d91e192) |
| **P1** | 10. Log 筛选 / 搜索     | History 核心体验      | ✅ 已合入 (b2315ce) |
| **P2** | 11. 文件历史            | 调查单文件演变        | ✅ 已合入 (083891c) |
| **P2** | 12. Git blame 行注解    | WebStorm 招牌,UI 较重 | ✅ 已合入 (830bec2) |

**第三批(收尾小项)**:

| 优先级 | 任务                      | 价值               | 状态                               |
| ------ | ------------------------- | ------------------ | ---------------------------------- |
| **P3** | 13. blame 点 sha 跳转提交 | 卡 12 招牌细节补全 | ✅ 已提交 (b35a0f2)·B 方案内联详情 |
| **P3** | 14. 文件历史 History 入口 | 卡 11 入口覆盖     | ✅ 已提交 (a7dc6d4)                |

**第四批(分支管理,本次新增)**:

| 优先级    | 任务                              | 价值                                            | 状态                                                |
| --------- | --------------------------------- | ----------------------------------------------- | --------------------------------------------------- |
| **P1**    | 15. 每仓分支展示                  | 直击"看不到各仓分支",纯前端                     | ✅ 已合入 (6273d1b)                                 |
| **P1**    | 16. 分支切换 checkout             | 直击"不能切换",底层现成                         | ✅ 已合入 (6273d1b)                                 |
| **P2**    | 17. 新建 / 删除分支               | gitcore 已有,接 UI                              | ✅ 已合入 (6273d1b)                                 |
| **P2/P3** | 18. 多仓统一切换 + 进阶           | WebStorm 招牌 + 后置重项                        | ✅ P2 已合入 (be409ff,二次确认 682e544);P3 进阶待做 |
| **P2**    | 19. 远程分支查看 + 检出           | 对标 WebStorm Remote Branches,当前最大缺口      | ✅ 已合入 (66a4e64)                                 |
| **P1**    | 20. 全部更新子仓改"当前分支 pull" | 修 detached + 脏子仓无 autostash,对标 WebStorm  | ✅ 已合入 (0d44de6)·子仓冲突/detached 收尾见卡21    |
| **P2/P3** | 21. 全部更新对标 WebStorm 收尾    | A 子仓冲突进 ConflictView·B detached 同步记录点 | ✅ 已合入 (A 506f0b7·B c2dc9c8)                     |

---

## 第五批:对标 WebStorm P0 缺口(2026-06-24 调研)

> 来源:WebStorm/IntelliJ IDEA Git 竞品对比,找出 git-gui 日常使用频率最高、但尚未覆盖的功能缺口。
> P0=高频痛点,共 6 项。其中局部暂存(逐行/hunk stage)**经核实已完整实现**(gitcore+hunk→backend→DiffView 三级),跳过。
> 其余 5 项按实现复杂度排序:轻量改动优先、需新组件/新概念的后置。

### 22. Log 分支筛选器 〔P0 · 简单,后端已完备,只缺 UI〕

**痛点:History 日志只显示当前分支的提交。** 在活跃仓库中想看另一个分支的历史 → 得先切分支再切回来看,打断工作流。WebStorm Log 顶部有分支下拉框(All/本地/远程分支),快速切换视角。

**已就绪**:`LogOptions` 已有 `branch: Option<String>`(`log.rs:21`),`repo_log_topology` 已接受 branch 参数。缺口只在**前端**——HistoryView 传 `branch: null`,没有选择 UI。

**先读**:`crates/gitcore/src/log.rs`(`LogOptions`)、`gui/src-tauri/src/lib.rs` 的 `repo_log_topology`(branch 参数)、`gui/src/lib/HistoryView.svelte`(`load` 中的 log 查询 + filter 行 UI)。

**做**:

1. HistoryView 的 filter 行(作者输入 + 消息关键词)加一个**分支下拉框**:`<select>` 或 combobox,选项 `["全部", ...来自 repo_branches 的本地分支名]`
2. 加载分支列表:invoke `repo_branches(path)`,取各 `BranchInfo.name` 填充选项;默认选中「全部」(`branch: null`)
3. `loadLog` 里把选中分支传给 `repo_log_topology` 的 `branch: selectedBranch || null`
4. `repo_log_merged`(多仓库版本)也透传 branch 参数(当前硬编码 `None`,后端 command 补参数)

**注意**:

- 全分支(=null)时 topology 图正常显示;单分支时 parent 链可能断裂 → **复用卡 10 的修法**:筛选激活时切线性列表、不画拓扑图(`filtering = !!selectedBranch || !!authorFilter || !!grepFilter`)
- 分支列表要和 BranchPicker 一样**实时刷新**(切分支/新建/删除后下拉框内容跟着变)
- 只包本地分支;远程分支在本地没有 tracking 分支时看历史是无意义的(可后续加 `--all` 选项)

**验收**:切分支下拉 → History 显示对应分支的提交(线性列表);切回「全部」→ 恢复拓扑图;多仓库时每个仓库按各自分支筛选。

---

### 23. 提交并推送一键(Commit and Push) 〔P0 · 简单,后端已有 push 基础〕

**痛点:提交和推送是独立的两次操作。** WebStorm commit 按钮有下拉菜单默认"Commit and Push",提交后立即弹 Push 对话框。当前 git-gui commit 后要单独找 push 入口。

**先读**:`gui/src/routes/+page.svelte` commit 区(按钮 + `doCommit`)、push 触发(`doPush`/`openUpdateThenPush`)。

**做**:

1. commit 区加复选框「**提交后推送**」(默认关闭,记住上次选择)
2. 提交完成后(无错误),若勾选 → 自动调 `doPushAll()` 或 `doPush()`(参照现有 `openUpdateThenPush` 的 push 逻辑)
3. 推送失败不阻塞、不吞错——在前端就地展示推送失败信息

**注意**:

- 这是纯前端改动,不碰 gitcore/后端
- 当前 push 无进度/预览/force push(那是卡 24),先做最简版:提交→推,够用
- 多仓库提交时推送也跟着多仓循环(已有 `doPushAll` 模式)

**验收**:勾选提交后推送 → commit 成功 → 自动 push;push 成功/失败都有反馈;不勾选时行为不变。

---

### 24. Push 对话框:提交预览 + 强制推送 + 进度 〔P0 · 中等,gitcore 已有 push_streaming,补 IPC+UI〕

**痛点:push 是单向不可逆操作,当前"点一下就开始推"没有预览、没有进度、没有 force push,缺安全网。** WebStorm Push 对话框(Ctrl+Shift+K)列出待推送提交、显示目标远程分支、提供 force push(--force-with-lease)选项、有进度条。

**已就绪**:gitcore `push.rs` 有 `push_streaming()`(流式进度 + 取消)。缺口:后端无对应 Tauri command(只有阻塞式 `repo_push`)、前端无预览/确认/进度 UI。

**先读**:`crates/gitcore/src/push.rs`(`push_streaming`)、`gui/src-tauri/src/lib.rs` 的 `repo_push`(阻塞式)、`gui/src/routes/+page.svelte` 的 `doPush`/`doPushAll`、`gui/src/lib/UpdateView.svelte`(pull 有进度,参照其进度模式)。

**做**:

1. 后端:加 `repo_push_streaming`(async + spawn_blocking,参照 `execute_update` 的 emit 进度 + CancelToken 模式),返回 `PushOutcome { pushed, error? }`
2. 后端:加 `repo_push_force(path, force_with_lease: bool)`——`git push --force-with-lease`(安全强制推送),不提供裸 `--force`
3. 前端:Push 对话框组件 `PushDialog.svelte`(弹窗/模态):
   - **待推送提交列表**:`git log <upstream>..HEAD --oneline` 列出本地领先远程的提交(预览安全网)
   - 目标远程分支名(当前仓库 upstream 的 remote/branch)
   - 「强制推送(--force-with-lease)」复选框(默认关,危险操作带确认)
   - 推送进度条 + 取消按钮
   - 推送结果(成功/失败/NonFastForward)
4. 替换现有"doPush 直接推"→ 先弹 PushDialog 确认
5. 多仓库时逐仓弹或一次列出所有仓库的待推汇总(建议先逐仓,后续升级)

**注意**:

- `push_streaming` 是网络操作 → 必须 async+spawn_blocking
- `--force-with-lease` 比 `--force` 安全(只有没人在你之后推过才允许),不做裸 `--force`
- PushDialog 回调使 `onClose(result)` 让 page 统一处理 reload

**验收**:push 前看到待推提交列表和远程目标;正常推送到远程、进度条显示;force-with-lease 可推到已更新远程、被别人抢先推时正确报错;取消按钮终止推送。

---

### 25. Changelists:多组命名变更集 〔P0 · 大功能,全新概念,纯前端状态管理〕

**痛点:同一分支上并行开展多项独立修改,无法分别提交。** 比如一边修 bug 一边写新功能,两个改动混在同一个 Changes 面板里,想分别提交只能手动 git add 或用命令行 `git add -p`。WebStorm 的 Changelists 允许创建多个命名分组(如 "bugfix"、"feature-a"),新修改自动进当前活跃分组,可拖拽文件跨组移动,按 changelist 分别 commit。

**当前基础:零。** 文件仅按仓库 + staged/unstaged 两级固定分组(`RepoView` 里 `unstaged: FileEntry[]` / `staged: FileEntry[]`),没有用户自定义分组的存储和渲染。

**先读**:`gui/src/routes/+page.svelte` 的 `RepoView` 接口、`buildRepoView`、`doCommit`/`doStageAll`(按仓库循环)、Changes 区域渲染(左侧仓库分组 + 文件列表)。

**设计决策(先定,再实现)**:

| 决策点     | 选择                                                                                  | 理由                                       |
| ---------- | ------------------------------------------------------------------------------------- | ------------------------------------------ |
| 存储方式   | 客户端 localStorage / Tauri store                                                     | 非 git 数据,不存 .git 里;跨会话保留        |
| 默认行为   | 始终存在一个"默认" changelist,新修改自动归入                                          | 不给未分组用户加额外操作                   |
| 多仓库     | 每个仓库独立 changelist 集合                                                          | 不同仓库的修改混在一个 changelist 没意义   |
| Stage 关系 | changelist 内的文件仍分 staged/unstaged;**暂存区是 git 概念,changelist 只是视图分组** | 不改变 git 暂存区模型,只在 UI 层做文件分组 |
| Commit     | 可 commit 单个 changelist(仅该组文件),也保留"全提交"入口                              | 弹性;默认行为不破坏                        |

**做**:

1. gitcore/后端:**不动**(非 git 概念,纯前端状态)
2. 前端状态管理:每个仓库的 changelist 列表 + active changelist ID,存储到 localStorage(`repoPath → { changelists: [{id, name}], activeId, fileAssignments: {filePath: changelistId} }`)
3. UI 改造(Changes 左侧):
   - 仓库分组内再加一层 **changelist 分组**:`<changelist-header>(可折叠)` + 其下 staged/unstaged 文件
   - 顶部:活跃 changelist 下拉切换 + 新建/重命名/删除 changelist 按钮
   - 文件行右侧:小下拉或拖拽手柄移到其他 changelist
4. Commit:提交区显示"提交 changelist X 的改动(N 个文件)",只 stage+commit 该 changelist 的 unstaged 文件
5. 「全部提交」入口保留(跳过 changelist 分组,所有文件一起提交)

**注意**:

- 这是**纯前端功能**,不碰 gitcore。不需要新依赖。
- 存储用 `localStorage`,简单直接;后续可升级到 Tauri store plugin
- 新修改自动进 active changelist → 不用手动分配
- 删除 changelist 时其文件自动移回默认 changelist(不丢修改)

**验收**:创建 changelist,活跃修改自动归入;跨 changelist 拖拽文件;按单个 changelist 提交(仅该组文件进入 commit);切换活跃 changelist 后新修改归入新组。

---

### 26. 文件查看器 + 行内变更标记 〔P0 · 大功能,需要新 gitcore 能力 + 新组件〕

**痛点:看不到文件的完整内容,更看不到文件内容的变更位置。** git-gui 只有 diff 视图(只显示差异片段),没有"打开这个文件看全文各行"的能力。WebStorm 在编辑器行号旁有彩色标记(蓝=改/绿=增/灰=删),点标记看旧版内容,一键 revert 该块。这是 JetBrains 用户每天看到最多次的 Git 功能。

**当前基础:零。** gitcore 无 `cat_file_at_rev`(按 revision 读文件内容);只有 `read_repo_file`(std::fs 读工作树,不认 revision)。BlameView 显示了行级内容但仅限 HEAD 当前版且无变更感知。

**先读**:`crates/gitcore/src/git.rs`(git 命令执行模式)、`gui/src-tauri/src/lib.rs` 的 `read_repo_file`(用作参照)、`gui/src/lib/BlameView.svelte`(行级内容渲染,可复用样式)、`gui/src/lib/DiffView.svelte`(diff 渲染,对照)。

**设计决策**:

| 决策点     | 选择                                                                                               | 理由                                                                               |
| ---------- | -------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| git 命令   | `git show <revision>:<path>`                                                                       | 读取任意 revision 的文件内容(包括 blob SHA);空结果 = 该提交中文件不存在            |
| 文件查看器 | 新组件 `FileViewer.svelte`,props `{ repoPath, filePath, revision? }`,不传 revision=工作区最新      | 通用;BlameView 后续可复用 FileViewer 渲染内容部分                                  |
| 变更标记   | FileViewer 内置 gutter(左侧列):工作区文件 vs HEAD 逐行对比,着色 `added/modified/deleted/unchanged` | 这是"内联标记"在纯 Git GUI 中的等价物;工作区文件点一下就能看每行相对于 HEAD 的状态 |
| 行点击行为 | 点变更行 gutter → 弹出小提示(旧值 → 新值);后续可升级为"Revert 此变更"(慎,改动不可逆,下一批)        | 先做到"看"不做"改"                                                                 |
| 入口       | Changes 面板文件行"查看"按钮 → 弹窗打开 FileViewer;BlameView 的文件名做成可点 → 打开 FileViewer    | 多入口、同一个 FileViewer 组件                                                     |

**做**:

1. gitcore:加 `cat_file_at_rev(&self, revision: &str, path: &Path) -> Result<String>`——`git show <revision>:<path>`,返回文件全文(大文件需考虑截断,如 >1MB 提示切换命令行)
2. 后端:`repo_cat_file(path, revision, file_path)` + `repo_get_head_sha(path)`(取 HEAD SHA 做对比基准)
3. 前端组件 `FileViewer.svelte`:
   - 行号列(等宽,左边固定)
   - 内容列(等宽 monospace,语法高亮可选不做)
   - **变更 gutter**:每行左侧 4px 色条;判定逻辑:取当前文件内容(`revision=HEAD` 或 `read_repo_file`) → 逐行比较 → 着 unmodified(无色)/added(绿)/modified(蓝)/deleted(灰,只在 diff 视图出现,查看器里就是 deleted)
   - 点变更行 → tooltip 显示原行内容(从 HEAD 版本取该行)
4. 入口:Changes 面板文件行右侧加"📄 查看"图标按钮 → 弹窗 `<FileViewer repoPath path={file.path} />`

**注意**:

- `cat_file_at_rev` 加进 gitcore → `BlameLine`/`FileEntry` 不变,不跨 IPC 不加 serde(仅后端接收参数)
- revision 是任意 git reference(sha/branch/tag/HEAD~3 等),命令透传
- 大文件:限制输出大小(如 1MB),超出提示"文件过大,请在命令行查看"
- 只读视图,不做编辑(不做代码编辑器)

**验收**:打开文件看到完整内容 + 行号;变更行左侧有彩色标记;点变更行看到旧行内容;BlameView 里文件名可点打开文件查看器;大文件有提示不卡死。

---

## 第五批优先级小结

| 优先级 | 任务                             | 价值                 | 复杂度                            |
| ------ | -------------------------------- | -------------------- | --------------------------------- |
| **P0** | 22. Log 分支筛选器               | 高频痛点,后端已完备  | 轻量(纯前端 1 组件)               |
| **P0** | 23. 提交并推送一键               | 高频操作,纯前端      | 轻量                              |
| **P0** | 24. Push 对话框(预览+force+进度) | 安全操作+用户体验    | 中等(gitcore 已有 push_streaming) |
| **P0** | 25. Changelists                  | 并行工作流,大功能    | 大(纯前端状态管理)                |
| **P0** | 26. 文件查看器 + 行内变更标记    | WebStorm 招牌,新组件 | 大(需 gitcore 新能力+FileViewer)  |

**建议实施顺序**:22→23(轻量收口)→24(补 push 安全网)→25(大功能,独立)→26(大功能,需新组件,最后做)。

---

## 第六批:收尾补缺

> 来源:全量对标扫尾。第五批 22–26(P0 缺口)完成后,剩余 4 项功能缺口与安全加固,按优先级排列。2026-06-25。

### 27. 一键推送全部 Tag 〔P1 · 功能缺口〕

**痛点:Tag 视图只能逐条推送(tag 列表每行一个"推送"按钮),有多条未推送 tag 时要逐个点击。**
对标 WebStorm 的 Push Tags 对话框支持一次推送全部 tag。

**先读**:`crates/gitcore/src/tags.rs`(`push_tag` + `default_remote` 已有先例,照着加)、
`crates/gitcore/src/lib.rs`(`push_tag` 暴露模式)、
`gui/src-tauri/src/lib.rs`(`repo_push_tag` 命令,参照其 async+spawn_blocking 模式)、
`gui/src/lib/TagView.svelte`(tag 列表 UI + 已有 per-tag push 按钮)。

**已就绪**:`push_tag(name)` 已有推单个 tag 的完整流程(gitcore → Repo → Tauri command → UI);
`default_remote()` 已抽成独立函数,直接复用。

**做**:

1. **gitcore** `tags.rs`:加 `push_all_tags(repo) -> Result<(), Error>`——复用 `default_remote()`,
   跑 `git push <remote> --tags`(用 `repo.git_checked`,非零时构造 `Error::Git` 返回,与 `push_tag` 一致)。
2. **gitcore** `lib.rs`:暴露 `pub fn push_all_tags(&self) -> Result<(), Error>` 一行的桥方法(放 `push_tag` 下面)。
3. **后端** `lib.rs`:加 `#[tauri::command] async fn repo_push_all_tags(path: String) -> Result<(), String>`——
   网络操作,**必须 async + `tauri::async_runtime::spawn_blocking`**(参照 `repo_push_tag` 的 async 模式)。
4. **前端** `TagView.svelte`:在 tag 列表 header 行加「推送全部 Tag」按钮,
   `onclick` 调 `invoke("repo_push_all_tags", { path })`,成功后标记全部 tag 为 ☑ 已推送。

**注意**:

- `push_all_tags` 是网络操作 → 后端命令必须 async+spawn_blocking。
- 用 `repo.git_checked`(非零报错),与 `push_tag` 一致。git 对已存在的 tag 会报 non-zero,
  前端展示 stderr 即可——比静默吞掉真正失败更安全。
- 按钮放在列表上方 header 行,与逐条推送按钮区分;加上 `disabled={busy}`。

**验收核心**:点"推送全部 Tag"后远程出现所有本地 tag;按钮逐条推送和批量推送行为一致;失败有错误展示。

---

### 28. Reflog 恢复可自选软/混合/硬重置模式 〔P1 · 功能缺口〕

**痛点:Reflog 恢复时只能做混合重置,不能选择软重置(改动留在暂存区)或硬重置(丢弃全部未提交改动)。**
HistoryView 的"重置到此"面板已有三模式 radio 选择器,ReflogView 完全缺失此能力。

**先读**:`gui/src/lib/ReflogView.svelte`(`restore()` 函数硬编码 `mode: "Mixed"`——唯一的改动点)、
`gui/src/lib/HistoryView.svelte` 的 reset-mode radio 按钮布局 + CSS(直接照搬)、
`crates/gitcore/src/reset.rs`(`ResetMode` 枚举:Soft/Mixed/Hard,已就绪)。

**已就绪**:`repo_reset` Tauri 命令已接受 `mode: ResetMode` 参数(后端零改动);
HistoryView 已有三模式 radio 选择器的完整实现(HTML + CSS),直接照搬样式和结构。

**做**:

1. **前端** `ReflogView.svelte` 加状态:`let resetMode = $state<"Soft"|"Mixed"|"Hard">("Mixed")`——默认 Mixed,
   保持当前行为不变。
2. 在 `restore()` 的 `ask()` 确认文案中去掉"混合重置"字眼,根据 `resetMode` 动态描述。
3. `invoke("repo_reset", ...)` 的 mode 参数用 `resetMode` 变量替代硬编码 `"Mixed"`。
4. 插入模式选择 UI:三个 `<label>` 包 `radio` + 说明文字,照搬 HistoryView 的 `.reset-mode` 样式。
5. Hard 模式加二次确认(与 HistoryView 的 `doReset` 一致)。

**注意**:

- 纯前端改动,不碰 gitcore/后端。
- 默认 Mixed 保持向后兼容。
- ResetMode 前端传字符串 `"Soft"` / `"Mixed"` / `"Hard"`,与 HistoryView 一致。

**验收核心**:Reflog 恢复时可自选 Soft/Mixed/Hard;选 Soft 后改动留在暂存区;选 Hard 有二次确认;默认 Mixed 行为不变。

---

### 29. Stash 应用前加确认对话框 〔P2 · 安全性〕

**痛点:点击 Stash 的"应用"按钮直接修改工作区文件,无任何确认。** 应用 stash 可能覆盖当前正在编辑的文件。
`pop()` 和 `drop()` 已有确认,唯独 `apply()` 缺。

**先读**:`gui/src/lib/StashView.svelte` 的 `apply()` 函数、`drop()` 的 `ask()` 调用模式(照抄)。

**已就绪**:`ask` 已 import;`drop()` 已有完全相同的确认模式,直接照搬。

**做**:

1. **前端** `StashView.svelte` 的 `apply()` 函数:在 `busy = true` 之前加 `ask()` 确认,
   文案包含 stash message,`kind: "warning"` 与 `drop()` 一致。

**注意**:

- 纯前端改动,不碰 gitcore/后端。
- `kind: "warning"` 与 `drop()` 一致。
- 确认走 `@tauri-apps/plugin-dialog` 的 `ask()`,不用 `window.confirm`。

**验收核心**:点"应用"弹出确认对话框,取消则工作区不变,确认后正常应用 stash。

---

### 30. 交互式变基开始前加确认汇总 〔P2 · 安全性〕

**痛点:点击"开始变基"直接执行,无任何确认。** 交互式变基改写提交历史(squash/fixup/drop/reword),
是不可逆操作,应在执行前展示变更汇总。

**先读**:`gui/src/lib/RebaseTodoView.svelte` 的 `summary` derived + `start()` 函数、
`gui/src/lib/StashView.svelte` 的 `ask()` 调用模式(参照)。

**已就绪**:`summary` derived 已计算 keep/fold/drop 计数,可直接用于确认文案。

**做**:

1. **前端** `RebaseTodoView.svelte`:
   a. 加 `import { ask } from "@tauri-apps/plugin-dialog"`(当前未 import)。
   b. 在 `start()` 函数中加 `ask()` 确认,文案包含保留/折叠/丢弃数量汇总 + 改写历史警告,
   `kind: "warning"`。
   c. 确认后才设 `busy = true`(调整顺序:确认→busy→invoke)。

**注意**:

- 纯前端改动,不碰 gitcore/后端。
- 确认文案必须包含改写历史警告,与面板顶部 hint 文案一致。

**验收核心**:点"开始变基"弹出确认对话框含操作汇总;取消不变基;确认后正常执行。

---

## 第七批:分支体验 + 仓库运维补全(2026-06-25 调研)

> 来源:全量对标 WebStorm/IntelliJ IDEA Git 文档 + 代码交叉验证。前六批 30 张卡覆盖了高频操作与核心安全网,
> 本轮纵深扫描找出 20 项剩余差距,筛选 9 项高价值、可自包含的纳入第七批。
> 筛选原则:①日常高频但当前体验缺位(32/33/34/35/38)②安全网补全(31/37)③仓库运维必需(36)④git-native 优先于 JetBrains 专有(Shelve → 选择性 stash)。

### 31. 删除远程分支 〔P1 · 功能缺口,小〕

**痛点:BranchPicker 远程区只能检出/比较,无法删除已合并的远程分支。** 当前远程分支行菜单只有「检出」「差异」「比较」三项,删除远程分支得回命令行 `git push origin --delete <branch>`。

**先读**:`crates/gitcore/src/branch.rs`(`delete_branch` 模式,只删本地)、`crates/gitcore/src/tags.rs`(`push_tag` 用 `default_remote` 解析远程名,复用)、`gui/src/lib/BranchPicker.svelte`(远程分支行 622-668,菜单项 647-663)。

**做**:

1. gitcore `branch.rs`:加 `delete_remote_branch(repo, remote: &str, branch: &str)`——`git push <remote> --delete <branch>`,用 `repo.git_checked`(非零报 Error::Git,与 `delete_branch` 一致)
2. gitcore `lib.rs`:暴露 `pub fn delete_remote_branch(&self, remote: &str, branch: &str)`(放 `delete_branch` 旁)
3. 后端:`repo_delete_remote_branch(path, remote, branch)`——**网络操作**,async + spawn_blocking + CancelToken 取消(参照 `repo_push_tag`)
4. 前端 BranchPicker 远程行 `...` 菜单加「删除」项(红色,与本地删除同款确认 `ask()` kind: "warning"),确认后 invoke → 重新 `loadBranches()`

**注意**:

- `default_remote()` 已在 `tags.rs`,挪到 `repo` 或复用——删除时 remote 名前端从 BranchPicker 的远程分支 `name` 解析(`origin/feature` → remote=`origin`, branch=`feature`)
- 远程分支名以 `origin/` 开头,解析用 `split_once('/')`;注意分支名本身含 `/` 的情况,取**第一个** `/` 分割

**验收核心**:点删除远程分支 → 二次确认 → 远程分支消失;失败(无网络/权限)有错误提示。

---

### 32. BranchPicker 分支搜索 〔P1 · 体验,小,纯前端〕

**痛点:仓库分支多了(几十个),肉眼扫列表找分支效率低。** WebStorm 分支弹窗顶部有搜索框(Ctrl+F 聚焦),输入即过滤。当前 BranchPicker 是扁平全量渲染,无搜索。

**先读**:`gui/src/lib/BranchPicker.svelte`(本地 507-620 / 远程 622-668 两段渲染)。

**做**:

1. 加 `let search = $state("")` 状态 + 列表上方 `<input>` 搜索框(placeholder "搜索分支...",自动聚焦)
2. filter 逻辑:`filtered = branches.filter(b => b.name.toLowerCase().includes(search.toLowerCase()))`;搜索为空时显示全部
3. 本地/远程**分别过滤**,保持分组标题(过滤后某组为空则不渲染该组)
4. `{#each}` 遍历 `filteredLocal` / `filteredRemote` 替代原 `localBranches` / `remoteBranches`

**注意**:

- 纯前端,不碰 gitcore/后端
- 搜索框样式参考 HistoryView 的 filter 行(`.log-filter`)
- `prefix-group`(卡 33)与搜索的逻辑叠加:先分组 → 组内搜索,或先搜索 → 搜索结果内分组。建议先做搜索(简单),分组后做时再协调

**验收核心**:输入分支名片段 → 列表实时过滤;清空搜索框恢复全量;匹配不到时显示"无匹配分支"。

---

### 33. 分支名前缀分组 〔P2 · 体验,中,纯前端〕

**痛点:分支命名用 `/` 分层(feature/login、feature/signup、bugfix/oom)很常见,扁平列表失去层级感。** WebStorm 分支弹窗有「Group by Prefix」开关,自动按 `/` 第一段折叠。

**先读**:`gui/src/lib/BranchPicker.svelte`(本地/远程两段渲染)、卡 32 搜索实现。

**做**:

1. 加 `let groupByPrefix = $state(true)`(默认开启)+ 列表上方 `☰ 分组` toggle
2. 分组逻辑:`Map<prefix, BranchInfo[]>`——`prefix = name.split('/')[0]`,无 `/` 的分支进 `"(无分组)"` 或直接列出
3. 每组渲染为可折叠区:`<details open>` 或自定义折叠头(prefix 名 + 分支数),组内分支如现有 `<li>` 渲染
4. 与搜索联动:搜索先过滤 → 分组作用于过滤结果 → 空组不渲染
5. 远程分支同理(远程前缀如 `origin/feature/login` 先去 `origin/` 再取 prefix)

**注意**:

- 纯前端,不碰 gitcore/后端
- localStorage 记住 `groupByPrefix` 偏好
- 单层分组即可(取第一段),不嵌套

**验收核心**:`feature/login`、`feature/signup` 归入「feature」组;切换 toggle 恢复扁平;搜索后分组仍正确。

---

### 34. 最近分支 + 收藏置顶 〔P2 · 体验,小,纯前端〕

**痛点:频繁切换的几个分支每次都得从列表里找。** WebStorm 分支弹窗顶部有「Recent Branches」(最近 checkout 过的 5 个)和 Favorite 星标置顶。当前 git-gui 零记录。

**先读**:`gui/src/lib/BranchPicker.svelte`(列表渲染)、`gui/src-tauri/src/lib.rs` 的 `ProjectHistory`(localStorage 模式参照)。

**做**:

1. **最近分支**:localStorage `git-gui:recent-branches` 存 `{repoPath: [branchName, ...]}`(最近 5 个,新的在前,去重);`switchTo()` 成功后 push 到列表头部;BranchPicker 顶部渲染「最近」区(最多 5 条,与主列表用分隔线隔开)
2. **收藏分支**:localStorage `git-gui:favorite-branches` 存 `{repoPath: [branchName, ...]}`;分支行名旁加 ☆ 按钮(filled=已收藏,outline=未收藏);收藏分支出现在「最近」区下方「收藏」区;**`main`/`master` 自动收藏**(对标 WebStorm)
3. 两区都在搜索结果为空时隐藏(与主列表一致)

**注意**:

- 纯前端,不碰 gitcore/后端
- 最近/收藏里的分支在主列表中仍正常显示(不重复过滤)
- 收藏星标在远程分支行也显示(可收藏 `origin/feature`)

**验收核心**:切换分支后 BranchPicker 顶部显示最近切换的分支;点 ☆ 收藏某分支,出现在收藏区;`main` 自动在收藏区;最近/收藏区随搜索过滤。

---

### 35. 复制分支名 〔P2 · 体验,极小,纯前端〕

**痛点:想用分支名去命令行操作或在别处引用,只能手动打字。** WebStorm 分支弹窗 hover 分支名按 Ctrl+C 即复制。

**先读**:`gui/src/lib/HistoryView.svelte` 的 `copySha`(navigator.clipboard.writeText,已有先例)、`gui/src/lib/BranchPicker.svelte`(分支行渲染)。

**做**:

1. 分支行名旁 hover 显示 📋 小按钮
2. `onclick` → `navigator.clipboard.writeText(branch.name)` → 短暂显示"已复制" tooltip(或按钮变 ✓ 1s 后恢复)

**注意**:

- 纯前端,不碰 gitcore/后端
- 远程分支复制完整名(如 `origin/feature/x`)

**验收核心**:点 📋 后分支名入剪贴板;粘贴验证内容正确。

---

### 36. Remotes 管理(增/删/改远程仓库)〔P2 · 仓库运维,中〕

**痛点:当前完全看不到远程仓库配置,无法添加第二个远程(fork 工作流)、修改 URL、删除远程。** WebStorm 有 `Git | Manage Remotes` 对话框。当前仅在 PushDialog 看到默认 remote 名,TagView 用到 `default_remote()`。

**先读**:`crates/gitcore/src/tags.rs`(`default_remote` 用 `git remote` 取列表)、`gui/src/lib/Settings.svelte`(弹窗模态模式参照)。

**做**:

1. gitcore:加 `remote` 模块(`remote.rs`):
   - `RemoteInfo { name, url, push_url, fetch }`(Serialize)
   - `list_remotes(repo)` → `git remote -v` 解析
   - `add_remote(repo, name, url)` → `git remote add <name> <url>`
   - `remove_remote(repo, name)` → `git remote remove <name>`
   - `set_remote_url(repo, name, url)` → `git remote set-url <name> <url>`
2. gitcore `lib.rs`:暴露四个 Repo 方法
3. 后端:四个命令——list sync,add/remove/set-url 网络操作 async+spawn_blocking
4. 前端:Entry 点在 Settings 页→「管理远程仓库」按钮,弹 `RemoteManager.svelte` 模态:
   - 远程列表(名称 + URL,当前默认 remote 高亮)
   - 添加(名称 input + URL input)
   - 编辑(修改 URL,不支持 rename——git remote rename 低频)
   - 删除(二次确认,至少保留一个 remote 时警告)

**注意**:

- 改了 gitcore 公共类型(`RemoteInfo`)→ 跑全 workspace
- `remove_remote` 用 `repo.git_checked` 非零报 Error
- 移除唯一 remote 前端给警告但不禁用(允许用户清空后重建)

**验收核心**:列出现有 remote;添加新 remote 后 `git remote -v` 可见;修改 URL 生效;删除 remote 后分支列表远程区该 remote 的分支消失(需 refresh)。

---

### 37. Git Clean 清理未跟踪文件 〔P2 · 安全网,中〕

**痛点:构建产物、临时文件等未跟踪文件积累多了,没有 GUI 方式清理。** 虽可命令行 `git clean`,但对 GUI 用户存在安全盲区——不知道会删哪些文件。WebStorm 无直接 clean 入口(通过 Local Changes 的「Rollback」间接处理),但作为纯 Git 工具,clean 是合理补充。

**先读**:`crates/gitcore/src/git.rs`(命令执行模式)、`gui/src/lib/FileTree.svelte`(未跟踪文件列表渲染)、`gui/src/lib/StashView.svelte`(`ask()` 确认模式)。

**做**:

1. gitcore:加 `clean` 模块(`clean.rs`):
   - `clean_preview(repo, directories: bool)` → `git clean -n` + 可选 `-d`(含目录),返回 `Vec<PathBuf>`
   - `clean_force(repo, directories: bool)` → `git clean -f` + 可选 `-d`,返回删除数
2. gitcore `lib.rs`:暴露两个方法
3. 后端:`repo_clean_preview(path, include_dirs)`(sync,轻量)、`repo_clean_force(path, include_dirs)`(sync)
4. 前端:Changes 面板或顶栏 `⋯ 更多` 加「清理未跟踪文件...」入口 → 弹窗:
   - 先调 `clean_preview` 展示将被删除的文件列表(不可勾选,整批删——对标 `git clean` 原子性)
   - check「同时清理空目录」
   - 「确认清理 N 个文件」按钮(红色,二次 ask)
   - 清理后 reload status

**注意**:

- **不做 `-x`**(不删 .gitignore 忽略的文件)——安全优先。需要时后续加选项
- clean 是本地操作,sync command 即可(非网络)
- 预览列表为空时显示"没有需要清理的未跟踪文件"

**验收核心**:clean --dry-run 列出文件 → 确认后文件被删 → status 刷新后未跟踪文件消失;取消不删任何文件。

---

### 38. DiffView hunk 间跳转 〔P2 · 体验,小,纯前端〕

**痛点:大 diff 有多个 hunk,只能手动滚动找下一个变更位置。** WebStorm 有「Jump to Next/Previous Change」(Ctrl+Alt+Shift+Down/Up)。当前 DiffView 无跳转按钮。

**先读**:`gui/src/lib/DiffView.svelte`(hunk 渲染,hunk 行有 `.hunk` class)、`gui/src/lib/FileViewer.svelte`(行渲染参考)。

**做**:

1. DiffView 每个 `.hunk` 行加 `id="hunk-{fileIdx}-{hunkIdx}"`
2. 工具栏或浮层加 ↓↑ 跳转按钮(仅多 hunk 时显示,单 hunk 隐藏)
3. `scrollToHunk(n)`:`document.getElementById("hunk-{fileIdx}-{n}")?.scrollIntoView({behavior:"smooth",block:"start"})`
4. 支持键盘快捷键:J/K 或 Ctrl+↓/↑(加 `svelte:window onkeydown`,仅在 DiffView 可见时生效)
5. 跨文件跳转:到最后一个 hunk 的 next → 下一个文件的第一个 hunk

**注意**:

- 纯前端,不碰 gitcore/后端
- `id` 放在 hunk header 行(已有 `@@` 行,加 `id` 不破坏现有结构)
- 键盘快捷键只在 DiffView mounted 时监听,unmount 时移除(避免全局污染)

**验收核心**:点 ↓ 跳到下一个 hunk;到文件末尾最后一个 hunk 再点 ↓ 跳到下一个文件(如有);快捷键生效;单 hunk 不显示按钮。

---

### 39. 选择性 Stash(按文件储藏)〔P2 · git-native Shelve 等价,小〕

**痛点:git stash 是全或无——要么全部未提交改动一起 stash,要么不 stash。想只搁置部分文件、保留另一些继续工作时,只能手动 `git stash push -- <path>`。** JetBrains Shelve 的核心差异化就是选择性搁置。做 git-native 版本:给 `stash_push` 加 pathspec 参数。

**先读**:`crates/gitcore/src/stash.rs`(`stash_push` line 147,当前无 pathspec)、`gui/src/lib/StashView.svelte`(当前"创建 Stash"是全量、无文件选择)。

**做**:

1. gitcore `stash_push`:加 `paths: Option<Vec<PathBuf>>` 参数 → 有值则在 `--include-untracked` 后加 `--` + paths(每项 `.to_string_lossy()` 空格分隔,不加引号——git CLI 接受裸路径)
2. gitcore `lib.rs`:暴露签名更新
3. 后端 `repo_stash_push(message, paths)`——sync,不变
4. 前端 StashView:
   - "创建 Stash"改为下拉:默认「储藏全部改动」,加「储藏选中文件...」
   - 选后者 → 弹出文件列表(从 Changes 文件取,多仓仓库标签区分),多选后确认 → invoke 带 paths
   - 快捷入口:Changes 面板文件行右键「储藏此文件」→ 单文件 stash

**注意**:

- gitcore `stash_push` **签名变更**→ 补全所有调用点(至少 `gui/src-tauri/src/lib.rs` 的 `repo_stash_push` + TUI `stage_ui.rs` 如有引用)
- `--include-untracked` + pathspec:未跟踪文件需要 `git add --intent-to-add` 先标记才能被 stash 纳入——**核实 git stash push -- <untracked> 的行为**,可能需要先 `add -N`
- 这是 git-native 选择性储藏,不是 JetBrains Shelve 的 patch 文件体系(不做 Shelf tab、Recently Deleted、base revision 等)

**验收核心**:选中 2 个文件的改动 stash → 工作区只剩另 1 个文件改动;stash list 可见新 stash;apply 后 2 个文件改动恢复。

---

## 第七批优先级小结

| 优先级 | 任务                    | 价值              | 复杂度                                  |
| ------ | ----------------------- | ----------------- | --------------------------------------- |
| **P1** | 31. 删除远程分支        | 功能缺口,日常用   | 小(gitcore 1 函数 + 后端 + UI 1 菜单项) |
| **P1** | 32. BranchPicker 搜索   | 体验,分支多必用   | 小(纯前端 1 input + filter)             |
| **P2** | 33. 分支名前缀分组      | 体验,分支命名规范 | 中(纯前端分组折叠 UI)                   |
| **P2** | 34. 最近分支 + 收藏置顶 | 体验,高频切换     | 小(纯前端 localStorage)                 |
| **P2** | 35. 复制分支名          | 便利              | 极小(1 按钮 + clipboard API)            |
| **P2** | 36. Remotes 管理        | 仓库运维必需      | 中(gitcore 新模块 + UI 弹窗)            |
| **P2** | 37. Git Clean           | 安全网,清理积压   | 中(gitcore 新模块 + dry-run 预览)       |
| **P2** | 38. DiffView hunk 跳转  | 体验,大 diff 导航 | 小(纯前端 scrollIntoView + 键盘)        |
| **P2** | 39. 选择性 Stash        | git-native Shelve | 小(gitcore stash_push +paths + UI)      |

**建议实施顺序**:31(删远程分支,独立)→32(搜索)+35(复制)一起做(同组件)→34(最近/收藏)→33(分组,叠加在搜索上)→38(hunk 跳转)→36(Remotes)+37(Clean)+39(选择性 stash)逐步推进。

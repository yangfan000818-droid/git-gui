# git-gui

把 git 操作"降维"成可见、可操作对象的 git 客户端 —— 对标 WebStorm/JetBrains 的 git 体验,主打**可见性**与**安全网**两条护城河。

## 形态路线

解耦核心 + 先 TUI 验证 + 后接 GUI:git 编排做成 UI 无关的核心库,先配 TUI 跑通交互,确认后在同一核心上接 GUI。

## 架构

cargo workspace:

| crate | 职责 |
|-------|------|
| `gitcore` | UI 无关的 git 编排库(spawn git CLI + plumbing 命令,零外部依赖) |
| `tui` | 终端前端(ratatui 全屏界面 + 冲突解决三栏视图 + Stage/Log/Diff/Submodule) |

## 已实现

**Update 状态机**(脏工作区安全的 pull):

- 脏工作区自动 stash / restore(用 `apply`+`drop` 兜底,失败不丢改动)
- fetch 分离 + fast-forward 优先
- merge / rebase 整合,`-Xignore-space-change` 消解空白伪冲突
- 冲突三版本提取(`:1:` / `:2:` / `:3:`),供三栏合并 UI 使用
- preflight 防止在已有 merge/rebase 时重入
- 崩溃恢复:检测未完成的整合(中断/崩溃后扫回 autostash + 冲突文件列表)

**执行层 进度 / 取消**(长操作不冻结界面):

- fetch / push 流式执行,进度从 git stderr 解析(`Receiving objects: 45%` 等)经回调实时上报
- 协作式取消(`CancelToken`):置位即 kill 子进程并返回 `Cancelled`,改动有兜底不残留
- `execute_update` 可取消:fetch 阶段中止发生在 autostash 之前,工作区不受影响

**冲突解决**(零依赖 diff3 + 行级魔法棒):

- 零依赖行级 diff3 算法(Myers LCS + 三版本合并逻辑)
- 整块魔法棒:单边改动的冲突块自动定夺
- 行级魔法棒:块内逐行 diff3,自动解决行级单边改动
- rerere:记住冲突解法并在下次相同冲突时自动重放
- 冲突解决后自动 `git add` + 继续整合或放弃回退

**日常提交链路**(stage → commit → push):

- 文件级 status 解析(`git status --porcelain=v1`)
- 暂存/取消暂存(stage / unstage / stage_all);按目录批量暂存/取消
- 回滚改动(discard,stash 兜底可 pop 找回,含未跟踪文件)
- 提交 + 空暂存区拒绝(commit + CommitOptions)
- 推送 + 边界处理(NoUpstream / NonFastForward / Success);非快进自动整合后重推

**log(分支图) / diff 查看 + hunk 级暂存**:

- 提交历史(log + LogEntry,默认最近 50 条,支持指定分支)
- 分支拓扑图(log_graph + GraphRow):`git log --graph` 生成图形列,解析为每行图形前缀 + 可选 commit
- 工作区 diff(diff / show_commit / commit_message);DiffOptions 开关 `--cached`、`-- <path>`
- 结构化 diff 解析(unstaged_diff / staged_diff / commit_files → 文件 → hunk → 行),供 UI 着色展示
- 逐 hunk 暂存/取消(stage_hunk / unstage_hunk):截取"文件头 + 该 hunk"喂 `git apply --cached`,基准为 index 故行号精确
- 逐行暂存/取消(stage_lines / unstage_lines):只保留选中行,未选新增行丢弃、未选删除行转上下文,重算行数后 apply

**branch / stash 管理**:

- 分支:列出 / 创建 / 切换 / 删除(`b` 键进入)
- stash:列出 / 创建 / apply / pop / drop(`h` 键进入)

**子仓库检测 + 多仓库配置**:

- submodule 自动检测(git submodule status)
- 配置文件 `.git-gui/repos.toml`(手写简单 toml 解析器,零依赖)
- Tab 键切换仓库;单仓库模式无左侧边栏,多仓库模式左侧边栏列出所有仓库

**TUI 交互**:

- ratatui 全屏界面:status 面板 + 实时仓库状态(分支/upstream/领先落后/脏状态)
- update / push 后台异步执行,底部栏渲染进度条(百分比 + 阶段名),`Esc` 取消,界面不冻结
- push 被拒自动整合再推(WebStorm 式):无冲突全自动,有冲突进解决视图、解决后自动推
- `m` 键切换 merge / rebase 整合策略,状态栏常驻显示当前策略
- Branch 视图(`b`):分支列表 + 创建/切换/删除;Stash 视图(`h`):列表 + 创建/应用/弹出/丢弃
- 冲突解决三栏视图:`ours │ base │ theirs` 并排显示
- 多文件冲突导航:顶部概览条 + `n`/`p` 切换,每文件独立保留选择与进度
- 冲突块内行级滚动(`J`/`K`),`←`/`→` 在 ours/base/theirs 间切换选择
- Stage 视图:可折叠目录树(j/k 导航/l/h 展开折叠/Space 暂存或取消(文件或整目录)/a 全暂存/d 回滚/c 提交)
- Log 视图:提交历史 + 分支拓扑图(蓝色图形列);Enter 进**双栏详情**(左侧提交信息+**文件目录树**,右侧按文件看 diff)
- Diff 视图:**双栏**(左侧**文件目录树** + 右侧单文件 hunk diff);左栏 j/k 选、l/h 展开折叠或进 diff、Space 暂存整文件/整目录,右栏 Space 选行/暂存整 hunk、s 暂存选中行,t 切未暂存/已暂存,`d` 键进入
- Submodule 视图:子仓库列表,`S` 键进入
- 左侧边栏(多仓库时):状态图标 + Tab 切换
- 所有列表视图跟随光标自动滚动(列表超出可视区时)
- Status 面板文本过长时 j/k/↑/↓ 滚动
- 魔法棒可视化:自动解决的行标绿色 ✓,待处理行标黄色,当前选择栏加粗

**键位总览**: `j/k/↑/↓` 导航/滚动 · `Space` 暂存 · `a` 全暂存 · `c` 提交/创建 · `d` Diff/删除 · `s` Stage · `b` Branch · `h` Stash · `S` 子仓库 · `l` Log · `p` Push · `u` Update · `m` 策略 · `R` 恢复 · `r` 刷新 · `q` 退出 · `q/Esc` 返回(各子视图)/取消

## 构建与测试

需要 Rust(`brew install rust`)与 git。

```bash
cargo build                # 构建
cargo test --workspace     # 跑全部测试(60 个:gitcore 51 + tui 9)
cargo run -p tui           # 启动 TUI(在 git 仓库目录下运行)
```

## 多仓库配置

在仓库根目录创建 `.git-gui/repos.toml`:

```toml
# 主仓库(可选,默认自动添加当前目录)
[[repos]]
name = "主仓库"
path = "."

# 独立仓库
[[repos]]
name = "前端"
path = "../frontend"

# 绝对路径
[[repos]]
name = "后端"
path = "/Users/yfan/work/backend"
```

- 不创建配置文件 → 只操作当前仓库
- 配置文件存在 → 按列表显示(左侧边栏),Tab 键切换
- submodules 自动检测,合并到列表(去重,配置优先)
- 路径相对于 `.git-gui/` 所在目录,支持绝对路径
- 配置文件格式:简单的 `[[repos]]` + `name`/`path`,不支持数组/嵌套

## 路线图

- [x] ratatui 全屏界面(status 面板 + 按键触发 update)
- [x] 冲突三栏 + 魔法棒(整块 + 行级自动解决单边改动)
- [x] rerere(记住冲突解法,自动重放)
- [x] 崩溃恢复(检测未完成的 update / 残留 autostash)
- [x] stage / commit / push 日常提交链路
- [x] log(分支图) / diff 查看 + hunk/行级暂存(结构化 diff + 逐 hunk + 逐行 stage/unstage)
- [x] submodule 检测 + 多仓库配置
- [x] branch 管理(创建/切换/删除)
- [x] stash 管理(手动 stash/pop)
- [x] 执行层进度 / 取消(fetch/push 流式 + 协作式取消)
- [x] push 自动整合再推 + merge/rebase 策略选择
- [ ] 接 GUI(Tauri / iced)

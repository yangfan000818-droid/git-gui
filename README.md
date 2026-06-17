# git-gui

把 git 操作"降维"成可见、可操作对象的 git 客户端 —— 对标 WebStorm/JetBrains 的 git 体验,主打**可见性**与**安全网**两条护城河。

## 形态路线

解耦核心 + 先 TUI 验证 + 后接 GUI:git 编排做成 UI 无关的核心库,先配 TUI 跑通交互,确认后在同一核心上接 GUI。

## 架构

cargo workspace:

| crate | 职责 |
|-------|------|
| `gitcore` | UI 无关的 git 编排库(spawn git CLI + plumbing 命令,零外部依赖) |
| `tui` | 终端前端(ratatui 全屏界面 + 冲突解决三栏视图) |

## 已实现

**Update 状态机**(脏工作区安全的 pull):

- 脏工作区自动 stash / restore(用 `apply`+`drop` 兜底,失败不丢改动)
- fetch 分离 + fast-forward 优先
- merge / rebase 整合,`-Xignore-space-change` 消解空白伪冲突
- 冲突三版本提取(`:1:` / `:2:` / `:3:`),供三栏合并 UI 使用
- preflight 防止在已有 merge/rebase 时重入
- 崩溃恢复:检测未完成的整合(中断/崩溃后扫回 autostash + 冲突文件列表)

**冲突解决**(零依赖 diff3 + 行级魔法棒):

- 零依赖行级 diff3 算法(Myers LCS + 三版本合并逻辑)
- 整块魔法棒:单边改动的冲突块自动定夺
- 行级魔法棒:块内逐行 diff3,自动解决行级单边改动
- rerere:记住冲突解法并在下次相同冲突时自动重放
- 冲突解决后自动 `git add` + 继续整合或放弃回退

**TUI 交互**:

- ratatui 全屏界面:status 面板 + 实时仓库状态(分支/upstream/领先落后/脏状态)
- 冲突解决三栏视图:`ours │ base │ theirs` 并排显示
- 多文件冲突导航:顶部概览条 + `n`/`p` 切换,每文件独立保留选择与进度
- 魔法棒可视化:自动解决的行标绿色 ✓,待处理行标黄色,当前选择栏加粗
- 按键操作:`o`/`t`/`b` 选边 · `j`/`k` 切块 · `w` 写回 · `c` 完成 · `x` 放弃

## 构建与测试

需要 Rust(`brew install rust`)与 git。

```bash
cargo build                # 构建
cargo test --workspace     # 跑全部测试(27 个:gitcore 24 + tui 3)
cargo run -p tui           # 启动 TUI(在 git 仓库目录下运行)
```

## 路线图

- [x] ratatui 全屏界面(status 面板 + 按键触发 update)
- [x] 冲突三栏 + 魔法棒(整块 + 行级自动解决单边改动)
- [x] rerere(记住冲突解法,自动重放)
- [x] 崩溃恢复(检测未完成的 update / 残留 autostash)
- [ ] commit / push / stage 管理
- [ ] log / diff 查看
- [ ] 接 GUI(Tauri / iced)

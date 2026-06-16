# git-gui

把 git 操作"降维"成可见、可操作对象的 git 客户端 —— 对标 WebStorm/JetBrains 的 git 体验,主打**可见性**与**安全网**两条护城河。

## 形态路线

解耦核心 + 先 TUI 验证 + 后接 GUI:git 编排做成 UI 无关的核心库,先配 TUI 跑通交互,确认后在同一核心上接 GUI。

## 架构

cargo workspace:

| crate | 职责 |
|-------|------|
| `gitcore` | UI 无关的 git 编排库(spawn git CLI + plumbing 命令,零外部依赖) |
| `tui` | 终端前端(当前为 CLI 壳,ratatui 化进行中) |

## 已实现

**Update 状态机**(脏工作区安全的 pull):

- 脏工作区自动 stash / restore(用 `apply`+`drop` 兜底,失败不丢改动)
- fetch 分离 + fast-forward 优先
- merge / rebase 整合,`-Xignore-space-change` 消解空白伪冲突
- 冲突三版本提取(`:1:` / `:2:` / `:3:`),供三栏合并 UI 使用
- preflight 防止在已有 merge/rebase 时重入

## 构建与测试

需要 Rust(`brew install rust`)与 git。

```bash
cargo build              # 构建
cargo test -p gitcore    # 跑核心集成测试
cargo run -p tui status  # 查看当前仓库状态
cargo run -p tui update  # 执行一次 Update
```

## 路线图

- [ ] ratatui 全屏界面(status 面板 + 按键触发 update)
- [ ] 冲突三栏 + 魔法棒(自动应用非冲突块)
- [ ] rerere(记住冲突解法,自动重放)
- [ ] 崩溃恢复(检测未完成的 update / 残留 autostash)
- [ ] 接 GUI(Tauri / iced)

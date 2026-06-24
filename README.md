# git-gui

把 git 操作"降维"成**可见、可操作对象**的桌面 git 客户端 —— 对标 WebStorm / JetBrains 的 git 体验,主打**可见性**与**安全网**两条护城河。

> 形态:**Tauri 2 + SvelteKit 桌面 GUI**(产品主体)· **ratatui 终端 TUI**(早期验证形态,仍可用)· 二者共享同一个**零依赖 Rust 核心库 `gitcore`**。

## 两条护城河

- **可见性** —— 把 git 内部状态摊开给你看:三栏 diff、SVG 彩色分支拓扑图、冲突 `ours │ base │ theirs` 并排、blame 行注解、文件历史。
- **安全网** —— 危险操作有兜底:更新自动 stash、冲突可视化解决 + 魔法棒自动定夺、崩溃后恢复、rerere 重放解法。

## 架构

cargo workspace,同一核心库接两种前端:

| 成员 | 职责 |
|------|------|
| `crates/gitcore` | UI 无关的 git 编排核心库(spawn git CLI + plumbing 命令,**零外部依赖**) |
| `gui/`(`src` + `src-tauri`) | **桌面应用**:SvelteKit 前端 + Tauri 2 后端(Rust 命令直接调用 `gitcore`) |
| `crates/tui` | 终端界面(ratatui 全屏 + 冲突三栏视图),最初用来快速验证核心交互 |

设计理念:git 编排逻辑全部沉到 `gitcore`,前端只负责呈现与交互。换前端不改核心,核心零依赖、可单测。

## 功能一览(GUI)

### 安全网

- **一键安全更新**:点「全部更新」即 autostash → fetch(ff 优先)→ merge / rebase 整合(`-Xignore-space-change` 消解空白伪冲突)→ 自动还原;只有冲突时才停下。整合策略全局配置一次即生效。
- **冲突可视化解决**:三栏 `ours │ base │ theirs` 并排,**魔法棒**自动定夺单边改动(整块级 + 行级),真冲突留给你逐块选边或手动编辑,写回后继续 / 放弃整合。
- **崩溃恢复**:中断 / 崩溃后重新打开,自动扫回未完成的整合(autostash + 冲突文件)继续解决。
- **rerere**:记住冲突解法,下次相同冲突自动重放。

### 可见性

- **Changes 视图**:工作区改动按文件树呈现,结构化彩色 diff,支持**文件 / hunk / 行级**的暂存 · 取消暂存 · 丢弃;字符级行内 diff 高亮具体改动。
- **History 视图**:SVG 彩色分支拓扑图 + 提交详情(完整 message + 改动文件 diff);父仓提交若改了子模块指针,展开成区间内的子仓提交列表。
- **blame 行注解** + **文件历史**(`git log --follow`),点 SHA 可跳到对应提交。

### 历史操作

amend 上次提交 · cherry-pick / revert · reset(soft / mixed / hard)· **交互式变基**(reword / squash / fixup / drop / 重排)· **reflog 查看 + 一键恢复**(变基 / 重置的安全网)。

### 分支管理

点仓库分支名进入:列出 / 切换 / 新建 / 删除 / 检出远程分支 / merge · rebase 到当前 / 重命名 / 从选中提交建分支 / 与工作区比对 / 与当前分支对比 / **smart checkout**(切换被脏工作区挡住时自动 stash 兜底)。

### 标签 · 储藏

- **Tags**:创建(轻量 / 注释)· 删除 · 推送到远程。
- **Stash**:创建 · 应用 · 弹出 · 丢弃。

### 多仓库 & 子仓库

- `.git-gui/repos.toml` 配置多个独立仓库;git submodule 自动检测合并到列表。
- 提交历史把**主仓 + 各子仓提交合并成一个列表**按时间排序,每条带仓库标识;针对某提交的操作自动路由到其所属仓库。
- 子仓库一键更新:未初始化的 `init`,已初始化的在**各自当前分支**上更新(留在原分支,不 detach)。

### 其它

- **从远程 clone**:输入 URL + 父目录,带进度条 / 可取消,失败自动清理残留。
- **全局设置**:整合策略(merge / rebase)、忽略空白、跳过 git 钩子。
- **新版本提醒**:启动检查 GitHub Releases,有新版顶部提示。
- **文件变更自动刷新**:监听工作区(含子仓库)外部改动,自动刷新。

> TUI 形态提供同一套核心能力的终端操作界面(全屏 + 快捷键),详见 [使用指南](docs/USAGE.md)。

## 构建与运行

前置:[Rust](https://www.rust-lang.org/)(`brew install rust`)、[Node.js](https://nodejs.org/)、git。

```bash
# 桌面 GUI(开发模式,自动开窗口)
cd gui && npm install && npm run tauri dev

# 桌面 GUI(打包发布版)
cd gui && npm run tauri build

# 终端 TUI(在任意 git 仓库目录下运行)
cargo run -p tui

# 跑全部测试(51 个:gitcore 42 + tui 9)
cargo test --workspace
```

## 多仓库配置

在仓库根目录创建 `.git-gui/repos.toml`:

```toml
# 主仓库(可选,默认自动添加当前目录)
[[repos]]
name = "主仓库"
path = "."

# 独立仓库(相对 .git-gui/ 所在目录,或绝对路径)
[[repos]]
name = "前端"
path = "../frontend"
```

- 不创建配置文件 → 只操作当前仓库。
- 配置文件存在 → 按列表显示,可切换仓库。
- submodule 自动检测并合并到列表(去重,配置优先)。
- 格式:简单的 `[[repos]]` + `name` / `path`,不支持数组 / 嵌套(零依赖手写解析)。

## 发布

经 GitHub Releases 分发:macOS(aarch64,`.dmg`)+ Windows(x64,`.exe`),推 `v*` tag 触发 CI 构建。**当前未签名 / 未公证**,macOS 首次打开若被 Gatekeeper 拦,执行 `xattr -cr <App>` 解除隔离属性。

## 路线图

- [x] 零依赖核心库 `gitcore`(UI 无关 git 编排)
- [x] TUI 全功能(冲突三栏 + 魔法棒 + 崩溃恢复 + rerere + stage/commit/push + log/diff + branch/stash + 多仓库)
- [x] 桌面 GUI:Changes / History 两大视图 + 安全更新 + 冲突可视化解决
- [x] 历史操作:amend / cherry-pick / revert / reset / 交互式变基 / reflog 恢复
- [x] 分支管理(切换 / 新建删除 / 远程检出 / merge·rebase / rename / compare / smart checkout)
- [x] 标签 / 储藏 / 多仓库 + 子仓库合并提交历史
- [x] 从远程 clone + 全局设置 + 新版本提醒 + 文件变更自动刷新
- [x] 跨平台 CI + Release(macOS aarch64 / Windows x64)
- [ ] Windows 平台实机验证
- [ ] 安装包签名 / 公证(macOS notarization + Windows 证书)

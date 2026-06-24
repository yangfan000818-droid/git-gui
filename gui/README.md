# git-gui 桌面 GUI

git-gui 的桌面客户端 —— SvelteKit 前端 + Tauri 2 后端(Rust);后端命令直接调用零依赖核心库 [`gitcore`](../crates/gitcore)。

- 产品介绍与功能全景:仓库根 [README](../README.md)
- 使用指南(GUI / TUI):[docs/USAGE.md](../docs/USAGE.md)
- 前端开发约定(架构 / 加功能流程 / 约束):[docs/gui-dev-guide.md](../docs/gui-dev-guide.md)

## 开发

```bash
npm install
npm run tauri dev      # 开发模式(自动开窗口)
npm run tauri build    # 打包发布版
```

## 质量门

```bash
npm run check                                              # svelte-check(类型 + a11y)
npx prettier --check src/                                  # 格式(须在 gui/ 下运行)
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

## 结构

- `src/routes/+page.svelte` —— 主界面(顶栏 + Changes / History 两个标签页)
- `src/lib/*.svelte` —— 各视图与弹层组件(UpdateView / ConflictView / HistoryView / BranchPicker 等)
- `src-tauri/src/lib.rs` —— Tauri 命令(前端 `invoke` 入口),调用 `gitcore`

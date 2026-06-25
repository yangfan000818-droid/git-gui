# 多仓库配置功能测试计划

## 测试环境

- 仓库：git-gui (~/work/git-gui)
- 分支：feat/multi-repo-config
- 版本：commit 2b4b5ea

## 测试场景

### 场景 1：单仓库模式（无配置文件）

**前置条件**：

- 删除 `.git-gui/repos.toml`（如果存在）
- 当前仓库无 submodules

**预期行为**：

- [x] 加载当前仓库为"主仓库"
- [ ] 不显示左侧边栏（保持原布局）
- [ ] Tab 键无效（只有一个仓库）
- [ ] 状态面板正常显示
- [ ] 所有原有功能正常（s/l/d/p/u/r/q）

**执行**：

```bash
rm -f .git-gui/repos.toml
cd gui && npm run tauri dev
```

**验证结果**：

- 启动正常：✅
- 布局正确：⏳ 待手动验证
- 功能正常：⏳ 待手动验证

---

### 场景 2：多仓库模式（配置文件）

**前置条件**：
创建 `.git-gui/repos.toml`：

```toml
[[repos]]
name = "git-gui"
path = "."

[[repos]]
name = "另一个项目"
path = "/path/to/another/repo"
```

**预期行为**：

- [ ] 显示左侧边栏（30 列宽）
- [ ] 列表显示 2 个仓库
- [ ] 当前仓库高亮（REVERSED）
- [ ] 状态图标正确：
  - ✓ 绿色：干净
  - ● 黄色：有改动
  - ! 红色：有冲突
  - ✗ 红色：打开失败
- [ ] Tab 键切换仓库
- [ ] 切换后状态重新加载
- [ ] 右侧显示当前仓库名称

**执行**：

```bash
mkdir -p .git-gui
cat > .git-gui/repos.toml <<'EOF'
[[repos]]
name = "git-gui"
path = "."
EOF
cd gui && npm run tauri dev
```

**验证结果**：

- 启动正常：⏳
- 左侧边栏：⏳
- Tab 切换：⏳
- 状态独立：⏳

---

### 场景 3：配置文件 + submodules

**前置条件**：

- 当前仓库有 submodules
- 配置文件包含部分仓库

**预期行为**：

- [ ] 列表 = 配置文件 + submodules
- [ ] 去重：配置文件优先
- [ ] 主仓库自动添加（如果配置未包含）

**执行**：

```bash
# 添加一个 submodule（如果有的话）
git submodule add https://github.com/example/repo vendor/example
cd gui && npm run tauri dev
```

**验证结果**：

- 合并正确：⏳
- 去重生效：⏳

---

### 场景 4：仓库打开失败

**前置条件**：
配置文件指向不存在的路径：

```toml
[[repos]]
name = "不存在"
path = "/not/exist"
```

**预期行为**：

- [ ] 状态图标显示 ✗ 红色
- [ ] 切换到该仓库时提示"当前仓库不可用"
- [ ] 不崩溃

**验证结果**：

- 错误处理：⏳
- 稳定性：⏳

---

## 自动化测试补充

当前测试覆盖：

- [x] config.rs::parse_simple_config（配置文件解析）
- [x] 所有 gitcore 单元测试（21 个）
- [x] 所有 gitcore 单元测试

需要补充的集成测试：

- [ ] load_repos 函数测试（配置 + submodules + 去重）
- [ ] Tab 切换后状态独立
- [ ] 仓库打开失败的错误处理

---

## 手动验证步骤

### 快速验证命令

```bash
# 1. 单仓库模式
rm -f .git-gui/repos.toml
cd gui && npm run tauri dev
# 观察：无左侧边栏

# 2. 多仓库模式（创建临时测试仓库）
mkdir -p /tmp/test-repo && cd /tmp/test-repo && git init
cd ~/work/git-gui
mkdir -p .git-gui
cat > .git-gui/repos.toml <<'EOF'
[[repos]]
name = "git-gui"
path = "."

[[repos]]
name = "测试仓库"
path = "/tmp/test-repo"
EOF
cd gui && npm run tauri dev
# 观察：左侧边栏 + Tab 切换
```

---

## 验证清单

UI 验证：

- [ ] 单仓库：无左侧边栏
- [ ] 多仓库：左侧边栏 30 列
- [ ] 状态图标：✓/●/!/✗/? 颜色正确
- [ ] 当前仓库：高亮显示
- [ ] Tab 切换：动画流畅
- [ ] 右侧标题：显示当前仓库名称

功能验证：

- [ ] 切换后状态重新加载
- [ ] 所有键位正常（s/S/l/d/p/u/r/q）
- [ ] 错误仓库不影响其他仓库
- [ ] 配置文件热加载（重启后生效）

性能验证：

- [ ] 10+ 仓库切换流畅
- [ ] 启动时间 < 1s
- [ ] 内存占用合理

---

## 已知限制

1. **配置文件不热加载**：修改 repos.toml 需要重启应用
2. **相对路径**：相对于配置文件所在目录（.git-gui/），不是 cwd
3. **仓库数量**：无硬性限制，但左侧边栏高度受终端限制
4. **submodules**：只自动检测主仓库的 submodules，不递归

---

## 下一步

验证完成后：

1. 更新 README.md 添加配置文件示例
2. 补充集成测试（如果需要）
3. 合并 PR #5

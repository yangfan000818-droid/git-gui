# 多仓库配置示例

## 配置文件路径
`.git-gui/repos.toml`（相对于主仓库根目录）

## 格式

```toml
# 主仓库（可选，默认自动添加）
[[repos]]
name = "主仓库"
path = "."  # 相对于配置文件所在目录

# 独立仓库
[[repos]]
name = "前端"
path = "../frontend"  # 相对路径

[[repos]]
name = "后端"
path = "/absolute/path/to/backend"  # 绝对路径

# 子仓库会自动从 .gitmodules 检测并合并
```

## 行为

1. **启动时**：
   - 检查 `.git-gui/repos.toml` 是否存在
   - 不存在 → 只加载当前仓库 + 自动检测的 submodules
   - 存在 → 解析配置 + 合并 submodules

2. **仓库列表来源**：
   - 配置文件中的 `[[repos]]`
   - 自动检测的 submodules（如果有）
   - 合并去重（按 path 去重，配置优先）

3. **切换逻辑**：
   - Tab：下一个仓库
   - Shift+Tab：上一个仓库
   - 1-9：直接跳到对应索引（超出范围忽略）

4. **左侧边栏显示**：
   ```
   ┌─ 仓库列表 ─────┐
   │ ● 主仓库       │  ← 当前仓库（高亮）
   │ ✓ 前端         │
   │ ● 后端         │
   │ ? vendor/dep   │  ← submodule（未初始化）
   └────────────────┘
   ```

## 实现步骤

1. 添加 toml 依赖（或手写简单解析器）
2. 定义 RepoConfig 结构
3. 解析 repos.toml
4. 合并 submodules
5. 重构 AppState 支持多仓库
6. 实现切换逻辑
7. 左侧边栏 UI

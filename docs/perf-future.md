# 性能优化未落地项

> 记录于 2026-06-25，commit `d08ce6d` 落地了 P0+P1 六项。以下为评估后暂缓的项。

## P2-7: 常驻 git 进程

**问题**：`gitcore` 每次操作 fork `git` 子进程，Windows `CreateProcess` 成本高（10–50× macOS/Linux `fork+exec`），高频刷新下仍是瓶颈之一。

**方案**：启动时 spawn 长驻 `git cat-file --batch` / `git update-index` 等进程，通过 stdin/stdout 管道复用进程，免重复创建。

**暂缓原因**：

- 架构级重构：需改动 `gitcore` 核心调用层（`git.rs` run/run_checked），新增进程池管理、管道复用、超时回收、异常重启等基础设施。
- 回归面大：所有 git 调用都经此层，需大量端到端测试覆盖（fetch/push/commit/rebase/submodule 等）。
- P0/P1 已大幅降低进程数（status 4→1、并发 log、监听收窄），Windows 卡顿预期已显著改善。

**回访条件**：P0/P1 在 Windows 实机验证后仍不满足，且瓶颈确认在 git 进程创建开销。

## P2-8: Defender 排除自动执行

**问题**：`Add-MpPreference -ExclusionPath` 需管理员权限，且涉及系统安全配置，不宜由应用静默执行。

**当前做法**：Settings 页显示可复制的 PowerShell 命令（仅 Windows 可见），用户自行在管理员终端执行。

**可能的增强**（待评估）：

- 首启弹窗引导：检测 Windows 且仓库不在排除列表时，弹出引导对话框，提供"一键复制命令"+"打开管理员 PowerShell"按钮。
- 风险：弹窗打扰可能引起反感；部分用户用其他杀软（ESET / Kaspersky），Defender 排除无效。

**回访条件**：用户反馈 Windows 上即便 P0/P1 落地后仍有明显卡顿，且确认是杀软扫描导致。

## 未纳入方案的杂项

以下在探查时注意到但未列入优化方案的潜在点，仅作备忘：

- **SVG 拓扑图 lane 爆炸**：筛选态退化为线性列表（`filtering` 标志已处理），但未筛选时极端多分支仓库（几百个并行分支）的 lane 展开可能导致 SVG 图过宽。当前无实际案例触发，暂不设限。
- **DiffView 全量字符 diff**：`buildCharSegments` 对每个 hunk 的连续删/增行做逐字符 diff（O(n×m)），600 行熔断已覆盖极端情况，普通 diff 可接受。
- **reload 后 reconcileSelection 的 O(n) find**：每次刷新遍历 repos 查找选中文件，仓库数通常 < 10，暂不优化。

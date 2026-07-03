# 危险操作统一撤销 — 设计方案

> 2026-07-03 起草并落地,v1.1 安全网第一项。矩阵与代码同步(实现时的三处偏差已回写)。目标:危险操作完成后 toast 提供一键撤销,
> 把「做错了 → 翻 reflog 自救」升级为「做错了 → 点一下回来」。

## 原则

- **撤销 = 已有恢复原语的便捷入口**,不发明新的持久化机制;reflog 页仍是兜底。
- **诚实标注不可恢复项**:撤销矩阵之外的损失(如 hard reset 清掉的未提交改动)
  不假装能救,在确认弹窗里说清楚。
- 撤销 token 仅存于前端内存、单次有效、被下一个 toast 顶掉即失效;不做跨会话持久化。

## 撤销矩阵(P1)

| 操作 | 事前捕获 | 撤销原语 | 失效窗口 / 边界 |
| --- | --- | --- | --- |
| reset --hard/mixed/soft | 操作前 `HEAD` sha(`rev-parse HEAD`)+ 原 mode | **按原 mode** `git reset <pre-sha>`(soft 的撤销用 hard 会误杀工作区改动) | 撤销只还 HEAD 位置;hard 清掉的**未提交改动不在恢复范围**(确认弹窗已警示)。hard 的撤销前若工作区又有新改动 → 弹确认 |
| 删除本地分支 | 分支 tip sha(`rev-parse <branch>`) | `git branch <name> <sha>` | tip 变为不可达对象后受 gc 窗口限制(默认约 2 周);会话内撤销远早于此 |
| stash drop | 该 stash 的 commit sha + 原始 reflog subject(`%gs`) | `git stash store -m "<subject>" <sha>` | 同上 gc 窗口;store 回列表顶部(位置可能与原来不同,可接受) |
| 丢弃文件改动(discard) | 现有实现本就 `stash push --include-untracked` 兜底 → 记该兜底的 commit sha | `git stash apply <sha>`,成功后按 sha 定位兜底条目 drop 掉 | **含 untracked**(比原方案的 stash create 更全)。apply 冲突失败时不 drop,兜底仍留在 Stash 列表可手动找回——天然守卫,无需前置确认 |

### P2(顺延,不在本期)

- **删远程分支**:捕获 tip → `git push origin <sha>:refs/heads/<name>` 恢复。
- **force push 回滚**:push 前捕获远端 tip → `push --force-with-lease` 推回旧值(撤销本身有覆盖他人提交风险,需单独确认文案)。
- **hard reset 前自动备份工作区**(`stash create` + 撤销时贴回),把矩阵第一行的"不在恢复范围"也补上。
- merge / rebase / cherry-pick 完成态的 toast 撤销(`ORIG_HEAD`);现有 reflog 页已可恢复,优先级最低。

## 后端设计(gitcore 沉淀编排,零新依赖)

新增 `crates/gitcore/src/undo.rs`:

```rust
/// 一次危险操作的撤销凭据(serde 直通前端,前端原样传回)。
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum UndoAction {
    ResetTo { sha: String, mode: ResetMode },
    RestoreBranch { name: String, sha: String },
    RestoreStash { sha: String, message: String },
    RestoreDiscard { sha: String },
}

pub(crate) fn undo(repo: &Repo, action: &UndoAction) -> Result<(), Error>;
```

现有四个操作函数各加"捕获 + 返回凭据"的轻改(签名变化,调用点仅 lib.rs command 层):

- `reset.rs::reset` → 执行前 `rev-parse HEAD`,返回 `UndoAction::ResetTo`。
- `branch.rs::delete_branch` → 执行前 `rev-parse <name>`,返回 `RestoreBranch`。
- `stash.rs::stash_drop` → 执行前 `rev-parse <ref>` + 读 message,返回 `RestoreStash`。
- `stage.rs::discard_files` → 复用既有 stash 兜底,记 `refs/stash` sha 返回
  `RestoreDiscard`;无实际改动返回 `None`(无凭据,不出撤销按钮)。

Tauri 侧:上述 4 个 command 返回值加 `Option<UndoAction>`;新增 `repo_undo(path, action)`。

**撤销守卫**:仅 `ResetTo { mode: Hard }` 需要——hard 重置后工作区应干净,撤销时
status 有改动即期间新产生,前端 `ask` 确认后才执行。`RestoreDiscard` 靠 `stash apply`
冲突自然拒绝(失败不 drop 兜底);soft/mixed 撤销不动工作区。后端 undo 保持纯执行。

## 前端设计

1. **Toast.svelte 扩展**:加可选 `action?: { label: string; onAction: () => void }`,
   渲染为消息右侧按钮;带撤销的 toast `duration` 用 8000ms(普通 toast 维持 3500)。
   保持现有单实例替换语义:新 toast 顶掉旧的,撤销入口随之失效(P1 可接受)。
2. **调用点接线**(操作成功 → toast 文案 + 撤销按钮):
   - reset:HistoryView 的 reset 面板。
   - 删本地分支:BranchPicker。
   - stash drop:StashView。
   - discard:+page(Changes 视图,文件/多选丢弃)。
3. 撤销执行:`invoke("repo_undo", { path, action })` → 成功后 `refresh()` + 成功 toast
   (无撤销按钮,防套娃);失败把错误进 toast(error)。

## 测试与验收

- gitcore:`undo.rs` 集成测试(fixture 仓库),每种 UndoAction 一条
  「操作 → undo → 状态断言还原」;discard 案例含 tracked 混合改动。
- 前端:svelte-check + 手测清单(四个操作各一遍撤销、toast 顶掉后无入口、
  守卫弹窗触发)。
- 门禁:`cargo clippy -p gitcore -p gui --all-targets` 零新增警告。

## 工作量预估

gitcore(undo.rs + 4 处轻改 + 测试)约半天;前端(Toast 扩展 + 4 个调用点)约半天。

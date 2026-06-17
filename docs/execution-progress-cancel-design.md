# 执行层 进度 / 取消 方案

> 状态:设计草案,待评审。范围限定 gitcore 执行层(`git.rs` + `Repo`),不含具体 UI。

## 1. 背景:当前执行模型

gitcore 所有 git 调用都收敛到两个原语([git.rs](../crates/gitcore/src/git.rs)):

```rust
run(workdir, args)         -> Result<String, Error>      // 非零退出即 Err
run_checked(workdir, args) -> Result<Output, Error>      // 非零退出原样返回
```

两者都是 `Command::new("git").args(..).current_dir(..).output()` ——
**阻塞调用,一次性缓冲全部 stdout/stderr,进程退出后才返回**。
`Repo` 仅通过 `git()` / `git_checked()` 暴露执行能力,上层(TUI / 未来 GUI)
无法介入进程生命周期。

## 2. 问题

对 `status` / `log` / `stash list` 这类毫秒级命令,现模型完全够用,**不必改**。
问题集中在少数长操作 —— `fetch` / `push` / 未来的 `clone`:

1. **UI 假死**:`.output()` 在进程退出前不返回,调用线程被独占。fetch 一个大仓
   可能几十秒,期间界面无响应、无任何反馈。
2. **无进度**:git 的进度写在 stderr(`Receiving objects: 45% (450/1000)`,以 `\r` 刷新),
   但 `.output()` 把它整段吞掉,结束后才能看到,失去进度意义。
3. **不可取消**:没有 `Child` 句柄,无法中止一个慢/卡住的 fetch;用户只能等或杀进程。

## 3. 目标 / 非目标

**目标**
- 长操作(fetch / push,后续 clone)能流式上报进度。
- 长操作可被协作式取消,且取消后仓库不残留半完成状态。
- 核心库保持 UI 无关、同步 API;线程编排留给上层。
- 增量演进:现有 `run` / `run_checked` 与所有即时命令保持不变。

**非目标(本期不做)**
- 不引入 async 运行时(tokio 等);用 std 线程 + channel 足矣。
- 不做并发限流 / 队列调度。
- 不替换 git CLI(仍 spawn 子进程,不引入 libgit2)。
- merge/rebase 本身不上报进度(本地操作通常很快;只保证可取消 + 可清理)。

## 4. 总体设计

三块,彼此正交:

1. **流式执行原语** `run_streaming`:`spawn()` 子进程,逐段读 stderr 解析进度,
   通过回调上报;轮询取消令牌,置位则 kill 子进程。
2. **取消令牌** `CancelToken`:`Arc<AtomicBool>` 薄封装,UI 持一份、执行层轮询一份。
3. **取消后清理契约**:kill 只停进程,半完成的 merge/rebase 必须由调用方跑 abort
   还原 —— 复用现有 [`abort_update`](../crates/gitcore/src/update.rs) /
   `recover_or_strand` 的清理路径。

## 5. 进度:流式读取与解析

git 对 fetch/push/clone 加 `--progress` 会强制把进度写 stderr(即使输出非 tty)。
进度行以 `\r` 分隔刷新、阶段结束以 `\n` 收尾,形如:

```
Receiving objects:  45% (450/1000), 1.2 MiB | 600 KiB/s\r
Receiving objects: 100% (1000/1000), 2.5 MiB | 600 KiB/s, done.\n
Resolving deltas:   30% (90/300)\r
```

**执行流程(`run_streaming` 草案)**

```rust
// git.rs —— 草案,非最终代码
pub(crate) fn run_streaming(
    workdir: &Path,
    args: &[&str],
    on_progress: &mut dyn FnMut(Progress),
    cancel: &CancelToken,
) -> Result<Output, Error> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // stdout 另起线程抽干,避免双管道写满导致死锁(porcelain 输出走这里)。
    let mut out = child.stdout.take().unwrap();
    let stdout_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = out.read_to_string(&mut buf);
        buf
    });

    // stderr:按 `\r` / `\n` 切段,解析进度,同时轮询取消。
    let err = child.stderr.take().unwrap();
    let mut stderr_buf = String::new();
    for chunk in read_progress_chunks(err) {        // 自定义分隔读取
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Cancelled);
        }
        if let Some(p) = parse_progress(&chunk) {
            on_progress(p);
        }
        stderr_buf.push_str(&chunk);
    }

    let status = child.wait()?;
    let stdout = stdout_handle.join().unwrap_or_default();
    Ok(Output { success: status.success(), code: status.code(), stdout, stderr: stderr_buf })
}
```

```rust
/// 一次进度事件。
pub struct Progress {
    pub phase: String,         // "Receiving objects" / "Resolving deltas" ...
    pub percent: Option<u8>,   // 解析失败则 None,raw 仍可显示
    pub raw: String,           // 原始行,UI 可直接展示
}
```

`parse_progress` 只做"尽力而为":能抽出 `phase` + `percent` 就抽,抽不出就
只带 `raw`。解析失败绝不报错,进度是展示信息不是控制流。

**双管道死锁**:stdout / stderr 任一管道写满而无人读会阻塞子进程。
故 stdout 必须并行抽干(上面用一个 reader 线程),stderr 在主循环读。

## 6. 取消:令牌 + kill + 清理契约

```rust
/// 取消令牌:UI 置位,执行层轮询。clone 出的副本共享同一标志位。
#[derive(Clone, Default)]
pub struct CancelToken(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl CancelToken {
    pub fn cancel(&self)        { self.0.store(true, Ordering::SeqCst); }
    pub fn is_cancelled(&self)  -> bool { self.0.load(Ordering::SeqCst) }
}
```

**取消粒度**:协作式 —— 在 stderr 读循环的每段间隙检查令牌。git 进度刷新频繁
(亚秒级),响应延迟可接受;无需强中断。

**进程组(Unix)**:fetch 经 ssh/https 会派生子进程,只 kill `git` 主进程可能留下
孤儿。Unix 下用 `CommandExt::process_group(0)` 让 git 自成进程组,取消时 kill 整组。
Windows 用 Job Object,本期可先只 kill 主进程,留待后续。

**取消后清理契约(关键,接已修的 autostash 洞)**

kill 只是停进程,**不负责回滚仓库状态**。按操作分类:

| 操作 | kill 后残留 | 清理动作 |
|---|---|---|
| `fetch` | 无(写对象是原子的,半包丢弃) | 无需清理 |
| `push` | 无(远端原子接收) | 无需清理 |
| `merge` / `rebase` | 可能留 MERGE_HEAD / rebase-merge | 调 `abort_update` |
| 带 autostash 的 update | 半完成整合 + stash 未还原 | 走 `recover_or_strand` 同款路径 |

即:**取消一个 autostash update,与"整合非冲突失败"是同一个收尾问题** ——
撤销半完成整合 + 还原 autostash,绝不把脏改动遗弃在 stash 里。
本次修复的 `recover_or_strand` 已经是这条路径的实现,取消路径应复用它,
而非另写一份。

## 7. 错误处理

`Error` 新增一个变体([error.rs](../crates/gitcore/src/error.rs)):

```rust
/// 操作被用户取消。
Cancelled,
```

调用方据此与真实失败区分:取消是预期结果,不应弹错误,通常静默回到操作前状态。
注意现有 `Display` 已把"无退出码"渲染成"信号中断";被 kill 的子进程若走到
`run_checked` 路径会得到 `code: None`,但取消路径应优先返回 `Error::Cancelled`
而不是 `Error::Git`,避免歧义。

## 8. API 草案:上层怎么用

核心库保持同步;UI 自己开线程,用 channel 收进度。

```rust
// gitcore 暴露(草案)
impl Repo {
    pub fn fetch_streaming(
        &self,
        on_progress: &mut dyn FnMut(Progress),
        cancel: &CancelToken,
    ) -> Result<(), Error>;
}

// TUI/GUI 侧(示意)
let cancel = CancelToken::default();
let (tx, rx) = std::sync::mpsc::channel();
let token = cancel.clone();
let repo = repo.clone();                       // Repo: Clone,仅含 workdir
std::thread::spawn(move || {
    let mut cb = |p: Progress| { let _ = tx.send(p); };
    let r = repo.fetch_streaming(&mut cb, &token);
    // 把最终结果也送回主线程
});
// 主循环:rx.try_recv() 刷新进度条;用户按 Esc → cancel.cancel();
```

`Progress` 回调在工作线程触发,故经 `mpsc` 转发到 UI 线程,UI 线程只读 channel,
不跨线程碰 UI 状态。

## 9. 分阶段落地

1. **P1 基础设施**:`CancelToken`、`Progress`、`Error::Cancelled`、`run_streaming`
   + `parse_progress`(纯函数,单元测试覆盖)。
2. **P2 fetch**:`fetch_streaming`(`fetch --prune --progress`)。最常见、最该有进度的操作。
   `plan_update` / `execute_update` 内部的 fetch 可切到此路径并接受可选进度回调。
3. **P3 push**:`push --progress`,复用同套设施。
4. **P4 取消接清理**:update 流程接 `CancelToken`,取消时走 `recover_or_strand` 收尾;
   补集成测试:autostash update 中途取消 → stash 必须还原、无半完成 merge
   (与 `update_restores_autostash_when_integration_fails` 同源的不变量)。
5. **P5(可选)**:clone 进度、进程组 kill、Windows Job Object。

## 10. 测试策略

- **`parse_progress` 纯函数**:喂真实 git 进度行样本,断言 phase/percent 提取。无需 spawn。
- **取消**:对一个可控的慢命令(如 fetch 一个故意 sleep 的本地 remote,或注入
  `GIT_TRACE` 拖慢)置位令牌,断言返回 `Error::Cancelled` 且进程已回收。
- **取消后清理**:复刻 autostash 不变量 —— 中途取消后 `stashes().is_empty()`、
  `dirty.txt` 还原、`status().conflicted` 为空。直接照搬本次修复测试的断言骨架。
- **死锁回归**:对会产生大 stdout 的命令跑 `run_streaming`,确认不挂死。

## 11. 未决问题

- 进度回调用 `&mut dyn FnMut` 还是直接传 `mpsc::Sender<Progress>`?前者更通用、
  可同步可异步;后者更省事但把 channel 焊进核心库。倾向前者。
- `--progress` 输出格式跨 git 版本是否稳定?解析需容错(已按"尽力而为"设计),
  必要时锁定最低 git 版本。
- 取消的响应延迟上界?当前依赖 stderr 刷新频率;静默阶段(如 `Resolving deltas`
  前的长暂停)可能延迟感知,必要时加超时轮询。

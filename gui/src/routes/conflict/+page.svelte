<script lang="ts">
  // 独立冲突解决窗口:取上下文(仓库路径)→ 渲染 ConflictView;解决/放弃后
  // emit conflict-done 通知主窗刷新并自关。可自由调整大小 / 全屏。
  import { invoke } from "@tauri-apps/api/core";
  import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { onMount, onDestroy } from "svelte";
  import ConflictView from "$lib/ConflictView.svelte";
  import "../../lib/themes.css";

  interface StashRef {
    label: string;
  }
  type IntegrationKind = "Merge" | "Rebase" | "CherryPick" | "Revert" | "None";
  interface ConflictState {
    kind: IntegrationKind;
    files: { path: string; kind: string }[];
    autostash: StashRef | null;
  }

  let path = $state("");
  let initialFile = $state<string | undefined>(undefined);
  let files = $state<string[]>([]);
  let autostash = $state<StashRef | null>(null);
  let kind = $state<IntegrationKind>("None");
  let stashRestore = $derived(kind === "None");
  let ready = $state(false);
  let error = $state("");
  let notice = $state(""); // 非错误提示(如多提交变基推进到下一个冲突)
  // 窗口内容已过期(整合在别处结束 / 外部新增了冲突文件);提示用户刷新,不自动重挂打断操作。
  let stale = $state("");
  let dirty = $state(false); // ConflictView 上报:编辑页有未写入的取舍
  let busy = $state(false); // 继续 / 放弃进行中
  let reloadKey = $state(0); // 上下文变化 / 多提交再冲突时强制 ConflictView 重挂

  async function applyAppearance() {
    try {
      const s = await invoke<{
        theme: string;
        density: string;
        font_size: string;
      }>("get_settings");
      const b = document.body;
      b.setAttribute("data-theme", s.theme || "neon-dark");
      b.setAttribute("data-density", s.density || "comfortable");
      b.setAttribute("data-font-size", s.font_size || "medium");
    } catch {
      document.body.setAttribute("data-theme", "neon-dark");
    }
  }

  async function loadState() {
    const cs = await invoke<ConflictState>("repo_conflict_state", { path });
    kind = cs.kind;
    files = cs.files.map((f) => f.path);
    autostash = cs.autostash;
  }

  // 重获焦点时静默探测:窗口只在 onMount / 上下文切换时取过状态,期间用户可能在终端
  // 解决了冲突,或在主窗 banner 放弃了整合。只提示,不自动重挂——重挂会丢正在做的取舍。
  async function probeStale() {
    if (!ready || !path || busy) return;
    try {
      const cs = await invoke<ConflictState>("repo_conflict_state", { path });
      // 整合已在别处收尾(完成或放弃):本窗口的继续 / 放弃都已失去对象。
      // stash 还原模式本就 kind=None,无从区分,跳过这一判定。
      if (!stashRestore && cs.kind === "None") {
        stale = "本次整合已在别处结束(已完成或已放弃),此窗口的内容已过期。";
        return;
      }
      const added = cs.files.filter((f) => !files.includes(f.path));
      if (added.length > 0) {
        stale = `检测到 ${added.length} 个新的冲突文件,点「刷新」重新载入。`;
        return;
      }
      stale = "";
    } catch {
      // 探测失败不打扰用户,保持现状。
    }
  }

  // 重载会重建 ConflictView,正在进行的取舍会丢 → 先确认。
  async function reloadContext() {
    if (dirty) {
      const ok = await ask(
        "当前文件有未写入的取舍,载入新的冲突上下文会丢弃它们。确定继续?",
        { title: "未保存", kind: "warning" },
      );
      if (!ok) return;
      dirty = false;
    }
    await loadContext();
  }

  async function loadContext() {
    ready = false;
    error = "";
    notice = "";
    stale = "";
    try {
      const ctx = await invoke<{
        path: string;
        initial_file: string | null;
      } | null>("get_conflict_context");
      if (!ctx) {
        error = "无冲突上下文";
        return;
      }
      path = ctx.path;
      initialFile = ctx.initial_file ?? undefined;
      await loadState();
      reloadKey++;
      ready = true;
    } catch (e) {
      error = String(e);
    }
  }

  let unlisten: UnlistenFn | null = null;
  let unlistenClose: UnlistenFn | null = null;
  let unlistenFocus: UnlistenFn | null = null;
  onMount(async () => {
    await applyAppearance();
    unlisten = await listen("conflict-context-changed", () => {
      void reloadContext();
    });
    // 切出去(终端解冲突 / 主窗放弃整合)再切回来 → 探测一次,对标主窗的 focus 刷新。
    unlistenFocus = await getCurrentWindow().onFocusChanged(
      ({ payload: focused }) => {
        if (focused) void probeStale();
      },
    );
    // 关窗前确认:未写入的取舍不能被一个红叉静默吞掉。
    unlistenClose = await getCurrentWindow().onCloseRequested(async (e) => {
      if (!dirty) return;
      e.preventDefault(); // 必须在 await 之前调用才生效
      const ok = await ask("有未写入的取舍,关闭窗口会丢弃。确定关闭?", {
        title: "未保存",
        kind: "warning",
      });
      if (ok) {
        dirty = false;
        await getCurrentWindow().destroy(); // close() 会再次触发本回调
      }
    });
    await loadContext();
  });
  onDestroy(() => {
    unlisten?.();
    unlistenClose?.();
    unlistenFocus?.();
  });

  async function finishAndClose(action: "resolved" | "aborted") {
    dirty = false; // 已落盘,别再拦关窗
    await emit("conflict-done", { path, action });
    await getCurrentWindow().close();
  }

  async function doContinue() {
    if (busy) return;
    busy = true;
    error = "";
    notice = "";
    try {
      if (stashRestore) {
        await invoke("finish_stash_restore_cmd", { path, autostash });
      } else {
        const r = await invoke<unknown>("continue_update_cmd", {
          path,
          autostash,
          recurseSubmodules: false,
        });
        // 仍冲突 → 重载续解,不关窗。两种成因看文件列表变没变区分,
        // 否则用户只看到窗口一闪回到清单页、列表还变了,完全不知道发生了什么。
        if (typeof r === "object" && r !== null && "Conflicted" in r) {
          const next =
            (r as { Conflicted: { files: string[] } }).Conflicted.files ?? [];
          const unchanged =
            next.length === files.length &&
            next.every((f) => files.includes(f));
          await loadState();
          reloadKey++;
          notice = unchanged
            ? "git 认为这些文件仍未解决(可能被外部改动了),请重新处理后再继续。"
            : `上一段整合已完成,下一个提交又产生了冲突,已载入新的 ${files.length} 个冲突文件。`;
          return;
        }
      }
      await finishAndClose("resolved");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function doAbort() {
    if (busy) return;
    const msg = stashRestore
      ? "确定放弃还原？工作区回到整合后状态,你的改动仍保留在 stash 里可重试。"
      : "确定放弃本次整合？工作区将回到整合前的状态。";
    if (!(await ask(msg, { title: "放弃", kind: "warning" }))) return;
    busy = true;
    error = "";
    notice = "";
    try {
      if (stashRestore) {
        await invoke("abort_stash_restore_cmd", { path });
      } else {
        await invoke("abort_update_cmd", { path, autostash });
      }
      await finishAndClose("aborted");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="conflict-window">
  {#if error}
    <pre class="cw-error">{error}</pre>
  {/if}
  {#if notice}
    <p class="cw-notice" role="status">{notice}</p>
  {/if}
  {#if stale}
    <div class="cw-stale" role="status">
      <span>{stale}</span>
      <button class="cw-stale-btn" onclick={() => void reloadContext()}
        >刷新</button
      >
    </div>
  {/if}
  {#if ready}
    {#key reloadKey}
      <ConflictView
        {path}
        conflictFiles={files}
        {autostash}
        {initialFile}
        {stashRestore}
        onContinue={doContinue}
        onAbort={doAbort}
        onReload={reloadContext}
        onDirtyChange={(d) => (dirty = d)}
      />
    {/key}
  {:else if !error}
    <p class="cw-loading">加载冲突…</p>
  {/if}
</div>

<style>
  :global(html),
  :global(body) {
    height: 100%;
    margin: 0;
  }
  .conflict-window {
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--bg-base, #0d1117);
    color: var(--text-primary, #e6edf3);
    font-family:
      -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  }
  .cw-error {
    background: #3a1d1d;
    border: 1px solid rgba(247, 120, 139, 0.25);
    border-radius: 6px;
    padding: 8px 12px;
    color: var(--color-error, #f7788b);
    white-space: pre-wrap;
    font-size: 12px;
    margin: 12px;
  }
  .cw-loading {
    color: var(--text-muted, #6e7681);
    padding: 16px;
  }
  .cw-notice {
    background: rgba(88, 166, 255, 0.12);
    border: 1px solid rgba(88, 166, 255, 0.3);
    border-radius: 6px;
    padding: 8px 12px;
    color: var(--text-primary, #e6edf3);
    font-size: 12.5px;
    margin: 12px 12px 0;
  }
  .cw-stale {
    display: flex;
    align-items: center;
    gap: 10px;
    background: rgba(210, 153, 34, 0.12);
    border: 1px solid rgba(210, 153, 34, 0.35);
    border-radius: 6px;
    padding: 8px 12px;
    color: var(--text-primary, #e6edf3);
    font-size: 12.5px;
    margin: 12px 12px 0;
  }
  .cw-stale span {
    flex: 1;
  }
  .cw-stale-btn {
    flex-shrink: 0;
    padding: 4px 12px;
    border-radius: 5px;
    border: 1px solid var(--border-default, #30363d);
    background: var(--bg-surface, #161b22);
    color: var(--text-primary, #e6edf3);
    font-size: 12px;
    cursor: pointer;
  }
  .cw-stale-btn:hover {
    border-color: var(--accent-cyan, #58a6ff);
  }
</style>

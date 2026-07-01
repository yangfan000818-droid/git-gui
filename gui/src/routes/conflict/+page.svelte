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

  async function loadContext() {
    ready = false;
    error = "";
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
  onMount(async () => {
    await applyAppearance();
    unlisten = await listen("conflict-context-changed", () => {
      void loadContext();
    });
    await loadContext();
  });
  onDestroy(() => {
    unlisten?.();
  });

  async function finishAndClose(action: "resolved" | "aborted") {
    await emit("conflict-done", { path, action });
    await getCurrentWindow().close();
  }

  async function doContinue() {
    try {
      if (stashRestore) {
        await invoke("finish_stash_restore_cmd", { path, autostash });
      } else {
        const r = await invoke<unknown>("continue_update_cmd", {
          path,
          autostash,
          recurseSubmodules: false,
        });
        // 仍冲突(多提交变基的下一个)→ 重载续解,不关窗。
        if (typeof r === "object" && r !== null && "Conflicted" in r) {
          await loadState();
          reloadKey++;
          return;
        }
      }
      await finishAndClose("resolved");
    } catch (e) {
      error = String(e);
    }
  }

  async function doAbort() {
    const msg = stashRestore
      ? "确定放弃还原？工作区回到整合后状态,你的改动仍保留在 stash 里可重试。"
      : "确定放弃本次整合？工作区将回到整合前的状态。";
    if (!(await ask(msg, { title: "放弃", kind: "warning" }))) return;
    try {
      if (stashRestore) {
        await invoke("abort_stash_restore_cmd", { path });
      } else {
        await invoke("abort_update_cmd", { path, autostash });
      }
      await finishAndClose("aborted");
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="conflict-window">
  {#if error}
    <pre class="cw-error">{error}</pre>
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
</style>

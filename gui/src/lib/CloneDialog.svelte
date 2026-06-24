<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onDestroy } from "svelte";

  interface Progress {
    phase: string;
    percent: number | null;
    raw: string;
  }

  let {
    onClose,
    onCloned,
  }: {
    onClose: () => void;
    onCloned: (path: string) => void;
  } = $props();

  let url = $state("");
  let parentDir = $state("");
  let cloning = $state(false);
  let error = $state("");
  let progress = $state<Progress | null>(null);
  let opId = $state("");
  let unlisten: UnlistenFn | null = null;

  // 目标子目录名预览(镜像后端 repo_name_from_url:取末段去 .git)。
  let repoName = $derived.by(() => {
    const t = url.trim().replace(/\/+$/, "");
    const last = t.split(/[/:]/).pop() ?? "";
    return last.replace(/\.git$/, "") || "repo";
  });

  function cleanup() {
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
  }
  onDestroy(cleanup);

  async function selectDir() {
    const result = await open({
      directory: true,
      multiple: false,
      title: "选择克隆到的父目录",
    });
    if (typeof result === "string") parentDir = result;
  }

  async function doClone() {
    if (!url.trim() || !parentDir || cloning) return;
    cloning = true;
    error = "";
    progress = null;
    opId = crypto.randomUUID();
    try {
      unlisten = await listen<Progress>("clone-progress", (e) => {
        progress = e.payload;
      });
    } catch {
      // 监听失败不阻塞克隆
    }
    try {
      const path = await invoke<string>("repo_clone", {
        url: url.trim(),
        parentDir,
        opId,
      });
      cleanup();
      onCloned(path);
    } catch (e) {
      error = String(e);
      cloning = false;
      cleanup();
    }
  }

  function cancelClone() {
    if (cloning && opId) invoke("cancel_op", { opId });
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !cloning) onClose();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="clone-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={() => !cloning && onClose()}
  onkeydown={onKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="clone-panel" onclick={(e) => e.stopPropagation()}>
    <div class="clone-header">
      <h3>克隆仓库</h3>
      <button
        class="btn-close"
        onclick={onClose}
        disabled={cloning}
        aria-label="关闭">×</button
      >
    </div>

    <label class="field">
      <span class="field-label">远程 URL</span>
      <input
        class="field-input"
        type="text"
        bind:value={url}
        placeholder="https://github.com/user/repo.git"
        disabled={cloning}
      />
    </label>

    <div class="field">
      <span class="field-label">克隆到</span>
      <div class="dir-row">
        <span class="dir-path" class:dir-empty={!parentDir}
          >{parentDir || "选择一个父目录…"}</span
        >
        <button class="btn-dir" onclick={selectDir} disabled={cloning}
          >选择…</button
        >
      </div>
      {#if parentDir && url.trim()}
        <span class="dir-hint">→ {parentDir}/{repoName}</span>
      {/if}
    </div>

    {#if error}
      <pre class="clone-error">{error}</pre>
    {/if}

    {#if cloning}
      <div class="progress-bar-wrap">
        <div
          class="progress-bar-fill"
          style="width: {progress?.percent ?? 0}%"
          role="progressbar"
          aria-valuenow={progress?.percent ?? 0}
          aria-valuemin="0"
          aria-valuemax="100"
        ></div>
        <span class="progress-text">
          {progress?.phase ?? "准备中…"}
          {#if progress?.percent != null}({progress?.percent}%){/if}
        </span>
      </div>
    {/if}

    <div class="clone-actions">
      {#if cloning}
        <button class="btn-cancel" onclick={cancelClone}>取消</button>
      {:else}
        <button
          class="btn-primary"
          onclick={doClone}
          disabled={!url.trim() || !parentDir}>克隆</button
        >
        <button class="btn-secondary" onclick={onClose}>关闭</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .clone-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .clone-panel {
    background: var(--bg-base);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 480px;
    max-width: 90vw;
    padding: 18px 20px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }
  .clone-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }
  .clone-header h3 {
    margin: 0;
    font-size: 16px;
    color: var(--text-primary);
  }
  .btn-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 22px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }
  .btn-close:hover:not(:disabled) {
    color: var(--text-primary);
  }
  .btn-close:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin-bottom: 14px;
  }
  .field-label {
    font-size: 12px;
    color: var(--text-secondary);
  }
  .field-input {
    background: var(--bg-surface);
    border: 1px solid var(--bg-hover);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    padding: 7px 10px;
  }
  .field-input:focus {
    outline: none;
    border-color: var(--accent-cyan);
  }
  .field-input:disabled {
    opacity: 0.5;
  }
  .dir-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .dir-path {
    flex: 1;
    min-width: 0;
    background: var(--bg-surface);
    border: 1px solid var(--bg-hover);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    padding: 7px 10px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dir-path.dir-empty {
    color: var(--text-muted);
  }
  .btn-dir {
    flex-shrink: 0;
    background: var(--bg-hover);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 7px 12px;
  }
  .btn-dir:hover:not(:disabled) {
    border-color: var(--accent-cyan);
  }
  .btn-dir:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .dir-hint {
    font-size: 11px;
    color: var(--text-muted);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .clone-error {
    background: rgba(247, 120, 139, 0.12);
    border: 1px solid rgba(247, 120, 139, 0.25);
    border-radius: 4px;
    color: var(--color-error);
    font-size: 12px;
    padding: 8px 10px;
    margin: 0 0 14px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .progress-bar-wrap {
    position: relative;
    background: var(--bg-surface);
    border-radius: 4px;
    height: 24px;
    overflow: hidden;
    margin-bottom: 14px;
  }
  .progress-bar-fill {
    position: absolute;
    inset: 0 auto 0 0;
    background: rgba(88, 166, 255, 0.3);
    transition: width 0.2s;
  }
  .progress-text {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    padding: 0 10px;
    font-size: 11px;
    color: var(--text-primary);
  }
  .clone-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .btn-primary {
    background: var(--accent-cyan);
    border: 1px solid #58a6ff;
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    padding: 7px 18px;
  }
  .btn-primary:hover:not(:disabled) {
    background: #58a6ff;
  }
  .btn-primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-secondary,
  .btn-cancel {
    background: var(--border-default);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 13px;
    padding: 7px 18px;
  }
  .btn-secondary:hover,
  .btn-cancel:hover {
    background: var(--bg-hover);
  }
</style>

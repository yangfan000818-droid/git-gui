<script lang="ts">
  // 清理未跟踪文件(git clean):先 dry-run 预览将删除的文件,确认后整批删。
  // 不含被 .gitignore 忽略的文件(不做 -x),安全优先。
  import { invoke } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";

  let {
    path,
    onCleaned,
    onClose,
  }: {
    path: string;
    onCleaned: () => void;
    onClose: () => void;
  } = $props();

  let files = $state<string[]>([]);
  let includeDirs = $state(true); // 同时清理未跟踪目录(-d)
  let loading = $state(true);
  let busy = $state(false);
  let error = $state("");
  let reqId = 0; // 预览请求序号:快速切换 includeDirs 时丢弃过期的并发响应

  async function preview() {
    const id = ++reqId;
    loading = true;
    error = "";
    try {
      const result = await invoke<string[]>("repo_clean_preview", {
        path,
        includeDirs,
      });
      if (id === reqId) files = result;
    } catch (e) {
      if (id === reqId) error = String(e);
    } finally {
      if (id === reqId) loading = false;
    }
  }

  async function doClean() {
    if (files.length === 0) return;
    if (
      !(await ask(
        `确定删除 ${files.length} 个未跟踪文件/目录?此操作不可恢复。`,
        { title: "清理未跟踪文件", kind: "warning" },
      ))
    )
      return;
    busy = true;
    error = "";
    try {
      await invoke<number>("repo_clean_force", { path, includeDirs });
      onCleaned();
      onClose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }

  // includeDirs 变化时重新预览。
  $effect(() => {
    void includeDirs;
    preview();
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="cl-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={onClose}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="cl-panel" onclick={(e) => e.stopPropagation()}>
    <div class="cl-header">
      <h3>清理未跟踪文件</h3>
      <button class="cl-close" onclick={onClose} aria-label="关闭">×</button>
    </div>

    {#if error}
      <div class="cl-error">{error}</div>
    {/if}

    <div class="cl-opts">
      <label class="cl-check">
        <input type="checkbox" bind:checked={includeDirs} disabled={busy} />
        同时清理未跟踪目录
      </label>
      <span class="cl-note">不含被 .gitignore 忽略的文件</span>
    </div>

    {#if loading}
      <p class="cl-muted">扫描中…</p>
    {:else if files.length === 0}
      <p class="cl-muted">没有需要清理的未跟踪文件</p>
    {:else}
      <div class="cl-count">将删除 {files.length} 项:</div>
      <ul class="cl-list">
        {#each files as f (f)}
          <li class="cl-file">{f}</li>
        {/each}
      </ul>
    {/if}

    <div class="cl-actions">
      <button class="cl-cancel" onclick={onClose} disabled={busy}>取消</button>
      <button
        class="cl-danger"
        disabled={busy || loading || files.length === 0}
        onclick={doClean}
      >
        {busy ? "清理中…" : `确认清理 ${files.length} 项`}
      </button>
    </div>
  </div>
</div>

<style>
  .cl-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .cl-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 480px;
    max-width: 92%;
    max-height: 82%;
    display: flex;
    flex-direction: column;
  }
  .cl-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 10px;
    flex-shrink: 0;
  }
  .cl-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
  .cl-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .cl-close:hover {
    color: var(--text-primary);
  }
  .cl-error {
    background: #3a1d1d;
    border-top: 1px solid rgba(247, 120, 139, 0.25);
    border-bottom: 1px solid rgba(247, 120, 139, 0.25);
    padding: 8px 18px;
    color: var(--color-error);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .cl-opts {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 18px 10px;
    border-bottom: 1px solid var(--bg-hover);
    flex-shrink: 0;
  }
  .cl-check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }
  .cl-note {
    font-size: 11px;
    color: var(--text-muted);
    margin-left: auto;
  }
  .cl-muted {
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
    padding: 22px 18px;
    margin: 0;
  }
  .cl-count {
    padding: 10px 18px 4px;
    font-size: 12px;
    color: var(--text-secondary);
    flex-shrink: 0;
  }
  .cl-list {
    list-style: none;
    margin: 0;
    padding: 0 18px 8px;
    overflow-y: auto;
    flex: 1;
  }
  .cl-file {
    font-size: 12px;
    color: var(--text-primary);
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    padding: 2px 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cl-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 18px;
    border-top: 1px solid var(--bg-hover);
    flex-shrink: 0;
  }
  .cl-cancel {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 13px;
    padding: 6px 16px;
  }
  .cl-cancel:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .cl-danger {
    background: rgba(247, 120, 139, 0.2);
    border: none;
    border-radius: 6px;
    color: #fff;
    cursor: pointer;
    font-size: 13px;
    padding: 6px 16px;
  }
  .cl-danger:hover:not(:disabled) {
    background: rgba(247, 120, 139, 0.3);
  }
  .cl-danger:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>

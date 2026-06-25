<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";

  interface ReflogEntry {
    selector: string;
    sha: string;
    full_sha: string;
    action: string;
  }

  let {
    path,
    repoLabel = "",
    onChanged,
    onClose,
  }: {
    path: string;
    repoLabel?: string; // 非空 = 子仓相对路径,标在标题区
    onChanged: () => void;
    onClose: () => void;
  } = $props();

  let entries = $state<ReflogEntry[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state("");
  let resetMode = $state<"Soft" | "Mixed" | "Hard">("Mixed");
  let showModePicker = $state(false);
  let pendingEntry = $state<ReflogEntry | null>(null);

  async function load() {
    loading = true;
    error = "";
    try {
      entries = await invoke<ReflogEntry[]>("repo_reflog", {
        path,
        maxCount: 100,
      });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // 把当前分支移回某条 reflog 状态。
  async function restore(e: ReflogEntry) {
    const modes: Record<string, string> = {
      Soft: "改动保留在暂存区",
      Mixed: "改动退回工作区（未暂存，不丢失）",
      Hard: "⚠ 丢弃工作区与暂存区的所有未提交改动（不可恢复）",
    };
    if (
      !(await ask(
        `把当前分支移到 ${e.selector}（${e.sha}）：${modes[resetMode]}。确定?`,
        { title: "重置到此", kind: "warning" },
      ))
    )
      return;
    // Hard 二次确认
    if (
      resetMode === "Hard" &&
      !(await ask("硬重置会永久丢弃未提交的改动，不可恢复。确认?", {
        title: "二次确认:硬重置",
        kind: "warning",
      }))
    )
      return;
    busy = true;
    error = "";
    try {
      await invoke("repo_reset", { path, sha: e.full_sha, mode: resetMode });
      await load();
      onChanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  // 先收起模式选择,确认后再执行
  function prepareRestore(e: ReflogEntry) {
    pendingEntry = e;
    showModePicker = true;
  }

  function cancelModePicker() {
    showModePicker = false;
    pendingEntry = null;
  }

  function handleKeydown(ev: KeyboardEvent) {
    if (ev.key === "Escape" && !busy) onClose();
  }

  $effect(() => {
    load();
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="rl-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={() => !busy && onClose()}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="rl-panel" onclick={(e) => e.stopPropagation()}>
    <div class="rl-header">
      <h3>Reflog · {repoLabel ? `${repoLabel} ` : ""}HEAD 历史</h3>
      <button
        class="rl-close"
        onclick={onClose}
        disabled={busy}
        aria-label="关闭">×</button
      >
    </div>

    <p class="rl-hint">
      HEAD 走过的每一步(含
      commit/reset/rebase/checkout)。变基或重置搞砸了,可从这里「重置到此」找回旧状态。
    </p>

    {#if error}
      <pre class="rl-error">{error}</pre>
    {/if}

    {#if loading}
      <p class="rl-muted">加载中…</p>
    {:else if entries.length === 0}
      <p class="rl-muted">没有 reflog 记录</p>
    {:else}
      {#if showModePicker && pendingEntry}
        <div class="rl-mode-picker">
          <p class="rl-mode-title">
            重置到 <code>{pendingEntry.sha}</code>：
          </p>
          <label class="rl-mode">
            <input
              type="radio"
              name="rlmode"
              value="Mixed"
              bind:group={resetMode}
            />
            <span>
              <b>混合（默认）</b>
              <small>移动分支指针，改动退回工作区（未暂存），不丢失</small>
            </span>
          </label>
          <label class="rl-mode">
            <input
              type="radio"
              name="rlmode"
              value="Soft"
              bind:group={resetMode}
            />
            <span>
              <b>软</b>
              <small>移动分支指针，改动保留在暂存区</small>
            </span>
          </label>
          <label class="rl-mode">
            <input
              type="radio"
              name="rlmode"
              value="Hard"
              bind:group={resetMode}
            />
            <span>
              <b>硬</b>
              <small class="rl-mode-danger"
                >移动分支指针，丢弃工作区与暂存区改动（不可恢复）</small
              >
            </span>
          </label>
          <div class="rl-mode-actions">
            <button
              class="rl-mode-confirm"
              class:danger={resetMode === "Hard"}
              disabled={busy}
              onclick={() => {
                const e = pendingEntry!;
                showModePicker = false;
                pendingEntry = null;
                restore(e);
              }}
            >
              确认重置
            </button>
            <button
              class="rl-mode-cancel"
              disabled={busy}
              onclick={cancelModePicker}
            >
              取消
            </button>
          </div>
        </div>
      {/if}
      <ul class="rl-list">
        {#each entries as e (e.selector)}
          <li class="rl-item">
            <span class="rl-selector">{e.selector}</span>
            <span class="rl-sha">{e.sha}</span>
            <span class="rl-action" title={e.action}>{e.action}</span>
            <button
              class="rl-restore"
              disabled={busy}
              onclick={() => prepareRestore(e)}
              title="把当前分支移到该状态">重置到此</button
            >
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .rl-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .rl-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 620px;
    max-width: 94%;
    max-height: 84%;
    display: flex;
    flex-direction: column;
  }
  .rl-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 6px;
  }
  .rl-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
  .rl-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .rl-close:hover:not(:disabled) {
    color: var(--text-primary);
  }
  .rl-close:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .rl-hint {
    color: var(--text-muted);
    font-size: 12px;
    margin: 0;
    padding: 0 18px 8px;
  }
  .rl-error {
    background: rgba(247, 120, 139, 0.12);
    border-top: 1px solid rgba(247, 120, 139, 0.25);
    border-bottom: 1px solid rgba(247, 120, 139, 0.25);
    padding: 8px 18px;
    color: var(--color-error);
    font-size: 12px;
    white-space: pre-wrap;
    margin: 0;
  }
  .rl-muted {
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
    padding: 22px 18px;
    margin: 0;
  }
  .rl-list {
    list-style: none;
    margin: 0;
    padding: 6px 0 12px;
    overflow-y: auto;
  }
  .rl-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 18px;
  }
  .rl-item:hover {
    background: var(--bg-elevated);
  }
  .rl-selector {
    color: var(--accent-cyan);
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    flex-shrink: 0;
    width: 92px;
  }
  .rl-sha {
    color: var(--text-muted);
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    flex-shrink: 0;
  }
  .rl-action {
    color: var(--text-secondary);
    font-size: 13px;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rl-restore {
    background: transparent;
    border: 1px solid var(--border-default);
    border-radius: 3px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
    flex-shrink: 0;
  }
  .rl-restore:hover:not(:disabled) {
    background: rgba(88, 166, 255, 0.12);
    border-color: rgba(88, 166, 255, 0.2);
    color: var(--accent-cyan);
  }
  .rl-restore:disabled {
    opacity: 0.4;
    cursor: default;
  }
  /* ── 重置模式选择器 ── */
  .rl-mode-picker {
    margin: 4px 18px 0;
    padding: 10px 12px;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 6px;
  }
  .rl-mode-title {
    font-size: 12px;
    color: var(--text-secondary);
    margin: 0 0 6px;
  }
  .rl-mode-title code {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
  }
  .rl-mode {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    padding: 4px 0;
    cursor: pointer;
    font-size: 12px;
  }
  .rl-mode b {
    font-weight: 500;
    color: var(--text-primary);
  }
  .rl-mode small {
    display: block;
    color: var(--text-muted);
    font-size: 11px;
  }
  .rl-mode-danger {
    color: var(--color-error) !important;
  }
  .rl-mode-actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .rl-mode-confirm {
    background: rgba(86, 211, 100, 0.12);
    border: 1px solid rgba(86, 211, 100, 0.25);
    border-radius: 4px;
    color: #fff;
    cursor: pointer;
    font-size: 12px;
    padding: 5px 14px;
  }
  .rl-mode-confirm:hover:not(:disabled) {
    background: rgba(86, 211, 100, 0.18);
  }
  .rl-mode-confirm.danger {
    background: rgba(247, 120, 139, 0.15);
    border-color: rgba(247, 120, 139, 0.3);
  }
  .rl-mode-confirm.danger:hover:not(:disabled) {
    background: rgba(247, 120, 139, 0.22);
  }
  .rl-mode-confirm:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .rl-mode-cancel {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 5px 12px;
  }
  .rl-mode-cancel:hover:not(:disabled) {
    background: var(--bg-hover);
  }
</style>

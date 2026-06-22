<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface StashEntry {
    reff: string;
    message: string;
    branch: string;
  }
  // PopResult: externally tagged
  type PopResult = "Clean" | { Conflict: string[] };

  let {
    path,
    hasChanges,
    onClose,
    onChanged,
  }: {
    path: string;
    hasChanges: boolean;
    onClose: () => void;
    onChanged: () => void;
  } = $props();

  let stashes = $state<StashEntry[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state("");
  let newMessage = $state("");

  async function load() {
    loading = true;
    error = "";
    try {
      stashes = await invoke<StashEntry[]>("repo_stashes", { path });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // 储藏当前工作区改动(含未跟踪);成功后刷新列表 + 外部(工作区已清空)。
  async function createStash() {
    busy = true;
    error = "";
    try {
      await invoke("repo_stash_push", {
        path,
        message: newMessage.trim() || null,
      });
      newMessage = "";
      await load();
      onChanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  // 应用(保留 stash):改动回到工作区,关闭面板让用户在改动列表查看。
  async function apply(reff: string) {
    busy = true;
    error = "";
    try {
      await invoke("repo_stash_apply", { path, reff });
      onChanged();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  // 弹出(应用 + 删除);冲突则改动带标记留工作区,提示去改动列表解决。
  async function pop(reff: string) {
    busy = true;
    error = "";
    try {
      const r = await invoke<PopResult>("repo_stash_pop", { path, reff });
      if (typeof r === "object" && "Conflict" in r) {
        alert("弹出时有冲突,改动已带冲突标记留在工作区,请在改动列表中解决。");
      }
      onChanged();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function drop(reff: string, message: string) {
    if (!confirm(`确定丢弃 stash「${message}」?此操作不可恢复。`)) return;
    busy = true;
    error = "";
    try {
      await invoke("repo_stash_drop", { path, reff });
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }

  $effect(() => {
    load();
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="sv-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={onClose}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="sv-panel" onclick={(e) => e.stopPropagation()}>
    <div class="sv-header">
      <h3>Stash 储藏</h3>
      <button class="sv-close" onclick={onClose} aria-label="关闭">×</button>
    </div>

    {#if error}
      <div class="sv-error">{error}</div>
    {/if}

    <!-- 储藏当前改动 -->
    <div class="sv-create">
      <input
        class="sv-input"
        type="text"
        bind:value={newMessage}
        placeholder={hasChanges ? "储藏说明(可选)" : "工作区干净,无改动可储藏"}
        disabled={busy || !hasChanges}
        onkeydown={(e) => e.key === "Enter" && hasChanges && createStash()}
      />
      <button
        class="sv-create-btn"
        disabled={busy || !hasChanges}
        onclick={createStash}
        title="把当前工作区改动(含未跟踪)储藏起来,工作区恢复干净"
      >
        储藏改动
      </button>
    </div>

    {#if loading}
      <p class="sv-muted">加载中…</p>
    {:else if stashes.length === 0}
      <p class="sv-muted">没有储藏</p>
    {:else}
      <ul class="sv-list">
        {#each stashes as s (s.reff)}
          <li class="sv-item">
            <div class="sv-info">
              <span class="sv-msg">{s.message}</span>
              <span class="sv-meta">{s.reff} · {s.branch}</span>
            </div>
            <div class="sv-actions">
              <button
                class="sv-apply"
                disabled={busy}
                onclick={() => apply(s.reff)}
                title="应用此储藏,保留在列表中(git stash apply)">应用</button
              >
              <button
                class="sv-pop"
                disabled={busy}
                onclick={() => pop(s.reff)}
                title="应用并从列表删除(git stash pop)">弹出</button
              >
              <button
                class="sv-drop"
                disabled={busy}
                onclick={() => drop(s.reff, s.message)}
                title="丢弃此储藏(git stash drop)">丢弃</button
              >
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .sv-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .sv-panel {
    background: #1e1e1e;
    border: 1px solid #444;
    border-radius: 8px;
    width: 460px;
    max-width: 92%;
    max-height: 82%;
    overflow-y: auto;
  }
  .sv-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 10px;
  }
  .sv-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: #e4e4e4;
    margin: 0;
  }
  .sv-close {
    background: transparent;
    border: none;
    color: #888;
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .sv-close:hover {
    color: #e4e4e4;
  }
  .sv-error {
    background: #3a1d1d;
    border-top: 1px solid #6a2b2b;
    border-bottom: 1px solid #6a2b2b;
    padding: 8px 18px;
    color: #f3b4b4;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .sv-muted {
    color: #666;
    font-size: 12px;
    text-align: center;
    padding: 22px 18px;
    margin: 0;
  }
  .sv-create {
    display: flex;
    gap: 8px;
    padding: 8px 18px 12px;
    border-bottom: 1px solid #333;
  }
  .sv-input {
    flex: 1;
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #e4e4e4;
    font-size: 12px;
    padding: 6px 8px;
    min-width: 0;
  }
  .sv-input:disabled {
    opacity: 0.5;
  }
  .sv-create-btn {
    background: #1d5a1d;
    border: 1px solid #3a7a3a;
    border-radius: 4px;
    color: #fff;
    cursor: pointer;
    font-size: 12px;
    padding: 6px 14px;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .sv-create-btn:hover:not(:disabled) {
    background: #256a25;
  }
  .sv-create-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .sv-list {
    list-style: none;
    margin: 0;
    padding: 6px 0 12px;
  }
  .sv-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 18px;
  }
  .sv-item:hover {
    background: #252525;
  }
  .sv-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .sv-msg {
    font-size: 13px;
    color: #e4e4e4;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sv-meta {
    font-size: 11px;
    color: #888;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .sv-actions {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }
  .sv-apply,
  .sv-pop,
  .sv-drop {
    background: transparent;
    border: 1px solid #444;
    border-radius: 3px;
    color: #999;
    cursor: pointer;
    font-size: 11px;
    padding: 3px 8px;
    flex-shrink: 0;
  }
  .sv-apply:hover:not(:disabled) {
    background: #1d3a24;
    border-color: #3a7a3a;
    color: #7ee29a;
  }
  .sv-pop:hover:not(:disabled) {
    background: #1d2b3a;
    border-color: #2b5a7a;
    color: #7ab8e2;
  }
  .sv-drop:hover:not(:disabled) {
    background: #3a2020;
    border-color: #7a3a3a;
    color: #f88;
  }
  .sv-apply:disabled,
  .sv-pop:disabled,
  .sv-drop:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>

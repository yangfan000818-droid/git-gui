<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface BranchInfo {
    name: string;
    is_current: boolean;
    upstream: string | null;
    ahead: number;
    behind: number;
  }

  interface Props {
    repoPath: string;
    onClose: () => void;
    onSwitched: () => void;
  }

  let { repoPath, onClose, onSwitched }: Props = $props();

  let branches = $state<BranchInfo[]>([]);
  let loading = $state(true);
  let error = $state("");
  let switching = $state(false);

  // 新建分支
  let newBranchName = $state("");

  async function load() {
    loading = true;
    error = "";
    try {
      branches = await invoke<BranchInfo[]>("repo_branches", {
        path: repoPath,
      });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function switchTo(name: string) {
    switching = true;
    error = "";
    try {
      await invoke("repo_switch_branch", { path: repoPath, name });
      onSwitched();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  async function createAndSwitch() {
    const name = newBranchName.trim();
    if (!name) return;
    switching = true;
    error = "";
    try {
      await invoke("repo_create_branch", { path: repoPath, name });
      await invoke("repo_switch_branch", { path: repoPath, name });
      newBranchName = "";
      onSwitched();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  async function deleteBranch(name: string) {
    if (!confirm(`确定删除分支 "${name}"?（仅安全删除：已合并 + 非当前分支）`))
      return;
    switching = true;
    error = "";
    try {
      await invoke("repo_delete_branch", { path: repoPath, name });
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }

  function handleNewKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") createAndSwitch();
  }

  $effect(() => {
    load();
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="bp-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={onClose}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="bp-panel" onclick={(e) => e.stopPropagation()}>
    <div class="bp-header">
      <h3>分支</h3>
      <button class="bp-close" onclick={onClose} aria-label="关闭">×</button>
    </div>

    {#if error}
      <div class="bp-error">{error}</div>
    {/if}

    <!-- 新建分支 -->
    <div class="bp-create">
      <input
        class="bp-input"
        type="text"
        bind:value={newBranchName}
        placeholder="+ 新建分支名称"
        disabled={switching}
        onkeydown={handleNewKeydown}
      />
      <button
        class="bp-create-btn"
        disabled={switching || !newBranchName.trim()}
        onclick={createAndSwitch}
      >
        新建并切换
      </button>
    </div>

    {#if loading}
      <p class="bp-muted">加载中…</p>
    {:else if branches.length === 0}
      <p class="bp-muted">没有本地分支</p>
    {:else}
      <ul class="bp-list">
        {#each branches as b}
          <li class="bp-item" class:bp-current={b.is_current}>
            <button
              class="bp-btn"
              disabled={switching || b.is_current}
              onclick={() => switchTo(b.name)}
            >
              <span class="bp-name">{b.name}</span>
              {#if b.is_current}
                <span class="bp-check">✓</span>
              {/if}
              {#if b.upstream}
                <span class="bp-upstream">{b.upstream}</span>
              {/if}
              <span class="bp-stats">
                {#if b.ahead > 0}<span class="badge ahead">↑{b.ahead}</span
                  >{/if}
                {#if b.behind > 0}<span class="badge behind">↓{b.behind}</span
                  >{/if}
              </span>
            </button>
            {#if !b.is_current}
              <button
                class="bp-del"
                disabled={switching}
                onclick={() => deleteBranch(b.name)}
                aria-label="删除分支 {b.name}"
                title="删除分支"
              >
                ×
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .bp-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .bp-panel {
    background: #1e1e1e;
    border: 1px solid #444;
    border-radius: 8px;
    width: 400px;
    max-width: 92%;
    max-height: 80%;
    overflow-y: auto;
  }
  .bp-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 10px;
  }
  .bp-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: #e4e4e4;
    margin: 0;
  }
  .bp-close {
    background: transparent;
    border: none;
    color: #888;
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .bp-close:hover {
    color: #e4e4e4;
  }
  .bp-error {
    background: #3a1d1d;
    border-top: 1px solid #6a2b2b;
    border-bottom: 1px solid #6a2b2b;
    padding: 8px 18px;
    color: #f3b4b4;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .bp-muted {
    color: #666;
    font-size: 12px;
    text-align: center;
    padding: 24px 18px;
    margin: 0;
  }

  /* 新建分支 */
  .bp-create {
    display: flex;
    gap: 6px;
    padding: 8px 18px 2px;
  }
  .bp-input {
    flex: 1;
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #e4e4e4;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    padding: 5px 8px;
    min-width: 0;
  }
  .bp-input:disabled {
    opacity: 0.4;
  }
  .bp-create-btn {
    background: #1d5a1d;
    border: 1px solid #3a7a3a;
    border-radius: 4px;
    color: #fff;
    cursor: pointer;
    font-size: 11px;
    padding: 5px 10px;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .bp-create-btn:hover:not(:disabled) {
    background: #256a25;
  }
  .bp-create-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* 分支列表 */
  .bp-list {
    list-style: none;
    margin: 0;
    padding: 6px 0 12px;
  }
  .bp-item {
    display: flex;
    margin: 0;
  }
  .bp-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    background: transparent;
    border: none;
    border-radius: 0;
    color: #e4e4e4;
    cursor: pointer;
    font-size: 13px;
    padding: 7px 0 7px 18px;
    text-align: left;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    min-width: 0;
  }
  .bp-btn:hover:not(:disabled) {
    background: #2a2a2a;
  }
  .bp-btn:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .bp-current .bp-btn {
    background: #1a2d3d;
  }
  .bp-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bp-check {
    color: #7ee29a;
    font-weight: 700;
    flex-shrink: 0;
  }
  .bp-upstream {
    font-size: 11px;
    color: #888;
    flex-shrink: 0;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bp-stats {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .badge {
    font-size: 10px;
    border-radius: 10px;
    padding: 1px 7px;
  }
  .ahead {
    background: #1d3a24;
    color: #7ee29a;
  }
  .behind {
    background: #1d2b3a;
    color: #7ab8e2;
  }

  /* 删除按钮 */
  .bp-del {
    background: transparent;
    border: none;
    color: #666;
    cursor: pointer;
    font-size: 16px;
    padding: 7px 12px;
    line-height: 1;
    flex-shrink: 0;
  }
  .bp-del:hover:not(:disabled) {
    color: #f88;
    background: #3a2020;
  }
  .bp-del:disabled {
    opacity: 0.3;
    cursor: default;
  }
</style>

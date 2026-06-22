<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface BranchInfo {
    name: string;
    is_current: boolean;
  }

  interface RepoRef {
    path: string;
    label: string;
  }

  interface Props {
    repos: RepoRef[];
    onClose: () => void;
    onSwitched: () => void;
  }

  let { repos, onClose, onSwitched }: Props = $props();

  let commonBranches = $state<string[]>([]);
  let loading = $state(true);
  let error = $state("");
  let switching = $state(false);
  // 逐仓切换结果
  let results = $state<{ repo: string; ok: boolean; msg: string }[]>([]);
  let done = $state(false);

  async function load() {
    loading = true;
    error = "";
    try {
      const all: string[][] = [];
      for (const r of repos) {
        const brs = await invoke<BranchInfo[]>("repo_branches", {
          path: r.path,
        });
        all.push(brs.map((b) => b.name));
      }
      // 所有仓库共有的分支(交集)
      if (all.length > 0) {
        let common = all[0];
        for (let i = 1; i < all.length; i++) {
          common = common.filter((n) => all[i].includes(n));
        }
        commonBranches = common.sort();
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function switchAll(name: string) {
    if (!confirm(`确定把全部 ${repos.length} 个仓库切换到 "${name}"?`)) return;
    switching = true;
    error = "";
    results = [];
    done = false;
    for (const r of repos) {
      try {
        await invoke("repo_switch_branch", { path: r.path, name });
        results = [...results, { repo: r.label, ok: true, msg: "已切换" }];
      } catch (e) {
        results = [...results, { repo: r.label, ok: false, msg: String(e) }];
      }
    }
    switching = false;
    done = true;
    onSwitched();
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
  class="cb-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={onClose}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="cb-panel" onclick={(e) => e.stopPropagation()}>
    <div class="cb-header">
      <h3>全部切换分支</h3>
      <button class="cb-close" onclick={onClose} aria-label="关闭">×</button>
    </div>

    <p class="cb-subtitle">所有仓库共有的分支（{repos.length} 个仓库）</p>

    {#if error}
      <div class="cb-error">{error}</div>
    {/if}

    {#if loading}
      <p class="cb-muted">加载中…</p>
    {:else if commonBranches.length === 0}
      <p class="cb-muted">没有共有分支</p>
    {:else}
      <ul class="cb-list">
        {#each commonBranches as name}
          <li>
            <button
              class="cb-btn"
              disabled={switching}
              onclick={() => switchAll(name)}
            >
              <span class="cb-name">{name}</span>
              <span class="cb-action">全部切换</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    {#if results.length > 0}
      <div class="cb-results">
        {#each results as r}
          <div class="cb-result" class:cb-ok={r.ok} class:cb-fail={!r.ok}>
            <span class="cb-repo">{r.repo}</span>
            <span class="cb-msg">{r.msg}</span>
          </div>
        {/each}
      </div>
      {#if done}
        <button class="cb-done" onclick={onClose}>关闭</button>
      {/if}
    {/if}
  </div>
</div>

<style>
  .cb-overlay {
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
  .cb-panel {
    background: #1e1e1e;
    border: 1px solid #444;
    border-radius: 8px;
    width: 420px;
    max-width: 92%;
    max-height: 80%;
    overflow-y: auto;
  }
  .cb-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 6px;
  }
  .cb-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: #e4e4e4;
    margin: 0;
  }
  .cb-close {
    background: transparent;
    border: none;
    color: #888;
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .cb-close:hover {
    color: #e4e4e4;
  }
  .cb-subtitle {
    font-size: 11px;
    color: #888;
    margin: 2px 18px 8px;
  }
  .cb-error {
    background: #3a1d1d;
    border-top: 1px solid #6a2b2b;
    border-bottom: 1px solid #6a2b2b;
    padding: 8px 18px;
    color: #f3b4b4;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .cb-muted {
    color: #666;
    font-size: 12px;
    text-align: center;
    padding: 24px 18px;
    margin: 0;
  }
  .cb-list {
    list-style: none;
    margin: 0;
    padding: 4px 0 8px;
  }
  .cb-btn {
    display: flex;
    align-items: center;
    width: 100%;
    background: transparent;
    border: none;
    border-radius: 0;
    color: #e4e4e4;
    cursor: pointer;
    font-size: 13px;
    padding: 8px 18px;
    text-align: left;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .cb-btn:hover:not(:disabled) {
    background: #2a2a2a;
  }
  .cb-btn:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .cb-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cb-action {
    font-size: 11px;
    color: #0e639c;
    flex-shrink: 0;
    margin-left: 8px;
  }

  .cb-results {
    border-top: 1px solid #383838;
    padding: 10px 18px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .cb-result {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 12px;
    padding: 3px 0;
  }
  .cb-ok .cb-msg {
    color: #7ee29a;
  }
  .cb-fail .cb-msg {
    color: #f3b4b4;
  }
  .cb-repo {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: #aaa;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cb-msg {
    flex-shrink: 0;
  }
  .cb-done {
    display: block;
    width: calc(100% - 36px);
    margin: 0 18px 14px;
    background: #333;
    border: 1px solid #555;
    border-radius: 4px;
    color: #ddd;
    cursor: pointer;
    font-size: 12px;
    padding: 6px 0;
  }
  .cb-done:hover {
    background: #444;
  }
</style>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface TagInfo {
    name: string;
    target: string;
    message: string;
  }

  let {
    path,
    onChanged,
    onClose,
  }: {
    path: string;
    onChanged: () => void;
    onClose: () => void;
  } = $props();

  let tags = $state<TagInfo[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state("");
  let newName = $state("");
  let newMessage = $state("");
  let pushedNames = $state<string[]>([]); // 本次会话已推送的 tag(显示 ✓)

  async function load() {
    loading = true;
    error = "";
    try {
      tags = await invoke<TagInfo[]>("repo_tags", { path });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // 在 HEAD 创建 tag(message 非空 → 注释标签)。
  async function create() {
    const name = newName.trim();
    if (!name) return;
    busy = true;
    error = "";
    try {
      await invoke("repo_create_tag", {
        path,
        name,
        target: null,
        message: newMessage.trim() || null,
      });
      newName = "";
      newMessage = "";
      await load();
      onChanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function remove(name: string) {
    if (!confirm(`确定删除 tag「${name}」?`)) return;
    busy = true;
    error = "";
    try {
      await invoke("repo_delete_tag", { path, name });
      await load();
      onChanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  // 推送该 tag 到默认远程。
  async function push(name: string) {
    busy = true;
    error = "";
    try {
      await invoke("repo_push_tag", { path, name });
      if (!pushedNames.includes(name)) pushedNames = [...pushedNames, name];
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
  class="tv-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={onClose}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="tv-panel" onclick={(e) => e.stopPropagation()}>
    <div class="tv-header">
      <h3>Tag 标签</h3>
      <button class="tv-close" onclick={onClose} aria-label="关闭">×</button>
    </div>

    {#if error}
      <div class="tv-error">{error}</div>
    {/if}

    <!-- 在 HEAD 创建 -->
    <div class="tv-create">
      <input
        class="tv-input"
        type="text"
        bind:value={newName}
        placeholder="新 tag 名称"
        disabled={busy}
      />
      <input
        class="tv-input"
        type="text"
        bind:value={newMessage}
        placeholder="注释(可选,留空为轻量标签)"
        disabled={busy}
        onkeydown={(e) => e.key === "Enter" && create()}
      />
      <button
        class="tv-create-btn"
        disabled={busy || !newName.trim()}
        onclick={create}
        title="在当前 HEAD 创建 tag(填注释则为注释标签)">创建</button
      >
    </div>

    {#if loading}
      <p class="tv-muted">加载中…</p>
    {:else if tags.length === 0}
      <p class="tv-muted">没有 tag</p>
    {:else}
      <ul class="tv-list">
        {#each tags as t (t.name)}
          <li class="tv-item">
            <div class="tv-info">
              <span class="tv-name">{t.name}</span>
              <span class="tv-meta">
                <span class="tv-target">{t.target}</span>
                {#if t.message}<span class="tv-msg">{t.message}</span>{/if}
              </span>
            </div>
            <button
              class="tv-push"
              disabled={busy || pushedNames.includes(t.name)}
              onclick={() => push(t.name)}
              title="把该 tag 推送到远程(git push <remote> <tag>)"
              >{pushedNames.includes(t.name) ? "已推送 ✓" : "推送"}</button
            >
            <button
              class="tv-del"
              disabled={busy}
              onclick={() => remove(t.name)}
              title="删除此 tag(git tag -d)">删除</button
            >
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .tv-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .tv-panel {
    background: #1e1e1e;
    border: 1px solid #444;
    border-radius: 8px;
    width: 480px;
    max-width: 92%;
    max-height: 82%;
    overflow-y: auto;
  }
  .tv-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 10px;
  }
  .tv-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: #e4e4e4;
    margin: 0;
  }
  .tv-close {
    background: transparent;
    border: none;
    color: #888;
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .tv-close:hover {
    color: #e4e4e4;
  }
  .tv-error {
    background: #3a1d1d;
    border-top: 1px solid #6a2b2b;
    border-bottom: 1px solid #6a2b2b;
    padding: 8px 18px;
    color: #f3b4b4;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .tv-muted {
    color: #666;
    font-size: 12px;
    text-align: center;
    padding: 22px 18px;
    margin: 0;
  }
  .tv-create {
    display: flex;
    gap: 8px;
    padding: 8px 18px 12px;
    border-bottom: 1px solid #333;
  }
  .tv-input {
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #e4e4e4;
    font-size: 12px;
    padding: 6px 8px;
    min-width: 0;
  }
  .tv-input:first-of-type {
    flex: 0 0 130px;
  }
  .tv-input:nth-of-type(2) {
    flex: 1;
  }
  .tv-input:disabled {
    opacity: 0.5;
  }
  .tv-create-btn {
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
  .tv-create-btn:hover:not(:disabled) {
    background: #256a25;
  }
  .tv-create-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .tv-list {
    list-style: none;
    margin: 0;
    padding: 6px 0 12px;
  }
  .tv-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 18px;
  }
  .tv-item:hover {
    background: #252525;
  }
  .tv-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .tv-name {
    font-size: 13px;
    color: #e2c47a;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tv-meta {
    display: flex;
    gap: 8px;
    min-width: 0;
    font-size: 11px;
  }
  .tv-target {
    color: #888;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    flex-shrink: 0;
  }
  .tv-msg {
    color: #999;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tv-push {
    background: transparent;
    border: 1px solid #444;
    border-radius: 3px;
    color: #bbb;
    cursor: pointer;
    font-size: 11px;
    padding: 3px 8px;
    flex-shrink: 0;
  }
  .tv-push:hover:not(:disabled) {
    background: #2a3a4a;
    border-color: #3a5a7a;
    color: #cfe2ff;
  }
  .tv-push:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .tv-del {
    background: transparent;
    border: 1px solid #444;
    border-radius: 3px;
    color: #999;
    cursor: pointer;
    font-size: 11px;
    padding: 3px 8px;
    flex-shrink: 0;
  }
  .tv-del:hover:not(:disabled) {
    background: #3a2020;
    border-color: #7a3a3a;
    color: #f88;
  }
  .tv-del:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>

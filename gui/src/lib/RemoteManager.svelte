<script lang="ts">
  // 远程仓库管理(对标 WebStorm Manage Remotes):列出/添加/改 URL/删除 remote。
  import { invoke } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";

  interface RemoteInfo {
    name: string;
    url: string;
    push_url: string;
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

  let remotes = $state<RemoteInfo[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state("");
  let newName = $state("");
  let newUrl = $state("");
  let editing = $state<string | null>(null); // 正在编辑 URL 的 remote 名
  let editUrl = $state("");

  async function load() {
    loading = true;
    error = "";
    try {
      remotes = await invoke<RemoteInfo[]>("repo_remotes", { path });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function add() {
    const name = newName.trim();
    const url = newUrl.trim();
    if (!name || !url) return;
    busy = true;
    error = "";
    try {
      await invoke("repo_remote_add", { path, name, url });
      newName = "";
      newUrl = "";
      await load();
      onChanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function startEdit(r: RemoteInfo) {
    editing = r.name;
    editUrl = r.url;
  }
  function cancelEdit() {
    editing = null;
    editUrl = "";
  }
  async function saveEdit(name: string) {
    const url = editUrl.trim();
    if (!url) return;
    busy = true;
    error = "";
    try {
      await invoke("repo_remote_set_url", { path, name, url });
      cancelEdit();
      await load();
      onChanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function remove(name: string) {
    const msg =
      remotes.length <= 1
        ? `确定删除唯一的远程「${name}」?删除后将无法推送/拉取,直到重新添加远程。`
        : `确定删除远程「${name}」?`;
    if (!(await ask(msg, { title: "删除远程", kind: "warning" }))) return;
    busy = true;
    error = "";
    try {
      await invoke("repo_remote_remove", { path, name });
      await load();
      onChanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (editing) cancelEdit();
      else onClose();
    }
  }

  $effect(() => {
    load();
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="rm-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={onClose}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="rm-panel" onclick={(e) => e.stopPropagation()}>
    <div class="rm-header">
      <h3>远程仓库</h3>
      <button class="rm-close" onclick={onClose} aria-label="关闭">×</button>
    </div>

    {#if error}
      <div class="rm-error">{error}</div>
    {/if}

    <!-- 添加 remote -->
    <div class="rm-create">
      <input
        class="rm-input rm-input-name"
        type="text"
        bind:value={newName}
        placeholder="名称(如 origin)"
        disabled={busy}
      />
      <input
        class="rm-input rm-input-url"
        type="text"
        bind:value={newUrl}
        placeholder="URL(https:// 或 git@…)"
        disabled={busy}
        onkeydown={(e) => e.key === "Enter" && add()}
      />
      <button
        class="rm-add-btn"
        disabled={busy || !newName.trim() || !newUrl.trim()}
        onclick={add}
        title="添加远程仓库(git remote add)">添加</button
      >
    </div>

    {#if loading}
      <p class="rm-muted">加载中…</p>
    {:else if remotes.length === 0}
      <p class="rm-muted">没有配置远程仓库</p>
    {:else}
      <ul class="rm-list">
        {#each remotes as r (r.name)}
          <li class="rm-item">
            <div class="rm-info">
              <span class="rm-name">{r.name}</span>
              {#if editing === r.name}
                <input
                  class="rm-input rm-edit-input"
                  type="text"
                  bind:value={editUrl}
                  disabled={busy}
                  onkeydown={(e) => e.key === "Enter" && saveEdit(r.name)}
                />
              {:else}
                <span class="rm-url" title={r.url}>{r.url}</span>
                {#if r.push_url && r.push_url !== r.url}
                  <span class="rm-url rm-pushurl" title={r.push_url}
                    >push: {r.push_url}</span
                  >
                {/if}
              {/if}
            </div>
            {#if editing === r.name}
              <button
                class="rm-btn rm-save"
                disabled={busy || !editUrl.trim()}
                onclick={() => saveEdit(r.name)}>保存</button
              >
              <button class="rm-btn" disabled={busy} onclick={cancelEdit}
                >取消</button
              >
            {:else}
              <button
                class="rm-btn"
                disabled={busy}
                onclick={() => startEdit(r)}
                title="修改 URL(git remote set-url)">改 URL</button
              >
              <button
                class="rm-btn rm-del"
                disabled={busy}
                onclick={() => remove(r.name)}
                title="删除此远程(git remote remove)">删除</button
              >
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .rm-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .rm-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 540px;
    max-width: 92%;
    max-height: 82%;
    overflow-y: auto;
  }
  .rm-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 10px;
  }
  .rm-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
  .rm-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .rm-close:hover {
    color: var(--text-primary);
  }
  .rm-error {
    background: #3a1d1d;
    border-top: 1px solid rgba(247, 120, 139, 0.25);
    border-bottom: 1px solid rgba(247, 120, 139, 0.25);
    padding: 8px 18px;
    color: var(--color-error);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .rm-muted {
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
    padding: 22px 18px;
    margin: 0;
  }
  .rm-create {
    display: flex;
    gap: 8px;
    padding: 8px 18px 12px;
    border-bottom: 1px solid var(--bg-hover);
  }
  .rm-input {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    padding: 6px 8px;
    min-width: 0;
  }
  .rm-input-name {
    flex: 0 0 130px;
  }
  .rm-input-url {
    flex: 1;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .rm-input:disabled {
    opacity: 0.5;
  }
  .rm-add-btn {
    background: rgba(86, 211, 100, 0.12);
    border: 1px solid rgba(86, 211, 100, 0.12);
    border-radius: 4px;
    color: #fff;
    cursor: pointer;
    font-size: 12px;
    padding: 6px 14px;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .rm-add-btn:hover:not(:disabled) {
    background: #256a25;
  }
  .rm-add-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .rm-list {
    list-style: none;
    margin: 0;
    padding: 6px 0 12px;
  }
  .rm-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 18px;
  }
  .rm-item:hover {
    background: var(--bg-surface);
  }
  .rm-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .rm-name {
    font-size: 13px;
    color: var(--accent-cyan);
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-weight: 600;
  }
  .rm-url {
    font-size: 11px;
    color: var(--text-muted);
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rm-pushurl {
    color: var(--text-secondary);
  }
  .rm-edit-input {
    width: 100%;
    box-sizing: border-box;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .rm-btn {
    background: transparent;
    border: 1px solid var(--border-default);
    border-radius: 3px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 11px;
    padding: 3px 8px;
    flex-shrink: 0;
  }
  .rm-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .rm-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .rm-save {
    border-color: rgba(86, 211, 100, 0.3);
    color: var(--accent-neon);
  }
  .rm-del:hover:not(:disabled) {
    background: #3a2020;
    border-color: #7a3a3a;
    color: #f88;
  }
</style>

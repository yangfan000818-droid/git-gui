<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask, message } from "@tauri-apps/plugin-dialog";

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
    changedFiles = [],
    onClose,
    onChanged,
  }: {
    path: string;
    hasChanges: boolean;
    changedFiles?: { path: string }[];
    onClose: () => void;
    onChanged: () => void;
  } = $props();

  let stashes = $state<StashEntry[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state("");
  let newMessage = $state("");
  let selectedFiles = $state<Set<string>>(new Set());

  // ── 目录树 ──
  interface StashDirNode {
    name: string;
    path: string; // 目录路径，如 "src/lib"
    dirs: StashDirNode[];
    files: { name: string; path: string }[];
    allPaths: string[];
  }

  function buildStashTree(files: { path: string }[]): StashDirNode {
    const root: StashDirNode = {
      name: "",
      path: "",
      dirs: [],
      files: [],
      allPaths: [],
    };
    for (const f of files) {
      const parts = f.path.split("/");
      const fileName = parts.pop() ?? f.path;
      let cur = root;
      let prefix = "";
      for (const part of parts) {
        prefix = prefix ? `${prefix}/${part}` : part;
        let next = cur.dirs.find((d) => d.name === part);
        if (!next) {
          next = {
            name: part,
            path: prefix,
            dirs: [],
            files: [],
            allPaths: [],
          };
          cur.dirs.push(next);
        }
        next.allPaths.push(f.path);
        cur = next;
      }
      cur.files.push({ name: fileName, path: f.path });
    }
    compressTree(root);
    return root;
  }

  // 连续单子目录合并
  function compressTree(dir: StashDirNode): void {
    for (const d of dir.dirs) compressTree(d);
    while (dir.name !== "" && dir.dirs.length === 1 && dir.files.length === 0) {
      const child = dir.dirs[0];
      dir.name = `${dir.name}/${child.name}`;
      dir.path = child.path;
      dir.dirs = child.dirs;
      dir.files = child.files;
    }
  }

  let tree = $derived(buildStashTree(changedFiles));
  let expandedDirs = $state(new Set<string>());

  function toggleDirExpand(path: string) {
    const next = new Set(expandedDirs);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    expandedDirs = next;
  }

  function dirCheckState(dir: StashDirNode): "all" | "partial" | "none" {
    const sel = dir.allPaths.filter((p) => selectedFiles.has(p)).length;
    if (sel === 0) return "none";
    if (sel === dir.allPaths.length) return "all";
    return "partial";
  }

  function toggleDir(dir: StashDirNode) {
    const state = dirCheckState(dir);
    const next = new Set(selectedFiles);
    if (state === "all") {
      for (const p of dir.allPaths) next.delete(p);
    } else {
      for (const p of dir.allPaths) next.add(p);
    }
    selectedFiles = next;
  }

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
        paths: null,
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

  // 储藏选中文件
  async function createSelectiveStash() {
    if (selectedFiles.size === 0) return;
    busy = true;
    error = "";
    try {
      await invoke("repo_stash_push", {
        path,
        message: newMessage.trim() || null,
        paths: [...selectedFiles],
      });
      newMessage = "";
      selectedFiles = new Set();
      await load();
      onChanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function toggleFile(fp: string) {
    const next = new Set(selectedFiles);
    if (next.has(fp)) next.delete(fp);
    else next.add(fp);
    selectedFiles = next;
  }

  // 应用(保留 stash):改动回到工作区,关闭面板让用户在改动列表查看。
  async function apply(reff: string) {
    const s = stashes.find((x) => x.reff === reff);
    const label = s?.message || reff;
    if (
      !(await ask(`确定应用 stash「${label}」? 应用会修改当前工作区文件。`, {
        title: "应用 Stash",
        kind: "warning",
      }))
    )
      return;
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
        await message(
          "弹出时有冲突,改动已带冲突标记留在工作区,请在改动列表中解决。",
          { title: "弹出有冲突", kind: "warning" },
        );
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
    if (
      !(await ask(`确定丢弃 stash「${message}」?此操作不可恢复。`, {
        title: "丢弃 Stash",
        kind: "warning",
      }))
    )
      return;
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
        onkeydown={(e) => {
          if (e.key !== "Enter" || !hasChanges || busy) return;
          if (selectedFiles.size > 0) createSelectiveStash();
          else createStash();
        }}
      />
      {#if changedFiles.length > 0}
        <button
          class="sv-create-btn"
          disabled={busy || selectedFiles.size === 0}
          onclick={createSelectiveStash}
          title={selectedFiles.size > 0
            ? `储藏选中的 ${selectedFiles.size} 个文件`
            : "请先在下方勾选文件"}
        >
          储藏选中{selectedFiles.size > 0 ? `(${selectedFiles.size})` : ""}
        </button>
        <button
          class="sv-all-btn"
          disabled={busy}
          onclick={createStash}
          title="储藏全部 {changedFiles.length} 个文件"
        >
          储藏全部
        </button>
      {:else}
        <button
          class="sv-create-btn"
          disabled={busy || !hasChanges}
          onclick={createStash}
          title="把当前工作区全部改动(含未跟踪)储藏起来"
        >
          储藏全部
        </button>
      {/if}
    </div>

    <!-- 目录树（有改动文件时始终展示） -->
    {#if changedFiles.length > 0}
      <div class="sv-filelist">
        {#each tree.dirs as d (d.path)}
          {@render stashTreeDir(d, 0)}
        {/each}
        {#each tree.files as f (f.path)}
          {@render stashTreeFile(f, 0)}
        {/each}
      </div>
    {/if}

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

<!-- ── 目录树渲染 snippet ── -->
{#snippet stashTreeDir(dir: StashDirNode, depth: number)}
  {@const checkState = dirCheckState(dir)}
  {@const open = expandedDirs.has(dir.path)}
  <div
    class="st-row st-dir"
    role="button"
    tabindex="0"
    style="padding-left: {4 + depth * 10}px"
    onclick={() => toggleDirExpand(dir.path)}
    onkeydown={(e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        toggleDirExpand(dir.path);
      }
    }}
  >
    <input
      type="checkbox"
      class="st-check"
      checked={checkState === "all"}
      class:st-partial={checkState === "partial"}
      onclick={(e) => {
        e.stopPropagation();
        toggleDir(dir);
      }}
      onkeydown={(e) => e.stopPropagation()}
      disabled={busy}
      aria-label="选择目录 {dir.name}"
    />
    <span class="st-caret">{open ? "▾" : "▸"}</span>
    <span class="st-dname">{dir.name}/</span>
    <span class="st-count">{dir.allPaths.length}</span>
  </div>
  {#if open}
    {#each dir.dirs as d (d.path)}
      {@render stashTreeDir(d, depth + 1)}
    {/each}
    {#each dir.files as f (f.path)}
      {@render stashTreeFile(f, depth + 1)}
    {/each}
  {/if}
{/snippet}

{#snippet stashTreeFile(file: { name: string; path: string }, depth: number)}
  <label class="st-row st-file" style="padding-left: {4 + depth * 10}px">
    <input
      type="checkbox"
      class="st-check"
      checked={selectedFiles.has(file.path)}
      onchange={() => toggleFile(file.path)}
      disabled={busy}
    />
    <span class="st-fname">{file.name}</span>
  </label>
{/snippet}

<style>
  .sv-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .sv-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 520px;
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
    color: var(--text-primary);
    margin: 0;
  }
  .sv-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .sv-close:hover {
    color: var(--text-primary);
  }
  .sv-error {
    background: #3a1d1d;
    border-top: 1px solid rgba(247, 120, 139, 0.25);
    border-bottom: 1px solid rgba(247, 120, 139, 0.25);
    padding: 8px 18px;
    color: var(--color-error);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .sv-muted {
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
    padding: 22px 18px;
    margin: 0;
  }
  .sv-create {
    display: flex;
    gap: 8px;
    padding: 8px 18px 12px;
    border-bottom: 1px solid var(--bg-hover);
  }
  .sv-input {
    flex: 1;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    padding: 6px 8px;
    min-width: 0;
  }
  .sv-input:disabled {
    opacity: 0.5;
  }
  .sv-create-btn {
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
  .sv-create-btn:hover:not(:disabled) {
    background: #256a25;
  }
  .sv-create-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .sv-all-btn {
    background: rgba(86, 211, 100, 0.06);
    border: 1px solid rgba(86, 211, 100, 0.18);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 6px 10px;
    white-space: nowrap;
    flex-shrink: 0;
    transition:
      background 0.15s,
      border-color 0.15s,
      color 0.15s;
  }
  .sv-all-btn:hover:not(:disabled) {
    background: rgba(86, 211, 100, 0.14);
    border-color: rgba(86, 211, 100, 0.35);
    color: #fff;
  }
  .sv-all-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .sv-filelist {
    max-height: 260px;
    overflow-y: auto;
    padding: 4px 12px 8px 0;
    border-bottom: 1px solid var(--bg-hover);
  }

  /* ── 目录树行 ── */
  .st-row {
    display: flex;
    align-items: center;
    gap: 3px;
    min-height: 24px;
    cursor: pointer;
    user-select: none;
  }
  .st-row:hover {
    background: var(--bg-hover);
  }
  .st-check {
    accent-color: var(--color-accent, #569cd6);
    flex-shrink: 0;
    width: 12px;
    height: 12px;
    margin: 0;
  }
  .st-check.st-partial {
    accent-color: var(--color-warning, #d4a72c);
  }
  .st-caret {
    color: var(--text-secondary);
    font-size: 11px;
    width: 10px;
    flex-shrink: 0;
    text-align: center;
  }
  .st-dir:hover .st-caret {
    color: var(--text-primary);
  }
  .st-dname {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .st-fname {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .st-file:hover .st-fname {
    color: var(--text-primary);
  }
  .st-count {
    font-size: 10px;
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .st-file {
    cursor: pointer;
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
    background: var(--bg-elevated);
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
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sv-meta {
    font-size: 11px;
    color: var(--text-muted);
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
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
    border: 1px solid var(--border-default);
    border-radius: 3px;
    color: var(--text-muted);
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

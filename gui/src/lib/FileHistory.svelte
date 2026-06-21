<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import DiffView from "./DiffView.svelte";

  interface LogEntry {
    sha: string;
    full_sha: string;
    message: string;
    author: string;
    date: string;
  }
  interface DiffLine {
    kind: "Context" | "Added" | "Removed";
    content: string;
  }
  interface Hunk {
    old_start: number;
    new_start: number;
    heading: string;
    lines: DiffLine[];
    raw: string;
  }
  interface FileDiff {
    path: string;
    binary: boolean;
    hunks: Hunk[];
    header_raw: string;
  }

  let {
    path,
    filePath,
    onClose,
  }: {
    path: string;
    filePath: string;
    onClose: () => void;
  } = $props();

  let commits: LogEntry[] = $state([]);
  let selectedCommit: LogEntry | null = $state(null);
  let diff: FileDiff | null = $state(null);
  let loading = $state(true);
  let diffLoading = $state(false);
  let error = $state(""); // 列表加载错误
  let diffError = $state(""); // diff 加载错误(独立,避免污染左侧列表)

  onMount(async () => {
    try {
      commits = await invoke<LogEntry[]>("repo_file_history", {
        path,
        filePath,
        maxCount: 100,
      });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  async function selectCommit(commit: LogEntry) {
    selectedCommit = commit;
    diffLoading = true;
    diff = null;
    diffError = "";
    try {
      diff = await invoke<FileDiff | null>("repo_commit_file_diff", {
        path,
        sha: commit.full_sha,
        filePath,
      });
    } catch (e) {
      diffError = String(e);
    } finally {
      diffLoading = false;
    }
  }

  function onActivate(e: KeyboardEvent, fn: () => void) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fn();
    }
  }
</script>

<div class="overlay" role="dialog" aria-label="文件历史">
  <div class="panel">
    <div class="header">
      <h2>文件历史: {filePath}</h2>
      <button class="close-btn" onclick={onClose} aria-label="关闭">✕</button>
    </div>

    <div class="content">
      <div class="commits-list">
        {#if loading}
          <p class="placeholder">加载中…</p>
        {:else if error}
          <p class="error">{error}</p>
        {:else if commits.length === 0}
          <p class="placeholder">该文件无提交历史</p>
        {:else}
          {#each commits as commit (commit.full_sha)}
            <div
              class="commit-row"
              class:selected={selectedCommit?.full_sha === commit.full_sha}
              role="button"
              tabindex="0"
              onclick={() => selectCommit(commit)}
              onkeydown={(e) => onActivate(e, () => selectCommit(commit))}
            >
              <span class="commit-sha">{commit.sha}</span>
              <span class="commit-message">{commit.message}</span>
              <span class="commit-author">{commit.author}</span>
              <span class="commit-date">{commit.date}</span>
            </div>
          {/each}
        {/if}
      </div>

      <div class="diff-panel">
        {#if diffLoading}
          <p class="placeholder">加载 diff 中…</p>
        {:else if diffError}
          <p class="error">{diffError}</p>
        {:else if selectedCommit && diff === null}
          <p class="placeholder">该提交未改动此文件</p>
        {:else if diff}
          <DiffView files={[diff]} />
        {:else}
          <p class="placeholder">选择一个提交查看 diff</p>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .panel {
    background: #1a1a1a;
    border: 1px solid #383838;
    border-radius: 8px;
    width: 90vw;
    height: 85vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid #383838;
    background: #252525;
  }
  .header h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: #ddd;
  }
  .close-btn {
    background: transparent;
    border: none;
    color: #aaa;
    font-size: 18px;
    cursor: pointer;
    padding: 4px 8px;
    line-height: 1;
    transition: color 0.15s;
  }
  .close-btn:hover {
    color: #fff;
  }

  .content {
    display: flex;
    flex: 1;
    overflow: hidden;
  }
  .commits-list {
    width: 40%;
    border-right: 1px solid #383838;
    overflow-y: auto;
    padding: 8px;
  }
  .diff-panel {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
  }

  .commit-row {
    display: grid;
    grid-template-columns: 70px 1fr 120px 100px;
    gap: 8px;
    padding: 8px;
    border-radius: 4px;
    font-size: 12px;
    cursor: pointer;
    transition: background 0.15s;
  }
  .commit-row:hover {
    background: #252525;
  }
  .commit-row.selected {
    background: #2a3a4a;
  }
  .commit-sha {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: #888;
  }
  .commit-message {
    color: #ddd;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .commit-author {
    color: #aaa;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .commit-date {
    color: #888;
    text-align: right;
  }

  .placeholder {
    color: #666;
    font-size: 12px;
    padding: 12px;
  }
  .error {
    color: #d88;
    font-size: 12px;
    padding: 12px;
  }
</style>

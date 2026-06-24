<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import DiffView from "./DiffView.svelte";

  interface BlameLine {
    sha: string;
    full_sha: string;
    author: string;
    time: number;
    line_no: number;
    content: string;
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

  let lines: BlameLine[] = $state([]);
  let loading = $state(true);
  let error = $state("");

  onMount(async () => {
    try {
      lines = await invoke<BlameLine[]>("repo_blame", { path, filePath });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  // unix 时间戳(秒)→ 本地日期;0 表示未知。
  function fmtDate(time: number): string {
    if (!time) return "";
    return new Date(time * 1000).toLocaleDateString();
  }

  // 点 sha → 显示该提交详情(message + 改动文件 diff);返回回到 blame 列表。
  let detailSha = $state<string | null>(null);
  let detailMsg = $state("");
  let detailDiffs = $state<FileDiff[]>([]);
  let detailLoading = $state(false);

  async function showDetail(sha: string) {
    detailSha = sha;
    detailLoading = true;
    detailMsg = "";
    detailDiffs = [];
    try {
      const [msg, diffs] = await Promise.all([
        invoke<string>("repo_commit_message", { path, sha }),
        invoke<FileDiff[]>("repo_commit_files", { path, sha }),
      ]);
      detailMsg = msg;
      detailDiffs = diffs;
    } catch (e) {
      error = String(e);
    } finally {
      detailLoading = false;
    }
  }
</script>

<div class="overlay" role="dialog" aria-label="blame">
  <div class="panel">
    <div class="header">
      <div class="header-left">
        {#if detailSha}
          <button class="back-btn" onclick={() => (detailSha = null)}
            >← 返回</button
          >
          <h2>{detailSha.slice(0, 8)}</h2>
        {:else}
          <h2>blame: {filePath}</h2>
        {/if}
      </div>
      <button class="close-btn" onclick={onClose} aria-label="关闭">✕</button>
    </div>
    <div class="content">
      {#if detailSha}
        {#if detailLoading}
          <p class="placeholder">加载提交详情…</p>
        {:else}
          {#if detailMsg}
            <pre class="detail-msg">{detailMsg}</pre>
          {/if}
          <DiffView files={detailDiffs} compact />
        {/if}
      {:else if loading}
        <p class="placeholder">加载中…</p>
      {:else if error}
        <p class="error">{error}</p>
      {:else if lines.length === 0}
        <p class="placeholder">无 blame 信息</p>
      {:else}
        {#each lines as line, i (line.line_no)}
          {@const showAnno = i === 0 || lines[i - 1].full_sha !== line.full_sha}
          <div class="blame-row">
            <span class="blame-anno">
              {#if showAnno}
                <button
                  class="anno-sha"
                  title={line.full_sha}
                  onclick={() => showDetail(line.full_sha)}>{line.sha}</button
                >
                <span class="anno-author">{line.author}</span>
                <span class="anno-date">{fmtDate(line.time)}</span>
              {/if}
            </span>
            <span class="blame-lineno">{line.line_no}</span>
            <span class="blame-content">{line.content}</span>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
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
    padding: var(--space-md) var(--space-lg);
    border-bottom: 1px solid var(--border-default);
    background: var(--bg-elevated);
  }
  .header-left {
    display: flex;
    align-items: center;
  }
  .header h2 {
    margin: 0;
    font-size: var(--fs-lg);
    font-weight: 600;
    color: var(--text-primary);
  }
  .back-btn {
    background: var(--bg-hover);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    cursor: pointer;
    font-size: var(--fs-sm);
    padding: 3px var(--space-md);
    margin-right: var(--space-md);
  }
  .back-btn:hover {
    background: var(--bg-hover);
  }
  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    font-size: var(--fs-xl);
    cursor: pointer;
    padding: var(--space-xs) var(--space-sm);
    line-height: 1;
  }
  .close-btn:hover {
    color: var(--text-primary);
  }
  .content {
    flex: 1;
    overflow: auto;
    padding: var(--space-sm) 0;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--fs-code);
  }
  .detail-msg {
    margin: 0 0 var(--space-md);
    padding: var(--space-md) 14px;
    background: var(--bg-elevated);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font-size: var(--fs-sm);
    white-space: pre-wrap;
  }
  .blame-row {
    display: grid;
    grid-template-columns: 220px 48px 1fr;
    line-height: 1.5;
  }
  .blame-row:hover {
    background: var(--bg-surface);
  }
  .blame-anno {
    display: flex;
    gap: var(--space-sm);
    padding: 0 var(--space-md);
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    border-right: 1px solid var(--border-default);
  }
  .anno-sha {
    color: #d48899;
    flex-shrink: 0;
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    cursor: pointer;
  }
  .anno-sha:hover {
    text-decoration: underline;
  }
  .anno-author {
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .anno-date {
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .blame-lineno {
    color: var(--text-muted);
    text-align: right;
    padding-right: 10px;
    user-select: none;
  }
  .blame-content {
    color: var(--text-primary);
    white-space: pre;
    overflow-x: auto;
  }
  .placeholder {
    color: var(--text-muted);
    padding: var(--space-md);
  }
  .error {
    color: var(--color-error);
    padding: var(--space-md);
  }
</style>

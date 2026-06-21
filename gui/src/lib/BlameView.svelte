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
          <DiffView files={detailDiffs} />
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
  .header-left {
    display: flex;
    align-items: center;
  }
  .header h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: #ddd;
  }
  .back-btn {
    background: #333;
    border: 1px solid #444;
    border-radius: 4px;
    color: #ddd;
    cursor: pointer;
    font-size: 12px;
    padding: 3px 10px;
    margin-right: 10px;
  }
  .back-btn:hover {
    background: #3a3a3a;
  }
  .close-btn {
    background: transparent;
    border: none;
    color: #aaa;
    font-size: 18px;
    cursor: pointer;
    padding: 4px 8px;
    line-height: 1;
  }
  .close-btn:hover {
    color: #fff;
  }
  .content {
    flex: 1;
    overflow: auto;
    padding: 8px 0;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
  }
  .detail-msg {
    margin: 0 0 12px;
    padding: 10px 14px;
    background: #222;
    border-radius: 4px;
    color: #ddd;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .blame-row {
    display: grid;
    grid-template-columns: 220px 48px 1fr;
    line-height: 1.5;
  }
  .blame-row:hover {
    background: #232323;
  }
  .blame-anno {
    display: flex;
    gap: 8px;
    padding: 0 10px;
    color: #777;
    white-space: nowrap;
    overflow: hidden;
    border-right: 1px solid #333;
  }
  .anno-sha {
    color: #cc8899;
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
    color: #888;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .anno-date {
    color: #666;
    flex-shrink: 0;
  }
  .blame-lineno {
    color: #555;
    text-align: right;
    padding-right: 10px;
    user-select: none;
  }
  .blame-content {
    color: #ddd;
    white-space: pre;
    overflow-x: auto;
  }
  .placeholder {
    color: #666;
    padding: 12px;
  }
  .error {
    color: #d88;
    padding: 12px;
  }
</style>

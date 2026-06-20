<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import DiffView from "$lib/DiffView.svelte";

  // ── 类型（与 gitcore serde 对应） ──
  interface LogEntry {
    sha: string;
    full_sha: string;
    message: string;
    author: string;
    date: string;
  }
  interface GraphEdge {
    from_lane: number;
    to_lane: number;
  }
  interface GraphCommit {
    entry: LogEntry;
    parents: string[];
    lane: number;
    edges: GraphEdge[];
  }
  type LineKind = "Context" | "Added" | "Removed";
  interface DiffLine {
    kind: LineKind;
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

  // ── Props ──
  let { path }: { path: string } = $props();

  // ── 状态 ──
  let commits = $state<GraphCommit[]>([]);
  let loading = $state(false);
  let error = $state("");
  let maxCount = $state(50);
  let selectedCommit = $state<LogEntry | null>(null);
  let commitMsg = $state("");
  let commitDiffs = $state<FileDiff[]>([]);
  let detailLoading = $state(false);
  let detailError = $state("");
  let copied = $state(false);
  let authorFilter = $state("");
  let grepFilter = $state("");
  let filterTimeout: number | undefined;

  // ── 数据加载 ──
  async function load() {
    loading = true;
    error = "";
    try {
      commits = await invoke<GraphCommit[]>("repo_log_topology", {
        path,
        maxCount,
        branch: null,
        author: authorFilter || null,
        grep: grepFilter || null,
      });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function loadMore() {
    maxCount += 50;
    await load();
  }

  async function selectCommit(entry: LogEntry) {
    selectedCommit = entry;
    detailLoading = true;
    detailError = "";
    commitMsg = "";
    commitDiffs = [];
    copied = false;
    try {
      const [msg, diffs] = await Promise.all([
        invoke<string>("repo_commit_message", { path, sha: entry.full_sha }),
        invoke<FileDiff[]>("repo_commit_files", { path, sha: entry.full_sha }),
      ]);
      commitMsg = msg;
      commitDiffs = diffs;
    } catch (e) {
      detailError = String(e);
    } finally {
      detailLoading = false;
    }
  }

  async function copySha() {
    if (!selectedCommit) return;
    try {
      await navigator.clipboard.writeText(selectedCommit.full_sha);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      // 某些环境 clipboard API 不可用,静默失败
    }
  }

  // 键盘激活
  function onActivate(e: KeyboardEvent, fn: () => void) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fn();
    }
  }

  // ── SVG 图常量 ──
  const ROW_H = 24;
  const LANE_W = 16;
  const NODE_R = 5;

  const LANE_COLORS = [
    "#e2c47a", // gold
    "#7ac4e2", // light blue
    "#7ae2a4", // green
    "#e27ac4", // pink
    "#c4e27a", // lime
    "#7a9fe2", // blue
    "#e29f7a", // orange
    "#b47ae2", // purple
    "#e27a7a", // red
    "#7ae2e2", // cyan
  ];

  function laneColor(lane: number): string {
    return LANE_COLORS[lane % LANE_COLORS.length];
  }

  function laneX(lane: number): number {
    return lane * LANE_W + LANE_W / 2;
  }

  // ── 派生:最大 lane 号 + SVG 尺寸 ──
  let maxLane = $derived(
    commits.reduce((m, c) => {
      m = Math.max(m, c.lane);
      for (const e of c.edges) {
        m = Math.max(m, e.from_lane, e.to_lane);
      }
      return m;
    }, 0),
  );
  let svgWidth = $derived((maxLane + 1) * LANE_W);
  let svgHeight = $derived(commits.length * ROW_H);
  // 筛选激活时,过滤后的子集 parent 链断裂会让 topology lane 爆炸(图无意义且会撑爆布局挤掉提交列),
  // 故筛选态下不画拓扑图,退化为线性提交列表。
  let filtering = $derived(authorFilter !== "" || grepFilter !== "");

  // ── Edge → SVG path d ──
  function edgePath(edge: GraphEdge, rowIndex: number): string {
    const x1 = laneX(edge.from_lane);
    const x2 = laneX(edge.to_lane);
    const y1 = rowIndex * ROW_H + ROW_H / 2;
    const y2 = (rowIndex + 1) * ROW_H + ROW_H / 2;

    if (edge.from_lane === edge.to_lane) {
      return `M ${x1} ${y1} L ${x1} ${y2}`;
    }
    // 平滑贝塞尔 S 曲线:水平差越大,控制点偏移越多
    const dx = Math.abs(x2 - x1);
    const dy = Math.min(ROW_H * 0.65, Math.max(dx * 0.5, ROW_H * 0.35));
    return `M ${x1} ${y1} C ${x1} ${y1 + dy}, ${x2} ${y2 - dy}, ${x2} ${y2}`;
  }

  // 筛选变化:debounce 300ms 后重载,并重置 maxCount
  function onFilterChange() {
    if (filterTimeout !== undefined) {
      clearTimeout(filterTimeout);
    }
    filterTimeout = setTimeout(() => {
      maxCount = 50;
      load();
    }, 300) as unknown as number;
  }

  // 初始化 / path 变化时重置并重载
  let prevPath = $state("");
  $effect(() => {
    if (path && path !== prevPath) {
      prevPath = path;
      maxCount = 50;
      authorFilter = "";
      grepFilter = "";
      selectedCommit = null;
      commitMsg = "";
      commitDiffs = [];
      load();
    }
  });
</script>

<div class="history">
  {#if error}
    <pre class="error">{error}</pre>
  {/if}

  <div class="split">
    <!-- ── 左侧:提交列表 + SVG 图 ── -->
    <aside class="commit-list">
      <div class="list-header">
        <span class="list-title">提交历史 ({commits.length})</span>
        <button class="btn-load-more" disabled={loading} onclick={loadMore}
          >加载更多</button
        >
      </div>

      <!-- 筛选栏 -->
      <div class="filter-row">
        <input
          type="text"
          class="filter-input"
          placeholder="作者"
          aria-label="按作者筛选"
          bind:value={authorFilter}
          oninput={onFilterChange}
        />
        <input
          type="text"
          class="filter-input"
          placeholder="提交消息关键词"
          aria-label="按提交消息筛选"
          bind:value={grepFilter}
          oninput={onFilterChange}
        />
      </div>

      {#if commits.length === 0 && !loading}
        <p class="muted">无提交记录</p>
      {:else}
        <div class="log-scroll">
          {#if !filtering}
            <!-- SVG 拓扑图 -->
            <svg
              class="graph-svg"
              width={svgWidth}
              height={svgHeight}
              viewBox="0 0 {svgWidth} {svgHeight}"
              aria-hidden="true"
            >
              <!-- edges:先画连线,再画 node 覆盖 -->
              {#each commits as commit, i}
                {#each commit.edges as edge}
                  {@const fromColor = laneColor(edge.from_lane)}
                  <path
                    d={edgePath(edge, i)}
                    stroke={fromColor}
                    stroke-width="1.5"
                    fill="none"
                    stroke-linecap="round"
                  />
                {/each}
              {/each}
              <!-- nodes -->
              {#each commits as commit, i}
                {@const cx = laneX(commit.lane)}
                {@const cy = i * ROW_H + ROW_H / 2}
                {@const color = laneColor(commit.lane)}
                <circle
                  {cx}
                  {cy}
                  r={NODE_R}
                  fill={color}
                  stroke={color}
                  stroke-width="1"
                />
              {/each}
            </svg>
          {/if}

          <!-- 提交信息列(与 SVG 行对齐) -->
          <div class="info-rows">
            {#each commits as commit, i}
              {@const entry = commit.entry}
              <div
                class="log-row"
                class:selected={selectedCommit?.full_sha === entry.full_sha}
                role="button"
                tabindex="0"
                style="height:{ROW_H}px"
                onclick={() => selectCommit(entry)}
                onkeydown={(e) => onActivate(e, () => selectCommit(entry))}
              >
                <span class="log-sha">{entry.sha}</span>
                <span class="log-author">{entry.author}</span>
                <span class="log-date">{entry.date}</span>
                <span class="log-msg">{entry.message}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      {#if loading}
        <p class="muted">加载中…</p>
      {/if}
    </aside>

    <!-- ── 右侧:提交详情 ── -->
    <section class="detail-view">
      {#if selectedCommit}
        <!-- 提交信息头部 -->
        <div class="commit-header">
          <div class="commit-title-row">
            <h3 class="commit-title">{selectedCommit.message}</h3>
            <button class="btn-copy" onclick={copySha}
              >{copied ? "已复制 ✓" : "复制 SHA"}</button
            >
          </div>
          <div class="commit-meta">
            <span class="meta-sha" title={selectedCommit.full_sha}
              >{selectedCommit.full_sha}</span
            >
            <span class="meta-author">{selectedCommit.author}</span>
            <span class="meta-date">{selectedCommit.date}</span>
          </div>
        </div>

        <!-- 完整提交消息 -->
        {#if commitMsg}
          <pre class="commit-message">{commitMsg}</pre>
        {/if}

        <!-- 文件 diff -->
        {#if detailLoading}
          <p class="muted">加载 diff…</p>
        {:else if detailError}
          <pre class="error">{detailError}</pre>
        {:else if commitDiffs.length === 0}
          <p class="muted">无文件改动</p>
        {:else}
          <div class="diff-list">
            <DiffView files={commitDiffs} />
          </div>
        {/if}
      {:else}
        <p class="muted placeholder">← 选择左侧提交查看详情</p>
      {/if}
    </section>
  </div>
</div>

<style>
  .history {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
  }
  .error {
    background: #3a1d1d;
    border-bottom: 1px solid #6a2b2b;
    padding: 8px 14px;
    color: #f3b4b4;
    white-space: pre-wrap;
    font-size: 12px;
    margin: 0;
  }
  .split {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  /* ── 左侧提交列表 ── */
  .commit-list {
    width: 420px;
    flex-shrink: 0;
    border-right: 1px solid #383838;
    overflow-y: auto;
    background: #212121;
    display: flex;
    flex-direction: column;
  }
  .list-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid #383838;
    flex-shrink: 0;
  }
  .list-title {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #888;
  }
  .btn-load-more {
    background: #333;
    border: 1px solid #555;
    border-radius: 4px;
    color: #ccc;
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
  }
  .btn-load-more:hover {
    background: #444;
  }
  .btn-load-more:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* ── 筛选栏 ── */
  .filter-row {
    display: flex;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid #383838;
    flex-shrink: 0;
  }
  .filter-input {
    flex: 1;
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #ddd;
    font-size: 12px;
    padding: 4px 8px;
  }
  .filter-input:focus {
    outline: none;
    border-color: #0e639c;
  }
  .filter-input::placeholder {
    color: #666;
  }

  /* ── SVG + 信息行(同滚容器) ── */
  .log-scroll {
    display: flex;
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .graph-svg {
    flex-shrink: 0;
    display: block;
  }

  .info-rows {
    flex: 1;
    min-width: 0;
  }

  .log-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1;
    white-space: nowrap;
  }
  .log-row:hover {
    background: #2a2a2a;
    cursor: pointer;
  }
  .log-row.selected {
    background: #0e639c55;
  }
  .log-sha {
    color: #e2c47a;
    flex-shrink: 0;
  }
  .log-author {
    color: #888;
    flex-shrink: 0;
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .log-date {
    color: #666;
    flex-shrink: 0;
  }
  .log-msg {
    color: #ddd;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ── 右侧详情 ── */
  .detail-view {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px;
  }
  .placeholder {
    margin-top: 40px;
    text-align: center;
  }
  .commit-header {
    margin-bottom: 16px;
  }
  .commit-title-row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 6px;
  }
  .commit-title {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: #e4e4e4;
    line-height: 1.35;
  }
  .btn-copy {
    flex-shrink: 0;
    background: #333;
    border: 1px solid #555;
    border-radius: 4px;
    color: #ccc;
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
  }
  .btn-copy:hover {
    background: #444;
  }
  .commit-meta {
    display: flex;
    gap: 16px;
    font-size: 12px;
    color: #888;
  }
  .meta-sha {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: #e2c47a;
  }
  .commit-message {
    margin: 0 0 20px;
    padding: 12px;
    background: #252525;
    border: 1px solid #383838;
    border-radius: 6px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1.55;
    color: #ccc;
    white-space: pre-wrap;
    word-break: break-word;
  }

  /* ── Diff 列表 ── */
  .diff-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .muted {
    color: #666;
    font-size: 12px;
    padding: 4px 14px;
  }
</style>

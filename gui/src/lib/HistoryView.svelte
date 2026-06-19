<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

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

  // ── 数据加载 ──
  async function load() {
    loading = true;
    error = "";
    try {
      commits = await invoke<GraphCommit[]>("repo_log_topology", {
        path,
        maxCount,
        branch: null,
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

  // ── diff 渲染辅助 ──
  function hunkLines(h: Hunk): {
    oldNo: number | null;
    newNo: number | null;
    line: DiffLine;
    idx: number;
  }[] {
    let oldNo = h.old_start;
    let newNo = h.new_start;
    return h.lines.map((line, idx) => {
      let curOld: number | null = null;
      let curNew: number | null = null;
      if (line.kind === "Context") {
        curOld = oldNo++;
        curNew = newNo++;
      } else if (line.kind === "Added") {
        curNew = newNo++;
      } else {
        curOld = oldNo++;
      }
      return { oldNo: curOld, newNo: curNew, line, idx };
    });
  }

  // 初始化 / path 变化时重置并重载
  let prevPath = $state("");
  $effect(() => {
    if (path && path !== prevPath) {
      prevPath = path;
      maxCount = 50;
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

      {#if commits.length === 0 && !loading}
        <p class="muted">无提交记录</p>
      {:else}
        <div class="log-scroll">
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
            {#each commitDiffs as filediff}
              <div class="diff-file">
                <h4 class="diff-fpath">{filediff.path}</h4>
                {#if filediff.binary}
                  <p class="muted">二进制文件</p>
                {:else if filediff.hunks.length === 0}
                  <p class="muted">空文件</p>
                {:else}
                  <div class="diff-content">
                    {#each filediff.hunks as hunk}
                      <div class="hunk">
                        <div class="hunk-header">
                          <span
                            >@@ -{hunk.old_start},{hunk.lines.filter(
                              (l) => l.kind !== "Added",
                            ).length} +{hunk.new_start},{hunk.lines.filter(
                              (l) => l.kind !== "Removed",
                            ).length} @@ {hunk.heading}</span
                          >
                        </div>
                        {#each hunkLines(hunk) as { oldNo, newNo, line }}
                          <div
                            class="diff-line"
                            class:line-added={line.kind === "Added"}
                            class:line-removed={line.kind === "Removed"}
                          >
                            <span class="ln ln-old">{oldNo ?? ""}</span>
                            <span class="ln ln-new">{newNo ?? ""}</span>
                            <span class="line-content"
                              >{line.kind === "Added"
                                ? "+"
                                : line.kind === "Removed"
                                  ? "-"
                                  : " "}{line.content}</span
                            >
                          </div>
                        {/each}
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/each}
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
  .diff-file {
    border: 1px solid #383838;
    border-radius: 6px;
    overflow: hidden;
  }
  .diff-fpath {
    margin: 0;
    padding: 6px 12px;
    background: #252525;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    font-weight: 600;
    color: #ddd;
    border-bottom: 1px solid #383838;
  }
  .diff-content {
    padding: 4px 0;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1.55;
  }
  .hunk {
    margin-bottom: 4px;
  }
  .hunk-header {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #999;
    padding: 4px 12px;
  }
  .diff-line {
    display: flex;
    white-space: pre;
    padding: 0 12px;
  }
  .line-added {
    background: #1d3520;
  }
  .line-removed {
    background: #351d1d;
  }
  .ln {
    width: 48px;
    text-align: right;
    padding-right: 8px;
    color: #666;
    flex-shrink: 0;
    user-select: none;
  }
  .line-content {
    flex: 1;
  }
  .line-added .line-content {
    color: #a8d8ab;
  }
  .line-removed .line-content {
    color: #d8a8a8;
  }
  .muted {
    color: #666;
    font-size: 12px;
    padding: 4px 14px;
  }
</style>

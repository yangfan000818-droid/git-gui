<script lang="ts">
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

  let {
    files,
    interactive = false,
    selectedLines = new Map() as Map<number, Set<number>>,
    onToggleLine,
    onStageHunk,
    onStageLines,
    onUnstageHunk,
    onUnstageLines,
    activeList,
    operating = false,
    onFileHistory,
    onBlame,
  }: {
    files: FileDiff[];
    interactive?: boolean;
    selectedLines?: Map<number, Set<number>>;
    onToggleLine?: (hunkIndex: number, lineIndex: number) => void;
    onStageHunk?: (hunk: Hunk) => void;
    onStageLines?: (hunk: Hunk, hunkIndex: number) => void;
    onUnstageHunk?: (hunk: Hunk) => void;
    onUnstageLines?: (hunk: Hunk, hunkIndex: number) => void;
    activeList?: "unstaged" | "staged";
    operating?: boolean;
    onFileHistory?: (filePath: string) => void;
    onBlame?: (filePath: string) => void;
  } = $props();

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

  function onActivate(e: KeyboardEvent, fn: () => void) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fn();
    }
  }

  function selectedCount(hi: number): number {
    return selectedLines.get(hi)?.size ?? 0;
  }

  function isLineSelected(hi: number, idx: number): boolean {
    return selectedLines.get(hi)?.has(idx) ?? false;
  }

  // ── 行内字符级 diff(LCS) ──
  type CharKind = "same" | "removed" | "added";
  interface CharSegment {
    text: string;
    kind: CharKind;
  }
  interface HunkLine {
    oldNo: number | null;
    newNo: number | null;
    line: DiffLine;
    idx: number;
  }

  function lcsTable(a: string, b: string): number[][] {
    const m = a.length;
    const n = b.length;
    const dp: number[][] = Array.from({ length: m + 1 }, () =>
      new Array(n + 1).fill(0),
    );
    for (let i = 1; i <= m; i++) {
      for (let j = 1; j <= n; j++) {
        dp[i][j] =
          a[i - 1] === b[j - 1]
            ? dp[i - 1][j - 1] + 1
            : Math.max(dp[i - 1][j], dp[i][j - 1]);
      }
    }
    return dp;
  }

  function backtrackSegments(
    a: string,
    b: string,
    dp: number[][],
  ): CharSegment[] {
    const segs: CharSegment[] = [];
    let i = a.length;
    let j = b.length;
    while (i > 0 || j > 0) {
      if (i > 0 && j > 0 && a[i - 1] === b[j - 1]) {
        segs.push({ text: a[i - 1], kind: "same" });
        i--;
        j--;
      } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
        segs.push({ text: b[j - 1], kind: "added" });
        j--;
      } else {
        segs.push({ text: a[i - 1], kind: "removed" });
        i--;
      }
    }
    segs.reverse();
    return segs;
  }

  function charDiff(
    oldStr: string,
    newStr: string,
  ): { old: CharSegment[]; new: CharSegment[] } | null {
    if (oldStr === newStr) return null;

    // 公共前缀
    let prefix = 0;
    while (
      prefix < oldStr.length &&
      prefix < newStr.length &&
      oldStr[prefix] === newStr[prefix]
    ) {
      prefix++;
    }
    // 公共后缀
    let oldEnd = oldStr.length;
    let newEnd = newStr.length;
    while (
      oldEnd > prefix &&
      newEnd > prefix &&
      oldStr[oldEnd - 1] === newStr[newEnd - 1]
    ) {
      oldEnd--;
      newEnd--;
    }

    const oldMid = oldStr.slice(prefix, oldEnd);
    const newMid = newStr.slice(prefix, newEnd);

    // 超大行退化为整行 diff
    if (
      oldMid.length * newMid.length > 10000 ||
      (oldMid.length === 0 && newMid.length === 0)
    ) {
      return null;
    }

    const dp = lcsTable(oldMid, newMid);
    const unified = backtrackSegments(oldMid, newMid, dp);

    const oldSegs: CharSegment[] = [];
    const newSegs: CharSegment[] = [];

    if (prefix > 0) {
      const pre = oldStr.slice(0, prefix);
      oldSegs.push({ text: pre, kind: "same" });
      newSegs.push({ text: pre, kind: "same" });
    }

    for (const seg of unified) {
      if (seg.kind === "same") {
        oldSegs.push(seg);
        newSegs.push(seg);
      } else if (seg.kind === "removed") {
        oldSegs.push(seg);
      } else {
        newSegs.push(seg);
      }
    }

    if (oldEnd < oldStr.length) {
      const suf = oldStr.slice(oldEnd);
      oldSegs.push({ text: suf, kind: "same" });
      newSegs.push({ text: suf, kind: "same" });
    }

    return { old: oldSegs, new: newSegs };
  }

  /** 配对 hunk 内连续删除/新增行,返回 idx→CharSegment[] 的映射。 */
  function buildCharSegments(lines: HunkLine[]): Map<number, CharSegment[]> {
    const map = new Map<number, CharSegment[]>();
    let i = 0;
    while (i < lines.length) {
      if (lines[i].line.kind === "Context") {
        i++;
        continue;
      }
      const removed: number[] = [];
      const added: number[] = [];
      while (i < lines.length && lines[i].line.kind !== "Context") {
        if (lines[i].line.kind === "Removed") removed.push(i);
        else added.push(i);
        i++;
      }
      const n = Math.min(removed.length, added.length);
      for (let j = 0; j < n; j++) {
        const diff = charDiff(
          lines[removed[j]].line.content,
          lines[added[j]].line.content,
        );
        if (diff) {
          map.set(removed[j], diff.old);
          map.set(added[j], diff.new);
        }
      }
    }
    return map;
  }
</script>

{#each files as file (file.path)}
  <div class="diff-file">
    <div class="diff-header">
      <h3 class="diff-path">{file.path}</h3>
      {#if onFileHistory}
        <button
          class="history-btn"
          onclick={() => onFileHistory?.(file.path)}
          aria-label="查看文件历史"
        >
          历史
        </button>
      {/if}
      {#if onBlame}
        <button
          class="history-btn"
          onclick={() => onBlame?.(file.path)}
          aria-label="查看 blame"
        >
          blame
        </button>
      {/if}
    </div>
    {#if file.binary}
      <p class="muted">二进制文件，无法显示 diff</p>
    {:else if file.hunks.length === 0}
      <p class="muted">空文件或无改动行</p>
    {:else}
      <div class="diff-content">
        {#each file.hunks as hunk, hi}
          {@const lines = hunkLines(hunk)}
          {@const segMap = buildCharSegments(lines)}
          <div class="hunk">
            <div class="hunk-header">
              <span
                >@@ -{hunk.old_start},{hunk.lines.filter(
                  (l) => l.kind !== "Added",
                ).length} +{hunk.new_start},{hunk.lines.filter(
                  (l) => l.kind !== "Removed",
                ).length} @@ {hunk.heading}</span
              >
              {#if interactive}
                <div class="hunk-actions">
                  {#if activeList === "unstaged"}
                    {@const selCount = selectedCount(hi)}
                    {#if selCount > 0}
                      <button
                        class="btn-act btn-stage"
                        disabled={operating}
                        title="暂存选中行"
                        onclick={() => onStageLines?.(hunk, hi)}
                      >
                        暂存 {selCount} 行
                      </button>
                    {/if}
                    <button
                      class="btn-act btn-stage"
                      disabled={operating}
                      title="暂存整个 hunk"
                      onclick={() => onStageHunk?.(hunk)}>暂存 Hunk</button
                    >
                  {:else}
                    {@const selCount = selectedCount(hi)}
                    {#if selCount > 0}
                      <button
                        class="btn-act btn-unstage"
                        disabled={operating}
                        title="取消暂存选中行"
                        onclick={() => onUnstageLines?.(hunk, hi)}
                      >
                        取消暂存 {selCount} 行
                      </button>
                    {/if}
                    <button
                      class="btn-act btn-unstage"
                      disabled={operating}
                      title="取消暂存整个 hunk"
                      onclick={() => onUnstageHunk?.(hunk)}
                      >取消暂存 Hunk</button
                    >
                  {/if}
                </div>
              {/if}
            </div>
            {#each lines as { oldNo, newNo, line, idx }}
              {@const segments = segMap.get(idx)}
              {@const prefix =
                line.kind === "Added"
                  ? "+"
                  : line.kind === "Removed"
                    ? "-"
                    : " "}
              {#if interactive && line.kind !== "Context"}
                {@const selected = isLineSelected(hi, idx)}
                <div
                  class="diff-line line-selectable"
                  class:line-added={line.kind === "Added"}
                  class:line-removed={line.kind === "Removed"}
                  class:line-selected={selected}
                  role="checkbox"
                  aria-checked={selected}
                  tabindex="0"
                  onclick={() => onToggleLine?.(hi, idx)}
                  onkeydown={(e) =>
                    onActivate(e, () => onToggleLine?.(hi, idx))}
                >
                  <span class="ln ln-old">{oldNo ?? ""}</span>
                  <span class="ln ln-new">{newNo ?? ""}</span>
                  <span class="line-content">
                    {prefix}
                    {#if segments}
                      {#each segments as seg}
                        <mark class="char-{seg.kind}">{seg.text}</mark>
                      {/each}
                    {:else}
                      {line.content}
                    {/if}
                  </span>
                </div>
              {:else}
                <div
                  class="diff-line"
                  class:line-added={line.kind === "Added"}
                  class:line-removed={line.kind === "Removed"}
                >
                  <span class="ln ln-old">{oldNo ?? ""}</span>
                  <span class="ln ln-new">{newNo ?? ""}</span>
                  <span class="line-content">
                    {prefix}
                    {#if segments}
                      {#each segments as seg}
                        <mark class="char-{seg.kind}">{seg.text}</mark>
                      {/each}
                    {:else}
                      {line.content}
                    {/if}
                  </span>
                </div>
              {/if}
            {/each}
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/each}

<style>
  .diff-file {
    border: 1px solid #383838;
    border-radius: 6px;
    overflow: hidden;
  }
  .diff-header {
    display: flex;
    align-items: center;
    gap: 12px;
    background: #252525;
    border-bottom: 1px solid #383838;
    padding: 6px 12px;
  }
  .diff-path {
    margin: 0;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    font-weight: 600;
    color: #ddd;
    flex: 1;
  }
  .history-btn {
    padding: 4px 10px;
    font-size: 11px;
    background: transparent;
    border: 1px solid #444;
    border-radius: 4px;
    color: #aaa;
    cursor: pointer;
    transition: all 0.15s;
  }
  .history-btn:hover {
    background: #2a2a2a;
    border-color: #666;
    color: #ddd;
  }
  .diff-content {
    padding: 4px 0;
    overflow-x: auto;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1.55;
  }

  .hunk {
    margin-bottom: 4px;
    width: max-content;
    min-width: 100%; /* 行等宽=max(最长行,容器):背景铺满+横向滚动,精确实测不裁 */
  }
  .hunk-header {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #999;
    padding: 4px 12px;
  }
  .hunk-header span {
    flex: 1;
  }

  .hunk-actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
    position: sticky;
    right: 12px;
    padding-left: 12px;
    background: #1e1e1e; /* 钉在横向可视区右侧,底色同 diff 背景,避免内容超宽时按钮被裁 */
  }
  .hunk-actions::before {
    content: "";
    position: absolute;
    left: -16px;
    top: 0;
    bottom: 0;
    width: 16px;
    background: linear-gradient(to right, transparent, #1e1e1e);
    pointer-events: none;
  }
  .btn-act {
    background: transparent;
    border: 1px solid #555;
    border-radius: 3px;
    color: #ccc;
    cursor: pointer;
    font-size: 11px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    width: auto;
    padding: 1px 8px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
  }
  .btn-act:hover {
    background: #444;
  }
  .btn-act:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .btn-stage {
    color: #7ee29a;
    border-color: #3a5a3a;
  }
  .btn-stage:hover {
    background: #2a4a2a;
  }
  .btn-unstage {
    color: #e29a7e;
    border-color: #5a3a3a;
  }
  .btn-unstage:hover {
    background: #4a2a2a;
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
  .line-selectable {
    cursor: pointer;
  }
  .line-selectable:hover {
    filter: brightness(1.3);
  }
  .line-selected {
    outline: 1px solid #5a8af0;
    outline-offset: -1px;
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

  /* ── 行内字符级 diff ── */
  .char-same {
    background: none;
    color: inherit;
  }
  .char-removed {
    background: #5a2525;
    border-radius: 2px;
  }
  .char-added {
    background: #255a25;
    border-radius: 2px;
  }

  .muted {
    color: #666;
    font-size: 12px;
    padding: 4px 14px;
  }
</style>

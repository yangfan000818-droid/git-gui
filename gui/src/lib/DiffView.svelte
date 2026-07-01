<script lang="ts">
  import { onMount } from "svelte";
  import { createHighlighter, type HighlighterGeneric } from "shiki";
  
  let highlighter = $state<HighlighterGeneric<any, any> | null>(null);
  
  onMount(async () => {
    highlighter = await createHighlighter({
      themes: ["vitesse-dark"],
      langs: [
        "javascript", "typescript", "svelte", "html", "css", "json", "rust", 
        "markdown", "bash", "toml", "yaml"
      ]
    });
  });

  function getLang(path: string) {
    const ext = path.split('.').pop()?.toLowerCase() || '';
    switch(ext) {
      case 'ts': case 'tsx': return 'typescript';
      case 'js': case 'jsx': return 'javascript';
      case 'svelte': return 'svelte';
      case 'html': return 'html';
      case 'css': case 'scss': case 'less': return 'css';
      case 'json': return 'json';
      case 'rs': return 'rust';
      case 'md': case 'mdx': return 'markdown';
      case 'sh': case 'bat': return 'bash';
      case 'toml': return 'toml';
      case 'yml': case 'yaml': return 'yaml';
      default: return 'javascript';
    }
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

  let {
    files,
    interactive = false,
    compact = false,
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
    compact?: boolean;
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

  // ── 树形目录分组 ──
  interface TreeNode {
    name: string;
    path: string;
    children: TreeNode[];
    file: FileDiff | null;
  }

  let collapsedDirs = $state(new Set<string>());
  let expandedFiles = $state(new Set<string>());

  function toggleDir(path: string) {
    if (collapsedDirs.has(path)) collapsedDirs.delete(path);
    else collapsedDirs.add(path);
    collapsedDirs = new Set(collapsedDirs);
  }

  // ── 大 diff 熔断:超大文件默认折叠、超长 hunk 默认截断,点击可展开 ──
  let expandedHunks = $state(new Set<string>());

  function fileLineCount(file: FileDiff): number {
    return file.hunks.reduce((n, h) => n + h.lines.length, 0);
  }
  function toggleFile(path: string) {
    if (expandedFiles.has(path)) expandedFiles.delete(path);
    else expandedFiles.add(path);
    expandedFiles = new Set(expandedFiles);
  }
  function hunkKey(path: string, hi: number): string {
    return `${path}:${hi}`;
  }
  function toggleHunk(key: string) {
    if (expandedHunks.has(key)) expandedHunks.delete(key);
    else expandedHunks.add(key);
    expandedHunks = new Set(expandedHunks);
  }

  let rootNodes = $derived.by(() => {
    const root: TreeNode = { name: "", path: "", children: [], file: null };
    for (const f of files) {
      const parts = f.path.split("/");
      let node = root;
      for (let i = 0; i < parts.length; i++) {
        const name = parts[i];
        const isFile = i === parts.length - 1;
        const fullPath = parts.slice(0, i + 1).join("/");
        let child = node.children.find((c) => c.name === name);
        if (!child) {
          child = {
            name,
            path: fullPath,
            children: [],
            file: isFile ? f : null,
          };
          node.children.push(child);
        }
        node = child;
      }
    }
    // Sort: directories first, then files, alphabetical within each
    const sort = (nodes: TreeNode[]) => {
      nodes.sort((a, b) => {
        const aIsDir = a.file === null;
        const bIsDir = b.file === null;
        if (aIsDir !== bIsDir) return aIsDir ? -1 : 1;
        return a.name.localeCompare(b.name);
      });
      for (const n of nodes) sort(n.children);
    };
    sort(root.children);
    return root.children;
  });

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

  // 单 hunk 行数超此值跳过字符级 diff(charDiff 逐行配对开销大,大改动块逐字符高亮意义也不大)。
  const CHAR_SEG_LIMIT = 600;
  // 单 hunk 默认最多渲染行数(超出折叠)、超大文件默认整体折叠的行数预算。
  const HUNK_CAP = 800;
  const FILE_LINE_BUDGET = 3000;

  /** 配对 hunk 内连续删除/新增行,返回 idx→CharSegment[] 的映射。 */
  function buildCharSegments(lines: HunkLine[]): Map<number, CharSegment[]> {
    const map = new Map<number, CharSegment[]>();
    // 超大 hunk 跳过字符级 diff,避免 O(n) 配对计算与海量 <mark>。
    if (lines.length > CHAR_SEG_LIMIT) return map;
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

  // 计算一个 hunk 里的 shiki tokens，只在 highlighter 准备好后执行
  function getHunkTokens(hunk: Hunk, path: string) {
    if (!highlighter) return [];
    const text = hunk.lines.map(l => l.content).join('\n');
    try {
      return highlighter.codeToTokensBase(text, { 
        lang: getLang(path), 
        theme: 'vitesse-dark' 
      });
    } catch(e) {
      return [];
    }
  }
</script>

{#snippet renderFileDiff(file: FileDiff)}
  {#if file.binary}
    <p class="muted">二进制文件，无法显示 diff</p>
  {:else if file.hunks.length === 0}
    <p class="muted">空文件或无改动行</p>
  {:else if fileLineCount(file) > FILE_LINE_BUDGET && !expandedFiles.has(file.path)}
    <button class="diff-collapsed" onclick={() => toggleFile(file.path)}>
      该文件改动较大（{fileLineCount(file)} 行），点击展开 diff
    </button>
  {:else}
    <div class="diff-content">
      {#each file.hunks as hunk, hi}
        {@const lines = hunkLines(hunk)}
        {@const segMap = buildCharSegments(lines)}
        {@const hkey = hunkKey(file.path, hi)}
        {@const hunkCapped =
          lines.length > HUNK_CAP && !expandedHunks.has(hkey)}
        {@const shownLines = hunkCapped ? lines.slice(0, HUNK_CAP) : lines}
        {@const hunkTokens = getHunkTokens(hunk, file.path)}
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
                      >暂存 {selCount} 行</button
                    >
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
                      >取消暂存 {selCount} 行</button
                    >
                  {/if}
                  <button
                    class="btn-act btn-unstage"
                    disabled={operating}
                    title="取消暂存整个 hunk"
                    onclick={() => onUnstageHunk?.(hunk)}>取消暂存 Hunk</button
                  >
                {/if}
              </div>
            {/if}
          </div>
          {#each shownLines as { oldNo, newNo, line, idx }}
            {@const segments = segMap.get(idx)}
            {@const shikiTokens = hunkTokens[idx] || []}
            {@const prefix =
              line.kind === "Added" ? "+" : line.kind === "Removed" ? "-" : " "}
            {@const selected = interactive && line.kind !== "Context" ? isLineSelected(hi, idx) : false}
            
            <div
              class="diff-line {interactive && line.kind !== 'Context' ? 'line-selectable' : ''}"
              class:line-added={line.kind === "Added"}
              class:line-removed={line.kind === "Removed"}
              class:line-selected={selected}
              role={interactive && line.kind !== "Context" ? "checkbox" : null}
              aria-checked={selected}
              tabindex={interactive && line.kind !== "Context" ? 0 : null}
              onclick={interactive && line.kind !== "Context" ? () => onToggleLine?.(hi, idx) : null}
              onkeydown={interactive && line.kind !== "Context" ? (e) => onActivate(e, () => onToggleLine?.(hi, idx)) : null}
            >
              <div class="gutter">
                <span class="ln ln-old">{oldNo ?? ""}</span>
                <span class="ln ln-new">{newNo ?? ""}</span>
                <span class="ln-prefix">{prefix}</span>
              </div>
              <span class="line-content">
                {#if segments}
                  {#each segments as seg}<mark class="char-{seg.kind}">{seg.text}</mark>{/each}
                {:else if shikiTokens.length > 0}
                  {#each shikiTokens as token}
                    <span style="color: {token.color || 'inherit'}">{token.content}</span>
                  {/each}
                {:else}
                  {line.content || " "}
                {/if}
              </span>
            </div>
          {/each}
          {#if hunkCapped}
            <button class="diff-more" onclick={() => toggleHunk(hkey)}>
              … 还有 {lines.length - HUNK_CAP} 行未显示，点击展开整个 hunk
            </button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
{/snippet}

{#snippet renderTree(nodes: TreeNode[], depth: number)}
  {#each nodes as node}
    {#if node.file === null}
      <!-- directory node -->
      <button
        class="tree-row"
        style="padding-left:{12 + depth * 16}px"
        onclick={() => toggleDir(node.path)}
      >
        <span class="tree-caret"
          >{collapsedDirs.has(node.path) ? "▸" : "▾"}</span
        >
        <span class="tree-name">{node.name}/</span>
      </button>
      {#if !collapsedDirs.has(node.path)}
        {@render renderTree(node.children, depth + 1)}
      {/if}
    {:else}
      <!-- file node -->
      <div
        class="diff-file"
        style="margin-left:{8 + depth * 16}px; margin-right:0; width:auto;"
      >
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div
          class="diff-header"
          role="button"
          tabindex="0"
          onclick={() => toggleFile(node.path)}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              toggleFile(node.path);
            }
          }}
        >
          <span class="file-caret"
            >{expandedFiles.has(node.path) ? "▾" : "▸"}</span
          >
          <span class="diff-path">{node.name}</span>
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span
            class="file-actions"
            onclick={(e) => e.stopPropagation()}
            onkeydown={(e) => e.stopPropagation()}
          >
            {#if onFileHistory}
              <button
                class="history-btn"
                onclick={() => onFileHistory?.(node.file!.path)}
                title="查看文件历史">历史</button
              >
            {/if}
            {#if onBlame}
              <button
                class="history-btn"
                onclick={() => onBlame?.(node.file!.path)}
                title="查看 blame">blame</button
              >
            {/if}
          </span>
        </div>
        {#if expandedFiles.has(node.path)}
          {@render renderFileDiff(node.file!)}
        {/if}
      </div>
    {/if}
  {/each}
{/snippet}

{#if compact}
  {#each files as file (file.path)}
    <div class="diff-file">
      <div class="diff-header" style="cursor:default">
        <span class="diff-path"
          >{file.path.includes("/")
            ? file.path.slice(file.path.lastIndexOf("/") + 1)
            : file.path}</span
        >
        <span class="file-actions">
          {#if onFileHistory}
            <button
              class="history-btn"
              onclick={() => onFileHistory?.(file.path)}
              title="查看文件历史">历史</button
            >
          {/if}
          {#if onBlame}
            <button
              class="history-btn"
              onclick={() => onBlame?.(file.path)}
              title="查看 blame">blame</button
            >
          {/if}
        </span>
      </div>
      {@render renderFileDiff(file)}
    </div>
  {/each}
{:else}
  {@render renderTree(rootNodes, 0)}
{/if}

<style>
  /* ═══ Tree rows (directory nodes) ═══ */
  .tree-row {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    font-family:
      ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    padding: 3px 8px;
    transition: background 0.1s;
  }
  .tree-row:hover {
    background: var(--bg-hover);
  }
  .tree-caret {
    width: 14px;
    flex-shrink: 0;
    text-align: center;
    font-size: 10px;
    color: var(--text-muted);
  }
  .tree-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--accent-cyan);
  }

  /* ═══ File diff card ═══ */
  .diff-file {
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    overflow: hidden;
    margin-top: 3px;
    margin-bottom: 3px;
  }
  .diff-header {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    box-sizing: border-box; /* 否则 content-box 下 width:100% 把左右 padding 加宽,header 溢出右侧致右上角按钮被裁 */
    background: var(--bg-elevated);
    border: none;
    border-bottom: 1px solid var(--border-default);
    padding: 6px 12px;
    cursor: pointer;
    font-size: inherit;
    font-family: inherit;
    color: inherit;
    transition: background 0.15s;
  }
  .diff-header:hover {
    background: var(--bg-hover);
  }
  .file-caret {
    width: 12px;
    flex-shrink: 0;
    text-align: center;
    font-size: 10px;
    color: var(--text-muted);
  }
  .diff-path {
    margin: 0;
    font-family:
      ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    flex: 1;
    min-width: 0; /* 允许收缩,否则长文件名把右侧按钮顶出 header 被 overflow:hidden 裁切 */
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
  .file-actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .history-btn {
    padding: 4px 10px;
    font-size: 11px;
    background: transparent;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s;
  }
  .history-btn:hover {
    background: var(--bg-hover);
    border-color: var(--accent-cyan);
    color: var(--accent-cyan);
  }
  .diff-content {
    padding: 4px 0;
    overflow-x: auto;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1.55;
  }

  .diff-collapsed,
  .diff-more {
    display: block;
    width: 100%;
    text-align: left;
    padding: 8px 12px;
    background: var(--bg-surface);
    border: 1px dashed var(--border-subtle, rgba(255, 255, 255, 0.12));
    border-radius: 4px;
    color: var(--text-muted);
    font-size: 12px;
    cursor: pointer;
  }
  .diff-collapsed:hover,
  .diff-more:hover {
    color: var(--text-primary);
    background: var(--bg-elevated, rgba(255, 255, 255, 0.04));
  }
  .diff-more {
    margin-top: 2px;
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
    color: var(--text-muted);
    padding: 6px 12px;
    background: rgba(0, 0, 0, 0.2);
    border-top: 1px solid var(--border-default);
    border-bottom: 1px solid var(--border-default);
    font-size: 11px;
    margin-top: -1px;
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
    background: var(--bg-void);
  }
  .hunk-actions::before {
    content: "";
    position: absolute;
    left: -16px;
    top: 0;
    bottom: 0;
    width: 16px;
    background: linear-gradient(to right, transparent, var(--bg-void));
    pointer-events: none;
  }
  .btn-act {
    background: transparent;
    border: 1px solid var(--border-default);
    border-radius: 3px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 11px;
    font-family:
      ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    width: auto;
    padding: 1px 8px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
  }
  .btn-act:hover {
    background: var(--bg-hover);
  }
  .btn-act:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .btn-stage {
    color: var(--accent-neon);
    border-color: rgba(86, 211, 100, 0.25);
  }
  .btn-stage:hover {
    background: rgba(86, 211, 100, 0.1);
  }
  .btn-unstage {
    color: var(--color-error);
    border-color: rgba(247, 120, 139, 0.25);
  }
  .btn-unstage:hover {
    background: rgba(247, 120, 139, 0.1);
  }

  .diff-line {
    display: flex;
    white-space: pre;
    padding: 0;
    min-height: 20px;
    align-items: stretch;
    border-left: 2px solid transparent;
  }
  .line-added {
    background: rgba(46, 160, 67, 0.15);
    border-left-color: #2ea043;
  }
  .line-removed {
    background: rgba(248, 81, 73, 0.15);
    border-left-color: #f85149;
  }
  .line-selectable {
    cursor: pointer;
  }
  .line-selectable:hover {
    background: rgba(255, 255, 255, 0.08);
  }
  .line-added.line-selectable:hover {
    background: rgba(46, 160, 67, 0.25);
  }
  .line-removed.line-selectable:hover {
    background: rgba(248, 81, 73, 0.25);
  }
  .line-selected {
    box-shadow: inset 0 0 0 1px var(--accent-cyan);
  }

  .gutter {
    display: flex;
    background: rgba(0, 0, 0, 0.15);
    border-right: 1px solid rgba(255, 255, 255, 0.05);
    padding: 0 4px;
    user-select: none;
    min-width: 90px;
    color: var(--text-muted);
    font-size: 11px;
    align-items: center;
  }
  .ln {
    width: 36px;
    text-align: right;
    padding-right: 8px;
    opacity: 0.6;
  }
  .ln-prefix {
    width: 14px;
    text-align: center;
    font-weight: bold;
    opacity: 0.8;
  }
  .line-added .ln-prefix {
    color: #3fb950;
  }
  .line-removed .ln-prefix {
    color: #ff7b72;
  }
  
  .line-content {
    flex: 1;
    padding-left: 12px;
    padding-right: 12px;
    padding-top: 1px;
    padding-bottom: 1px;
  }

  /* ── 行内字符级 diff ── */
  .char-same {
    background: none;
    color: inherit;
  }
  .char-added {
    background: rgba(46, 160, 67, 0.4);
    color: #fff;
    border-radius: 2px;
  }
  .char-removed {
    background: rgba(248, 81, 73, 0.4);
    color: #fff;
    text-decoration: line-through;
    border-radius: 2px;
  }

  .muted {
    color: var(--text-muted);
    font-size: 12px;
    padding: 4px 14px;
  }
</style>

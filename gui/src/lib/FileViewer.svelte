<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

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
    repoPath,
    filePath,
    revision = null,
    onClose,
  }: {
    repoPath: string;
    filePath: string;
    // null = 工作区当前内容(带相对 HEAD 的行内变更标记);否则只读该 revision 全文。
    revision?: string | null;
    onClose: () => void;
  } = $props();

  // 单行的变更标记:added=纯新增,modified=改动(old 为对应的旧行,供展开对照)。
  type Mark = { kind: "added" | "modified"; old: string[] };

  let lines = $state<string[]>([]);
  let loading = $state(true);
  let error = $state("");
  let binary = $state(false);
  let truncated = $state(false);
  // 1-based 行号 → 变更标记。
  let lineMarks = $state<Map<number, Mark>>(new Map());
  // anchor(其后发生删除的工作区行号,0=首行之前)→ 被删除的旧行。
  let deletedAt = $state<Map<number, string[]>>(new Map());
  // 当前展开对照的行(modified 看旧行 / 删除点看被删行)。
  let expanded = $state<string | null>(null);

  const MAX_LINES = 5000;

  async function load() {
    loading = true;
    error = "";
    binary = false;
    truncated = false;
    lineMarks = new Map();
    deletedAt = new Map();
    expanded = null;
    try {
      let content: string;
      if (revision) {
        content = await invoke<string>("repo_cat_file", {
          path: repoPath,
          revision,
          filePath,
        });
      } else {
        content = await invoke<string>("read_repo_file", {
          path: repoPath,
          filePath,
        });
      }
      if (content.includes("\u0000")) {
        binary = true;
        lines = [];
        return;
      }
      let arr = content.split("\n");
      // 末尾换行会产生一个空串行,去掉以免多出一空行。
      if (arr.length && arr[arr.length - 1] === "") arr.pop();
      if (arr.length > MAX_LINES) {
        arr = arr.slice(0, MAX_LINES);
        truncated = true;
      }
      lines = arr;

      // 仅工作区视图计算行内变更标记(相对 HEAD)。
      if (!revision) {
        const fd = await invoke<FileDiff | null>("repo_file_diff_head", {
          path: repoPath,
          file: filePath,
        });
        if (fd && !fd.binary) computeMarks(fd);
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // 把 diff hunk 映射成逐行标记:同一变更块里有删有增=modified,只增=added,只删=删除点。
  function computeMarks(fd: FileDiff) {
    const marks = new Map<number, Mark>();
    const dels = new Map<number, string[]>();
    for (const h of fd.hunks) {
      let newLine = h.new_start; // 该 hunk 第一行对应的新文件行号(1-based)
      let i = 0;
      const L = h.lines;
      while (i < L.length) {
        if (L[i].kind === "Context") {
          newLine++;
          i++;
          continue;
        }
        // 收集一段连续的非上下文行(删/增混合)
        const removed: string[] = [];
        const added: { line: number; content: string }[] = [];
        while (i < L.length && L[i].kind !== "Context") {
          if (L[i].kind === "Removed") {
            removed.push(L[i].content);
          } else {
            added.push({ line: newLine, content: L[i].content });
            newLine++;
          }
          i++;
        }
        if (added.length > 0) {
          const kind = removed.length > 0 ? "modified" : "added";
          for (const a of added) marks.set(a.line, { kind, old: removed });
        } else if (removed.length > 0) {
          const anchor = newLine - 1; // 删除发生在该工作区行之后(0=首行前)
          dels.set(anchor, [...(dels.get(anchor) ?? []), ...removed]);
        }
      }
    }
    lineMarks = marks;
    deletedAt = dels;
  }

  function toggle(key: string) {
    expanded = expanded === key ? null : key;
  }

  function onActivate(e: KeyboardEvent, fn: () => void) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fn();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }

  let changeCount = $derived(lineMarks.size);

  onMount(load);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="fv-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={onClose}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fv-panel" onclick={(e) => e.stopPropagation()}>
    <div class="fv-header">
      <div class="fv-title">
        <span class="fv-path">{filePath}</span>
        {#if revision}
          <span class="fv-rev">@ {revision.slice(0, 12)}</span>
        {:else if changeCount > 0}
          <span class="fv-changes">{changeCount} 处改动</span>
        {/if}
      </div>
      <button class="fv-close" onclick={onClose} aria-label="关闭">×</button>
    </div>

    {#if error}
      <div class="fv-error">{error}</div>
    {/if}

    {#if loading}
      <p class="fv-muted">加载文件…</p>
    {:else if binary}
      <p class="fv-muted">二进制文件,无法显示文本内容。</p>
    {:else if lines.length === 0 && !error}
      <p class="fv-muted">空文件。</p>
    {:else}
      {#if truncated}
        <div class="fv-notice">
          文件较大,仅显示前 {MAX_LINES} 行。完整内容请在命令行查看。
        </div>
      {/if}
      <div class="fv-code">
        {#if deletedAt.has(0)}
          {@const key = "d0"}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="fv-del-strip"
            role="button"
            tabindex="0"
            onclick={() => toggle(key)}
            onkeydown={(e) => onActivate(e, () => toggle(key))}
            title="此处删除了 {deletedAt.get(0)!.length} 行,点击查看"
          >
            ▾ {deletedAt.get(0)!.length} 行被删除
          </div>
          {#if expanded === key}
            <div class="fv-old">
              {#each deletedAt.get(0)! as ol}
                <div class="fv-old-line">- {ol}</div>
              {/each}
            </div>
          {/if}
        {/if}
        {#each lines as line, i}
          {@const n = i + 1}
          {@const mark = lineMarks.get(n)}
          {@const rowKey = "m" + n}
          {@const clickable = mark?.kind === "modified"}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <div
            class="fv-row"
            class:added={mark?.kind === "added"}
            class:modified={mark?.kind === "modified"}
            class:clickable
            role={clickable ? "button" : undefined}
            tabindex={clickable ? 0 : undefined}
            onclick={clickable ? () => toggle(rowKey) : undefined}
            onkeydown={clickable
              ? (e) => onActivate(e, () => toggle(rowKey))
              : undefined}
            title={clickable ? "点击查看改动前内容" : undefined}
          >
            <span class="fv-gutter"></span>
            <span class="fv-num">{n}</span>
            <span class="fv-line">{line || " "}</span>
          </div>
          {#if clickable && expanded === rowKey && mark.old.length > 0}
            <div class="fv-old">
              {#each mark.old as ol}
                <div class="fv-old-line">- {ol}</div>
              {/each}
            </div>
          {/if}
          {#if deletedAt.has(n)}
            {@const key = "d" + n}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="fv-del-strip"
              role="button"
              tabindex="0"
              onclick={() => toggle(key)}
              onkeydown={(e) => onActivate(e, () => toggle(key))}
              title="此处删除了 {deletedAt.get(n)!.length} 行,点击查看"
            >
              ▾ {deletedAt.get(n)!.length} 行被删除
            </div>
            {#if expanded === key}
              <div class="fv-old">
                {#each deletedAt.get(n)! as ol}
                  <div class="fv-old-line">- {ol}</div>
                {/each}
              </div>
            {/if}
          {/if}
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .fv-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .fv-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 860px;
    max-width: 94%;
    height: 84%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .fv-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
  }
  .fv-title {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
  }
  .fv-path {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fv-rev {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    color: var(--accent-gold);
    flex-shrink: 0;
  }
  .fv-changes {
    font-size: 11px;
    color: var(--accent-cyan);
    flex-shrink: 0;
  }
  .fv-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .fv-close:hover {
    color: var(--text-primary);
  }
  .fv-error {
    background: #3a1d1d;
    border-bottom: 1px solid rgba(247, 120, 139, 0.25);
    padding: 8px 16px;
    color: var(--color-error);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .fv-muted {
    color: var(--text-muted);
    font-size: 12px;
    padding: 18px 16px;
    margin: 0;
  }
  .fv-notice {
    background: #3a2f1d;
    border-bottom: 1px solid #6a542b;
    color: var(--accent-gold);
    font-size: 12px;
    padding: 6px 16px;
    flex-shrink: 0;
  }
  .fv-code {
    flex: 1;
    overflow: auto;
    padding: 6px 0;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1.55;
  }
  .fv-row {
    display: flex;
    align-items: stretch;
    white-space: pre;
  }
  .fv-row.clickable {
    cursor: pointer;
  }
  .fv-gutter {
    width: 3px;
    flex-shrink: 0;
    background: transparent;
  }
  .fv-row.added .fv-gutter {
    background: var(--accent-neon);
  }
  .fv-row.modified .fv-gutter {
    background: var(--accent-cyan);
  }
  .fv-row.added {
    background: rgba(86, 211, 100, 0.06);
  }
  .fv-row.modified {
    background: rgba(88, 166, 255, 0.06);
  }
  .fv-row.clickable:hover {
    background: rgba(88, 166, 255, 0.14);
  }
  .fv-num {
    flex-shrink: 0;
    width: 48px;
    padding: 0 10px 0 8px;
    text-align: right;
    color: var(--text-muted);
    user-select: none;
  }
  .fv-line {
    flex: 1;
    color: var(--text-primary);
    padding-right: 16px;
  }
  /* ── 删除点 / 旧内容对照 ── */
  .fv-del-strip {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-left: 51px;
    padding: 1px 8px;
    color: var(--text-muted);
    font-size: 11px;
    cursor: pointer;
    border-top: 1px dashed rgba(247, 120, 139, 0.4);
  }
  .fv-del-strip:hover {
    color: var(--color-error);
  }
  .fv-old {
    background: rgba(247, 120, 139, 0.08);
    border-left: 3px solid rgba(247, 120, 139, 0.5);
    margin-left: 51px;
    padding: 2px 0;
  }
  .fv-old-line {
    color: #f79aa6;
    padding: 0 12px;
    white-space: pre;
  }
</style>

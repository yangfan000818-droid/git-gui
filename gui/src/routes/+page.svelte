<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  // ── 类型（与 gitcore serde 对应） ──
  type FileState = "Staged" | "Modified" | "Untracked" | "StagedAndModified";
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
  interface FileStatus {
    path: string;
    state: FileState;
  }
  interface RepoStatus {
    branch: string | null;
    upstream: string | null;
    behind: number;
    ahead: number;
    dirty: boolean;
    conflicted: string[];
    files: FileStatus[];
    submodules: { name: string; path: string; status: string }[];
  }

  // ── 状态 ──
  let path = $state("/Users/yfan/work/git-gui");
  let status = $state<RepoStatus | null>(null);
  let unstaged = $state<FileDiff[]>([]);
  let staged = $state<FileDiff[]>([]);
  let selectedFile = $state<FileDiff | null>(null);
  let activeList = $state<"unstaged" | "staged">("unstaged");
  let loading = $state(false);
  let error = $state("");

  // ── 数据加载 ──
  async function load() {
    loading = true;
    error = "";
    status = null;
    unstaged = [];
    staged = [];
    selectedFile = null;
    try {
      const [s, u, d] = await Promise.all([
        invoke<RepoStatus>("repo_status", { path }),
        invoke<FileDiff[]>("repo_unstaged_diff", { path }),
        invoke<FileDiff[]>("repo_staged_diff", { path }),
      ]);
      status = s;
      unstaged = u;
      staged = d;
      // 自动选中第一个文件
      if (unstaged.length > 0) {
        selectedFile = unstaged[0];
        activeList = "unstaged";
      } else if (staged.length > 0) {
        selectedFile = staged[0];
        activeList = "staged";
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function selectFile(file: FileDiff, list: "unstaged" | "staged") {
    selectedFile = file;
    activeList = list;
    selectedLines = new Map();
  }

  // 键盘激活(Enter / 空格)：让 role=button 的可点击 div 可用键盘操作。
  function onActivate(e: KeyboardEvent, fn: () => void) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fn();
    }
  }

  // ── 行选择 ──
  let selectedLines = $state<Map<number, Set<number>>>(new Map());

  function toggleLine(hunkIdx: number, lineIdx: number) {
    let set = selectedLines.get(hunkIdx);
    if (!set) {
      set = new Set();
      selectedLines.set(hunkIdx, set);
    }
    set = new Set(set); // 触发响应
    if (set.has(lineIdx)) {
      set.delete(lineIdx);
    } else {
      set.add(lineIdx);
    }
    if (set.size === 0) {
      selectedLines.delete(hunkIdx);
    } else {
      selectedLines.set(hunkIdx, set);
    }
    selectedLines = new Map(selectedLines);
  }

  function selectedCount(hunkIdx: number): number {
    return selectedLines.get(hunkIdx)?.size ?? 0;
  }

  function isLineSelected(hunkIdx: number, lineIdx: number): boolean {
    return selectedLines.get(hunkIdx)?.has(lineIdx) ?? false;
  }

  // ── 文件操作 ──
  let operating = $state(false);

  async function stageFile(file: FileDiff) {
    operating = true;
    error = "";
    try {
      await invoke("repo_stage", { path, files: [file.path] });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  async function unstageFile(file: FileDiff) {
    operating = true;
    error = "";
    try {
      await invoke("repo_unstage", { path, files: [file.path] });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  async function discardFile(file: FileDiff) {
    if (!confirm(`确定丢弃 ${file.path} 的改动?（stash 保存可找回）`)) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_discard", { path, files: [file.path] });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  // ── hunk / 行级操作 ──
  async function stageHunk(file: FileDiff, hunk: Hunk) {
    operating = true;
    error = "";
    try {
      await invoke("repo_stage_hunk", { path, file, hunk });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  async function unstageHunk(file: FileDiff, hunk: Hunk) {
    operating = true;
    error = "";
    try {
      await invoke("repo_unstage_hunk", { path, file, hunk });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  async function stageSelectedLines(
    file: FileDiff,
    hunk: Hunk,
    hunkIdx: number,
  ) {
    const sel = selectedLines.get(hunkIdx);
    if (!sel || sel.size === 0) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_stage_lines", {
        path,
        file,
        hunk,
        selected: Array.from(sel),
      });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  async function unstageSelectedLines(
    file: FileDiff,
    hunk: Hunk,
    hunkIdx: number,
  ) {
    const sel = selectedLines.get(hunkIdx);
    if (!sel || sel.size === 0) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_unstage_lines", {
        path,
        file,
        hunk,
        selected: Array.from(sel),
      });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  // ── commit ──
  let commitMessage = $state("");
  let commitResult = $state("");

  async function doCommit() {
    if (!commitMessage.trim()) return;
    operating = true;
    error = "";
    commitResult = "";
    try {
      const sha = await invoke<string>("repo_commit", {
        path,
        message: commitMessage,
      });
      commitResult = `提交成功: ${sha}`;
      commitMessage = "";
      await refresh();
    } catch (e) {
      commitResult = "";
      error = String(e);
    } finally {
      operating = false;
    }
  }

  async function refresh() {
    selectedLines = new Map();
    const [s, u, d] = await Promise.all([
      invoke<RepoStatus>("repo_status", { path }),
      invoke<FileDiff[]>("repo_unstaged_diff", { path }),
      invoke<FileDiff[]>("repo_staged_diff", { path }),
    ]);
    status = s;
    unstaged = u;
    staged = d;
    // 保持选中文件，并追踪它当前属于哪个列表
    if (selectedFile) {
      const inUnstaged = unstaged.find((f) => f.path === selectedFile!.path);
      const inStaged = staged.find((f) => f.path === selectedFile!.path);
      if (inUnstaged) {
        selectedFile = inUnstaged;
        activeList = "unstaged";
      } else if (inStaged) {
        selectedFile = inStaged;
        activeList = "staged";
      } else {
        selectedFile = null;
      }
    }
    if (!selectedFile) {
      if (unstaged.length > 0) {
        selectedFile = unstaged[0];
        activeList = "unstaged";
      } else if (staged.length > 0) {
        selectedFile = staged[0];
        activeList = "staged";
      }
    }
  }

  // ── 行号计算 ──
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
</script>

<main>
  <!-- ── 顶栏 ── -->
  <header class="topbar">
    <span class="logo">git-gui</span>
    <div class="path-bar">
      <input bind:value={path} placeholder="仓库路径" spellcheck="false" />
      <button onclick={load} disabled={loading}
        >{loading ? "读取中…" : "打开"}</button
      >
    </div>
    {#if status}
      <span class="branch">{status.branch ?? "(detached)"}</span>
      {#if status.ahead > 0}<span class="badge ahead">↑{status.ahead}</span
        >{/if}
      {#if status.behind > 0}<span class="badge behind">↓{status.behind}</span
        >{/if}
    {/if}
  </header>

  {#if error}
    <pre class="error">{error}</pre>
  {/if}

  {#if status}
    <div class="split">
      <!-- ── 左侧:文件列表 ── -->
      <aside class="file-list">
        <!-- 未暂存 -->
        <section>
          <h2 class={activeList === "unstaged" ? "active" : ""}>
            未暂存 ({unstaged.length})
          </h2>
          {#if unstaged.length === 0}
            <p class="muted">无改动</p>
          {:else}
            <div class="file-rows">
              {#each unstaged as f}
                <div
                  class="file-item"
                  class:selected={selectedFile === f}
                  role="button"
                  tabindex="0"
                  onclick={() => selectFile(f, "unstaged")}
                  onkeydown={(e) =>
                    onActivate(e, () => selectFile(f, "unstaged"))}
                >
                  <span class="fpath">{f.path}</span>
                  {#if f.binary}<span class="tag tag-binary">二进制</span>{/if}
                  <span class="file-actions">
                    <button
                      class="btn-act btn-stage"
                      disabled={operating}
                      title="暂存"
                      onclick={(e) => {
                        e.stopPropagation();
                        stageFile(f);
                      }}>+</button
                    >
                    <button
                      class="btn-act btn-discard"
                      disabled={operating}
                      title="丢弃改动（stash 保存可找回）"
                      onclick={(e) => {
                        e.stopPropagation();
                        discardFile(f);
                      }}>↺</button
                    >
                  </span>
                </div>
              {/each}
            </div>
          {/if}
        </section>

        <!-- 已暂存 -->
        <section>
          <h2 class={activeList === "staged" ? "active" : ""}>
            已暂存 ({staged.length})
          </h2>
          {#if staged.length === 0}
            <p class="muted">无暂存</p>
          {:else}
            <div class="file-rows">
              {#each staged as f}
                <div
                  class="file-item"
                  class:selected={selectedFile === f}
                  role="button"
                  tabindex="0"
                  onclick={() => selectFile(f, "staged")}
                  onkeydown={(e) =>
                    onActivate(e, () => selectFile(f, "staged"))}
                >
                  <span class="fpath">{f.path}</span>
                  {#if f.binary}<span class="tag tag-binary">二进制</span>{/if}
                  <span class="file-actions">
                    <button
                      class="btn-act btn-unstage"
                      disabled={operating}
                      title="取消暂存"
                      onclick={(e) => {
                        e.stopPropagation();
                        unstageFile(f);
                      }}>−</button
                    >
                  </span>
                </div>
              {/each}
            </div>
          {/if}
        </section>

        <!-- 冲突 -->
        {#if status.conflicted.length}
          <section>
            <h2>冲突 ({status.conflicted.length})</h2>
            <ul>
              {#each status.conflicted as c}
                <li class="file-item conflict">{c}</li>
              {/each}
            </ul>
          </section>
        {/if}

        <!-- commit -->
        <section class="commit-section">
          <h2>提交</h2>
          {#if staged.length === 0}
            <p class="muted">暂存文件以创建提交</p>
          {:else}
            <textarea
              bind:value={commitMessage}
              placeholder="提交信息（必填）"
              rows={3}
              disabled={operating}></textarea>
            <div class="commit-bar">
              <button
                class="btn-commit"
                disabled={operating || !commitMessage.trim()}
                onclick={doCommit}>提交（{staged.length} 个文件）</button
              >
            </div>
          {/if}
          {#if commitResult}
            <p class="commit-ok">{commitResult}</p>
          {/if}
        </section>
      </aside>

      <!-- ── 右侧:diff 视图 ── -->
      <section class="diff-view">
        {#if selectedFile}
          <h3 class="diff-path">{selectedFile.path}</h3>
          {#if selectedFile.binary}
            <p class="muted">二进制文件,无法显示 diff</p>
          {:else if selectedFile.hunks.length === 0}
            <p class="muted">空文件或无改动行</p>
          {:else}
            <div class="diff-content">
              {#snippet lineCells(
                oldNo: number | null,
                newNo: number | null,
                line: DiffLine,
              )}
                <span class="ln ln-old">{oldNo ?? ""}</span>
                <span class="ln ln-new">{newNo ?? ""}</span>
                <span class="line-content"
                  >{line.kind === "Added"
                    ? "+"
                    : line.kind === "Removed"
                      ? "-"
                      : " "}{line.content}</span
                >
              {/snippet}
              {#each selectedFile.hunks as hunk, hi}
                {@const selCount = selectedCount(hi)}
                <div class="hunk">
                  <div class="hunk-header">
                    <span
                      >@@ -{hunk.old_start},{hunk.lines.filter(
                        (l) => l.kind !== "Added",
                      ).length} +{hunk.new_start},{hunk.lines.filter(
                        (l) => l.kind !== "Removed",
                      ).length} @@ {hunk.heading}</span
                    >
                    {#if activeList === "unstaged"}
                      <div class="hunk-actions">
                        {#if selCount > 0}
                          <button
                            class="btn-act btn-stage"
                            disabled={operating}
                            title="暂存选中行"
                            onclick={() =>
                              stageSelectedLines(selectedFile!, hunk, hi)}
                          >
                            暂存 {selCount} 行
                          </button>
                        {/if}
                        <button
                          class="btn-act btn-stage"
                          disabled={operating}
                          title="暂存整个 hunk"
                          onclick={() => stageHunk(selectedFile!, hunk)}
                          >暂存 Hunk</button
                        >
                      </div>
                    {:else}
                      <div class="hunk-actions">
                        {#if selCount > 0}
                          <button
                            class="btn-act btn-unstage"
                            disabled={operating}
                            title="取消暂存选中行"
                            onclick={() =>
                              unstageSelectedLines(selectedFile!, hunk, hi)}
                          >
                            取消暂存 {selCount} 行
                          </button>
                        {/if}
                        <button
                          class="btn-act btn-unstage"
                          disabled={operating}
                          title="取消暂存整个 hunk"
                          onclick={() => unstageHunk(selectedFile!, hunk)}
                          >取消暂存 Hunk</button
                        >
                      </div>
                    {/if}
                  </div>
                  {#each hunkLines(hunk) as { oldNo, newNo, line, idx }}
                    {#if line.kind !== "Context"}
                      {@const selected = isLineSelected(hi, idx)}
                      <div
                        class="diff-line line-selectable"
                        class:line-added={line.kind === "Added"}
                        class:line-removed={line.kind === "Removed"}
                        class:line-selected={selected}
                        role="checkbox"
                        aria-checked={selected}
                        tabindex="0"
                        onclick={() => toggleLine(hi, idx)}
                        onkeydown={(e) =>
                          onActivate(e, () => toggleLine(hi, idx))}
                      >
                        {@render lineCells(oldNo, newNo, line)}
                      </div>
                    {:else}
                      <div class="diff-line">
                        {@render lineCells(oldNo, newNo, line)}
                      </div>
                    {/if}
                  {/each}
                </div>
              {/each}
            </div>
          {/if}
        {:else}
          <p class="muted placeholder">← 选择左侧文件查看 diff</p>
        {/if}
      </section>
    </div>
  {/if}

  {#if !status && !error && !loading}
    <p class="hint">打开一个 Git 仓库以查看 Changes</p>
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    background: #1e1e1e;
    color: #e4e4e4;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    font-size: 13px;
  }
  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  /* ── 顶栏 ── */
  .topbar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 14px;
    background: #252525;
    border-bottom: 1px solid #383838;
    flex-shrink: 0;
  }
  .logo {
    font-weight: 700;
    font-size: 15px;
    color: #ccc;
  }
  .path-bar {
    display: flex;
    gap: 6px;
    flex: 1;
  }
  .path-bar input {
    flex: 1;
    max-width: 360px;
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 6px;
    color: #e4e4e4;
    padding: 6px 10px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
  }
  .path-bar button {
    background: #0e639c;
    border: none;
    border-radius: 6px;
    color: #fff;
    padding: 6px 14px;
    font-size: 12px;
    cursor: pointer;
  }
  .path-bar button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .branch {
    font-weight: 600;
    font-size: 13px;
  }
  .badge {
    font-size: 11px;
    border-radius: 10px;
    padding: 1px 8px;
  }
  .ahead {
    background: #1d3a24;
    color: #7ee29a;
  }
  .behind {
    background: #1d2b3a;
    color: #7ab8e2;
  }

  /* ── 错误 ── */
  .error {
    background: #3a1d1d;
    border-bottom: 1px solid #6a2b2b;
    padding: 8px 14px;
    color: #f3b4b4;
    white-space: pre-wrap;
    font-size: 12px;
    margin: 0;
  }

  /* ── 分栏 ── */
  .split {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  /* ── 左侧文件列表 ── */
  .file-list {
    width: 260px;
    flex-shrink: 0;
    border-right: 1px solid #383838;
    overflow-y: auto;
    padding: 10px 0;
    background: #212121;
  }
  .file-list section {
    margin-bottom: 6px;
  }
  .file-list h2 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #888;
    margin: 0;
    padding: 6px 14px 4px;
  }
  .file-list h2.active {
    color: #ccc;
  }
  .file-list ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .file-item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 14px;
    cursor: pointer;
  }
  .file-item:hover {
    background: #2a2a2a;
  }
  .file-item.selected {
    background: #0e639c55;
  }
  .file-item.conflict {
    color: #f3b4b4;
  }
  .fpath {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted {
    color: #666;
    font-size: 12px;
    padding: 4px 14px;
  }
  .tag {
    font-size: 10px;
    border-radius: 4px;
    padding: 1px 5px;
    flex-shrink: 0;
  }
  .tag-binary {
    background: #2f2f2f;
    color: #999;
  }

  /* ── 文件操作按钮 ── */
  .file-actions {
    display: none;
    gap: 2px;
    margin-left: auto;
    flex-shrink: 0;
  }
  .file-item:hover .file-actions {
    display: flex;
  }
  .btn-act {
    background: transparent;
    border: 1px solid #555;
    border-radius: 3px;
    color: #ccc;
    cursor: pointer;
    font-size: 13px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    width: 22px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
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
    background: #1d3a24;
  }
  .btn-unstage {
    color: #e2c47a;
    border-color: #5a4a3a;
  }
  .btn-unstage:hover {
    background: #3a311d;
  }
  .btn-discard {
    color: #f3b4b4;
    border-color: #5a3a3a;
  }
  .btn-discard:hover {
    background: #3a1d1d;
  }

  /* ── commit ── */
  .commit-section {
    border-top: 1px solid #383838;
    margin-top: 6px;
    padding: 10px 14px 14px;
  }
  .commit-section textarea {
    width: 100%;
    box-sizing: border-box;
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 6px;
    color: #e4e4e4;
    padding: 8px 10px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    resize: vertical;
    margin-top: 4px;
  }
  .commit-section textarea:disabled {
    opacity: 0.5;
  }
  .commit-bar {
    margin-top: 6px;
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .btn-commit {
    background: #1d5a1d;
    border: 1px solid #3a7a3a;
    border-radius: 6px;
    color: #fff;
    padding: 6px 14px;
    font-size: 12px;
    cursor: pointer;
  }
  .btn-commit:hover {
    background: #256a25;
  }
  .btn-commit:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .commit-ok {
    color: #7ee29a;
    font-size: 12px;
    margin: 6px 0 0;
  }

  /* ── 右侧 diff ── */
  .diff-view {
    flex: 1;
    overflow-y: auto;
    padding: 12px 16px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1.55;
  }
  .diff-path {
    font-size: 14px;
    font-weight: 600;
    margin: 0 0 12px;
    color: #ddd;
  }
  .placeholder {
    margin-top: 40px;
    text-align: center;
  }

  .hunk {
    margin-bottom: 8px;
  }
  .hunk-header {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #999;
    margin: 8px 0 2px;
  }
  .hunk-header span {
    flex: 1;
  }
  .hunk-actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .hunk-actions .btn-act {
    font-size: 11px;
    width: auto;
    padding: 1px 8px;
    height: 20px;
  }
  .diff-line {
    display: flex;
    white-space: pre;
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

  .hint {
    color: #888;
    font-size: 13px;
    text-align: center;
    margin-top: 60px;
  }
</style>

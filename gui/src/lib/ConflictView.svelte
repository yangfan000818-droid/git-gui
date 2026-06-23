<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  // ── 类型（与 gitcore serde externally-tagged enum 对应） ──
  type Choice = "Ours" | "Theirs" | "Base";
  type SegmentVariant = "Clean" | "AutoResolved" | "Conflict";
  interface ConflictHunkData {
    ours: string;
    base: string;
    theirs: string;
  }
  type Segment =
    | { Clean: string }
    | { AutoResolved: string }
    | { Conflict: ConflictHunkData };

  function segVariant(s: Segment): SegmentVariant {
    return Object.keys(s)[0] as SegmentVariant;
  }
  function segData<T>(s: Segment): T {
    return (s as unknown as Record<string, T>)[Object.keys(s)[0]];
  }

  interface StashRef {
    label: string;
  }

  let {
    path,
    conflictFiles,
    autostash,
    onContinue,
    onAbort,
  }: {
    path: string;
    conflictFiles: string[];
    autostash: StashRef | null;
    onContinue: () => Promise<void>;
    onAbort: () => Promise<void>;
  } = $props();

  // ── 每个文件的独立状态 ──
  interface FileState {
    path: string;
    segments: Segment[] | null;
    conflictIndices: number[]; // segments 中 Conflict 段的下标
    choices: Choice[]; // 每个 conflict 段一个选择
    written: boolean;
    error: string;
  }

  let fileStates = $state<FileState[]>([]);

  let activeFile = $state(0);
  let activeHunk = $state(0); // 当前文件中 conflictIndices 的下标
  let loading = $state(true);
  let globalError = $state("");

  // ── 魔法棒（与 gitcore ConflictHunk::magic() 同逻辑） ──
  function magicChoice(h: ConflictHunkData): Choice {
    if (h.ours === h.base) return "Theirs";
    if (h.theirs === h.base) return "Ours";
    return "Ours"; // 两边都改了，默认取 ours
  }

  // ── 加载所有冲突文件的片段 ──
  onMount(async () => {
    loading = true;
    const states: FileState[] = [];
    for (const f of conflictFiles) {
      try {
        const segments = await invoke<Segment[]>("read_conflict_segments", {
          path,
          filePath: f,
        });
        const conflictIndices: number[] = [];
        segments.forEach((s, i) => {
          if (segVariant(s) === "Conflict") conflictIndices.push(i);
        });
        const choices = conflictIndices.map((i) =>
          magicChoice(segData<ConflictHunkData>(segments[i])),
        );
        states.push({
          path: f,
          segments,
          conflictIndices,
          choices,
          written: false,
          error: "",
        });
      } catch (e) {
        states.push({
          path: f,
          segments: null,
          conflictIndices: [],
          choices: [],
          written: false,
          error: String(e),
        });
      }
    }
    fileStates = states;
    // 聚焦第一个有真冲突的文件
    const firstConflict = states.findIndex((s) => s.conflictIndices.length > 0);
    if (firstConflict >= 0) activeFile = firstConflict;
    loading = false;
  });

  // ── 派生 ──
  let currentFile = $derived(fileStates[activeFile]);
  let currentHunkIdx = $derived(currentFile?.conflictIndices[activeHunk] ?? -1);
  let currentChoice = $derived(
    currentFile?.choices[activeHunk] ?? ("Ours" as Choice),
  );

  let allWritten = $derived(fileStates.every((fs) => fs.written));
  let totalConflictHunks = $derived(
    fileStates.reduce((sum, fs) => sum + fs.conflictIndices.length, 0),
  );

  // ── 自动定夺上下文（当前冲突块前后紧邻的 AutoResolved 段） ──
  function autoContextBefore(segIdx: number): Segment[] {
    if (!currentFile?.segments) return [];
    const ctx: Segment[] = [];
    for (let i = segIdx - 1; i >= 0; i--) {
      const s = currentFile.segments[i];
      if (segVariant(s) === "AutoResolved") ctx.unshift(s);
      else break;
    }
    return ctx;
  }

  function autoContextAfter(segIdx: number): Segment[] {
    if (!currentFile?.segments) return [];
    const ctx: Segment[] = [];
    for (let i = segIdx + 1; i < currentFile.segments.length; i++) {
      const s = currentFile.segments[i];
      if (segVariant(s) === "AutoResolved") ctx.push(s);
      else break;
    }
    return ctx;
  }

  function autoLinesCount(fs: FileState | null): number {
    if (!fs?.segments) return 0;
    return fs.segments
      .filter((s) => segVariant(s) === "AutoResolved")
      .reduce(
        (sum, s) => sum + (segData<string>(s).match(/\n/g) || []).length,
        0,
      );
  }

  // ── 操作 ──
  function setChoice(c: Choice) {
    const fs = fileStates[activeFile];
    if (fs && activeHunk < fs.choices.length) {
      fs.choices[activeHunk] = c;
    }
  }

  function cycleChoice(dir: number) {
    const order: Choice[] = ["Ours", "Base", "Theirs"];
    const cur = order.indexOf(currentChoice);
    setChoice(order[Math.max(0, Math.min(2, cur + dir))]);
  }

  function goPrevHunk() {
    if (activeHunk > 0) activeHunk--;
  }

  function goNextHunk() {
    if (currentFile && activeHunk < currentFile.conflictIndices.length - 1) {
      activeHunk++;
    }
  }

  function goPrevFile() {
    if (activeFile > 0) {
      activeFile--;
      activeHunk = 0;
    }
  }

  function goNextFile() {
    if (activeFile < fileStates.length - 1) {
      activeFile++;
      activeHunk = 0;
    }
  }

  async function writeCurrentFile() {
    const fs = fileStates[activeFile];
    if (!fs || fs.written) return;
    globalError = "";
    try {
      await invoke("resolve_conflict", {
        path,
        filePath: fs.path,
        choices: fs.choices,
      });
      fs.written = true;
      fs.error = "";
    } catch (e) {
      fs.error = String(e);
    }
  }

  function onActivate(e: KeyboardEvent, fn: () => void) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fn();
    }
  }
</script>

<div class="conflict-view">
  {#if globalError}
    <pre class="cv-error">{globalError}</pre>
  {/if}

  {#if loading}
    <p class="cv-status">加载冲突文件…</p>
  {:else}
    <!-- ── 文件概览条 ── -->
    <div class="file-bar" role="list" aria-label="冲突文件列表">
      {#each fileStates as fs, i}
        {@const name = fs.path.split("/").pop() ?? fs.path}
        {@const label = fs.written
          ? `✓ ${name}`
          : fs.conflictIndices.length === 0
            ? `○ ${name}`
            : `◆${fs.conflictIndices.length} ${name}`}
        <div
          class="file-chip"
          class:file-active={i === activeFile}
          class:file-written={fs.written}
          class:file-auto={fs.conflictIndices.length === 0 && !fs.written}
          class:file-pending={fs.conflictIndices.length > 0 && !fs.written}
          role="listitem"
        >
          <button
            class="file-chip-btn"
            onclick={() => {
              activeFile = i;
              activeHunk = 0;
            }}
          >
            {label}
          </button>
        </div>
      {/each}
    </div>

    <!-- ── 当前文件 ── -->
    {#if currentFile?.error}
      <pre class="cv-error">{currentFile.error}</pre>
    {:else if currentFile?.segments}
      <div class="file-header">
        <button
          class="btn-nav"
          disabled={activeFile === 0}
          onclick={goPrevFile}
          aria-label="上一个文件"
        >
          ← 文件
        </button>
        <span class="file-path">{currentFile.path}</span>
        <span class="file-stats">
          魔法棒自动解 {autoLinesCount(currentFile)} 行
        </span>
        <button
          class="btn-nav"
          disabled={activeFile >= fileStates.length - 1}
          onclick={goNextFile}
          aria-label="下一个文件"
        >
          文件 →
        </button>
      </div>

      {#if currentFile.conflictIndices.length === 0}
        <!-- 全自动解决 -->
        <div class="all-auto">
          <p>✓ 魔法棒已自动解决全部冲突，无需手动选择。</p>
          <button
            class="btn-primary"
            disabled={currentFile.written}
            onclick={writeCurrentFile}
            title="把所选版本写回该文件并标记为已解决（git add）"
          >
            {currentFile.written ? "已写入" : "写入: " + currentFile.path}
          </button>
        </div>
      {:else}
        <!-- ── 当前块信息 + 导航 ── -->
        <div class="hunk-nav">
          <button
            class="btn-nav"
            disabled={activeHunk === 0}
            onclick={goPrevHunk}
            aria-label="上一个冲突块"
          >
            ← 块
          </button>
          <span class="hunk-label">
            冲突块 {activeHunk + 1}/{currentFile.conflictIndices.length}
          </span>
          <button
            class="btn-nav"
            disabled={activeHunk >= currentFile.conflictIndices.length - 1}
            onclick={goNextHunk}
            aria-label="下一个冲突块"
          >
            块 →
          </button>
        </div>

        <!-- ── 三栏视图 ── -->
        {@const hunkSeg = currentFile.segments[currentHunkIdx]}
        {#if segVariant(hunkSeg) === "Conflict"}
          {@const h = segData<ConflictHunkData>(hunkSeg)}
          {@const before = autoContextBefore(currentHunkIdx)}
          {@const after = autoContextAfter(currentHunkIdx)}
          <div class="three-col" role="region" aria-label="三栏冲突对比">
            <!-- ours 栏 -->
            <div
              class="col col-ours"
              class:col-selected={currentChoice === "Ours"}
              role="region"
              aria-label="ours · 本地"
            >
              <div
                class="col-title"
                class:title-selected={currentChoice === "Ours"}
              >
                {currentChoice === "Ours" ? "●" : " "} ours · 本地
              </div>
              <div class="col-body">
                {#each before as s}
                  {#each segData<string>(s).split("\n") as line}
                    {#if line !== ""}
                      <div class="auto-line">✓ {line}</div>
                    {/if}
                  {/each}
                {/each}
                {#each h.ours.split("\n") as line}
                  {#if line !== ""}
                    <div class="conflict-line">{line}</div>
                  {/if}
                {/each}
                {#each after as s}
                  {#each segData<string>(s).split("\n") as line}
                    {#if line !== ""}
                      <div class="auto-line">✓ {line}</div>
                    {/if}
                  {/each}
                {/each}
              </div>
            </div>

            <!-- base 栏 -->
            <div
              class="col col-base"
              class:col-selected={currentChoice === "Base"}
              role="region"
              aria-label="base · 祖先"
            >
              <div
                class="col-title"
                class:title-selected={currentChoice === "Base"}
              >
                {currentChoice === "Base" ? "●" : " "} base · 祖先
              </div>
              <div class="col-body">
                {#each before as s}
                  {#each segData<string>(s).split("\n") as line}
                    {#if line !== ""}
                      <div class="auto-line">✓ {line}</div>
                    {/if}
                  {/each}
                {/each}
                {#each h.base.split("\n") as line}
                  {#if line !== ""}
                    <div class="conflict-line">{line}</div>
                  {/if}
                {/each}
                {#each after as s}
                  {#each segData<string>(s).split("\n") as line}
                    {#if line !== ""}
                      <div class="auto-line">✓ {line}</div>
                    {/if}
                  {/each}
                {/each}
              </div>
            </div>

            <!-- theirs 栏 -->
            <div
              class="col col-theirs"
              class:col-selected={currentChoice === "Theirs"}
              role="region"
              aria-label="theirs · 远端"
            >
              <div
                class="col-title"
                class:title-selected={currentChoice === "Theirs"}
              >
                {currentChoice === "Theirs" ? "●" : " "} theirs · 远端
              </div>
              <div class="col-body">
                {#each before as s}
                  {#each segData<string>(s).split("\n") as line}
                    {#if line !== ""}
                      <div class="auto-line">✓ {line}</div>
                    {/if}
                  {/each}
                {/each}
                {#each h.theirs.split("\n") as line}
                  {#if line !== ""}
                    <div class="conflict-line">{line}</div>
                  {/if}
                {/each}
                {#each after as s}
                  {#each segData<string>(s).split("\n") as line}
                    {#if line !== ""}
                      <div class="auto-line">✓ {line}</div>
                    {/if}
                  {/each}
                {/each}
              </div>
            </div>
          </div>

          <!-- ── 选择按钮 ── -->
          <div class="choice-bar" role="group" aria-label="版本选择">
            <button
              class="btn-choice"
              class:btn-choice-active={currentChoice === "Ours"}
              onclick={() => setChoice("Ours")}
              title="本块采用本地版本（ours,当前分支的改动）"
            >
              Ours
            </button>
            <button
              class="btn-choice"
              class:btn-choice-active={currentChoice === "Base"}
              onclick={() => setChoice("Base")}
              title="本块采用共同祖先版本（base,两边改动前的原始内容）"
            >
              Base
            </button>
            <button
              class="btn-choice"
              class:btn-choice-active={currentChoice === "Theirs"}
              onclick={() => setChoice("Theirs")}
              title="本块采用对方版本（theirs,合入分支的改动）"
            >
              Theirs
            </button>
            <span class="choice-arrows">
              <button
                class="btn-arrow"
                onclick={() => cycleChoice(-1)}
                aria-label="上一个选项"
              >
                ←
              </button>
              <button
                class="btn-arrow"
                onclick={() => cycleChoice(1)}
                aria-label="下一个选项"
              >
                →
              </button>
            </span>
          </div>
        {/if}

        <!-- ── 写入当前文件 ── -->
        <div class="write-bar">
          <button
            class="btn-primary"
            disabled={currentFile.written}
            onclick={writeCurrentFile}
            title="把所选版本写回该文件并标记为已解决（git add）"
          >
            {currentFile.written ? "已写入 ✓" : "写入: " + currentFile.path}
          </button>
        </div>
      {/if}
    {/if}

    <!-- ── 底部操作 ── -->
    <div class="bottom-actions">
      <button
        class="btn-primary"
        disabled={!allWritten}
        onclick={onContinue}
        title="所有冲突文件已写回后,完成本次整合（merge/rebase --continue）"
      >
        继续整合
      </button>
      <button
        class="btn-danger"
        onclick={onAbort}
        title="放弃整合,回到整合前的状态（merge/rebase --abort）"
      >
        放弃整合 (abort)
      </button>
    </div>
    {#if !allWritten && !loading}
      <p class="hint">
        {totalConflictHunks > 0
          ? `还有 ${fileStates.filter((fs) => !fs.written).length} 个文件未写入 — 请逐文件选择版本后写入`
          : "请写入所有文件后继续"}
      </p>
    {/if}
  {/if}
</div>

<style>
  .conflict-view {
    padding: 12px 16px;
    font-size: 13px;
    max-width: 960px;
  }
  .cv-error {
    background: #3a1d1d;
    border: 1px solid rgba(247, 120, 139, 0.25);
    border-radius: 6px;
    padding: 8px 12px;
    color: var(--color-error);
    white-space: pre-wrap;
    font-size: 12px;
    margin: 0 0 12px;
  }
  .cv-status {
    color: var(--text-muted);
  }
  .hint {
    color: var(--text-muted);
    font-size: 12px;
    margin: 6px 0 0;
  }

  /* ── 文件概览条 ── */
  .file-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 10px;
  }
  .file-chip {
    border-radius: 4px;
    overflow: hidden;
  }
  .file-chip-btn {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    padding: 3px 10px;
    white-space: nowrap;
  }
  .file-chip-btn:hover {
    background: var(--border-default);
  }
  .file-active .file-chip-btn {
    background: rgba(88, 166, 255, 0.12);
    border-color: var(--accent-cyan);
    color: #fff;
  }
  .file-written .file-chip-btn {
    color: var(--accent-neon);
  }
  .file-auto .file-chip-btn {
    color: var(--accent-neon);
  }
  .file-pending .file-chip-btn {
    color: var(--accent-gold);
  }

  /* ── 文件头 ── */
  .file-header {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 8px;
  }
  .file-path {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    color: var(--color-error);
    flex: 1;
  }
  .file-stats {
    color: var(--text-muted);
    font-size: 12px;
  }
  .btn-nav {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 3px 10px;
  }
  .btn-nav:hover {
    background: var(--border-default);
  }
  .btn-nav:disabled {
    opacity: 0.3;
    cursor: default;
  }

  /* ── 全自动解决 ── */
  .all-auto {
    background: #1d2a1d;
    border: 1px solid rgba(86, 211, 100, 0.2);
    border-radius: 6px;
    padding: 16px;
    text-align: center;
    margin-bottom: 12px;
  }
  .all-auto p {
    margin: 0 0 10px;
    color: var(--accent-neon);
  }

  /* ── 三栏视图 ── */
  .three-col {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 1px;
    background: var(--border-default);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    overflow: hidden;
    margin-bottom: 10px;
    min-height: 120px;
  }
  .col {
    background: var(--bg-elevated);
    display: flex;
    flex-direction: column;
  }
  .col-selected {
    outline: 2px solid var(--accent-cyan);
    outline-offset: -1px;
    z-index: 1;
  }
  .col-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    padding: 6px 10px 4px;
    border-bottom: 1px solid var(--bg-hover);
    flex-shrink: 0;
  }
  .title-selected {
    color: var(--accent-cyan);
    border-bottom-color: var(--accent-cyan);
  }
  .col-body {
    flex: 1;
    padding: 4px 0;
    overflow-x: auto;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1.55;
  }
  .auto-line {
    color: #5a8a5a;
    padding: 0 10px;
    white-space: pre;
  }
  .conflict-line {
    color: var(--accent-gold);
    padding: 0 10px;
    white-space: pre;
  }
  .col-selected .conflict-line {
    color: var(--accent-gold);
    font-weight: 600;
  }

  /* ── 冲突块导航 ── */
  .hunk-nav {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .hunk-label {
    color: var(--text-secondary);
    font-size: 12px;
    flex: 1;
    text-align: center;
  }

  /* ── 选择按钮 ── */
  .choice-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 10px;
  }
  .btn-choice {
    background: var(--bg-surface);
    border: 1px solid #3a3a4a;
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 13px;
    padding: 5px 16px;
    font-weight: 500;
  }
  .btn-choice:hover {
    background: var(--border-default);
  }
  .btn-choice-active {
    background: var(--accent-cyan);
    border-color: var(--accent-cyan);
    color: #fff;
  }
  .btn-choice-active:hover {
    background: #58a6ff;
  }
  .choice-arrows {
    margin-left: 8px;
    display: flex;
    gap: 4px;
  }
  .btn-arrow {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 14px;
    padding: 5px 10px;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .btn-arrow:hover {
    background: var(--border-default);
  }

  /* ── 写入 ── */
  .write-bar {
    margin-bottom: 14px;
    padding: 10px 0;
    border-top: 1px solid var(--bg-hover);
  }

  /* ── 底部操作 ── */
  .bottom-actions {
    display: flex;
    gap: 8px;
    border-top: 1px solid var(--border-default);
    padding-top: 12px;
  }
  .btn-primary {
    background: var(--accent-cyan);
    border: none;
    border-radius: 6px;
    color: #fff;
    padding: 6px 16px;
    font-size: 13px;
    cursor: pointer;
  }
  .btn-primary:hover {
    background: #58a6ff;
  }
  .btn-primary:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .btn-danger {
    background: rgba(247, 120, 139, 0.2);
    border: none;
    border-radius: 6px;
    color: #fff;
    padding: 6px 16px;
    font-size: 13px;
    cursor: pointer;
  }
  .btn-danger:hover {
    background: rgba(247, 120, 139, 0.25);
  }
</style>

<script lang="ts">
  // 冲突解决编排(两段式,对标 WebStorm):
  //  ① 清单页:三列表格(名称 | 您的更改 | 他人的更改)+ 右侧动作区,
  //     改/删·二进制直接「接受您的/他们的」整文件取舍,内容冲突点「合并…」下钻。
  //  ② 编辑页:MergeEditor 三栏逐冲突解决,顶部「← 返回列表」。
  //  底部继续/放弃整合。
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import MergeEditor from "$lib/MergeEditor.svelte";

  type ConflictKind =
    | "BothModified"
    | "ModifyDelete"
    | "DeleteModify"
    | "AddAdd"
    | "Binary";

  interface StashRef {
    label: string;
  }

  let {
    path,
    conflictFiles,
    autostash: _autostash = null,
    initialFile,
    stashRestore = false,
    onContinue,
    onAbort,
  }: {
    path: string;
    conflictFiles: string[];
    autostash?: StashRef | null;
    initialFile?: string; // 打开时定位到的文件
    stashRestore?: boolean; // true=stash 还原冲突,仅影响文案
    onContinue: () => Promise<void>;
    onAbort: () => Promise<void>;
  } = $props();

  interface FileState {
    path: string;
    kind: ConflictKind;
    written: boolean;
    resolving: boolean; // 解决动作进行中:屏蔽连点竞态
    error: string;
  }

  function isContentKind(k: ConflictKind): boolean {
    return k === "BothModified" || k === "AddAdd";
  }

  let fileStates = $state<FileState[]>([]);
  let activeFile = $state(0);
  let view = $state<"list" | "editor">("list");
  let loading = $state(true);
  let globalError = $state("");

  onMount(async () => {
    loading = true;
    const kinds: Record<string, ConflictKind> = {};
    try {
      const cs = await invoke<{
        files: { path: string; kind: ConflictKind }[];
      }>("repo_conflict_state", { path });
      for (const cf of cs.files) kinds[cf.path] = cf.kind;
    } catch {
      // 取不到分类:全按内容冲突处理。
    }
    fileStates = conflictFiles.map((f) => ({
      path: f,
      kind: kinds[f] ?? "BothModified",
      written: false,
      resolving: false,
      error: "",
    }));
    const wanted = initialFile
      ? fileStates.findIndex((s) => s.path === initialFile)
      : -1;
    if (wanted >= 0) {
      activeFile = wanted;
      // 指定文件且为内容冲突 → 直接进编辑页(保留"点文件即编辑")。
      if (isContentKind(fileStates[wanted].kind)) view = "editor";
    } else {
      activeFile = 0;
    }
    loading = false;
  });

  let currentFile = $derived(fileStates[activeFile]);
  let allWritten = $derived(
    fileStates.length > 0 && fileStates.every((fs) => fs.written),
  );
  let pendingCount = $derived(fileStates.filter((fs) => !fs.written).length);

  // ── 展示辅助 ──
  function repoName(): string {
    return path.split("/").filter(Boolean).pop() ?? path;
  }
  function fileName(p: string): string {
    return p.split("/").pop() ?? p;
  }
  function fileDir(p: string): string {
    const parts = p.split("/");
    parts.pop();
    return parts.length ? parts.join("/") : repoName();
  }
  function fileIcon(fs: FileState): string {
    if (fs.written) return "✓";
    if (fs.kind === "Binary") return "⬡";
    if (fs.kind === "ModifyDelete" || fs.kind === "DeleteModify") return "⊘";
    return "◆";
  }

  type Cell = { text: string; tone: "mod" | "del" | "add" };
  function statuses(k: ConflictKind): { yours: Cell; theirs: Cell } {
    switch (k) {
      case "ModifyDelete":
        return {
          yours: { text: "已修改", tone: "mod" },
          theirs: { text: "已删除", tone: "del" },
        };
      case "DeleteModify":
        return {
          yours: { text: "已删除", tone: "del" },
          theirs: { text: "已修改", tone: "mod" },
        };
      case "AddAdd":
        return {
          yours: { text: "已添加", tone: "add" },
          theirs: { text: "已添加", tone: "add" },
        };
      case "Binary":
        return {
          yours: { text: "已修改(二进制)", tone: "mod" },
          theirs: { text: "已修改(二进制)", tone: "mod" },
        };
      default:
        return {
          yours: { text: "已修改", tone: "mod" },
          theirs: { text: "已修改", tone: "mod" },
        };
    }
  }

  // ── 解决动作 ──
  function advanceToNextPending() {
    const next = fileStates.findIndex((f) => !f.written);
    if (next >= 0) activeFile = next;
  }

  // 整文件取一侧:按冲突类型映射到对应后端命令。
  // resolving 守卫防止连点竞态(await 完成前 written 仍为 false,两次会并发跑破坏性命令)。
  async function acceptSide(i: number, side: "yours" | "theirs") {
    const fs = fileStates[i];
    if (!fs || fs.written || fs.resolving) return;
    let cmd: string;
    const extra: Record<string, unknown> = {};
    switch (fs.kind) {
      case "BothModified":
      case "AddAdd":
      case "Binary":
        cmd = "resolve_conflict_take_side";
        extra.side = side === "yours" ? "Ours" : "Theirs";
        break;
      case "ModifyDelete":
        // 我改/对方删:接受您的=保留我的改;接受他们的=接受删除。
        cmd =
          side === "yours"
            ? "resolve_conflict_keep"
            : "resolve_conflict_remove";
        break;
      case "DeleteModify":
        // 我删/对方改:接受您的=接受删除;接受他们的=保留对方的改。
        cmd =
          side === "yours"
            ? "resolve_conflict_remove"
            : "resolve_conflict_keep";
        break;
    }
    globalError = "";
    fs.resolving = true;
    try {
      await invoke(cmd, { path, filePath: fs.path, ...extra });
      fs.written = true;
      fs.error = "";
    } catch (e) {
      fs.error = String(e);
    } finally {
      fs.resolving = false;
    }
  }

  function openMerge(i: number) {
    const fs = fileStates[i];
    if (!fs || !isContentKind(fs.kind)) return;
    activeFile = i;
    view = "editor";
  }

  function selectRow(i: number) {
    activeFile = i;
  }

  function onEditorWritten() {
    const fs = fileStates[activeFile];
    if (fs) {
      fs.written = true;
      fs.error = "";
    }
    view = "list";
    advanceToNextPending();
  }

  function backToList() {
    view = "list";
  }

  // ── 键盘:清单导航 / 继续 / 放弃;编辑页 Esc 返回 ──
  function handleKey(e: KeyboardEvent) {
    if (loading || e.ctrlKey || e.metaKey || e.altKey) return;
    const t = e.target as HTMLElement | null;
    if (
      t &&
      (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)
    )
      return;
    if (view === "editor") {
      if (e.key === "Escape") {
        backToList();
        e.preventDefault();
      }
      return;
    }
    let handled = true;
    switch (e.key) {
      case "ArrowDown":
      case "n":
        if (activeFile < fileStates.length - 1) activeFile++;
        break;
      case "ArrowUp":
      case "p":
        if (activeFile > 0) activeFile--;
        break;
      case "Enter":
        if (currentFile && isContentKind(currentFile.kind))
          openMerge(activeFile);
        break;
      case "c":
        if (allWritten) onContinue();
        break;
      case "x":
        onAbort();
        break;
      default:
        handled = false;
    }
    if (handled) e.preventDefault();
  }
</script>

<svelte:window onkeydown={handleKey} />

<div class="conflict-view">
  {#if globalError}
    <pre class="cv-error">{globalError}</pre>
  {/if}

  {#if loading}
    <p class="cv-status">加载冲突文件…</p>
  {:else if view === "editor" && currentFile}
    <!-- 编辑页:三栏合并 -->
    <div class="ed-bar">
      <button class="btn-nav" onclick={backToList} aria-label="返回文件列表"
        >← 返回列表</button
      >
      <span class="ed-path">{currentFile.path}</span>
      {#if pendingCount > 0}
        <span class="cv-hint">还有 {pendingCount} 个待解决</span>
      {/if}
    </div>
    {#if currentFile.error}
      <pre class="cv-error">{currentFile.error}</pre>
    {/if}
    <div class="ed-main">
      {#key currentFile.path}
        <MergeEditor
          {path}
          file={currentFile.path}
          onWritten={onEditorWritten}
        />
      {/key}
    </div>
  {:else}
    <!-- 清单页 -->
    <div class="cf-head-text">
      <h2 class="cf-title">冲突</h2>
      <p class="cf-sub">
        {#if stashRestore}
          还原暂存改动时检测到冲突。请先解决这些冲突,再完成还原。
        {:else}
          检测到合并冲突。请先解决这些冲突,然后再继续。
        {/if}
      </p>
    </div>

    <div class="cf-layout">
      <div class="cf-table">
        <div class="cf-row cf-row-head">
          <span class="cf-c-name">名称</span>
          <span class="cf-c-st">您的更改 (本地)</span>
          <span class="cf-c-st">他人的更改 (传入)</span>
        </div>
        <div class="cf-body">
          {#each fileStates as fs, i (fs.path)}
            {@const st = statuses(fs.kind)}
            <button
              class="cf-row"
              class:sel={i === activeFile}
              class:done={fs.written}
              onclick={() => selectRow(i)}
              ondblclick={() => openMerge(i)}
            >
              <span class="cf-c-name">
                <span class="cf-icon">{fileIcon(fs)}</span>
                <span class="cf-fn">{fileName(fs.path)}</span>
                <span class="cf-dir">{fileDir(fs.path)}</span>
              </span>
              {#if fs.written}
                <span class="cf-c-st cf-done-tag">已解决</span>
                <span class="cf-c-st"></span>
              {:else}
                <span class="cf-c-st cf-st-{st.yours.tone}"
                  >{st.yours.text}</span
                >
                <span class="cf-c-st cf-st-{st.theirs.tone}"
                  >{st.theirs.text}</span
                >
              {/if}
            </button>
          {/each}
        </div>
      </div>

      <aside class="cf-actions">
        {#if currentFile}
          {#if currentFile.written}
            <p class="cf-act-note">「{fileName(currentFile.path)}」已解决 ✓</p>
            <p class="cf-act-hint">
              整文件取舍不可逐文件撤销;如需改主意,请放弃整合后重来。
            </p>
          {:else}
            <button
              class="btn-act"
              disabled={currentFile.resolving}
              onclick={() => acceptSide(activeFile, "yours")}
            >
              接受您的更改
            </button>
            <button
              class="btn-act"
              disabled={currentFile.resolving}
              onclick={() => acceptSide(activeFile, "theirs")}
            >
              接受他们的
            </button>
            <button
              class="btn-act btn-act-merge"
              disabled={!isContentKind(currentFile.kind) ||
                currentFile.resolving}
              onclick={() => openMerge(activeFile)}
              title={isContentKind(currentFile.kind)
                ? "打开三栏合并编辑器"
                : "该类型无法逐行合并,请用上方按钮整文件取舍"}
            >
              合并…
            </button>
          {/if}
        {/if}
      </aside>
    </div>

    {#if currentFile?.error}
      <pre class="cv-error">{currentFile.error}</pre>
    {/if}

    <!-- 底部操作 -->
    <div class="bottom-actions">
      <button
        class="btn-primary"
        disabled={!allWritten}
        onclick={onContinue}
        title={stashRestore
          ? "所有冲突已解决后,完成还原(改动留在工作区,丢弃 stash)"
          : "所有冲突文件已解决后,完成本次整合"}
      >
        {stashRestore ? "完成还原" : "继续整合"}
      </button>
      <button
        class="btn-danger"
        onclick={onAbort}
        title={stashRestore
          ? "放弃还原,回到整合后状态(改动保留在 stash)"
          : "放弃整合,回到整合前的状态"}
      >
        {stashRestore ? "放弃还原" : "放弃整合 (abort)"}
      </button>
      {#if !allWritten}
        <span class="cv-hint">还有 {pendingCount} 个文件未解决</span>
      {:else}
        <span class="cv-ok">全部已解决 ✓</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .conflict-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    box-sizing: border-box;
    padding: 14px 18px;
    font-size: 13px;
  }
  .cv-error {
    background: #3a1d1d;
    border: 1px solid rgba(247, 120, 139, 0.25);
    border-radius: 6px;
    padding: 8px 12px;
    color: var(--color-error);
    white-space: pre-wrap;
    font-size: 12px;
    margin: 0 0 10px;
  }
  .cv-status {
    color: var(--text-muted);
  }

  /* ── 清单页 ── */
  .cf-head-text {
    flex-shrink: 0;
    margin-bottom: 12px;
  }
  .cf-title {
    margin: 0 0 4px;
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .cf-sub {
    margin: 0;
    color: var(--text-secondary);
    font-size: 12.5px;
  }

  .cf-layout {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 14px;
    margin-bottom: 12px;
  }

  .cf-table {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border-default);
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-elevated);
  }
  .cf-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .cf-row {
    display: grid;
    grid-template-columns: 1fr 150px 150px;
    align-items: center;
    width: 100%;
    box-sizing: border-box;
    gap: 8px;
    padding: 7px 12px;
    text-align: left;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border-default);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 13px;
    font-family: inherit;
  }
  .cf-row-head {
    cursor: default;
    background: var(--bg-surface);
    color: var(--text-muted);
    font-size: 11.5px;
    text-transform: none;
    position: sticky;
    top: 0;
    z-index: 1;
  }
  .cf-body .cf-row:hover {
    background: var(--bg-surface);
  }
  .cf-body .cf-row.sel {
    background: rgba(88, 166, 255, 0.14);
    box-shadow: inset 2px 0 0 var(--accent-cyan);
  }
  .cf-body .cf-row.done {
    opacity: 0.65;
  }
  .cf-c-name {
    display: flex;
    align-items: baseline;
    gap: 7px;
    min-width: 0;
    overflow: hidden;
  }
  .cf-icon {
    color: var(--accent-gold);
    flex-shrink: 0;
  }
  .cf-row.done .cf-icon {
    color: var(--accent-neon);
  }
  .cf-fn {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--text-primary);
    white-space: nowrap;
  }
  .cf-dir {
    color: var(--text-muted);
    font-size: 11.5px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cf-c-st {
    font-size: 12.5px;
  }
  .cf-st-mod {
    color: var(--accent-gold);
  }
  .cf-st-del {
    color: var(--color-error);
  }
  .cf-st-add {
    color: var(--accent-neon);
  }
  .cf-done-tag {
    color: var(--accent-neon);
    font-weight: 600;
  }

  /* 右侧动作区(对标 WebStorm 右栏) */
  .cf-actions {
    flex-shrink: 0;
    width: 168px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .cf-act-note {
    margin: 0 0 4px;
    color: var(--accent-neon);
    font-size: 12.5px;
  }
  .cf-act-hint {
    margin: 0;
    color: var(--text-muted);
    font-size: 11.5px;
    line-height: 1.5;
  }
  .btn-act {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    padding: 7px 12px;
    text-align: center;
    font-family: inherit;
  }
  .btn-act:hover:not(:disabled) {
    background: var(--border-default);
  }
  .btn-act:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .btn-act-merge {
    background: var(--accent-cyan);
    border-color: var(--accent-cyan);
    color: #fff;
    font-weight: 500;
  }
  .btn-act-merge:hover:not(:disabled) {
    background: #58a6ff;
  }

  /* ── 编辑页 ── */
  .ed-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 10px;
    flex-shrink: 0;
  }
  .ed-path {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    color: var(--text-primary);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ed-main {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .btn-nav {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 4px 12px;
    flex-shrink: 0;
  }
  .btn-nav:hover {
    background: var(--border-default);
  }

  /* ── 底部操作 ── */
  .bottom-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    border-top: 1px solid var(--border-default);
    padding-top: 12px;
    flex-shrink: 0;
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
  .btn-primary:hover:not(:disabled) {
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
  .cv-hint {
    color: var(--text-muted);
    font-size: 12px;
  }
  .cv-ok {
    color: var(--accent-neon);
    font-size: 12px;
  }
</style>

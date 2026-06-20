<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import UpdateView from "$lib/UpdateView.svelte";
  import HistoryView from "$lib/HistoryView.svelte";
  import DiffView from "$lib/DiffView.svelte";
  import FileTree from "$lib/FileTree.svelte";
  import ProjectPicker from "$lib/ProjectPicker.svelte";

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
  type SubStatus = "Clean" | "Dirty" | "Detached" | "Uninitialized";
  interface RepoStatus {
    branch: string | null;
    upstream: string | null;
    behind: number;
    ahead: number;
    dirty: boolean;
    conflicted: string[];
    files: FileStatus[];
    submodules: { name: string; path: string; status: SubStatus }[];
  }

  // 一个仓库（主仓库或某子仓库）在 Changes 视图里的本地状态。
  interface RepoView {
    path: string; // 绝对路径，用于 stage/diff/commit 等所有命令
    label: string; // 显示名
    isMain: boolean;
    subRelPath: string | null; // 子仓库相对主仓库的路径（管理操作用），主仓库为 null
    subStatus: SubStatus | null; // 子仓库状态徽章，主仓库为 null
    unstaged: FileDiff[];
    staged: FileDiff[];
  }

  const SUB_STATE_LABEL: Record<SubStatus, string> = {
    Clean: "干净",
    Dirty: "有改动",
    Detached: "游离 HEAD",
    Uninitialized: "未初始化",
  };

  // ── 状态 ──
  let path = $state(""); // 主仓库路径
  let status = $state<RepoStatus | null>(null); // 主仓库状态（分支/ahead-behind/冲突/子仓库列表）
  let repos = $state<RepoView[]>([]); // 主仓库 + 各子仓库
  let selectedRepoPath = $state<string | null>(null); // 当前选中文件所属仓库
  let selectedFile = $state<FileDiff | null>(null);
  let activeList = $state<"unstaged" | "staged">("unstaged");
  let loading = $state(false);
  let error = $state("");
  let opMessage = $state(""); // fetch/push 等操作的成功提示
  let tab = $state<"changes" | "update" | "history">("changes");
  let showProjectPicker = $state(false);

  // 统一提交框(WebStorm 风格):一条提交信息应用于所有有暂存改动的仓库
  let commitMessage = $state("");
  let commitResult = $state("");
  let totalUnstaged = $derived(
    repos.reduce((n, r) => n + r.unstaged.length, 0),
  );
  let stagedRepos = $derived(repos.filter((r) => r.staged.length > 0));
  let totalStaged = $derived(
    stagedRepos.reduce((n, r) => n + r.staged.length, 0),
  );
  let commitSummary = $derived(
    stagedRepos.map((r) => `${r.label} (${r.staged.length})`).join(" · "),
  );

  // ── 文件监视:后端 debounce 300ms 后 emit "repo-changed",前端再接一次 debounce 防抖 ──
  let repoChangedUnlisten: UnlistenFn | undefined = $state();
  let refreshTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    const init = async () => {
      repoChangedUnlisten = await listen("repo-changed", () => {
        if (refreshTimer) clearTimeout(refreshTimer);
        refreshTimer = setTimeout(() => {
          refreshTimer = null;
          if (status) refresh();
        }, 300);
      });
    };
    init();
    return () => {
      repoChangedUnlisten?.();
      if (refreshTimer) clearTimeout(refreshTimer);
    };
  });

  // ── 路径辅助 ──
  function joinPath(base: string, rel: string): string {
    return `${base.replace(/[/\\]+$/, "")}/${rel}`;
  }
  function repoLabel(p: string): string {
    return (
      p
        .replace(/[/\\]+$/, "")
        .split(/[/\\]/)
        .pop() || p
    );
  }

  // 读取某仓库的未暂存/已暂存 diff,构建 RepoView。
  async function buildRepoView(
    repoPath: string,
    label: string,
    isMain: boolean,
    subRelPath: string | null,
    subStatus: SubStatus | null,
  ): Promise<RepoView> {
    let unstaged: FileDiff[] = [];
    let staged: FileDiff[] = [];
    // 未初始化的子仓库没有 .git,无法读 diff。
    if (subStatus !== "Uninitialized") {
      try {
        [unstaged, staged] = await Promise.all([
          invoke<FileDiff[]>("repo_unstaged_diff", { path: repoPath }),
          invoke<FileDiff[]>("repo_staged_diff", { path: repoPath }),
        ]);
      } catch {
        // 子仓库读取失败(损坏等)时留空,不阻塞整体加载。
      }
    }
    return {
      path: repoPath,
      label,
      isMain,
      subRelPath,
      subStatus,
      unstaged,
      staged,
    };
  }

  // 重新读取主仓库状态 + 所有仓库 diff,重建 repos。
  async function reload() {
    const s = await invoke<RepoStatus>("repo_status", { path });
    status = s;
    const main = await buildRepoView(path, repoLabel(path), true, null, null);
    const subs = await Promise.all(
      s.submodules.map((sub) =>
        buildRepoView(
          joinPath(path, sub.path),
          sub.path,
          false,
          sub.path,
          sub.status,
        ),
      ),
    );
    repos = [main, ...subs];
  }

  // ── 数据加载 ──
  function toggleProjectPicker() {
    showProjectPicker = !showProjectPicker;
  }

  function handleProjectSelect(selectedPath: string) {
    showProjectPicker = false;
    openProject(selectedPath);
  }

  async function openProject(projectPath: string) {
    path = projectPath;
    await load();
  }

  async function load() {
    if (!path.trim()) return;
    loading = true;
    error = "";
    status = null;
    repos = [];
    selectedRepoPath = null;
    selectedFile = null;
    try {
      await invoke("check_git"); // git 不在 PATH 时给友好提示(Windows 不自带)
      await reload();
      // 启动文件监视(自动切仓:start_watch 内部会停旧启新)
      invoke("start_watch", { path }).catch(() => {});
      // 加载成功后保存到历史
      await invoke("add_recent_project", { path });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // 启动时自动加载上次项目
  $effect(() => {
    const initLoad = async () => {
      try {
        const lastProject = await invoke<string | null>("get_last_project");
        if (lastProject) {
          await openProject(lastProject);
        }
      } catch (e) {
        console.error("无法加载上次项目:", e);
      }
    };
    initLoad();
  });

  async function refresh() {
    selectedLines = new Map();
    await reload();
    reconcileSelection();
  }

  // 刷新后保持原选中文件;若它已消失则清空选中(不自动另选,保持收起的初始态)。
  function reconcileSelection() {
    if (selectedRepoPath && selectedFile) {
      const repo = repos.find((r) => r.path === selectedRepoPath);
      if (repo) {
        const inU = repo.unstaged.find((f) => f.path === selectedFile!.path);
        if (inU) {
          selectedFile = inU;
          activeList = "unstaged";
          return;
        }
        const inS = repo.staged.find((f) => f.path === selectedFile!.path);
        if (inS) {
          selectedFile = inS;
          activeList = "staged";
          return;
        }
      }
    }
    selectedRepoPath = null;
    selectedFile = null;
  }

  function selectFile(
    repoPath: string,
    file: FileDiff,
    list: "unstaged" | "staged",
  ) {
    selectedRepoPath = repoPath;
    selectedFile = file;
    activeList = list;
    selectedLines = new Map();
  }

  function isActive(repo: RepoView, list: "unstaged" | "staged"): boolean {
    return selectedRepoPath === repo.path && activeList === list;
  }
  // FileTree 的 selectedPath:仅当该仓库该列表正被选中时高亮。
  function selKey(repo: RepoView, list: "unstaged" | "staged"): string | null {
    return isActive(repo, list) ? (selectedFile?.path ?? null) : null;
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

  // ── 文件操作(均按 repoPath 定向到对应仓库) ──
  let operating = $state(false);

  async function stagePaths(repoPath: string, paths: string[]) {
    if (paths.length === 0) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_stage", { path: repoPath, files: paths });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  async function unstagePaths(repoPath: string, paths: string[]) {
    if (paths.length === 0) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_unstage", { path: repoPath, files: paths });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  async function discardPaths(repoPath: string, paths: string[]) {
    if (paths.length === 0) return;
    const msg =
      paths.length === 1
        ? `确定丢弃 ${paths[0]} 的改动?（stash 保存可找回）`
        : `确定丢弃这 ${paths.length} 个文件的改动?（stash 保存可找回）`;
    if (!confirm(msg)) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_discard", { path: repoPath, files: paths });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  // 提交所有有暂存改动的仓库:子仓库先提交(主仓库可能含其指针变更),同一条 message。
  async function doCommit() {
    if (totalStaged === 0 || !commitMessage.trim()) return;
    const targets = [...stagedRepos].sort(
      (a, b) => Number(a.isMain) - Number(b.isMain),
    );
    operating = true;
    error = "";
    commitResult = "";
    const message = commitMessage;
    try {
      for (const r of targets) {
        await invoke<string>("repo_commit", { path: r.path, message });
      }
      commitMessage = "";
      commitResult = `已提交 ${targets.length} 个仓库`;
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  // ── 子仓库管理(在主仓库执行 git submodule ...) ──
  async function submoduleAction(
    repo: RepoView,
    op: "update" | "remote" | "sync",
  ) {
    if (!repo.subRelPath) return;
    operating = true;
    error = "";
    try {
      const cmd =
        op === "update"
          ? "repo_submodule_update"
          : op === "remote"
            ? "repo_submodule_update_remote"
            : "repo_submodule_sync";
      await invoke(cmd, { path, subPath: repo.subRelPath });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  // ── 主仓库远程操作 ──
  async function doFetch(repoPath: string) {
    operating = true;
    error = "";
    opMessage = "";
    try {
      await invoke("repo_fetch", { path: repoPath });
      opMessage = "已拉取远程更新（fetch）";
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  async function doPush(repoPath: string) {
    operating = true;
    error = "";
    opMessage = "";
    try {
      const msg = await invoke<string>("repo_push", { path: repoPath });
      opMessage = msg;
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  // ── hunk / 行级操作(作用于当前选中文件所属仓库) ──
  async function stageHunk(file: FileDiff, hunk: Hunk) {
    if (!selectedRepoPath) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_stage_hunk", { path: selectedRepoPath, file, hunk });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  async function unstageHunk(file: FileDiff, hunk: Hunk) {
    if (!selectedRepoPath) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_unstage_hunk", { path: selectedRepoPath, file, hunk });
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
    if (!sel || sel.size === 0 || !selectedRepoPath) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_stage_lines", {
        path: selectedRepoPath,
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
    if (!sel || sel.size === 0 || !selectedRepoPath) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_unstage_lines", {
        path: selectedRepoPath,
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
</script>

<main>
  <!-- ── 项目选择器覆盖层 ── -->
  {#if showProjectPicker}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="picker-overlay"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={toggleProjectPicker}
      onkeydown={(e) => e.key === "Escape" && toggleProjectPicker()}
    >
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="picker-modal" onclick={(e) => e.stopPropagation()}>
        <ProjectPicker onselect={handleProjectSelect} />
      </div>
    </div>
  {/if}

  <!-- ── 顶栏 ── -->
  <header class="topbar">
    <span class="logo">git-gui</span>
    <div class="path-bar">
      {#if status}
        <button
          class="btn-pick"
          onclick={toggleProjectPicker}
          disabled={loading}>切换项目</button
        >
        <span class="current-path" title={path}>{repoLabel(path)}</span>
        <button
          class="btn-refresh"
          onclick={refresh}
          disabled={loading || operating}
          title="刷新所有仓库（含子仓库的外部改动）">↻</button
        >
      {:else}
        <span class="current-path-empty">未选择项目</span>
        <button
          class="btn-pick"
          onclick={toggleProjectPicker}
          disabled={loading}>选择项目</button
        >
      {/if}
    </div>
    {#if status}
      <span class="branch">{status.branch ?? "(detached)"}</span>
      {#if status.ahead > 0}<span class="badge ahead">↑{status.ahead}</span
        >{/if}
      {#if status.behind > 0}<span class="badge behind">↓{status.behind}</span
        >{/if}
    {/if}
  </header>

  <!-- ── 标签栏 ── -->
  <nav class="tab-bar">
    <button
      class="tab-btn"
      class:tab-active={tab === "changes"}
      onclick={() => (tab = "changes")}
    >
      Changes
    </button>
    <button
      class="tab-btn"
      class:tab-active={tab === "update"}
      onclick={() => (tab = "update")}
    >
      Update
    </button>
    <button
      class="tab-btn"
      class:tab-active={tab === "history"}
      onclick={() => (tab = "history")}
    >
      History
    </button>
  </nav>

  {#if tab === "changes"}
    {#if error}
      <pre class="error">{error}</pre>
    {/if}
    {#if opMessage}
      <p class="op-message">{opMessage}</p>
    {/if}

    {#if status}
      <div class="split">
        <!-- ── 左侧:主仓库 + 各子仓库,每个都能独立暂存/提交 ── -->
        <aside class="file-list">
          <div class="repo-scroll">
            <!-- 未暂存区:各仓库分组(含仓库操作按钮) + 未暂存目录树 -->
            <section class="zone">
              <h2 class="zone-title">未暂存 ({totalUnstaged})</h2>
              {#each repos as repo (repo.path)}
                <div class="repo-group">
                  <div class="repo-grouphead">
                    {#if repo.isMain}
                      <span class="repo-title main">主仓库 · {repo.label}</span>
                    {:else}
                      <span class="sub-dot sub-{repo.subStatus?.toLowerCase()}"
                        >●</span
                      >
                      <span class="repo-title" title={repo.path}
                        >{repo.label}</span
                      >
                      <span class="sub-state"
                        >{repo.subStatus
                          ? SUB_STATE_LABEL[repo.subStatus]
                          : ""}</span
                      >
                    {/if}
                  </div>
                  {#if repo.isMain}
                    <div class="repo-manage">
                      <button
                        class="sub-btn"
                        disabled={operating}
                        title="合并远程改动:走 Update 流程,可处理冲突"
                        onclick={() => (tab = "update")}>更新</button
                      >
                      <button
                        class="sub-btn"
                        disabled={operating}
                        title="拉取远程(fetch:只下载不合并,无冲突)"
                        onclick={() => doFetch(repo.path)}>拉取</button
                      >
                      <button
                        class="sub-btn"
                        disabled={operating}
                        title="推送当前分支到远程"
                        onclick={() => doPush(repo.path)}>推送</button
                      >
                    </div>
                  {:else}
                    <div class="repo-manage">
                      <button
                        class="sub-btn"
                        disabled={operating}
                        title="初始化 / 更新到父仓库记录的提交（update --init）"
                        onclick={() => submoduleAction(repo, "update")}
                        >更新</button
                      >
                      {#if repo.subStatus !== "Uninitialized"}
                        <button
                          class="sub-btn"
                          disabled={operating}
                          title="拉取远程分支最新提交（update --remote）"
                          onclick={() => submoduleAction(repo, "remote")}
                          >拉取</button
                        >
                        <button
                          class="sub-btn"
                          disabled={operating}
                          title="同步子仓库 URL 配置（sync）"
                          onclick={() => submoduleAction(repo, "sync")}
                          >同步</button
                        >
                      {/if}
                    </div>
                  {/if}
                  {#if repo.subStatus === "Uninitialized"}
                    <p class="muted">未初始化 — 点"更新"执行 init</p>
                  {:else if repo.unstaged.length === 0}
                    <p class="muted">无改动</p>
                  {:else}
                    <FileTree
                      files={repo.unstaged}
                      selectedPath={selKey(repo, "unstaged")}
                      kind="unstaged"
                      {operating}
                      onSelect={(f) => selectFile(repo.path, f, "unstaged")}
                      onStage={(p) => stagePaths(repo.path, p)}
                      onUnstage={(p) => unstagePaths(repo.path, p)}
                      onDiscard={(p) => discardPaths(repo.path, p)}
                    />
                  {/if}
                </div>
              {/each}
            </section>

            <!-- 已暂存区:仅列出有已暂存改动的仓库,作为待提交预览 -->
            {#if totalStaged > 0}
              <section class="zone">
                <h2 class="zone-title">已暂存 ({totalStaged})</h2>
                {#each repos as repo (repo.path)}
                  {#if repo.staged.length > 0}
                    <div class="repo-group">
                      <div class="repo-grouphead">
                        {#if repo.isMain}
                          <span class="repo-title main"
                            >主仓库 · {repo.label}</span
                          >
                        {:else}
                          <span class="repo-title" title={repo.path}
                            >{repo.label}</span
                          >
                        {/if}
                      </div>
                      <FileTree
                        files={repo.staged}
                        selectedPath={selKey(repo, "staged")}
                        kind="staged"
                        {operating}
                        onSelect={(f) => selectFile(repo.path, f, "staged")}
                        onStage={(p) => stagePaths(repo.path, p)}
                        onUnstage={(p) => unstagePaths(repo.path, p)}
                        onDiscard={(p) => discardPaths(repo.path, p)}
                      />
                    </div>
                  {/if}
                {/each}
              </section>
            {/if}

            <!-- 冲突(主仓库) -->
            {#if status.conflicted.length}
              <section class="zone">
                <h2 class="zone-title">冲突 ({status.conflicted.length})</h2>
                <ul>
                  {#each status.conflicted as c}
                    <li class="file-item conflict">{c}</li>
                  {/each}
                </ul>
              </section>
            {/if}
          </div>

          <!-- 统一提交区(所有有暂存改动的仓库套用同一条 message) -->
          <div class="commit-area">
            {#if totalStaged === 0}
              <p class="muted">暂存文件以创建提交</p>
            {:else}
              <textarea
                bind:value={commitMessage}
                placeholder="提交信息（必填，应用于所有已暂存的仓库）"
                rows={3}
                disabled={operating}></textarea>
              <p class="commit-targets">将提交：{commitSummary}</p>
              <div class="commit-bar">
                <button
                  class="btn-commit"
                  disabled={operating || !commitMessage.trim()}
                  onclick={doCommit}>提交（{totalStaged} 个文件）</button
                >
              </div>
            {/if}
            {#if commitResult}
              <p class="commit-ok">{commitResult}</p>
            {/if}
          </div>
        </aside>

        <!-- ── 右侧:diff 视图 ── -->
        <section class="diff-view">
          {#if selectedFile}
            <DiffView
              files={[selectedFile]}
              interactive
              {selectedLines}
              onToggleLine={toggleLine}
              onStageHunk={(hunk) => stageHunk(selectedFile!, hunk)}
              onStageLines={(hunk, hi) =>
                stageSelectedLines(selectedFile!, hunk, hi)}
              onUnstageHunk={(hunk) => unstageHunk(selectedFile!, hunk)}
              onUnstageLines={(hunk, hi) =>
                unstageSelectedLines(selectedFile!, hunk, hi)}
              {activeList}
              {operating}
            />
          {:else}
            <p class="muted placeholder">← 选择左侧文件查看 diff</p>
          {/if}
        </section>
      </div>
    {/if}

    {#if !status && !error && !loading}
      <p class="hint">打开一个 Git 仓库以查看 Changes</p>
    {/if}
  {:else if tab === "update"}
    <UpdateView {path} onRefresh={refresh} />
  {:else if tab === "history"}
    <HistoryView {path} />
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
  /* ── 标签栏 ── */
  .tab-bar {
    display: flex;
    background: #1e1e1e;
    border-bottom: 1px solid #383838;
    flex-shrink: 0;
  }
  .tab-btn {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: #888;
    cursor: pointer;
    font-size: 13px;
    padding: 8px 18px;
    transition:
      color 0.15s,
      border-color 0.15s;
  }
  .tab-btn:hover {
    color: #ccc;
  }
  .tab-active {
    color: #e4e4e4;
    border-bottom-color: #0e639c;
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
  .current-path {
    flex: 1;
    max-width: 360px;
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 6px;
    color: #e4e4e4;
    padding: 6px 10px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .current-path-empty {
    flex: 1;
    max-width: 360px;
    color: #666;
    padding: 6px 10px;
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
  .btn-pick {
    background: #2a2a2a !important;
    border: 1px solid #444 !important;
    color: #e4e4e4 !important;
    flex-shrink: 0;
  }
  .btn-refresh {
    background: #2a2a2a !important;
    border: 1px solid #444 !important;
    color: #e4e4e4 !important;
    padding: 6px 10px !important;
    flex-shrink: 0;
  }

  /* ── 项目选择器覆盖层 ── */
  .picker-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .picker-modal {
    background: #1e1e1e;
    border: 1px solid #444;
    border-radius: 8px;
    padding: 24px;
    max-width: 90%;
    max-height: 90%;
    overflow-y: auto;
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
  .op-message {
    background: #1d3a24;
    border-bottom: 1px solid #2b6a3b;
    padding: 8px 14px;
    color: #7ee29a;
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
    width: 270px;
    flex-shrink: 0;
    border-right: 1px solid #383838;
    display: flex;
    flex-direction: column;
    background: #212121;
  }
  .repo-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 10px 0;
    contain: layout paint; /* 与右侧 diff 互相隔离重绘,大 diff 不拖累左侧滚动 */
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
    min-height: 26px;
  }
  .file-item:hover {
    background: #2a2a2a;
  }
  .file-item.conflict {
    color: #f3b4b4;
  }
  .muted {
    color: #666;
    font-size: 12px;
    padding: 4px 14px;
  }

  /* ── 暂存状态分区 + 仓库分组 ── */
  .zone {
    margin-bottom: 10px;
  }
  .zone-title {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #bbb;
    font-weight: 600;
    margin: 0;
    padding: 8px 14px 6px;
    position: sticky;
    top: 0;
    background: #212121;
    z-index: 1;
  }
  .repo-group {
    margin-bottom: 6px;
  }
  .repo-grouphead {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 12px 3px;
  }
  .repo-title {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    font-weight: 600;
    color: #cdcdcd;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .repo-title.main {
    color: #9ecbff;
  }
  .repo-manage {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    padding: 0 14px 6px 32px;
  }
  .sub-dot {
    font-size: 10px;
    flex-shrink: 0;
  }
  .sub-clean {
    color: #7ee29a;
  }
  .sub-dirty {
    color: #e2c47a;
  }
  .sub-detached {
    color: #f3b4b4;
  }
  .sub-uninitialized {
    color: #777;
  }
  .sub-state {
    font-size: 11px;
    color: #888;
    margin-left: auto;
    flex-shrink: 0;
  }
  .sub-btn {
    background: #333;
    border: 1px solid #555;
    border-radius: 4px;
    color: #ddd;
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
    line-height: 1.4;
  }
  .sub-btn:hover {
    background: #3d4f63;
    border-color: #0e639c;
    color: #fff;
  }
  .sub-btn:disabled {
    opacity: 0.35;
    cursor: default;
  }

  /* ── commit ── */
  .commit-area {
    flex-shrink: 0;
    border-top: 1px solid #383838;
    padding: 10px 14px 14px;
  }
  .commit-targets {
    font-size: 11px;
    color: #888;
    margin: 6px 0 0;
    word-break: break-all;
  }
  .commit-area textarea {
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
  .commit-area textarea:disabled {
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
    contain: layout paint; /* 隔离右侧大 diff 的重绘,左侧滚动更流畅 */
  }
  .placeholder {
    margin-top: 40px;
    text-align: center;
  }

  .hint {
    color: #888;
    font-size: 13px;
    text-align: center;
    margin-top: 60px;
  }
</style>

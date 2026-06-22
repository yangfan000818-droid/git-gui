<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import UpdateView from "$lib/UpdateView.svelte";
  import HistoryView from "$lib/HistoryView.svelte";
  import DiffView from "$lib/DiffView.svelte";
  import FileTree from "$lib/FileTree.svelte";
  import ProjectPicker from "$lib/ProjectPicker.svelte";
  import FileHistory from "$lib/FileHistory.svelte";
  import BlameView from "$lib/BlameView.svelte";
  import BranchPicker from "$lib/BranchPicker.svelte";
  import ConflictView from "$lib/ConflictView.svelte";

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

  // 列表用的轻量文件条目(只含路径和状态;完整 diff 在选中文件时懒加载)。
  interface FileEntry {
    path: string;
    state: FileState;
  }

  // 一个仓库（主仓库或某子仓库）在 Changes 视图里的本地状态。
  interface RepoView {
    path: string; // 绝对路径，用于 stage/diff/commit 等所有命令
    label: string; // 显示名
    isMain: boolean;
    subRelPath: string | null; // 子仓库相对主仓库的路径（管理操作用），主仓库为 null
    subStatus: SubStatus | null; // 子仓库状态徽章，主仓库为 null
    branch: string | null; // 当前分支，detached HEAD 时为 null
    ahead: number;
    behind: number;
    unstaged: FileEntry[];
    staged: FileEntry[];
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
  let selectedFilePath = $state<string | null>(null); // 选中文件路径(列表高亮 + 懒加载键)
  let selectedFile = $state<FileDiff | null>(null); // 懒加载的完整 diff
  let diffLoading = $state(false);
  let activeList = $state<"unstaged" | "staged">("unstaged");
  let loading = $state(false);
  let error = $state("");
  let opMessage = $state(""); // fetch/push 等操作的成功提示
  let tab = $state<"changes" | "history">("changes");
  let showProjectPicker = $state(false);
  let showUpdate = $state(false); // 更新弹层
  // 更新弹层入参:子仓列表 / 标题 / 是否仅子仓(跳过主仓库整合)。
  let updateSubmodules = $state<
    { name: string; path: string; status: string }[]
  >([]);
  let updateTitle = $state("全部更新");
  let updateSubsOnly = $state(false);
  let showFileHistory = $state(false); // 文件历史弹窗
  let fileHistoryPath = $state(""); // 文件历史查看的文件路径
  let showBlame = $state(false); // blame 弹窗
  let blamePath = $state(""); // blame 查看的文件路径
  let branchPickerRepo = $state<string | null>(null); // 哪个仓库的分支选择面板打开(null=关闭)
  interface StashRef {
    label: string;
  }
  interface MergeConflict {
    repoPath: string;
    files: string[];
    autostash: StashRef | null;
  }
  let mergeConflict = $state<MergeConflict | null>(null); // 合并/变基产生冲突时的 ConflictView 弹层
  let subCount = $derived(status?.submodules.length ?? 0); // 子仓库数(顶部「更新子仓库」据此启用)

  // 统一提交框(WebStorm 风格):一条提交信息应用于所有有暂存改动的仓库
  let commitMessage = $state("");
  let amendMode = $state(false); // amend 模式:仅修改主仓库上次提交
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
        // 更新进行中:git 大量改 .git/工作区会狂触发 watcher,此时自动刷新只会
        // 读到中间态并反复全量 reload 阻塞主线程(卡顿源),跳过;更新完成会主动刷新。
        if (showUpdate) return;
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

  // 用仓库的文件状态列表构建 RepoView(轻量,不含 diff 内容;diff 选中时懒加载)。
  function buildRepoView(
    repoPath: string,
    label: string,
    isMain: boolean,
    subRelPath: string | null,
    subStatus: SubStatus | null,
    branch: string | null,
    ahead: number,
    behind: number,
    files: FileStatus[],
  ): RepoView {
    const unstaged = files
      .filter(
        (f) =>
          f.state === "Modified" ||
          f.state === "Untracked" ||
          f.state === "StagedAndModified",
      )
      .map((f) => ({ path: f.path, state: f.state }));
    const staged = files
      .filter((f) => f.state === "Staged" || f.state === "StagedAndModified")
      .map((f) => ({ path: f.path, state: f.state }));
    return {
      path: repoPath,
      label,
      isMain,
      subRelPath,
      subStatus,
      branch,
      ahead,
      behind,
      unstaged,
      staged,
    };
  }

  // 重新读取主仓库 + 各子仓库状态,重建 repos(只拉文件状态,不拉 diff)。
  async function reload() {
    const s = await invoke<RepoStatus>("repo_status", { path });
    status = s;
    const main = buildRepoView(
      path,
      repoLabel(path),
      true,
      null,
      null,
      s.branch,
      s.ahead,
      s.behind,
      s.files,
    );
    const subs = await Promise.all(
      s.submodules.map(async (sub) => {
        let files: FileStatus[] = [];
        let branch: string | null = null;
        let ahead = 0;
        let behind = 0;
        // 未初始化的子仓库没有 .git,无法读状态。
        if (sub.status !== "Uninitialized") {
          try {
            const ss = await invoke<RepoStatus>("repo_status", {
              path: joinPath(path, sub.path),
            });
            files = ss.files;
            branch = ss.branch;
            ahead = ss.ahead;
            behind = ss.behind;
          } catch {
            // 子仓库读取失败(损坏等)时留空,不阻塞整体加载。
          }
        }
        return buildRepoView(
          joinPath(path, sub.path),
          sub.path,
          false,
          sub.path,
          sub.status,
          branch,
          ahead,
          behind,
          files,
        );
      }),
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
    selectedFilePath = null;
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
    await reload();
    await reconcileSelection();
  }

  // 刷新后保持原选中文件并重新懒加载其 diff;若它已消失则清空选中。
  // keepStale:重载同一文件,保留旧 diff 不闪占位符,行选择由 loadDiff 按内容是否变决定。
  async function reconcileSelection() {
    if (selectedRepoPath && selectedFilePath) {
      const repo = repos.find((r) => r.path === selectedRepoPath);
      if (repo) {
        if (repo.unstaged.find((f) => f.path === selectedFilePath)) {
          activeList = "unstaged";
          await loadDiff(selectedRepoPath, selectedFilePath, "unstaged", true);
          return;
        }
        if (repo.staged.find((f) => f.path === selectedFilePath)) {
          activeList = "staged";
          await loadDiff(selectedRepoPath, selectedFilePath, "staged", true);
          return;
        }
      }
    }
    selectedRepoPath = null;
    selectedFilePath = null;
    selectedFile = null;
    selectedLines = new Map();
  }

  async function selectFile(
    repoPath: string,
    entry: FileEntry,
    list: "unstaged" | "staged",
  ) {
    selectedRepoPath = repoPath;
    selectedFilePath = entry.path;
    activeList = list;
    selectedLines = new Map();
    await loadDiff(repoPath, entry.path, list);
  }

  // 懒加载选中文件的完整 diff(列表只有路径/状态,内容点开才拉)。
  // keepStale:重载当前已选中文件时(刷新/操作后)保留旧 diff,不闪"加载中"占位符,
  // 拿到新内容再原地替换;且仅当内容真变了(stage 致 hunk 重编号/外部改动)才清行选择,
  // 没变则保留——避免偶发刷新打扰用户正在进行的行级勾选。
  async function loadDiff(
    repoPath: string,
    filePath: string,
    list: "unstaged" | "staged",
    keepStale = false,
  ) {
    const prev = keepStale ? selectedFile : null;
    if (!keepStale) diffLoading = true;
    try {
      const cmd =
        list === "unstaged"
          ? "repo_file_unstaged_diff"
          : "repo_file_staged_diff";
      const next = await invoke<FileDiff | null>(cmd, {
        path: repoPath,
        file: filePath,
      });
      selectedFile = next;
      if (keepStale && !sameDiff(prev, next)) selectedLines = new Map();
    } catch (e) {
      error = String(e);
      selectedFile = null;
    } finally {
      if (!keepStale) diffLoading = false;
    }
  }

  // 两个 diff 是否内容相同(按 hunk 原始文本逐段比较;null 安全)。
  function sameDiff(a: FileDiff | null, b: FileDiff | null): boolean {
    if (a === b) return true;
    if (!a || !b) return false;
    if (a.hunks.length !== b.hunks.length) return false;
    return a.hunks.every((h, i) => h.raw === b.hunks[i].raw);
  }

  function isActive(repo: RepoView, list: "unstaged" | "staged"): boolean {
    return selectedRepoPath === repo.path && activeList === list;
  }
  // FileTree 的 selectedPath:仅当该仓库该列表正被选中时高亮。
  function selKey(repo: RepoView, list: "unstaged" | "staged"): string | null {
    return isActive(repo, list) ? selectedFilePath : null;
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

  // amend:勾选时预填主仓库上次 message;仅修改主仓库上次提交。
  async function toggleAmend(checked: boolean) {
    amendMode = checked;
    if (checked) {
      try {
        commitMessage = await invoke<string>("repo_commit_message", {
          path,
          sha: "HEAD",
        });
      } catch (e) {
        error = String(e);
        amendMode = false;
      }
    } else {
      commitMessage = "";
    }
  }

  async function doAmend() {
    if (!commitMessage.trim()) return;
    operating = true;
    error = "";
    commitResult = "";
    const message = commitMessage;
    try {
      await invoke<string>("repo_commit", { path, message, amend: true });
      commitMessage = "";
      amendMode = false;
      commitResult = "已修改主仓库上次提交";
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
        await invoke<string>("repo_commit", {
          path: r.path,
          message,
          amend: false,
        });
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
  // 同步子仓库 URL 配置(git submodule sync)。
  async function syncSubmodule(repo: RepoView) {
    if (!repo.subRelPath) return;
    operating = true;
    error = "";
    try {
      await invoke("repo_submodule_sync", { path, subPath: repo.subRelPath });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  // ── 远程操作 ──
  // 全部更新:主仓库整合 + 各子仓库在各自当前分支更新。
  function openUpdateAll() {
    if (!status) return;
    updateSubmodules = status.submodules;
    updateTitle = "全部更新";
    updateSubsOnly = false;
    error = "";
    opMessage = "";
    showUpdate = true;
  }

  // 仅更新主仓库(弹层不带子仓库)。
  function openUpdateMain() {
    updateSubmodules = [];
    updateTitle = "更新主仓库";
    updateSubsOnly = false;
    error = "";
    opMessage = "";
    showUpdate = true;
  }

  // 顶部「更新子仓库」:对所有子仓库按状态自动分流(未初始化→init,已初始化→on-branch 更新),不动主仓。
  function openUpdateSubs() {
    if (!status) return;
    updateSubmodules = status.submodules;
    updateTitle = "更新子仓库";
    updateSubsOnly = true;
    error = "";
    opMessage = "";
    showUpdate = true;
  }

  // 行内「更新」:单个子仓在当前分支更新,复用 UpdateView 冲突流程。
  function openUpdateSub(repo: RepoView) {
    if (!status || !repo.subRelPath) return;
    const sub = status.submodules.find((s) => s.path === repo.subRelPath);
    if (!sub) return;
    updateSubmodules = [sub];
    updateTitle = `更新子仓库 · ${repo.label}`;
    updateSubsOnly = true;
    error = "";
    opMessage = "";
    showUpdate = true;
  }

  // 推送单个仓库(主仓库组的独立「推送」)。
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

  // 全部推送:遍历主仓库 + 各子仓库,各自 push,逐个汇总(无 upstream / 失败不中断)。
  async function doPushAll() {
    if (repos.length === 0) return;
    operating = true;
    error = "";
    opMessage = "";
    const lines: string[] = [];
    for (const r of repos) {
      try {
        const msg = await invoke<string>("repo_push", { path: r.path });
        lines.push(`${r.label}：${msg}`);
      } catch (e) {
        lines.push(`${r.label}：${String(e)}`);
      }
    }
    opMessage = lines.join("\n");
    operating = false;
    await refresh();
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
          class="current-path"
          onclick={toggleProjectPicker}
          disabled={loading}
          title="点击切换项目 · {path}">{repoLabel(path)}</button
        >
        <button
          class="btn-refresh"
          onclick={refresh}
          disabled={loading || operating}
          title="刷新所有仓库（含子仓库的外部改动）">↻</button
        >
      {:else}
        <button
          class="current-path-empty"
          onclick={toggleProjectPicker}
          disabled={loading}>选择项目…</button
        >
      {/if}
    </div>
    {#if status}
      <span class="branch">{status.branch ?? "(detached)"}</span>
      {#if status.ahead > 0}<span class="badge ahead">↑{status.ahead}</span
        >{/if}
      {#if status.behind > 0}<span class="badge behind">↓{status.behind}</span
        >{/if}
      <div class="remote-actions">
        <button
          class="btn-remote"
          disabled={loading || operating}
          title="更新主仓库，并把各子仓库拉取到各自远程最新"
          onclick={openUpdateAll}>全部更新</button
        >
        <button
          class="btn-remote"
          disabled={loading || operating}
          title="推送主仓库及各子仓库到各自远程"
          onclick={doPushAll}>全部推送</button
        >
        <button
          class="btn-remote"
          disabled={loading || operating || subCount === 0}
          title="未初始化的子仓库执行 init，已初始化的在各自当前分支更新（留在原分支）"
          onclick={openUpdateSubs}>更新子仓库</button
        >
      </div>
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
      <div class="op-message">
        <span class="op-message-text">{opMessage}</span>
        <button
          class="op-message-close"
          onclick={() => (opMessage = "")}
          aria-label="关闭提示"
          title="关闭">×</button
        >
      </div>
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
                    {#if repo.branch != null}
                      <button
                        class="repo-branch-btn"
                        onclick={() => (branchPickerRepo = repo.path)}
                        title="点击切换分支"
                      >
                        <span class="repo-branch">{repo.branch}</span>
                        {#if repo.ahead > 0}<span class="badge ahead"
                            >↑{repo.ahead}</span
                          >{/if}
                        {#if repo.behind > 0}<span class="badge behind"
                            >↓{repo.behind}</span
                          >{/if}
                      </button>
                    {:else if repo.subStatus !== "Uninitialized"}
                      <button
                        class="repo-branch-btn"
                        onclick={() => (branchPickerRepo = repo.path)}
                        title="点击切换分支"
                      >
                        <span class="repo-branch detached">(detached)</span>
                      </button>
                    {/if}
                  </div>
                  {#if repo.isMain}
                    <div class="repo-manage">
                      <button
                        class="sub-btn"
                        disabled={operating}
                        title="更新主仓库:走 update 流程,可处理冲突"
                        onclick={openUpdateMain}>更新</button
                      >
                      <button
                        class="sub-btn"
                        disabled={operating}
                        title="推送主仓库当前分支到远程"
                        onclick={() => doPush(repo.path)}>推送</button
                      >
                    </div>
                  {:else if repo.subStatus !== "Uninitialized"}
                    <div class="repo-manage">
                      <button
                        class="sub-btn"
                        disabled={operating}
                        title="在当前分支上更新（留在原分支）"
                        onclick={() => openUpdateSub(repo)}>更新</button
                      >
                      <button
                        class="sub-btn"
                        disabled={operating}
                        title="推送子仓库当前分支到远程"
                        onclick={() => doPush(repo.path)}>推送</button
                      >
                      <button
                        class="sub-btn"
                        disabled={operating}
                        title="同步子仓库 URL 配置（sync）"
                        onclick={() => syncSubmodule(repo)}>同步</button
                      >
                    </div>
                  {/if}
                  {#if repo.subStatus === "Uninitialized"}
                    <p class="muted">未初始化 — 点顶部「更新子仓库」</p>
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
                        {#if repo.branch != null}
                          <button
                            class="repo-branch-btn"
                            onclick={() => (branchPickerRepo = repo.path)}
                            title="点击切换分支"
                          >
                            <span class="repo-branch">{repo.branch}</span>
                            {#if repo.ahead > 0}<span class="badge ahead"
                                >↑{repo.ahead}</span
                              >{/if}
                            {#if repo.behind > 0}<span class="badge behind"
                                >↓{repo.behind}</span
                              >{/if}
                          </button>
                        {:else if repo.subStatus !== "Uninitialized"}
                          <button
                            class="repo-branch-btn"
                            onclick={() => (branchPickerRepo = repo.path)}
                            title="点击切换分支"
                          >
                            <span class="repo-branch detached">(detached)</span>
                          </button>
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
            <label class="amend-toggle">
              <input
                type="checkbox"
                checked={amendMode}
                disabled={operating}
                onchange={(e) => toggleAmend(e.currentTarget.checked)}
              />
              修改主仓库上次提交（amend）
            </label>
            {#if totalStaged === 0 && !amendMode}
              <p class="muted">暂存文件以创建提交</p>
            {:else}
              <textarea
                bind:value={commitMessage}
                placeholder="提交信息（必填）"
                rows={3}
                disabled={operating}></textarea>
              {#if amendMode}
                <p class="commit-targets muted">
                  amend 仅改主仓库上次提交；子仓库改动请取消勾选后单独提交
                </p>
              {:else}
                <p class="commit-targets">将提交：{commitSummary}</p>
              {/if}
              <div class="commit-bar">
                <button
                  class="btn-commit"
                  disabled={operating || !commitMessage.trim()}
                  onclick={amendMode ? doAmend : doCommit}
                  >{amendMode
                    ? "修改主仓库上次提交"
                    : `提交（${totalStaged} 个文件）`}</button
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
          {#if diffLoading}
            <p class="muted placeholder">加载 diff 中…</p>
          {:else if selectedFile}
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
              onFileHistory={(filePath) => {
                fileHistoryPath = filePath;
                showFileHistory = true;
              }}
              onBlame={(filePath) => {
                blamePath = filePath;
                showBlame = true;
              }}
            />
          {:else if selectedFilePath}
            <p class="muted placeholder">该文件无差异内容</p>
          {:else}
            <p class="muted placeholder">← 选择左侧文件查看 diff</p>
          {/if}
        </section>
      </div>
    {/if}

    {#if !status && !error && !loading}
      <p class="hint">打开一个 Git 仓库以查看 Changes</p>
    {/if}
  {:else if tab === "history"}
    <HistoryView
      {path}
      onFileHistory={(filePath) => {
        fileHistoryPath = filePath;
        showFileHistory = true;
      }}
    />
  {/if}

  <!-- ── 全部更新弹层(主仓库 update + 各子仓库 update --remote) ── -->
  {#if showUpdate && status}
    <div class="update-overlay">
      <div class="update-modal">
        <UpdateView
          {path}
          submodules={updateSubmodules}
          title={updateTitle}
          subsOnly={updateSubsOnly}
          onRefresh={refresh}
          onClose={() => (showUpdate = false)}
        />
      </div>
    </div>
  {/if}

  <!-- ── 文件历史弹窗 ── -->
  {#if showFileHistory}
    <FileHistory
      {path}
      filePath={fileHistoryPath}
      onClose={() => (showFileHistory = false)}
    />
  {/if}

  {#if showBlame}
    <BlameView
      {path}
      filePath={blamePath}
      onClose={() => (showBlame = false)}
    />
  {/if}

  {#if branchPickerRepo}
    <BranchPicker
      repoPath={branchPickerRepo}
      onClose={() => (branchPickerRepo = null)}
      onSwitched={refresh}
      onConflict={(d) => {
        branchPickerRepo = null;
        mergeConflict = d;
      }}
    />
  {/if}

  <!-- ── 合并/变基冲突弹层 ── -->
  {#if mergeConflict}
    <div class="update-overlay">
      <div class="update-modal">
        <div class="update-header">
          <h2 class="update-title">解决冲突</h2>
          <button
            class="btn-close"
            onclick={() => {
              mergeConflict = null;
              refresh();
            }}
            title="关闭"
          >
            ✕
          </button>
        </div>
        <ConflictView
          path={mergeConflict.repoPath}
          conflictFiles={mergeConflict.files}
          autostash={mergeConflict.autostash}
          onContinue={async () => {
            const mc = mergeConflict!;
            try {
              await invoke("continue_update_cmd", {
                path: mc.repoPath,
                autostash: mc.autostash,
                recurseSubmodules: false,
              });
              mergeConflict = null;
              await refresh();
            } catch (e) {
              error = String(e);
            }
          }}
          onAbort={async () => {
            if (!confirm("确定放弃本次整合？工作区将回到整合前的状态。"))
              return;
            const mc = mergeConflict!;
            try {
              await invoke("abort_update_cmd", {
                path: mc.repoPath,
                autostash: mc.autostash,
              });
              mergeConflict = null;
              await refresh();
            } catch (e) {
              error = String(e);
            }
          }}
        />
      </div>
    </div>
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
    background: #2a2a2a !important;
    border: 1px solid #444 !important;
    border-radius: 6px;
    color: #e4e4e4 !important;
    padding: 6px 10px !important;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
    cursor: pointer;
  }
  .current-path:hover:not(:disabled) {
    border-color: #0e639c !important;
    background: #303030 !important;
  }
  .current-path-empty {
    flex: 1;
    max-width: 360px;
    background: #2a2a2a !important;
    border: 1px dashed #555 !important;
    border-radius: 6px;
    color: #999 !important;
    padding: 6px 10px !important;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .current-path-empty:hover:not(:disabled) {
    border-color: #0e639c !important;
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

  /* ── 全部更新弹层 ── */
  .update-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .update-modal {
    background: #1e1e1e;
    border: 1px solid #444;
    border-radius: 8px;
    width: 540px;
    max-width: 92%;
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
  .remote-actions {
    display: flex;
    gap: 6px;
    margin-left: 4px;
    flex-shrink: 0;
  }
  .btn-remote {
    background: #2a2a2a;
    border: 1px solid #0e639c;
    border-radius: 6px;
    color: #9ecbff;
    cursor: pointer;
    font-size: 12px;
    padding: 5px 12px;
    white-space: nowrap;
  }
  .btn-remote:hover:not(:disabled) {
    background: #0e639c;
    color: #fff;
  }
  .btn-remote:disabled {
    opacity: 0.5;
    cursor: default;
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
    display: flex;
    align-items: flex-start;
    gap: 8px;
    background: #1d3a24;
    border-bottom: 1px solid #2b6a3b;
    padding: 8px 14px;
    margin: 0;
  }
  .op-message-text {
    flex: 1;
    color: #7ee29a;
    font-size: 12px;
    white-space: pre-wrap;
    min-width: 0;
  }
  .op-message-close {
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: #7ee29a;
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 0 2px;
    opacity: 0.7;
  }
  .op-message-close:hover {
    opacity: 1;
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
    padding: 0 0 10px; /* 顶部不留白:否则 sticky 的 .zone-title 吸顶时上方会漏出滚动内容 */
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
  .repo-branch {
    font-size: 11px;
    color: #aaa;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    flex-shrink: 0;
  }
  .repo-branch.detached {
    color: #f3b4b4;
    font-style: italic;
  }
  .repo-branch-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    cursor: pointer;
    padding: 1px 6px;
    margin-left: 4px;
    flex-shrink: 0;
    min-width: 0;
  }
  .repo-branch-btn:hover {
    background: #333;
    border-color: #0e639c;
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
  .amend-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: #aaa;
    margin-bottom: 8px;
    cursor: pointer;
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

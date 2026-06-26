<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { open, ask } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import UpdateView from "$lib/UpdateView.svelte";
  import HistoryView from "$lib/HistoryView.svelte";
  import PushDialog from "$lib/PushDialog.svelte";
  import DiffView from "$lib/DiffView.svelte";
  import FileTree from "$lib/FileTree.svelte";
  import FileViewer from "$lib/FileViewer.svelte";
  import {
    type ChangelistStore,
    type Changelist,
    CL_DEFAULT,
    loadStore as loadCLStore,
    saveStore as saveCLStore,
    ensureRepo as ensureRepoCL,
    syncFiles as syncCLFiles,
    clOf as clOfFile,
    newId as newCLId,
  } from "$lib/changelists";
  import ProjectPicker from "$lib/ProjectPicker.svelte";
  import FileHistory from "$lib/FileHistory.svelte";
  import BlameView from "$lib/BlameView.svelte";
  import BranchPicker from "$lib/BranchPicker.svelte";
  import ConflictView from "$lib/ConflictView.svelte";
  import Settings from "$lib/Settings.svelte";
  import UpdateBanner from "$lib/UpdateBanner.svelte";
  import StashView from "$lib/StashView.svelte";
  import TagView from "$lib/TagView.svelte";
  import Toast from "$lib/Toast.svelte";
  import "../lib/themes.css";

  // ── 外观设置接口（与 Rust AppSettings 外观字段对应） ──
  interface AppearanceSettings {
    theme: string;
    density: string;
    font_size: string;
    animations_enabled: boolean;
    scanline_enabled: boolean;
    glow_intensity: string;
  }

  function applyAppearance(s: AppearanceSettings) {
    const body = document.body;
    body.setAttribute("data-theme", s.theme || "neon-dark");
    body.setAttribute("data-density", s.density || "comfortable");
    body.setAttribute("data-font-size", s.font_size || "medium");
    body.setAttribute(
      "data-animations",
      s.animations_enabled ? "true" : "false",
    );
    body.setAttribute("data-scanline", s.scanline_enabled ? "true" : "false");
    body.setAttribute("data-glow", s.glow_intensity || "medium");
  }

  // 启动时加载外观设置
  onMount(async () => {
    try {
      commitThenPush = localStorage.getItem(COMMIT_PUSH_KEY) === "1";
    } catch {
      // localStorage 不可用:用默认值
    }
    clStore = loadCLStore();
    clLoaded = true;
    try {
      const s = await invoke<AppearanceSettings>("get_settings");
      applyAppearance(s);
    } catch {
      // 使用默认外观
    }
    // AI 提交助手:读取启用状态和 API Key 配置
    try {
      const s = await invoke<{
        ai_enabled: boolean;
        ai_api_key: string;
      }>("get_settings");
      aiEnabled = !!s.ai_enabled;
      aiConfigured = aiEnabled && !!s.ai_api_key?.trim();
    } catch {
      aiEnabled = false;
      aiConfigured = false;
    }
  });

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
  // 「更新后推送」:推送被拒(远端领先)时,按全局策略更新该仓并在成功后自动推送。
  let pushAfterSuccess = $state(false);
  let pushTargetPath = $state("");
  // 「全部推送」队列:被拒的仓逐个「更新后推送」,队首为当前正在处理的仓。
  let pushQueue = $state<RepoView[]>([]);
  let pushDialogRepo = $state<RepoView | null>(null); // Push 对话框目标仓(null=关闭)

  // ── Toast 通知(静默更新结果用) ──
  type ToastKind = "success" | "error" | "info";
  let toast = $state<{
    message: string;
    kind: ToastKind;
    duration: number;
  } | null>(null);

  function showToast(message: string, kind: ToastKind, duration = 3500) {
    toast = { message, kind, duration };
  }

  function closeToast() {
    toast = null;
  }
  // 文件查看器目标(null=关闭):整文件 + 相对 HEAD 的行内变更标记。
  let fileViewer = $state<{ repoPath: string; filePath: string } | null>(null);
  function openFileViewer(repoPath: string, filePath: string) {
    fileViewer = { repoPath, filePath };
  }
  let showFileHistory = $state(false); // 文件历史弹窗
  let fileHistoryPath = $state(""); // 文件历史查看的文件路径
  let fileHistoryRepoPath = $state(""); // 该文件所属仓库(主仓或子仓),History 子仓提交用
  let showBlame = $state(false); // blame 弹窗
  let blamePath = $state(""); // blame 查看的文件路径
  let blameRepoPath = $state(""); // 该文件所属仓库(主仓或子仓)
  let branchPickerRepo = $state<string | null>(null); // 哪个仓库的分支选择面板打开(null=关闭)
  let showSettings = $state(false); // 全局设置弹层
  let showStash = $state(false); // Stash 储藏管理弹层
  let showTags = $state(false); // Tag 管理弹层
  let showMore = $state(false); // 顶栏「⋯ 更多」下拉(收纳次要操作,给顶栏减负)
  interface StashRef {
    label: string;
  }
  interface MergeConflict {
    repoPath: string;
    files: string[];
    autostash: StashRef | null;
  }
  let mergeConflict = $state<MergeConflict | null>(null); // 合并/变基产生冲突时的 ConflictView 弹层
  interface BranchDiff {
    branch: string;
    files: FileDiff[];
  }
  let branchDiff = $state<BranchDiff | null>(null); // 分支↔工作区差异弹层(Show Diff with Working Tree)
  interface LogEntry {
    sha: string;
    full_sha: string;
    message: string;
    author: string;
    date: string;
  }
  interface CompareResult {
    branch: string;
    incoming: LogEntry[]; // 在所选分支、不在当前(分支领先当前)
    outgoing: LogEntry[]; // 在当前、不在所选分支(当前领先分支)
  }
  let compareResult = $state<CompareResult | null>(null); // 分支↔当前提交对比弹层(Compare with Current)
  let subCount = $derived(status?.submodules.length ?? 0); // 子仓库数(顶部「更新子仓库」据此启用)

  // 统一提交框(WebStorm 风格):一条提交信息应用于所有有暂存改动的仓库
  let commitMessage = $state("");
  // ── AI 提交助手:启用时按仓库分别持有 message(未启用 / amend 仍用上面的 commitMessage)──
  let commitMessages = $state<Record<string, string>>({});
  let aiEnabled = $state(false); // 是否启用了 AI(来自 settings)
  let aiConfigured = $state(false); // 启用且填了 Key
  let generating = $state(false); // 批量生成进行中
  let generatingRepo = $state<string | null>(null); // 当前正在生成的仓库(单/批量)
  let aiError = $state<Record<string, string>>({}); // per-repo 生成错误
  let amendMode = $state(false); // amend 模式:仅修改主仓库上次提交
  let commitResult = $state("");
  // 提交后自动推送(记住上次选择,存 localStorage)。
  const COMMIT_PUSH_KEY = "git-gui:commit-then-push";
  let commitThenPush = $state(false);
  function toggleCommitThenPush(on: boolean) {
    commitThenPush = on;
    try {
      localStorage.setItem(COMMIT_PUSH_KEY, on ? "1" : "0");
    } catch {
      // localStorage 不可用:仅本会话生效
    }
  }

  // ── Changelist(命名变更集):每仓库独立,分组未暂存文件,可按组提交。纯前端 localStorage ──
  let clStore = $state<ChangelistStore>({});
  // 新建/重命名变更集的内联输入:{repoPath, mode, id?, value}(null=未在编辑)。
  let clEditing = $state<{
    repoPath: string;
    mode: "new" | "rename";
    id?: string;
    value: string;
  } | null>(null);

  let clLoaded = $state(false);
  // 持久化(任一变更集状态变动即写回);加载完成前不写,避免用 {} 覆盖已存数据。
  $effect(() => {
    void clStore; // 建立对 clStore 重新赋值的依赖
    if (!clLoaded) return;
    saveCLStore(clStore);
  });

  // 把每个仓库当前未暂存文件同步进其变更集分配:新文件归活跃组、消失文件清理(幂等)。
  $effect(() => {
    for (const r of repos) {
      const cl = ensureRepoCL(clStore, r.path);
      syncCLFiles(
        cl,
        r.unstaged.map((f) => f.path),
      );
    }
  });

  // 仓库是否启用了变更集分组(有非默认分组才显示分组 UI,否则维持原扁平列表零打扰)。
  function clActive(repoPath: string): boolean {
    const r = clStore[repoPath];
    return !!r && r.lists.length > 1;
  }

  // 仓库的变更集分组(每组带其未暂存文件);仅返回有文件或非默认的组,避免空默认组占位。
  function clGroups(
    repo: RepoView,
  ): { list: Changelist; files: FileEntry[] }[] {
    const r = clStore[repo.path];
    if (!r) return [];
    return r.lists
      .map((list) => ({
        list,
        files: repo.unstaged.filter((f) => clOfFile(r, f.path) === list.id),
      }))
      .filter((g) => g.files.length > 0 || g.list.id !== CL_DEFAULT);
  }

  function clName(repoPath: string, id: string): string {
    return clStore[repoPath]?.lists.find((l) => l.id === id)?.name ?? "变更集";
  }

  // 新建变更集:打开内联输入。
  function startNewChangelist(repoPath: string) {
    clEditing = { repoPath, mode: "new", value: "" };
  }
  function startRenameChangelist(repoPath: string, id: string) {
    clEditing = {
      repoPath,
      mode: "rename",
      id,
      value: clName(repoPath, id),
    };
  }
  function confirmClEditing() {
    if (!clEditing) return;
    const name = clEditing.value.trim();
    if (!name) {
      clEditing = null;
      return;
    }
    const r = ensureRepoCL(clStore, clEditing.repoPath);
    if (clEditing.mode === "new") {
      const id = newCLId();
      r.lists.push({ id, name });
      r.activeId = id; // 新建即设为活跃,后续新改动归入它
    } else if (clEditing.id) {
      const l = r.lists.find((x) => x.id === clEditing!.id);
      if (l) l.name = name;
    }
    clEditing = null;
  }

  function setActiveChangelist(repoPath: string, id: string) {
    const r = ensureRepoCL(clStore, repoPath);
    r.activeId = id;
  }

  // 删除变更集:其文件移回默认组(不丢改动),活跃组若被删则回默认。
  async function deleteChangelist(repoPath: string, id: string) {
    if (id === CL_DEFAULT) return;
    const r = clStore[repoPath];
    if (!r) return;
    if (
      !(await ask(`删除变更集「${clName(repoPath, id)}」？其文件移回默认组。`, {
        title: "删除变更集",
      }))
    )
      return;
    for (const p of Object.keys(r.assign)) {
      if (r.assign[p] === id) r.assign[p] = CL_DEFAULT;
    }
    r.lists = r.lists.filter((l) => l.id !== id);
    if (r.activeId === id) r.activeId = CL_DEFAULT;
  }

  function moveFileToChangelist(
    repoPath: string,
    filePath: string,
    id: string,
  ) {
    const r = ensureRepoCL(clStore, repoPath);
    r.assign[filePath] = id;
  }

  // 提交单个变更集:只提交该组文件(工作区内容,经 commit_paths 隔离其它已暂存改动)。
  async function commitChangelist(repo: RepoView, files: FileEntry[]) {
    const message = commitMessage.trim();
    if (!message || files.length === 0) return;
    operating = true;
    error = "";
    commitResult = "";
    try {
      await invoke<string>("repo_commit_paths", {
        path: repo.path,
        message,
        paths: files.map((f) => f.path),
      });
      commitMessage = "";
      commitResult = `已提交变更集（${files.length} 个文件）`;
      await refresh();
      if (commitThenPush) await pushAfterCommit([repo]);
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }
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

  // StashView 可选文件的列表（仅主仓库的未暂存文件）
  let stashableFiles = $derived(
    repos
      .filter((r) => r.path === path)
      .flatMap((r) => r.unstaged.map((f) => ({ path: f.path }))),
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
    if (!(await ask(msg, { title: "丢弃改动", kind: "warning" }))) return;
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

  // 未配置时引导去设置(用 plugin-dialog,不用原生 confirm)。
  async function ensureAiConfigured(): Promise<boolean> {
    if (aiConfigured) return true;
    const ok = await ask(
      "请先在「设置 → AI 提交助手」中填写 API Key 并启用。是否现在打开设置?",
      {
        title: "未配置 AI 提交助手",
        okLabel: "打开设置",
        cancelLabel: "取消",
      },
    );
    if (ok) showSettings = true;
    return false;
  }

  // 为单个仓库生成提交信息,结果填入 commitMessages(可编辑,不自动提交)。
  async function generateForRepo(r: { path: string }) {
    if (!(await ensureAiConfigured())) return;
    generatingRepo = r.path;
    try {
      const msg = await invoke<string>("ai_generate_commit_message", {
        path: r.path,
      });
      commitMessages[r.path] = msg;
      delete aiError[r.path];
    } catch (e) {
      aiError[r.path] = String(e);
    } finally {
      generatingRepo = null;
    }
  }

  // 顺序为所有暂存仓库生成(避免触发 API 限流),逐个填入。
  async function generateAll() {
    if (!(await ensureAiConfigured())) return;
    generating = true;
    try {
      for (const r of stagedRepos) {
        generatingRepo = r.path;
        try {
          const msg = await invoke<string>("ai_generate_commit_message", {
            path: r.path,
          });
          commitMessages[r.path] = msg;
          delete aiError[r.path];
        } catch (e) {
          aiError[r.path] = String(e);
        }
      }
      generatingRepo = null;
    } finally {
      generating = false;
    }
  }

  // 提交所有有暂存改动的仓库。
  // - 启用 AI 且非 amend:每仓库用各自 commitMessages[path](空则跳过)
  // - 否则(未启用 / amend):所有仓库共用 commitMessage(原逻辑,零改动)
  async function doCommit() {
    const targets = [...stagedRepos].sort(
      (a, b) => Number(a.isMain) - Number(b.isMain),
    );

    if (aiEnabled && !amendMode) {
      if (totalStaged === 0) return;
      operating = true;
      error = "";
      commitResult = "";
      try {
        let count = 0;
        for (const r of targets) {
          const message = commitMessages[r.path] ?? "";
          if (!message.trim()) continue;
          await invoke<string>("repo_commit", {
            path: r.path,
            message,
            amend: false,
          });
          count++;
        }
        if (count === 0) {
          commitResult = "没有可提交的信息(请先填写或生成提交信息)";
        } else {
          commitMessages = {};
          aiError = {};
          commitResult = `已提交 ${count} 个仓库`;
          await refresh();
          if (commitThenPush) await pushAfterCommit(targets);
        }
      } catch (e) {
        error = String(e);
      } finally {
        operating = false;
      }
      return;
    }

    // ── 原逻辑:未启用 AI / amend 模式 ──
    if (totalStaged === 0 || !commitMessage.trim()) return;
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
      if (commitThenPush) await pushAfterCommit(targets);
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
  }

  // 提交后推送:逐个推送刚提交的仓库,结果就地汇总到 opMessage(不阻塞、不吞错)。
  // 远端领先(NonFastForward)只如实提示,不自动开「更新后推送」流程(那走独立推送按钮)。
  async function pushAfterCommit(targets: RepoView[]) {
    const lines: string[] = [];
    for (const r of targets) {
      try {
        const out = await invoke<PushOutcome>("repo_push", { path: r.path });
        lines.push(`${r.label}：${pushMsg(out)}`);
      } catch (e) {
        lines.push(`${r.label}：推送失败 ${String(e)}`);
      }
    }
    opMessage = lines.join("\n");
    await refresh();
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
  async function openUpdateAll() {
    if (!status) return;
    updateSubmodules = status.submodules;
    updateTitle = "全部更新";
    updateSubsOnly = false;
    error = "";
    opMessage = "";
    await triggerUpdate(path);
  }

  // 仅更新主仓库(弹层不带子仓库)。
  async function openUpdateMain() {
    updateSubmodules = [];
    updateTitle = "更新主仓库";
    updateSubsOnly = false;
    error = "";
    opMessage = "";
    await triggerUpdate(path);
  }

  // 顶部「更新子仓库」:对所有子仓库按状态自动分流(未初始化→init,已初始化→on-branch 更新),不动主仓。
  async function openUpdateSubs() {
    if (!status) return;
    updateSubmodules = status.submodules;
    updateTitle = "更新子仓库";
    updateSubsOnly = true;
    error = "";
    opMessage = "";
    await triggerUpdate(path);
  }

  // 行内「更新」:单个子仓在当前分支更新,复用 UpdateView 冲突流程。
  async function openUpdateSub(repo: RepoView) {
    if (!status || !repo.subRelPath) return;
    const sub = status.submodules.find((s) => s.path === repo.subRelPath);
    if (!sub) return;
    updateSubmodules = [sub];
    updateTitle = `更新子仓库 · ${repo.label}`;
    updateSubsOnly = true;
    error = "";
    opMessage = "";
    await triggerUpdate(path);
  }

  // 统一更新入口:读 silent_update 设置,开启则后台静默执行,否则弹 UpdateView。
  // updateSubmodules / updateTitle / updateSubsOnly 已由调用方设置好。
  async function triggerUpdate(targetPath: string) {
    try {
      const s = await invoke<{ silent_update: boolean }>("get_settings");
      if (s.silent_update) {
        void silentUpdateRun({
          path: targetPath,
          submodules: updateSubmodules,
          subsOnly: updateSubsOnly,
          title: updateTitle,
        });
        return;
      }
    } catch {
      // 读设置失败:退回到弹窗模式
    }
    showUpdate = true;
  }

  // 静默更新:后台执行,不弹 UpdateView。成功 toast 提示;失败 toast 报错;
  // 冲突仍弹 UpdateView(其 onMount 会通过 resume_conflicts 进冲突解决)。
  async function silentUpdateRun(opts: {
    path: string;
    submodules: { name: string; path: string; status: string }[];
    subsOnly: boolean;
    title: string;
  }) {
    operating = true;
    showToast(opts.title + "…", "info", 0);
    let strategy: "Merge" | "Rebase" = "Merge";
    let ignoreWhitespace = true;
    try {
      const s = await invoke<{
        update_strategy: "Merge" | "Rebase";
        ignore_whitespace: boolean;
      }>("get_settings");
      strategy = s.update_strategy;
      ignoreWhitespace = s.ignore_whitespace;
    } catch {
      // 读设置失败用默认值
    }
    const options = {
      strategy,
      ignore_whitespace: ignoreWhitespace,
      recurse_submodules: false,
    };

    const lines: string[] = [];
    let hasWarn = false;

    try {
      // 主仓更新(仅子仓模式跳过)。
      if (!opts.subsOnly) {
        const outcome = await invoke<
          | "AlreadyUpToDate"
          | "Resolved"
          | { FastForwarded: { commits: number } }
          | { Integrated: { commits: number; strategy: string } }
          | { Conflicted: { files: string[]; autostash: unknown } }
          | { StashRestoreConflict: { files: string[] } }
          | { SubmoduleSyncFailed: { error: string } }
        >("execute_update", {
          path: opts.path,
          opId: crypto.randomUUID(),
          options,
        });
        if (typeof outcome === "object" && "Conflicted" in outcome) {
          // 主仓冲突:弹 UpdateView 走冲突解决。
          closeToast();
          showUpdate = true;
          return;
        }
        const m = describeMainOutcome(outcome);
        if (m) {
          lines.push(m);
          if (m.startsWith("警告") || m.startsWith("冲突")) hasWarn = true;
        }
      }

      // 逐个更新子仓(同 UpdateView.processSubmodulesFrom 逻辑)。
      for (const sub of opts.submodules) {
        if (sub.status === "Uninitialized") {
          try {
            await invoke("repo_submodule_update", {
              path: opts.path,
              subPath: sub.path,
            });
            lines.push(`${sub.path}:已初始化`);
          } catch (e) {
            lines.push(`${sub.path}:初始化失败 ${String(e)}`);
            hasWarn = true;
          }
          continue;
        }
        const r = await invoke<
          | "UpToDate"
          | "SyncedToRecorded"
          | "SkippedNoUpstream"
          | "StashConflict"
          | { Updated: { commits: number } }
          | {
              Conflicted: {
                repo_path: string;
                files: string[];
                autostash: unknown;
              };
            }
        >("repo_update_submodule_on_branch", {
          path: opts.path,
          subPath: sub.path,
          options,
        });
        if (typeof r === "object" && "Conflicted" in r) {
          // 子仓冲突:弹 UpdateView(其会重跑,主仓 AlreadyUpToDate,冲突子仓进 ConflictView)。
          closeToast();
          showUpdate = true;
          return;
        }
        const d = describeSubResult(r);
        lines.push(`${sub.path}:${d}`);
        if (d.includes("跳过") || d.includes("冲突")) hasWarn = true;
      }

      showToast(
        (hasWarn ? "更新完成(有警告):\n" : "更新完成:\n") + lines.join("\n"),
        "success",
        hasWarn ? 6000 : 3500,
      );
    } catch (e) {
      showToast(`${opts.title}失败:${String(e)}`, "error", 6000);
    } finally {
      operating = false;
      await refresh();
    }
  }

  // 把主仓 UpdateOutcome 映射为一行中文描述;null 表示无需展示(如已是最新可省略)。
  function describeMainOutcome(
    o:
      | string
      | { FastForwarded: { commits: number } }
      | { Integrated: { commits: number; strategy: string } }
      | { StashRestoreConflict: { files: string[] } }
      | { SubmoduleSyncFailed: { error: string } }
      | Record<string, unknown>,
  ): string | null {
    if (typeof o === "string") {
      if (o === "AlreadyUpToDate") return "主仓库已是最新";
      if (o === "Resolved") return "主仓库冲突已解决";
      return o;
    }
    if ("FastForwarded" in o) {
      const d = (o as { FastForwarded: { commits: number } }).FastForwarded;
      return `主仓库快进 ${d.commits} 个提交`;
    }
    if ("Integrated" in o) {
      const d = (o as { Integrated: { commits: number; strategy: string } })
        .Integrated;
      return `主仓库整合 ${d.commits} 个提交(${
        d.strategy === "Rebase" ? "变基" : "合并"
      })`;
    }
    if ("StashRestoreConflict" in o) {
      const d = (o as { StashRestoreConflict: { files: string[] } })
        .StashRestoreConflict;
      return `警告:stash 还原冲突,${d.files.length} 个文件需手动处理`;
    }
    if ("SubmoduleSyncFailed" in o) {
      const d = (o as { SubmoduleSyncFailed: { error: string } })
        .SubmoduleSyncFailed;
      return `警告:子仓库同步失败 - ${d.error}`;
    }
    return null;
  }

  // 把子仓 SubmoduleUpdate 映射为一行简短中文描述。
  function describeSubResult(
    r: string | { Updated: { commits: number } } | Record<string, unknown>,
  ): string {
    if (typeof r === "string") {
      switch (r) {
        case "UpToDate":
          return "已是最新";
        case "SyncedToRecorded":
          return "已同步到记录提交";
        case "SkippedNoUpstream":
          return "跳过:无上游分支";
        case "StashConflict":
          return "stash 还原冲突,需手动处理";
        default:
          return r;
      }
    }
    if ("Updated" in r) {
      const d = (r as { Updated: { commits: number } }).Updated;
      return `已更新 ${d.commits} 个提交`;
    }
    return "未知结果";
  }

  // repo_push 的返回(PushOutcome,外部 tagged 单元枚举 → 字符串)。
  type PushOutcome = "Success" | "NoUpstream" | "NonFastForward";

  function pushMsg(r: PushOutcome): string {
    if (r === "Success") return "推送成功";
    if (r === "NoUpstream") return "跳过:无 upstream";
    return "远端领先,待更新后推送";
  }

  // 推送单个仓库:打开 Push 对话框(待推预览 + force-with-lease + 进度),替代直接推。
  function openPushDialog(repo: RepoView) {
    pushDialogRepo = repo;
  }

  // 当前全局更新策略的中文名(合并/变基),用于确认文案。
  async function globalStrategyLabel(): Promise<string> {
    try {
      const s = await invoke<{ update_strategy: "Merge" | "Rebase" }>(
        "get_settings",
      );
      return s.update_strategy === "Rebase" ? "变基" : "合并";
    } catch {
      return "合并"; // 读设置失败:实际策略以 UpdateView 载入的全局设置为准
    }
  }

  // 打开 UpdateView 走「更新后推送」:按全局策略更新该仓,成功后自动推送。
  function openUpdateThenPush(repo: RepoView) {
    pushAfterSuccess = true;
    pushTargetPath = repo.path;
    updateSubmodules = [];
    updateTitle = `更新后推送 · ${repo.label}`;
    updateSubsOnly = false;
    showUpdate = true;
  }

  // 队列:从队首取下一个仓走「更新后推送」;队空则收尾关闭弹层。
  function advancePushQueue() {
    const next = pushQueue[0];
    if (!next) {
      showUpdate = false;
      pushAfterSuccess = false;
      return;
    }
    openUpdateThenPush(next);
  }

  // UpdateView 成功完成(含推送)→ 弹出队首,推进到下一个。
  function onPushQueueItemDone() {
    pushQueue = pushQueue.slice(1);
    advancePushQueue();
  }

  // 全部推送:遍历主仓库 + 各子仓库,各自 push,逐个汇总(无 upstream / 失败不中断)。
  // 被拒于"远端领先"的仓收集成队列,批量推送结束后逐个「更新后推送」。
  async function doPushAll() {
    if (repos.length === 0) return;
    operating = true;
    error = "";
    opMessage = "";
    // 预读静默设置:静默模式下 NonFastForward 仓由后台更新+推送,
    // 不生成"远端领先"中间行,顶部提示改由 toast 汇总最终结果。
    let silent = false;
    try {
      const s = await invoke<{ silent_update: boolean }>("get_settings");
      silent = s.silent_update;
    } catch {
      // 读设置失败:退回到确认 + 弹窗模式
    }
    const lines: string[] = [];
    const rejected: RepoView[] = [];
    for (const r of repos) {
      try {
        const out = await invoke<PushOutcome>("repo_push", { path: r.path });
        if (out === "NonFastForward") {
          rejected.push(r);
          // 静默模式:该仓结果由后续更新+推送替代,不生成中间行。
          if (!silent) lines.push(`${r.label}：${pushMsg(out)}`);
        } else {
          lines.push(`${r.label}：${pushMsg(out)}`);
        }
      } catch (e) {
        lines.push(`${r.label}：${String(e)}`);
      }
    }
    operating = false;
    await refresh();
    if (rejected.length === 0) {
      // 无远端领先:直接在顶部显示推送结果(静默/非静默一致)。
      opMessage = lines.join("\n");
      return;
    }

    if (silent) {
      // 静默模式:不在顶部显示中间结果,改由 toast 汇总更新后推送的完整结果。
      await silentUpdateThenPushBatch(rejected, lines);
      return;
    }

    // 非静默:顶部显示推送结果 + 一次确认 → 逐个「更新后推送」。
    opMessage = lines.join("\n");
    const strat = await globalStrategyLabel();
    const names = rejected.map((r) => r.label).join("、");
    if (
      await ask(
        `以下仓库远端领先:${names}\n按当前全局策略「${strat}」逐个更新后推送?`,
        { title: "更新后推送" },
      )
    ) {
      pushQueue = rejected;
      advancePushQueue();
    }
  }

  // 静默批量「更新后推送」:逐个仓后台更新+推送,结果汇总到 lines 并以 toast 展示。
  // 遇冲突弹 UpdateView(pushAfterSuccess=true,解决后自动推送)并停止批量,剩余仓需重试。
  async function silentUpdateThenPushBatch(
    rejected: RepoView[],
    lines: string[],
  ) {
    showToast("正在更新后推送…", "info", 0);
    const resultLines: string[] = [];
    let conflictHit = false;
    for (const repo of rejected) {
      const line = await silentUpdateThenPushOne(repo);
      if (line === null) {
        // 冲突:UpdateView 已弹出,停止批量(避免连环弹窗)。
        resultLines.push(`${repo.label}：冲突,已打开解决窗口`);
        conflictHit = true;
        break;
      }
      resultLines.push(`${repo.label}：${line}`);
    }
    closeToast();
    if (conflictHit) {
      // 冲突弹窗已开,部分结果就地显示在 opMessage。
      opMessage = lines.concat(resultLines).join("\n");
    } else {
      showToast(
        "推送完成:\n" + lines.concat(resultLines).join("\n"),
        "success",
        5000,
      );
    }
    await refresh();
  }

  // 静默「更新后推送」单仓:按全局策略更新,成功后推送,冲突弹 UpdateView。
  // 返回结果行;冲突返回 null(调用方应停止批量)。
  async function silentUpdateThenPushOne(
    repo: RepoView,
  ): Promise<string | null> {
    let strategy: "Merge" | "Rebase" = "Merge";
    let ignoreWhitespace = true;
    try {
      const s = await invoke<{
        update_strategy: "Merge" | "Rebase";
        ignore_whitespace: boolean;
      }>("get_settings");
      strategy = s.update_strategy;
      ignoreWhitespace = s.ignore_whitespace;
    } catch {
      // 读设置失败用默认值
    }
    const options = {
      strategy,
      ignore_whitespace: ignoreWhitespace,
      recurse_submodules: false,
    };

    try {
      const outcome = await invoke<
        | "AlreadyUpToDate"
        | "Resolved"
        | { FastForwarded: { commits: number } }
        | { Integrated: { commits: number; strategy: string } }
        | { Conflicted: { files: string[]; autostash: unknown } }
        | { StashRestoreConflict: { files: string[] } }
        | { SubmoduleSyncFailed: { error: string } }
      >("execute_update", {
        path: repo.path,
        opId: crypto.randomUUID(),
        options,
      });
      if (typeof outcome === "object" && "Conflicted" in outcome) {
        // 冲突:弹 UpdateView(pushAfterSuccess=true,解决后自动推送)。
        // 清空 pushQueue 避免UpdateView onFinished 推进连环弹窗。
        pushQueue = [];
        pushAfterSuccess = true;
        pushTargetPath = repo.path;
        updateSubmodules = [];
        updateTitle = `更新后推送 · ${repo.label}`;
        updateSubsOnly = false;
        showUpdate = true;
        return null;
      }
      // 更新成功 → 推送。
      const r = await invoke<PushOutcome>("repo_push", { path: repo.path });
      if (r === "Success") return "更新后推送成功";
      if (r === "NoUpstream") return "更新成功,但无 upstream 未推送";
      return "更新成功,但推送时远端再次领先";
    } catch (e) {
      return `失败:${String(e)}`;
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

<svelte:window onkeydown={(e) => e.key === "Escape" && (showMore = false)} />

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

  <!-- ── 新版本提醒条(有更新时显示) ── -->
  <UpdateBanner />

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
      <div class="remote-actions">
        <!-- 日常主操作常驻可见,不埋进菜单 -->
        <button
          class="btn-remote"
          disabled={loading || operating}
          title="更新主仓库，并把各子仓库拉取到各自远程最新"
          onclick={openUpdateAll}>全部更新</button
        >
        <button
          class="btn-remote"
          disabled={loading || operating || !repos.some((r) => r.ahead > 0)}
          title="推送主仓库及各子仓库到各自远程"
          onclick={doPushAll}>全部推送</button
        >
        <!-- 次要操作收进「⋯ 更多」,给顶栏减负 -->
        <div class="more-wrap">
          <button
            class="btn-remote"
            class:active={showMore}
            disabled={loading || operating}
            title="更多操作：更新子仓库 / Stash / Tags"
            onclick={() => (showMore = !showMore)}>⋯ 更多</button
          >
          {#if showMore}
            <!-- 点击空白处关闭菜单 -->
            <button
              class="more-backdrop"
              aria-label="关闭菜单"
              tabindex="-1"
              onclick={() => (showMore = false)}
            ></button>
            <div class="more-menu" role="menu">
              <button
                class="more-item"
                role="menuitem"
                disabled={subCount === 0}
                title="未初始化的子仓库执行 init，已初始化的在各自当前分支更新（留在原分支）"
                onclick={() => {
                  showMore = false;
                  openUpdateSubs();
                }}>更新子仓库</button
              >
              <button
                class="more-item"
                role="menuitem"
                title="储藏管理：把工作区改动暂存起来，或应用/弹出/丢弃已有储藏（git stash）"
                onclick={() => {
                  showMore = false;
                  showStash = true;
                }}>Stash</button
              >
              <button
                class="more-item"
                role="menuitem"
                title="标签管理：列出/删除/推送 tag，或在 HEAD 创建 tag（git tag）"
                onclick={() => {
                  showMore = false;
                  showTags = true;
                }}>Tags</button
              >
            </div>
          {/if}
        </div>
      </div>
    {/if}
    <button
      class="btn-settings"
      onclick={() => (showSettings = true)}
      title="设置:更新策略等全局配置"
      aria-label="设置">⚙</button
    >
  </header>

  <!-- ── 标签栏 ── -->
  <nav class="tab-bar">
    <button
      class="tab-btn"
      class:tab-active={tab === "changes"}
      onclick={() => (tab = "changes")}
      title="Changes:查看各仓库工作区改动,暂存/丢弃文件并统一提交"
    >
      Changes
    </button>
    <button
      class="tab-btn"
      class:tab-active={tab === "history"}
      onclick={() => (tab = "history")}
      title="History:浏览提交历史与分支图,查看每个提交的文件改动"
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
              <h2 class="zone-title zone-unstaged">
                <span class="zone-icon">—</span>
                未暂存
                <span class="zone-badge">{totalUnstaged}</span>
              </h2>
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
                        class="sub-btn sub-btn-update"
                        disabled={operating}
                        title="更新主仓库:走 update 流程,可处理冲突"
                        onclick={openUpdateMain}>↓ 更新</button
                      >
                      <button
                        class="sub-btn sub-btn-push"
                        disabled={operating || repo.ahead === 0}
                        title="推送主仓库当前分支到远程"
                        onclick={() => openPushDialog(repo)}>↑ 推送</button
                      >
                    </div>
                  {:else if repo.subStatus !== "Uninitialized"}
                    <div class="repo-manage">
                      <button
                        class="sub-btn sub-btn-update"
                        disabled={operating}
                        title="在当前分支上更新（留在原分支）"
                        onclick={() => openUpdateSub(repo)}>↓ 更新</button
                      >
                      <button
                        class="sub-btn sub-btn-push"
                        disabled={operating || repo.ahead === 0}
                        title="推送子仓库当前分支到远程"
                        onclick={() => openPushDialog(repo)}>↑ 推送</button
                      >
                      <button
                        class="sub-btn sub-btn-sync"
                        disabled={operating}
                        title="同步子仓库 URL 配置（sync）"
                        onclick={() => syncSubmodule(repo)}>⇄ 同步</button
                      >
                    </div>
                  {/if}
                  {#if repo.subStatus === "Uninitialized"}
                    <p class="muted">未初始化 — 点顶部「更新子仓库」</p>
                  {:else if repo.unstaged.length === 0 && !clActive(repo.path)}
                    <p class="muted">无改动</p>
                  {:else}
                    <!-- 变更集工具栏:活跃组切换 + 新建 -->
                    <div class="cl-bar">
                      {#if clActive(repo.path)}
                        <span class="cl-bar-label">活跃</span>
                        <select
                          class="cl-active-select"
                          value={clStore[repo.path]?.activeId}
                          title="新改动归入活跃变更集"
                          onchange={(e) =>
                            setActiveChangelist(
                              repo.path,
                              e.currentTarget.value,
                            )}
                        >
                          {#each clStore[repo.path]?.lists ?? [] as l (l.id)}
                            <option value={l.id}>{l.name}</option>
                          {/each}
                        </select>
                      {/if}
                      <button
                        class="cl-add-btn"
                        disabled={operating}
                        title="新建变更集(分组未暂存改动,可按组提交)"
                        onclick={() => startNewChangelist(repo.path)}
                        >＋ 变更集</button
                      >
                    </div>

                    {#if clEditing && clEditing.repoPath === repo.path}
                      <div class="cl-edit">
                        <!-- svelte-ignore a11y_autofocus -->
                        <input
                          class="cl-edit-input"
                          placeholder="变更集名称"
                          autofocus
                          bind:value={clEditing.value}
                          onkeydown={(e) => {
                            if (e.key === "Enter") confirmClEditing();
                            else if (e.key === "Escape") clEditing = null;
                          }}
                        />
                        <button class="cl-edit-ok" onclick={confirmClEditing}
                          >{clEditing.mode === "new" ? "新建" : "改名"}</button
                        >
                        <button
                          class="cl-edit-cancel"
                          onclick={() => (clEditing = null)}>取消</button
                        >
                      </div>
                    {/if}

                    {#if clActive(repo.path)}
                      {#each clGroups(repo) as g (g.list.id)}
                        <div class="cl-group">
                          <div class="cl-group-head">
                            <span class="cl-name">{g.list.name}</span>
                            {#if g.list.id === clStore[repo.path]?.activeId}
                              <span class="cl-active-tag">活跃</span>
                            {/if}
                            <span class="cl-count">{g.files.length}</span>
                            <button
                              class="cl-commit-btn"
                              disabled={operating ||
                                !commitMessage.trim() ||
                                g.files.length === 0}
                              title="只提交此变更集的文件(忽略其它已暂存改动)"
                              onclick={() => commitChangelist(repo, g.files)}
                              >提交此变更集</button
                            >
                            {#if g.list.id !== CL_DEFAULT}
                              <button
                                class="cl-mini"
                                title="重命名"
                                onclick={() =>
                                  startRenameChangelist(repo.path, g.list.id)}
                                >✎</button
                              >
                              <button
                                class="cl-mini"
                                title="删除(文件移回默认)"
                                onclick={() =>
                                  deleteChangelist(repo.path, g.list.id)}
                                >×</button
                              >
                            {/if}
                          </div>
                          {#if g.files.length > 0}
                            <FileTree
                              files={g.files}
                              selectedPath={selKey(repo, "unstaged")}
                              kind="unstaged"
                              {operating}
                              onSelect={(f) =>
                                selectFile(repo.path, f, "unstaged")}
                              onStage={(p) => stagePaths(repo.path, p)}
                              onUnstage={(p) => unstagePaths(repo.path, p)}
                              onDiscard={(p) => discardPaths(repo.path, p)}
                              onView={(f) => openFileViewer(repo.path, f.path)}
                              moveTargets={clStore[repo.path]?.lists ?? []}
                              clOf={(f) =>
                                clOfFile(clStore[repo.path]!, f.path)}
                              onMove={(f, id) =>
                                moveFileToChangelist(repo.path, f.path, id)}
                            />
                          {:else}
                            <p class="cl-empty">（空 — 用文件右侧下拉移入)</p>
                          {/if}
                        </div>
                      {/each}
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
                        onView={(f) => openFileViewer(repo.path, f.path)}
                      />
                    {/if}
                  {/if}
                </div>
              {/each}
            </section>

            <!-- 已暂存区:仅列出有已暂存改动的仓库,作为待提交预览 -->
            {#if totalStaged > 0}
              <section class="zone">
                <h2 class="zone-title zone-staged">
                  <span class="zone-icon">✓</span>
                  已暂存
                  <span class="zone-badge">{totalStaged}</span>
                </h2>
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
                        onView={(f) => openFileViewer(repo.path, f.path)}
                      />
                    </div>
                  {/if}
                {/each}
              </section>
            {/if}

            <!-- 冲突(主仓库) -->
            {#if status.conflicted.length}
              <section class="zone">
                <h2 class="zone-title zone-conflict">
                  <span class="zone-icon">!</span>
                  冲突
                  <span class="zone-badge">{status.conflicted.length}</span>
                </h2>
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
            {#if aiEnabled && !amendMode && totalStaged > 0}
              <!-- ── AI 多框列表:每仓库独立 message + 生成按钮 ── -->
              <div class="ai-commit-list">
                <button
                  class="btn-ai-all"
                  disabled={generating || operating}
                  onclick={generateAll}
                  >{generating
                    ? `生成中…${generatingRepo ? `(${generatingRepo})` : ""}`
                    : "✨ 全部生成"}</button
                >
                {#each stagedRepos as r (r.path)}
                  <div class="ai-commit-row">
                    <div class="ai-row-head">
                      <span>{r.label}</span>
                      <button
                        class="ai-gen"
                        disabled={generating || operating}
                        onclick={() => generateForRepo(r)}
                        >{generatingRepo === r.path
                          ? "生成中…"
                          : "✨ 生成"}</button
                      >
                    </div>
                    <textarea
                      bind:value={commitMessages[r.path]}
                      placeholder="提交信息(可生成后编辑)"
                      rows={2}
                      disabled={operating}></textarea>
                    {#if aiError[r.path]}
                      <p class="ai-err">{aiError[r.path]}</p>
                    {/if}
                  </div>
                {/each}
                <button
                  class="btn-commit"
                  disabled={operating}
                  onclick={doCommit}
                  title="把每个仓库的暂存改动各自提交(统一提交)"
                  >提交全部（{totalStaged} 个文件）</button
                >
              </div>
            {:else if totalStaged === 0 && !amendMode}
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
                {#if !amendMode}
                  <label class="push-toggle" title="提交成功后自动推送到远程">
                    <input
                      type="checkbox"
                      checked={commitThenPush}
                      disabled={operating}
                      onchange={(e) =>
                        toggleCommitThenPush(e.currentTarget.checked)}
                    />
                    提交后推送
                  </label>
                {/if}
                <button
                  class="btn-commit"
                  disabled={operating || !commitMessage.trim()}
                  onclick={amendMode ? doAmend : doCommit}
                  title={amendMode
                    ? "用当前消息修改主仓库的上次提交（git commit --amend）"
                    : commitThenPush
                      ? "提交并推送：提交到所有有暂存内容的仓库，成功后自动推送"
                      : "把暂存的改动提交到所有有暂存内容的仓库（统一提交）"}
                  >{amendMode
                    ? "修改主仓库上次提交"
                    : commitThenPush
                      ? `提交并推送（${totalStaged} 个文件）`
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
              compact
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
                fileHistoryRepoPath = selectedRepoPath ?? path;
                showFileHistory = true;
              }}
              onBlame={(filePath) => {
                blamePath = filePath;
                blameRepoPath = selectedRepoPath ?? path;
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
      submodules={status?.submodules ?? []}
      onFileHistory={(filePath, repoPath) => {
        fileHistoryPath = filePath;
        fileHistoryRepoPath = repoPath;
        showFileHistory = true;
      }}
    />
  {/if}

  <!-- ── 全部更新弹层(主仓库 update + 各子仓库 update --remote) ── -->
  {#if showUpdate && status}
    <div class="update-overlay">
      <div class="update-modal">
        <!-- key 绑定目标仓:队列推进时(pushTargetPath 变)强制重新挂载,重跑 onMount。 -->
        {#key pushAfterSuccess ? pushTargetPath : path}
          <UpdateView
            path={pushAfterSuccess ? pushTargetPath : path}
            submodules={updateSubmodules}
            title={updateTitle}
            subsOnly={updateSubsOnly}
            {pushAfterSuccess}
            onRefresh={refresh}
            onClose={() => {
              showUpdate = false;
              pushAfterSuccess = false;
              pushQueue = []; // 取消/放弃:停止整个队列
            }}
            onFinished={pushQueue.length > 0 ? onPushQueueItemDone : undefined}
          />
        {/key}
      </div>
    </div>
  {/if}

  <!-- ── Push 对话框(待推预览 + force-with-lease + 进度) ── -->
  {#if pushDialogRepo}
    <PushDialog
      path={pushDialogRepo.path}
      label={pushDialogRepo.label}
      onClose={(pushed) => {
        pushDialogRepo = null;
        if (pushed) void refresh();
      }}
    />
  {/if}

  <!-- ── Toast 通知(静默更新结果) ── -->
  {#if toast}
    <Toast
      message={toast.message}
      kind={toast.kind}
      duration={toast.duration}
      onClose={closeToast}
    />
  {/if}

  <!-- ── 文件查看器(整文件 + 行内变更标记) ── -->
  {#if fileViewer}
    <FileViewer
      repoPath={fileViewer.repoPath}
      filePath={fileViewer.filePath}
      onClose={() => (fileViewer = null)}
    />
  {/if}

  <!-- ── 文件历史弹窗 ── -->
  {#if showFileHistory}
    <FileHistory
      path={fileHistoryRepoPath}
      filePath={fileHistoryPath}
      onClose={() => (showFileHistory = false)}
    />
  {/if}

  {#if showBlame}
    <BlameView
      path={blameRepoPath}
      filePath={blamePath}
      onClose={() => (showBlame = false)}
      onViewFile={() => openFileViewer(blameRepoPath, blamePath)}
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
      onShowDiff={(d) => {
        branchPickerRepo = null;
        branchDiff = d;
      }}
      onCompare={(d) => {
        branchPickerRepo = null;
        compareResult = d;
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
            if (
              !(await ask("确定放弃本次整合？工作区将回到整合前的状态。", {
                title: "放弃整合",
                kind: "warning",
              }))
            )
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

  <!-- ── 分支 ↔ 工作区差异弹层(Show Diff with Working Tree) ── -->
  {#if branchDiff}
    <div class="update-overlay">
      <div class="branch-diff-modal">
        <div class="bd-header">
          <h2 class="bd-title">
            <span class="bd-branch">{branchDiff.branch}</span> ↔ 工作区
          </h2>
          <button
            class="bd-close"
            onclick={() => (branchDiff = null)}
            title="关闭">✕</button
          >
        </div>
        {#if branchDiff.files.length === 0}
          <p class="diff-empty">工作区与该分支无差异</p>
        {:else}
          <div class="branch-diff-body">
            <DiffView files={branchDiff.files} />
          </div>
        {/if}
      </div>
    </div>
  {/if}

  <!-- ── 分支↔当前提交对比弹层(Compare with Current) ── -->
  {#if compareResult}
    <div class="update-overlay">
      <div class="branch-diff-modal">
        <div class="bd-header">
          <h2 class="bd-title">
            对比 <span class="bd-branch">{compareResult.branch}</span> ↔ 当前分支
          </h2>
          <button
            class="bd-close"
            onclick={() => (compareResult = null)}
            title="关闭">✕</button
          >
        </div>
        <div class="branch-diff-body">
          {#snippet commitList(entries: LogEntry[])}
            <ul class="cmp-list">
              {#each entries as c (c.full_sha)}
                <li class="cmp-row">
                  <span class="cmp-sha">{c.sha}</span>
                  <span class="cmp-msg">{c.message}</span>
                  <span class="cmp-rmeta">{c.author} · {c.date}</span>
                </li>
              {/each}
            </ul>
          {/snippet}
          {#if compareResult.incoming.length === 0 && compareResult.outgoing.length === 0}
            <p class="diff-empty">两分支提交一致,无独有提交</p>
          {:else}
            <section class="cmp-section">
              <h3 class="cmp-head">
                <span class="cmp-branch">{compareResult.branch}</span> 领先当前
                <span class="cmp-count">{compareResult.incoming.length}</span>
              </h3>
              {#if compareResult.incoming.length === 0}
                <p class="cmp-none">无</p>
              {:else}
                {@render commitList(compareResult.incoming)}
              {/if}
            </section>
            <section class="cmp-section">
              <h3 class="cmp-head">
                当前领先 <span class="cmp-branch">{compareResult.branch}</span>
                <span class="cmp-count">{compareResult.outgoing.length}</span>
              </h3>
              {#if compareResult.outgoing.length === 0}
                <p class="cmp-none">无</p>
              {:else}
                {@render commitList(compareResult.outgoing)}
              {/if}
            </section>
          {/if}
        </div>
      </div>
    </div>
  {/if}

  <!-- ── 全局设置 ── -->
  {#if showSettings}
    <Settings
      onClose={() => (showSettings = false)}
      onAppearanceChanged={applyAppearance}
    />
  {/if}

  <!-- ── Stash 储藏管理 ── -->
  {#if showStash && status}
    <StashView
      {path}
      hasChanges={status.dirty}
      changedFiles={stashableFiles}
      onClose={() => (showStash = false)}
      onChanged={refresh}
    />
  {/if}

  <!-- ── Tag 管理 ── -->
  {#if showTags && status}
    <TagView {path} onClose={() => (showTags = false)} onChanged={refresh} />
  {/if}
</main>

<style>
  /* ═══════════════════════════════════════════════════
     Base body styles (theme-independent)
     Themes are imported via JS import "../lib/themes.css"
     ═══════════════════════════════════════════════════ */
  :global(body) {
    /* ── Radii (theme-independent) ── */
    --radius-sm: 4px;
    --radius-md: 6px;
    --radius-lg: 8px;
    --radius-xl: 12px;

    margin: 0;
    background: var(--bg-void);
    color: var(--text-primary);
    font-family:
      -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    font-size: var(--fs-base, 13px);
    -webkit-font-smoothing: antialiased;
    transition:
      background-color 0.35s ease,
      color 0.35s ease;
  }

  /* ═══ Subtle scanline texture (theme-aware) ═══ */
  :global(body)::after {
    content: "";
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 99999;
    background: repeating-linear-gradient(
      0deg,
      transparent,
      transparent 3px,
      var(--scanline-color, rgba(0, 0, 0, 0.015)) 3px,
      var(--scanline-color, rgba(0, 0, 0, 0.015)) 6px
    );
  }

  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  /* ═══ Global focus ring ═══ */
  :global(*:focus-visible) {
    outline: 2px solid var(--accent-cyan);
    outline-offset: 1px;
    border-radius: 2px;
  }

  /* ═══ Overlay fade-in ═══ */
  @keyframes overlay-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
  @keyframes modal-in {
    from {
      opacity: 0;
      transform: scale(0.96) translateY(8px);
    }
    to {
      opacity: 1;
      transform: scale(1) translateY(0);
    }
  }

  /* ═══ Button press feedback ═══ */
  :global(button:active:not(:disabled)) {
    transform: scale(0.97);
    transition: transform 0.08s ease;
  }

  /* ══════════════════════════════════════
     TOPBAR — Gradient + subtle edge accent
     ══════════════════════════════════════ */
  .topbar {
    display: flex;
    align-items: center;
    gap: var(--space-md, 12px);
    padding: var(--topbar-py, 8px) var(--topbar-px, 14px);
    background: var(
      --topbar-bg,
      linear-gradient(
        135deg,
        #0d111a 0%,
        #141c28 40%,
        #111928 70%,
        #0d111a 100%
      )
    );
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
    position: relative;
    z-index: 2;
  }
  /* Subtle top-edge accent line */
  .topbar::before {
    content: "";
    position: absolute;
    inset: 0 0 auto 0;
    height: 1px;
    background: var(
      --topbar-accent,
      linear-gradient(
        90deg,
        transparent,
        var(--accent-cyan),
        var(--accent-purple),
        transparent
      )
    );
    opacity: 0.2;
    pointer-events: none;
  }

  .logo {
    font-weight: 800;
    font-size: var(--fs-lg, 15px);
    letter-spacing: 0.02em;
    background: linear-gradient(135deg, var(--accent-neon), var(--accent-cyan));
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    background-clip: text;
    text-shadow: none;
    flex-shrink: 0;
    transition: filter 0.3s;
  }
  .logo:hover {
    filter: brightness(1.2);
  }

  .path-bar {
    display: flex;
    gap: 6px;
    flex: 1;
  }
  .current-path {
    flex: 1;
    max-width: 360px;
    background: var(--bg-surface) !important;
    border: 1px solid var(--border-default) !important;
    border-radius: var(--radius-md);
    color: var(--text-primary) !important;
    padding: 6px 10px !important;
    font-family:
      ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    font-size: var(--fs-code, 12px);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
    cursor: pointer;
    transition:
      border-color 0.2s,
      box-shadow 0.2s;
  }
  .current-path:hover:not(:disabled) {
    border-color: var(--accent-cyan) !important;
    box-shadow: 0 0 8px rgba(88, 166, 255, 0.15);
    background: var(--bg-elevated) !important;
  }
  .current-path-empty {
    flex: 1;
    max-width: 360px;
    background: var(--bg-surface) !important;
    border: 1px dashed var(--border-default) !important;
    border-radius: var(--radius-md);
    color: var(--text-muted) !important;
    padding: 6px 10px !important;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    transition:
      border-color 0.2s,
      box-shadow 0.2s;
  }
  .current-path-empty:hover:not(:disabled) {
    border-color: var(--accent-cyan) !important;
    box-shadow: 0 0 8px rgba(88, 166, 255, 0.15);
  }
  .path-bar button {
    background: var(--bg-elevated);
    border: 1px solid var(--accent-cyan);
    border-radius: var(--radius-md);
    color: var(--accent-cyan);
    padding: 6px 14px;
    font-size: 12px;
    cursor: pointer;
    transition: all 0.2s;
  }
  .path-bar button:hover:not(:disabled) {
    background: var(--accent-cyan);
    color: #000;
    box-shadow: var(--glow-cyan);
  }
  .path-bar button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .btn-refresh {
    background: var(--bg-surface) !important;
    border: 1px solid var(--border-default) !important;
    color: var(--text-secondary) !important;
    padding: 6px 10px !important;
    flex-shrink: 0;
  }
  .btn-refresh:hover:not(:disabled) {
    border-color: var(--accent-neon) !important;
    color: var(--accent-neon) !important;
    box-shadow: 0 0 8px rgba(86, 211, 100, 0.12);
  }

  /* ═══ TAB BAR ═══ */
  .tab-bar {
    display: flex;
    background: var(--bg-base);
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
  }
  .tab-btn {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-primary);
    cursor: pointer;
    font-size: var(--fs-sm, 13px);
    padding: var(--tab-py, 8px) var(--tab-px, 18px);
    transition:
      color 0.2s,
      border-color 0.2s,
      text-shadow 0.2s,
      background 0.15s;
    position: relative;
  }
  .tab-btn:hover {
    color: var(--text-secondary);
    background: rgba(255, 255, 255, 0.02);
  }
  .tab-active {
    color: var(--accent-neon);
    border-bottom-color: var(--accent-neon);
    text-shadow: 0 0 6px rgba(86, 211, 100, 0.25);
  }

  /* ═══ PROJECT PICKER ═══ */
  .picker-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    animation: overlay-in 0.2s ease;
  }
  .picker-modal {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    padding: 24px;
    max-width: 90%;
    max-height: 90%;
    overflow-y: auto;
    box-shadow: 0 8px 40px rgba(0, 0, 0, 0.6);
    animation: modal-in 0.25s ease;
  }

  /* ═══ UPDATE OVERLAY ═══ */
  .update-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    animation: overlay-in 0.2s ease;
  }
  .update-modal {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    width: 540px;
    max-width: 92%;
    max-height: 90%;
    overflow-y: auto;
    box-shadow: 0 8px 40px rgba(0, 0, 0, 0.6);
    animation: modal-in 0.25s ease;
  }

  /* ═══ BRANCH DIFF MODAL ═══ */
  .branch-diff-modal {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    width: 760px;
    max-width: 94%;
    max-height: 88%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 8px 40px rgba(0, 0, 0, 0.6);
    animation: modal-in 0.25s ease;
  }
  .bd-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
  }
  .bd-title {
    margin: 0;
    font-size: var(--fs-lg, 14px);
    font-weight: 600;
    color: var(--text-primary);
  }
  .bd-branch {
    font-family:
      ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    color: var(--accent-gold);
  }
  .bd-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    line-height: 1;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    transition: all 0.15s;
  }
  .bd-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .branch-diff-body {
    flex: 1;
    overflow-y: auto;
    padding: 12px 14px;
  }
  .diff-empty {
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
    padding: 32px 18px;
    margin: 0;
  }

  /* ═══ COMMIT COMPARE ═══ */
  .cmp-section {
    margin-bottom: 18px;
  }
  .cmp-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    border-bottom: 1px solid var(--border-default);
    padding-bottom: 6px;
  }
  .cmp-branch {
    font-family:
      ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    color: var(--accent-gold);
  }
  .cmp-count {
    background: rgba(188, 140, 255, 0.15);
    color: var(--accent-purple);
    font-size: 11px;
    border-radius: 10px;
    padding: 1px 8px;
    border: 1px solid rgba(188, 140, 255, 0.2);
  }
  .cmp-none {
    color: var(--text-muted);
    font-size: 12px;
    margin: 0 0 4px;
    padding-left: 2px;
  }
  .cmp-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .cmp-row {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 4px 2px;
    font-family:
      ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    border-bottom: 1px solid var(--border-dim);
  }
  .cmp-sha {
    color: var(--accent-gold);
    flex-shrink: 0;
  }
  .cmp-msg {
    flex: 1;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cmp-rmeta {
    color: var(--text-muted);
    flex-shrink: 0;
    font-size: 11px;
  }

  /* ═══ BADGES ═══ */
  .badge {
    font-size: 11px;
    border-radius: 10px;
    padding: 1px 8px;
  }
  .ahead {
    background: rgba(86, 211, 100, 0.12);
    color: var(--accent-neon);
    border: 1px solid rgba(86, 211, 100, 0.2);
  }
  .behind {
    background: rgba(88, 166, 255, 0.12);
    color: var(--accent-cyan);
    border: 1px solid rgba(88, 166, 255, 0.2);
  }

  /* ═══ REMOTE ACTIONS ═══ */
  .remote-actions {
    display: flex;
    gap: 6px;
    margin-left: 8px;
    flex-shrink: 0;
  }
  .btn-remote {
    background: var(--bg-surface);
    border: 1px solid var(--accent-cyan);
    border-radius: var(--radius-md);
    color: var(--accent-cyan);
    cursor: pointer;
    font-size: 12px;
    padding: 5px 12px;
    white-space: nowrap;
    transition: all 0.2s;
  }
  .btn-remote:hover:not(:disabled) {
    background: var(--accent-cyan);
    color: #000;
    box-shadow: var(--glow-cyan);
  }
  .btn-remote:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .btn-remote.active {
    background: var(--accent-cyan);
    color: #000;
    box-shadow: var(--glow-cyan);
  }

  /* ═══ MORE DROPDOWN ═══ */
  .more-wrap {
    position: relative;
    display: inline-flex;
  }
  .more-backdrop {
    position: fixed;
    inset: 0;
    background: transparent;
    border: none;
    padding: 0;
    cursor: default;
    z-index: 40;
  }
  .more-menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 50;
    display: flex;
    flex-direction: column;
    min-width: 132px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    padding: 4px;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.6);
    animation: modal-in 0.15s ease;
    transform-origin: top right;
  }
  .more-item {
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 12px;
    text-align: left;
    padding: 6px 10px;
    white-space: nowrap;
    transition: background 0.15s;
  }
  .more-item:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .more-item:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .btn-settings {
    margin-left: auto;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: var(--fs-xl, 17px);
    line-height: 1;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    flex-shrink: 0;
    transition: all 0.15s;
  }
  .btn-settings:hover {
    color: var(--accent-neon);
    background: var(--bg-hover);
    text-shadow: 0 0 6px rgba(86, 211, 100, 0.25);
  }

  /* ═══ ERROR & OP MESSAGE ═══ */
  .error {
    background: rgba(247, 120, 139, 0.1);
    border-bottom: 1px solid rgba(247, 120, 139, 0.25);
    padding: 8px 14px;
    color: var(--color-error);
    white-space: pre-wrap;
    font-size: 12px;
    margin: 0;
  }
  .op-message {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    background: rgba(86, 211, 100, 0.08);
    border-bottom: 1px solid rgba(86, 211, 100, 0.2);
    padding: 8px 14px;
    margin: 0;
  }
  .op-message-text {
    flex: 1;
    color: var(--accent-neon);
    font-size: 12px;
    white-space: pre-wrap;
    min-width: 0;
  }
  .op-message-close {
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: var(--accent-neon);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 0 2px;
    opacity: 0.7;
    transition: opacity 0.15s;
  }
  .op-message-close:hover {
    opacity: 1;
  }

  /* ═══ SPLIT LAYOUT ═══ */
  .split {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  /* ═══ FILE LIST (LEFT PANEL) ═══ */
  .file-list {
    width: 280px;
    flex-shrink: 0;
    border-right: 1px solid var(--border-default);
    display: flex;
    flex-direction: column;
    background: var(--bg-base);
  }
  .repo-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 0 10px;
    contain: layout paint;
  }
  .zone:first-child {
    margin-top: 6px;
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
    padding: var(--file-item-py, 4px) 14px var(--file-item-py, 4px)
      var(--file-item-pl, 20px);
    cursor: pointer;
    min-height: 26px;
    transition:
      background 0.15s,
      border-left-color 0.15s;
    border-radius: 0;
    border-left: 3px solid transparent;
  }
  .file-item:hover {
    background: var(--bg-hover);
    border-left-color: var(--border-default);
  }
  .file-item.conflict {
    color: var(--color-error);
  }
  .muted {
    color: var(--text-muted);
    font-size: 12px;
    padding: 4px 14px;
  }
  .repo-group .muted {
    padding: 8px 14px;
    margin: 0;
    font-style: italic;
    font-size: 11px;
  }

  /* ═══ ZONES ═══ */
  .zone {
    margin-bottom: 4px;
  }
  .zone-title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-xs, 11px);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    margin: 0;
    padding: 10px 14px 8px;
    position: sticky;
    top: 0;
    background: var(--bg-base);
    z-index: 2;
    border-bottom: 1px solid var(--border-dim);
  }
  .zone-icon {
    font-size: 12px;
    width: 14px;
    text-align: center;
    flex-shrink: 0;
  }
  .zone-badge {
    margin-left: auto;
    background: transparent;
    border: 1px solid var(--border-default);
    border-radius: 10px;
    font-size: 10px;
    padding: 1px 8px;
    font-weight: 600;
    letter-spacing: 0;
  }
  /* Zone accent colors */
  .zone-unstaged {
    color: var(--accent-gold);
    border-left: 3px solid var(--accent-gold);
  }
  .zone-unstaged .zone-icon {
    color: var(--accent-gold);
  }
  .zone-unstaged .zone-badge {
    color: var(--accent-gold);
    border-color: rgba(227, 179, 65, 0.25);
  }

  .zone-staged {
    color: var(--accent-neon);
    border-left: 3px solid var(--accent-neon);
  }
  .zone-staged .zone-icon {
    color: var(--accent-neon);
  }
  .zone-staged .zone-badge {
    color: var(--accent-neon);
    border-color: rgba(86, 211, 100, 0.25);
  }

  .zone-conflict {
    color: var(--color-error);
    border-left: 3px solid var(--color-error);
  }
  .zone-conflict .zone-icon {
    color: var(--color-error);
  }
  .zone-conflict .zone-badge {
    color: var(--color-error);
    border-color: rgba(247, 120, 139, 0.25);
  }

  /* ═══ REPO GROUPS ═══ */
  .repo-group {
    margin: 4px 8px;
    background: var(--bg-surface);
    border: 1px solid var(--border-dim);
    border-radius: var(--radius-md);
    overflow: hidden;
  }
  .repo-grouphead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: var(--bg-void);
    border-bottom: 1px solid var(--border-dim);
  }
  .repo-title {
    font-family:
      ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 1;
  }
  .repo-title.main {
    color: var(--accent-cyan);
  }
  .repo-branch {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-primary);
    font-family:
      ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    flex-shrink: 0;
    max-width: 130px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .repo-branch.detached {
    color: var(--color-error);
    font-style: italic;
  }
  .repo-branch-btn {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    cursor: pointer;
    padding: 2px 6px;
    margin-left: auto;
    flex-shrink: 0;
    min-width: 0;
    transition: all 0.15s;
  }
  .repo-branch-btn:hover {
    background: var(--bg-hover);
    border-color: var(--accent-cyan);
  }
  .repo-branch-btn::before {
    content: "";
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent-cyan);
    flex-shrink: 0;
  }
  .repo-manage {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    padding: 4px 10px;
    border-bottom: 1px solid var(--border-dim);
  }

  /* ── Changelist 变更集 ── */
  .cl-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px 2px;
  }
  .cl-bar-label {
    font-size: 11px;
    color: var(--text-muted);
  }
  .cl-active-select {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 11px;
    padding: 2px 4px;
    max-width: 160px;
  }
  .cl-add-btn {
    background: transparent;
    border: 1px dashed var(--border-default);
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 11px;
    padding: 2px 8px;
    transition: all 0.15s;
  }
  .cl-add-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .cl-add-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .cl-edit {
    display: flex;
    gap: 6px;
    padding: 2px 10px 6px;
  }
  .cl-edit-input {
    flex: 1;
    background: var(--bg-surface);
    border: 1px solid var(--accent-cyan);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    padding: 4px 8px;
    min-width: 0;
  }
  .cl-edit-ok,
  .cl-edit-cancel {
    background: var(--bg-hover);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 11px;
    padding: 2px 10px;
  }
  .cl-edit-ok {
    color: var(--accent-neon);
    border-color: rgba(86, 211, 100, 0.3);
  }
  .cl-group {
    border-top: 1px solid var(--border-dim);
  }
  .cl-group-head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
  }
  .cl-name {
    font-size: 12px;
    color: var(--text-secondary);
    font-weight: 600;
  }
  .cl-active-tag {
    font-size: 9px;
    color: var(--accent-cyan);
    border: 1px solid rgba(88, 166, 255, 0.3);
    border-radius: 3px;
    padding: 0 4px;
  }
  .cl-count {
    font-size: 10px;
    color: var(--text-muted);
    background: var(--bg-surface);
    border-radius: 8px;
    padding: 0 6px;
  }
  .cl-commit-btn {
    margin-left: auto;
    background: rgba(86, 211, 100, 0.1);
    border: 1px solid rgba(86, 211, 100, 0.3);
    border-radius: 4px;
    color: var(--accent-neon);
    cursor: pointer;
    font-size: 11px;
    padding: 2px 8px;
  }
  .cl-commit-btn:hover:not(:disabled) {
    background: var(--accent-neon);
    color: #000;
  }
  .cl-commit-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .cl-mini {
    background: transparent;
    border: 1px solid var(--border-default);
    border-radius: 3px;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 11px;
    width: 20px;
    height: 18px;
    padding: 0;
  }
  .cl-mini:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .cl-empty {
    color: var(--text-muted);
    font-size: 11px;
    padding: 2px 10px 6px;
    margin: 0;
  }
  .sub-dot {
    font-size: 8px;
    flex-shrink: 0;
  }
  .sub-clean {
    color: var(--accent-neon);
  }
  .sub-dirty {
    color: var(--accent-gold);
  }
  .sub-detached {
    color: var(--color-error);
  }
  .sub-uninitialized {
    color: var(--text-muted);
  }
  .sub-state {
    font-size: 10px;
    color: var(--text-muted);
    margin-left: auto;
    flex-shrink: 0;
  }
  .sub-btn {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 10px;
    font-weight: 500;
    padding: 3px 10px;
    line-height: 1.4;
    transition: all 0.15s;
  }
  .sub-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .sub-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }
  /* Action-specific accents */
  .sub-btn-update {
    border-color: rgba(86, 211, 100, 0.3);
    color: var(--accent-neon);
  }
  .sub-btn-update:hover {
    background: rgba(86, 211, 100, 0.12);
    border-color: var(--accent-neon);
  }
  .sub-btn-push {
    border-color: rgba(88, 166, 255, 0.3);
    color: var(--accent-cyan);
  }
  .sub-btn-push:hover {
    background: rgba(88, 166, 255, 0.12);
    border-color: var(--accent-cyan);
  }
  .sub-btn-sync {
    border-color: rgba(188, 140, 255, 0.3);
    color: var(--accent-purple);
  }
  .sub-btn-sync:hover {
    background: rgba(188, 140, 255, 0.12);
    border-color: var(--accent-purple);
  }

  /* ═══ COMMIT AREA ═══ */
  .commit-area {
    flex-shrink: 0;
    border-top: 2px solid var(--border-default);
    background: var(--bg-void);
    padding: var(--commit-py, 12px) var(--commit-px, 14px)
      var(--commit-py, 14px);
    box-shadow: 0 -4px 12px rgba(0, 0, 0, 0.3);
    position: relative;
    z-index: 1;
    transition:
      background 0.3s,
      border-color 0.3s;
  }
  .amend-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    margin-bottom: 8px;
    cursor: pointer;
  }
  .commit-targets {
    font-size: 11px;
    color: var(--text-muted);
    margin: 6px 0 0;
    word-break: break-all;
  }
  .commit-area textarea {
    width: 100%;
    box-sizing: border-box;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    padding: 10px 12px;
    font-family:
      ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    resize: vertical;
    margin-top: 6px;
    transition:
      border-color 0.2s,
      box-shadow 0.2s;
  }
  .commit-area textarea:focus {
    outline: none;
    border-color: var(--accent-neon);
    box-shadow: 0 0 0 2px rgba(86, 211, 100, 0.1);
  }
  .commit-area textarea:disabled {
    opacity: 0.4;
  }
  .commit-bar {
    margin-top: 6px;
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .push-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-right: auto;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }
  .btn-commit {
    background: rgba(86, 211, 100, 0.1);
    border: 1px solid var(--accent-neon);
    border-radius: var(--radius-md);
    color: var(--accent-neon);
    padding: 6px 18px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s;
  }
  .btn-commit:hover:not(:disabled) {
    background: var(--accent-neon);
    color: #000;
    box-shadow: var(--glow-neon);
  }
  .btn-commit:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .commit-ok {
    color: var(--accent-neon);
    font-size: 12px;
    margin: 6px 0 0;
    text-shadow: 0 0 4px rgba(86, 211, 100, 0.2);
  }

  /* ═══ DIFF VIEW (RIGHT PANEL) ═══ */
  .diff-view {
    flex: 1;
    overflow-y: auto;
    padding: 12px 16px;
    contain: layout paint;
  }
  .placeholder {
    margin-top: 40px;
    text-align: center;
  }

  .hint {
    color: var(--text-muted);
    font-size: var(--fs-base, 13px);
    text-align: center;
    margin-top: 60px;
  }

  /* ═══ AI COMMIT LIST ═══ */
  .ai-commit-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .btn-ai-all {
    align-self: flex-start;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-primary);
    cursor: pointer;
    font-size: var(--fs-sm, 12px);
    padding: 5px 12px;
  }
  .btn-ai-all:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .ai-commit-row {
    border: 1px solid var(--border-dim);
    border-radius: 4px;
    padding: 6px 8px;
  }
  .ai-row-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 4px;
    font-size: var(--fs-sm, 12px);
    color: var(--text-secondary);
  }
  .ai-gen {
    background: transparent;
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: var(--fs-xs, 11px);
    padding: 2px 8px;
  }
  .ai-gen:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--text-muted);
  }
  .ai-err {
    margin: 4px 0 0;
    color: var(--color-error);
    font-size: var(--fs-xs, 11px);
  }
</style>

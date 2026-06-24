<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { open, ask } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import UpdateView from "$lib/UpdateView.svelte";
  import HistoryView from "$lib/HistoryView.svelte";
  import DiffView from "$lib/DiffView.svelte";
  import FileTree from "$lib/FileTree.svelte";
  import ProjectPicker from "$lib/ProjectPicker.svelte";
  import FileHistory from "$lib/FileHistory.svelte";
  import BlameView from "$lib/BlameView.svelte";
  import BranchPicker from "$lib/BranchPicker.svelte";
  import ConflictView from "$lib/ConflictView.svelte";
  import Settings from "$lib/Settings.svelte";
  import UpdateBanner from "$lib/UpdateBanner.svelte";
  import StashView from "$lib/StashView.svelte";
  import TagView from "$lib/TagView.svelte";
  import Fireworks from "$lib/effects/Fireworks.svelte";
  import MatrixRain from "$lib/effects/MatrixRain.svelte";

  import Starfield from "$lib/effects/Starfield.svelte";
  import ParticleNetwork from "$lib/effects/ParticleNetwork.svelte";
  import CursorEffects from "$lib/effects/CursorEffects.svelte";
  import TerminalBoot from "$lib/effects/TerminalBoot.svelte";
  import ErrorBurst from "$lib/effects/ErrorBurst.svelte";
  import "../lib/themes.css";
  import "../lib/effects/effects.css";

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
    body.setAttribute("data-theme", s.theme || "cyberpunk");
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
      const s = await invoke<AppearanceSettings>("get_settings");
      applyAppearance(s);
    } catch {
      // 使用默认外观
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
  let amendMode = $state(false); // amend 模式:仅修改主仓库上次提交
  let commitResult = $state("");
  let showFireworks = $state(false);
  let matrixIntensity = $state<"subtle" | "medium" | "heavy">("subtle");
  let showBoot = $state(true);
  let errorBurst = $state(false);
  let prevError = $state("");

  $effect(() => {
    if (showFireworks) {
      matrixIntensity = "heavy";
      setTimeout(() => {
        showFireworks = false;
        matrixIntensity = "subtle";
      }, 2000);
    }
  });

  $effect(() => {
    if (error && error !== prevError) {
      errorBurst = true;
      prevError = error;
      setTimeout(() => (errorBurst = false), 1200);
    }
  });
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
      showFireworks = true;
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
      showFireworks = true;
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

  // repo_push 的返回(PushOutcome,外部 tagged 单元枚举 → 字符串)。
  type PushOutcome = "Success" | "NoUpstream" | "NonFastForward";

  function pushMsg(r: PushOutcome): string {
    if (r === "Success") return "推送成功";
    if (r === "NoUpstream") return "跳过:无 upstream";
    return "远端领先,待更新后推送";
  }

  // 推送单个仓库(主仓库组的独立「推送」)。被拒于"远端领先"时走「更新后推送」。
  async function doPush(repo: RepoView) {
    operating = true;
    error = "";
    opMessage = "";
    try {
      const r = await invoke<PushOutcome>("repo_push", { path: repo.path });
      if (r === "Success") {
        opMessage = "推送成功";
        await refresh();
      } else if (r === "NoUpstream") {
        error = "当前分支没有 upstream，请先设置上游分支";
      } else {
        await offerUpdateThenPush(repo);
      }
    } catch (e) {
      error = String(e);
    } finally {
      operating = false;
    }
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

  // 单仓:远端领先 → 确认(显示当前策略)后更新并推送。
  async function offerUpdateThenPush(repo: RepoView) {
    const strat = await globalStrategyLabel();
    if (
      !(await ask(
        `「${repo.label}」推送被拒绝:远端领先。\n按当前全局策略「${strat}」更新后再推送?`,
        { title: "更新后推送" },
      ))
    )
      return;
    openUpdateThenPush(repo);
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
    const lines: string[] = [];
    const rejected: RepoView[] = [];
    for (const r of repos) {
      try {
        const out = await invoke<PushOutcome>("repo_push", { path: r.path });
        if (out === "NonFastForward") rejected.push(r);
        lines.push(`${r.label}：${pushMsg(out)}`);
      } catch (e) {
        lines.push(`${r.label}：${String(e)}`);
      }
    }
    opMessage = lines.join("\n");
    operating = false;
    await refresh();
    // 远端领先的仓:一次确认 → 逐个「更新后推送」(复用 UpdateView,含冲突解决)。
    if (rejected.length > 0) {
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

<!-- ── Terminal Boot Sequence ── -->
{#if showBoot}
  <TerminalBoot ondone={() => (showBoot = false)} />
{/if}

<main class="screen-tear rgb-split">
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
  <header class="topbar data-stream">
    <span class="logo neon-flicker">git-gui</span>
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
      class="btn-settings energy-pulse"
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
        <aside class="file-list hud-brackets">
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
                        onclick={() => doPush(repo)}>↑ 推送</button
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
                        onclick={() => doPush(repo)}>↑ 推送</button
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
                  title={amendMode
                    ? "用当前消息修改主仓库的上次提交（git commit --amend）"
                    : "把暂存的改动提交到所有有暂存内容的仓库（统一提交）"}
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
        <section class="diff-view glitch-block">
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
      <div class="branch-diff-modal holo-scan">
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
      <div class="branch-diff-modal holo-scan">
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
      onClose={() => (showStash = false)}
      onChanged={refresh}
    />
  {/if}

  <!-- ── Tag 管理 ── -->
  {#if showTags && status}
    <TagView {path} onClose={() => (showTags = false)} onChanged={refresh} />
  {/if}

  <!-- ── Cyberpunk effects layer ── -->
  <Starfield active={true} density={100} speed={0.12} />
  <ParticleNetwork active={true} count={50} maxDist={140} />
  <div class="scan-beam" aria-hidden="true"></div>
  <CursorEffects active={true} />
  <MatrixRain active={true} intensity={matrixIntensity} />
  <Fireworks trigger={showFireworks} />
  <ErrorBurst trigger={errorBurst} />
</main>

<style>
  /* ═══════════════════════════════════════════════════
     Base body styles (theme-independent)
     Themes are imported via JS import "../lib/themes.css"
     ═══════════════════════════════════════════════════ */
  :global(body) {
    /* ── Radii: subtle rounding (softened cyberpunk) ── */
    --radius-sm: 3px;
    --radius-md: 5px;
    --radius-lg: 8px;
    --radius-xl: 12px;

    margin: 0;
    background: var(--bg-void);
    color: var(--text-primary);
    font-family: var(--font-ui);
    font-size: var(--fs-base, 13px);
    -webkit-font-smoothing: antialiased;
    transition:
      background-color 0.35s ease,
      color 0.35s ease;
  }

  /* ═══ CRT Scanline texture (cyberpunk) ═══ */
  :global(body)::after {
    content: "";
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 99998;
    opacity: var(--scanline-opacity, 0.03);
    background: repeating-linear-gradient(
      0deg,
      transparent,
      transparent 2px,
      rgba(0, 0, 0, 0.6) 2px,
      rgba(0, 0, 0, 0.6) 4px
    );
  }

  /* ═══ CRT Vignette ═══ */
  :global(body)::before {
    content: "";
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 99997;
    background: radial-gradient(
      ellipse at center,
      transparent 55%,
      rgba(0, 0, 0, 0.35) 100%
    );
  }

  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  /* ═══ Global focus ring (neon glow) ═══ */
  :global(*:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    box-shadow: 0 0 8px var(--accent);
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
     TOPBAR — Cyberpunk command bar with glitched logo
     ══════════════════════════════════════ */
  .topbar {
    display: flex;
    align-items: center;
    gap: var(--space-md, 12px);
    padding: var(--topbar-py, 8px) var(--topbar-px, 14px);
    background: var(--topbar-bg);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    position: relative;
    z-index: 2;
  }
  /* Acid-edge accent line */
  .topbar::before {
    content: "";
    position: absolute;
    inset: 0 0 auto 0;
    height: 1px;
    background: var(--topbar-accent);
    opacity: 0.5;
    pointer-events: none;
    animation: rgbShift 3s steps(4) infinite;
  }

  /* Glitched logo with chromatic aberration */
  .logo {
    font-family: var(--font-mono);
    font-weight: 900;
    font-size: var(--fs-lg, 15px);
    text-transform: uppercase;
    letter-spacing: 0.15em;
    color: var(--accent);
    text-shadow:
      0 0 10px var(--accent),
      -2px 0 rgba(255, 0, 255, 0.5),
      2px 0 rgba(0, 212, 255, 0.5);
    flex-shrink: 0;
    position: relative;
    animation: rgbShift 3s steps(4) infinite;
  }
  @keyframes rgbShift {
    0%,
    100% {
      text-shadow:
        0 0 10px var(--accent),
        -2px 0 rgba(255, 0, 255, 0.5),
        2px 0 rgba(0, 212, 255, 0.5);
    }
    33% {
      text-shadow:
        0 0 8px var(--accent),
        2px 0 rgba(255, 0, 255, 0.6),
        -2px 0 rgba(0, 212, 255, 0.3);
    }
    66% {
      text-shadow:
        0 0 12px var(--accent),
        -3px 0 rgba(255, 0, 255, 0.3),
        3px 0 rgba(0, 212, 255, 0.6);
    }
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
    border: 1px solid var(--border) !important;
    color: var(--text-primary) !important;
    padding: 6px 10px !important;
    font-family: var(--font-mono);
    font-size: var(--fs-code, 12px);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
    cursor: pointer;
    transition:
      border-color 100ms steps(4),
      box-shadow 100ms steps(4);
  }
  .current-path:hover:not(:disabled) {
    border-color: var(--accent) !important;
    box-shadow: var(--glow-neon);
    background: var(--bg-elevated) !important;
  }
  .current-path-empty {
    flex: 1;
    max-width: 360px;
    background: var(--bg-surface) !important;
    border: 1px dashed var(--border) !important;
    color: var(--text-muted) !important;
    padding: 6px 10px !important;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    transition: border-color 100ms steps(4);
  }
  .current-path-empty:hover:not(:disabled) {
    border-color: var(--accent) !important;
    box-shadow: var(--glow-neon);
  }
  .path-bar button {
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-size: 11px;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    padding: 6px 12px;
    cursor: pointer;
    transition: all 100ms steps(4);
  }
  .path-bar button:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
    box-shadow: var(--glow-neon);
  }
  .path-bar button:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .btn-refresh {
    background: var(--bg-surface) !important;
    border: 1px solid var(--border) !important;
    color: var(--text-muted) !important;
    padding: 6px 10px !important;
    flex-shrink: 0;
    font-family: var(--font-mono);
  }
  .btn-refresh:hover:not(:disabled) {
    border-color: var(--accent) !important;
    color: var(--accent) !important;
    box-shadow: var(--glow-neon);
  }

  /* ═══ TAB BAR — Neon underline nav ═══ */
  .tab-bar {
    display: flex;
    background: var(--bg-base);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .tab-btn {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-muted);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--fs-sm, 12px);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    padding: var(--tab-py, 8px) var(--tab-px, 18px);
    transition: color 150ms steps(3);
    position: relative;
  }
  .tab-btn::after {
    content: "";
    position: absolute;
    bottom: -1px;
    left: 0;
    right: 100%;
    height: 2px;
    background: var(--accent);
    transition: right 150ms steps(4);
  }
  .tab-btn:hover {
    color: var(--text-secondary);
  }
  .tab-btn:hover::after {
    right: 0;
  }
  .tab-active {
    color: var(--accent);
    border-bottom-color: var(--accent);
    text-shadow: 0 0 8px rgba(0, 255, 136, 0.4);
  }
  .tab-active::after {
    display: none;
  }

  /* ═══ PROJECT PICKER — Chamfered overlay ═══ */
  .picker-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.8);
    backdrop-filter: blur(6px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    animation: overlay-in 150ms steps(5);
  }
  .picker-modal {
    background: var(--bg-elevated);
    border: 1px solid var(--accent);
    padding: 24px;
    max-width: 90%;
    max-height: 90%;
    overflow-y: auto;
    box-shadow:
      0 0 15px rgba(0, 255, 136, 0.2),
      0 0 30px rgba(0, 255, 136, 0.05),
      0 8px 40px rgba(0, 0, 0, 0.6);
    animation: modal-in 200ms steps(6);
    border-radius: var(--radius-lg);
  }

  /* ═══ UPDATE OVERLAY ═══ */
  .update-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.8);
    backdrop-filter: blur(6px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    animation: overlay-in 150ms steps(5);
  }
  .update-modal {
    background: var(--bg-elevated);
    border: 1px solid var(--accent);
    width: 540px;
    max-width: 92%;
    max-height: 90%;
    overflow-y: auto;
    box-shadow:
      0 0 15px rgba(0, 255, 136, 0.2),
      0 8px 40px rgba(0, 0, 0, 0.6);
    animation: modal-in 200ms steps(6);
    border-radius: var(--radius-lg);
  }

  /* ═══ BRANCH DIFF MODAL ═══ */
  .branch-diff-modal {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    width: 760px;
    max-width: 94%;
    max-height: 88%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow:
      0 0 12px rgba(0, 255, 136, 0.15),
      0 8px 40px rgba(0, 0, 0, 0.6);
    animation: modal-in 200ms steps(6);
    border-radius: var(--radius-lg);
  }
  .bd-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    font-family: var(--font-mono);
  }
  .bd-title {
    margin: 0;
    font-size: var(--fs-lg, 14px);
    font-weight: 600;
    color: var(--accent);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .bd-branch {
    font-family: var(--font-mono);
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
    transition: all 100ms steps(4);
  }
  .bd-close:hover {
    color: var(--destructive);
    text-shadow: var(--glow-error);
  }
  .branch-diff-body {
    flex: 1;
    overflow-y: auto;
    padding: 12px 14px;
  }
  .diff-empty {
    color: var(--text-muted);
    font-family: var(--font-mono);
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
    font-family: var(--font-mono);
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--accent);
    border-bottom: 1px solid var(--border);
    padding-bottom: 6px;
  }
  .cmp-branch {
    font-family: var(--font-mono);
    color: var(--accent-gold);
  }
  .cmp-count {
    background: rgba(255, 0, 255, 0.12);
    color: var(--accent-secondary);
    font-family: var(--font-mono);
    font-size: 11px;
    padding: 1px 8px;
    border: 1px solid rgba(255, 0, 255, 0.3);
  }
  .cmp-none {
    color: var(--text-muted);
    font-family: var(--font-mono);
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
    font-family: var(--font-mono);
    font-size: 12px;
    border-bottom: 1px solid var(--border);
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

  /* ═══ BADGES — Neon pill badges ═══ */
  .badge {
    font-family: var(--font-mono);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 1px 6px;
  }
  .ahead {
    background: rgba(0, 255, 136, 0.12);
    color: var(--accent);
    border: 1px solid rgba(0, 255, 136, 0.3);
  }
  .behind {
    background: rgba(0, 212, 255, 0.12);
    color: var(--accent-tertiary);
    border: 1px solid rgba(0, 212, 255, 0.3);
  }

  /* ═══ REMOTE ACTIONS — Command chips ═══ */
  .remote-actions {
    display: flex;
    gap: 6px;
    margin-left: 8px;
    flex-shrink: 0;
  }
  .btn-remote {
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-size: 11px;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    cursor: pointer;
    padding: 5px 12px;
    white-space: nowrap;
    transition: all 100ms steps(4);
  }
  .btn-remote:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
    box-shadow: var(--glow-neon);
  }
  .btn-remote:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .btn-remote.active {
    border-color: var(--accent);
    color: var(--accent);
    box-shadow: var(--glow-neon);
  }

  /* ═══ MORE DROPDOWN — Cyberpunk flyout ═══ */
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
    min-width: 140px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    padding: 4px;
    box-shadow:
      0 4px 20px rgba(0, 0, 0, 0.7),
      var(--glow-neon);
    animation: modal-in 120ms steps(5);
    transform-origin: top right;
  }
  .more-item {
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-size: 11px;
    background: transparent;
    border: none;
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    padding: 6px 10px;
    white-space: nowrap;
    transition: all 100ms steps(4);
  }
  .more-item:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--accent);
    text-shadow: 0 0 6px rgba(0, 255, 136, 0.3);
  }
  .more-item:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .btn-settings {
    margin-left: auto;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-muted);
    cursor: pointer;
    font-size: var(--fs-xl, 17px);
    line-height: 1;
    padding: 4px 8px;
    flex-shrink: 0;
    transition: all 100ms steps(4);
  }
  .btn-settings:hover {
    border-color: var(--border);
    color: var(--accent);
    text-shadow: 0 0 8px rgba(0, 255, 136, 0.4);
  }

  /* ═══ ERROR & OP MESSAGE ═══ */
  .error {
    background: rgba(255, 51, 102, 0.08);
    border-left: 3px solid var(--destructive);
    border-bottom: 1px solid rgba(255, 51, 102, 0.2);
    padding: 8px 14px;
    color: var(--destructive);
    white-space: pre-wrap;
    font-family: var(--font-mono);
    font-size: 12px;
    margin: 0;
  }
  .op-message {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    background: rgba(0, 255, 136, 0.06);
    border-left: 3px solid var(--accent);
    border-bottom: 1px solid rgba(0, 255, 136, 0.15);
    padding: 8px 14px;
    margin: 0;
  }
  .op-message-text {
    flex: 1;
    color: var(--accent);
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: pre-wrap;
    min-width: 0;
  }
  .op-message-close {
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: var(--accent);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 0 2px;
    opacity: 0.6;
    transition: opacity 100ms steps(4);
  }
  .op-message-close:hover {
    opacity: 1;
  }

  /* ═══ SPLIT LAYOUT — Cyberpunk HUD panels ═══ */
  .split {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  /* ═══ FILE LIST (LEFT PANEL) — Chamfered + circuit BG ═══ */
  .file-list {
    width: 300px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    background: var(--bg-base);
    position: relative;
  }
  /* Circuit pattern overlay */
  .file-list::before {
    content: "";
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 0;
    opacity: 0.05;
    background-image:
      linear-gradient(90deg, var(--accent) 1px, transparent 1px),
      linear-gradient(0deg, var(--accent) 1px, transparent 1px);
    background-size: 40px 40px;
    background-position: 20px 20px;
  }
  .repo-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 0 10px;
    position: relative;
    z-index: 1;
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
      background 100ms steps(4),
      border-color 100ms steps(4);
    border-left: 3px solid transparent;
    font-family: var(--font-mono);
    font-size: var(--fs-code, 12px);
  }
  .file-item:hover {
    background: var(--bg-hover);
    border-left-color: var(--accent);
  }
  .file-item.conflict {
    color: var(--destructive);
    border-left-color: var(--destructive);
  }
  .muted {
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: 12px;
    padding: 4px 14px;
  }
  .repo-group .muted {
    padding: 8px 14px;
    margin: 0;
    font-size: 11px;
  }

  /* ═══ ZONES — Neon-accented section headers ═══ */
  .zone {
    margin-bottom: 4px;
  }
  .zone-title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-family: var(--font-mono);
    font-size: var(--fs-xs, 10px);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    margin: 0;
    padding: 10px 14px 8px;
    position: sticky;
    top: 0;
    background: var(--bg-base);
    z-index: 2;
    border-bottom: 1px solid var(--border);
  }
  .zone-icon {
    font-size: 12px;
    width: 14px;
    text-align: center;
    flex-shrink: 0;
  }
  .zone-badge {
    margin-left: auto;
    font-family: var(--font-mono);
    background: transparent;
    border: 1px solid;
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
    border-color: rgba(255, 170, 0, 0.3);
  }

  .zone-staged {
    color: var(--accent);
    border-left: 3px solid var(--accent);
    text-shadow: 0 0 6px rgba(0, 255, 136, 0.2);
  }
  .zone-staged .zone-icon {
    color: var(--accent);
  }
  .zone-staged .zone-badge {
    color: var(--accent);
    border-color: rgba(0, 255, 136, 0.3);
  }

  .zone-conflict {
    color: var(--destructive);
    border-left: 3px solid var(--destructive);
    text-shadow: 0 0 6px rgba(255, 51, 102, 0.2);
  }
  .zone-conflict .zone-icon {
    color: var(--destructive);
  }
  .zone-conflict .zone-badge {
    color: var(--destructive);
    border-color: rgba(255, 51, 102, 0.3);
  }

  /* ═══ REPO GROUPS ═══ */
  .repo-group {
    margin: 4px 8px;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .repo-grouphead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: var(--bg-void);
    border-bottom: 1px solid var(--border);
  }
  .repo-title {
    font-family: var(--font-mono);
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
    color: var(--accent-tertiary);
    text-shadow: 0 0 4px rgba(0, 212, 255, 0.2);
  }
  .repo-branch {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-primary);
    font-family: var(--font-mono);
    flex-shrink: 0;
    max-width: 130px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .repo-branch.detached {
    color: var(--destructive);
  }
  .repo-branch-btn {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    cursor: pointer;
    padding: 2px 6px;
    margin-left: auto;
    flex-shrink: 0;
    min-width: 0;
    transition: all 100ms steps(4);
  }
  .repo-branch-btn:hover {
    background: var(--bg-hover);
    border-color: var(--accent);
    box-shadow: var(--glow-neon);
  }
  .repo-branch-btn::before {
    content: "";
    display: inline-block;
    width: 6px;
    height: 6px;
    background: var(--accent);
    flex-shrink: 0;
  }
  .repo-manage {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    padding: 4px 10px;
    border-bottom: 1px solid var(--border);
  }
  .sub-dot {
    font-size: 8px;
    flex-shrink: 0;
  }
  .sub-clean {
    color: var(--accent);
    text-shadow: 0 0 4px var(--accent);
  }
  .sub-dirty {
    color: var(--accent-gold);
  }
  .sub-detached {
    color: var(--destructive);
  }
  .sub-uninitialized {
    color: var(--text-muted);
  }
  .sub-state {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--text-muted);
    margin-left: auto;
    flex-shrink: 0;
  }
  .sub-btn {
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 10px;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 10px;
    font-weight: 500;
    padding: 3px 10px;
    line-height: 1.4;
    transition: all 0.15s;
  }
  .sub-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .sub-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }
  /* Action-specific neon accents */
  .sub-btn-update {
    border-color: rgba(0, 255, 136, 0.3);
    color: var(--accent);
  }
  .sub-btn-update:hover:not(:disabled) {
    background: rgba(0, 255, 136, 0.12);
    border-color: var(--accent);
    box-shadow: var(--glow-neon);
  }
  .sub-btn-push {
    border-color: rgba(0, 212, 255, 0.3);
    color: var(--accent-tertiary);
  }
  .sub-btn-push:hover:not(:disabled) {
    background: rgba(0, 212, 255, 0.12);
    border-color: var(--accent-tertiary);
    box-shadow: var(--glow-cyan);
  }
  .sub-btn-sync {
    border-color: rgba(255, 0, 255, 0.3);
    color: var(--accent-secondary);
  }
  .sub-btn-sync:hover:not(:disabled) {
    background: rgba(255, 0, 255, 0.12);
    border-color: var(--accent-secondary);
    box-shadow: var(--glow-magenta);
  }

  /* ═══ COMMIT AREA — Terminal command panel ═══ */
  .commit-area {
    flex-shrink: 0;
    border-top: 1px solid var(--border);
    background: var(--bg-void);
    padding: var(--commit-py, 12px) var(--commit-px, 14px)
      var(--commit-py, 14px);
    box-shadow: 0 -4px 12px rgba(0, 0, 0, 0.3);
    position: relative;
    z-index: 1;
  }
  .amend-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    font-family: var(--font-mono);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
    margin-bottom: 8px;
    cursor: pointer;
  }
  .amend-toggle:hover {
    color: var(--accent);
  }
  .commit-targets {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
    margin: 6px 0 0;
    word-break: break-all;
  }
  .commit-targets::before {
    content: "> ";
    color: var(--accent);
  }
  .commit-area textarea {
    width: 100%;
    box-sizing: border-box;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    color: var(--text-primary);
    padding: 10px 12px;
    font-family: var(--font-mono);
    font-size: 12px;
    resize: vertical;
    margin-top: 6px;
    transition:
      border-color 100ms steps(4),
      box-shadow 100ms steps(4);
  }
  .commit-area textarea:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow:
      0 0 0 2px rgba(0, 255, 136, 0.15),
      var(--glow-neon);
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
  .btn-commit {
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-size: 12px;
    background: rgba(0, 255, 136, 0.08);
    border: 1px solid var(--accent);
    color: var(--accent);
    padding: 6px 18px;
    font-weight: 700;
    cursor: pointer;
    transition: all 100ms steps(4);
  }
  .btn-commit:hover:not(:disabled) {
    background: var(--accent);
    color: #000;
    box-shadow:
      0 0 20px rgba(0, 255, 136, 0.5),
      0 0 40px rgba(0, 255, 136, 0.15);
  }
  .btn-commit:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .commit-ok {
    color: var(--accent);
    font-family: var(--font-mono);
    font-size: 12px;
    margin: 6px 0 0;
    text-shadow: 0 0 8px rgba(0, 255, 136, 0.3);
  }

  /* ═══ DIFF VIEW (RIGHT PANEL) — Terminal window ═══ */
  .diff-view {
    flex: 1;
    overflow-y: auto;
    padding: 14px 16px;
    background: var(--bg-void);
    position: relative;
  }
  /* Terminal traffic light dots */
  .diff-view::before {
    content: "";
    position: sticky;
    top: 0;
    left: 0;
    display: block;
    width: 38px;
    height: 8px;
    margin-bottom: 8px;
    background:
      radial-gradient(circle 3px, #ff5f57 100%, transparent 100%) 0 50%,
      radial-gradient(circle 3px, #febc2e 100%, transparent 100%) 14px 50%,
      radial-gradient(circle 3px, #28c840 100%, transparent 100%) 28px 50%;
    background-size: 6px 6px;
    background-repeat: no-repeat;
    z-index: 2;
    pointer-events: none;
  }
  .placeholder {
    margin-top: 40px;
    text-align: center;
    font-family: var(--font-mono);
    color: var(--text-muted);
  }

  .hint {
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: var(--fs-base, 13px);
    text-align: center;
    margin-top: 60px;
  }
  .hint::before {
    content: "> ";
    color: var(--accent);
  }
</style>

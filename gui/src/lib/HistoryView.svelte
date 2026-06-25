<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask, message } from "@tauri-apps/plugin-dialog";
  import DiffView from "$lib/DiffView.svelte";
  import ConflictView from "$lib/ConflictView.svelte";
  import RebaseTodoView from "$lib/RebaseTodoView.svelte";
  import ReflogView from "$lib/ReflogView.svelte";

  // ── 类型（与 gitcore serde 对应） ──
  interface LogEntry {
    sha: string;
    full_sha: string;
    message: string;
    author: string;
    date: string;
  }
  interface GraphEdge {
    from_lane: number;
    to_lane: number;
  }
  interface GraphCommit {
    entry: LogEntry;
    parents: string[];
    lane: number;
    edges: GraphEdge[];
  }
  // 合并视图(主仓 + 各子仓)中的一条提交,带仓库标识。
  interface MergedLogEntry {
    entry: LogEntry;
    repo_label: string; // "" = 主仓,否则子仓相对路径
    repo_path: string; // 该提交所属仓库的绝对路径(操作定位用)
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
  interface StashRef {
    label: string;
  }
  // 分支下拉(仅取本地分支名,用于按分支筛选 log)
  interface BranchInfo {
    name: string;
    is_remote: boolean;
  }
  // smart checkout 结果(externally tagged)
  type SwitchOutcome = "Switched" | { StashConflict: { files: string[] } };
  interface ConflictedData {
    files: string[];
    autostash: StashRef | null;
  }
  type OutcomeVariant =
    | "AlreadyUpToDate"
    | "FastForwarded"
    | "Integrated"
    | "Conflicted"
    | "StashRestoreConflict"
    | "Resolved"
    | "SubmoduleSyncFailed";
  type UpdateOutcome =
    | "AlreadyUpToDate"
    | "Resolved"
    | { FastForwarded: unknown }
    | { Integrated: unknown }
    | { Conflicted: ConflictedData }
    | { StashRestoreConflict: unknown }
    | { SubmoduleSyncFailed: unknown };

  function outcomeVariant(o: UpdateOutcome): OutcomeVariant {
    if (typeof o === "string") return o as OutcomeVariant;
    return Object.keys(o)[0] as OutcomeVariant;
  }
  function outcomeData<T>(o: UpdateOutcome): T | undefined {
    if (typeof o === "string") return undefined;
    return (o as unknown as Record<string, T>)[Object.keys(o)[0]];
  }

  // ── Props ──
  let {
    path,
    submodules = [],
    onFileHistory,
  }: {
    path: string;
    submodules?: { path: string; name: string }[];
    onFileHistory?: (filePath: string, repoPath: string) => void;
  } = $props();

  // 有子仓 → 提交历史合并主仓 + 各子仓(线性 + 仓库标识);无子仓 → 沿用单仓拓扑图。
  let hasSub = $derived(submodules.length > 0);

  // ── 状态 ──
  let commits = $state<GraphCommit[]>([]);
  let mergedRows = $state<MergedLogEntry[]>([]);
  // 当前选中提交所属仓库的绝对路径(主仓或某子仓),详情与操作据此定位仓库。
  let selectedRepoPath = $state("");
  let loading = $state(false);
  let error = $state("");
  let maxCount = $state(50);

  function fmtDate(s: string): string {
    // "2025-06-20 14:30:00 +0800" → "2025-06-20 14:30"
    return s.replace(/(\d{2}:\d{2}):\d{2}.*/, "$1");
  }
  let selectedCommit = $state<LogEntry | null>(null);
  let commitMsg = $state("");
  let commitDiffs = $state<FileDiff[]>([]);
  // 子模块变更 → 该 old..new 区间的子仓提交(父仓 commit 展开子模块,对标 WebStorm)
  let submoduleRanges = $state<Record<string, LogEntry[]>>({});
  let detailLoading = $state(false);
  let detailError = $state("");
  let copied = $state(false);
  let authorFilter = $state("");
  let grepFilter = $state("");
  let filterTimeout: number | undefined;
  // 分支筛选:"" = 全部(当前 HEAD),否则按选中本地分支查 log。
  let branches = $state<string[]>([]);
  let selectedBranch = $state("");

  // ── 子模块变更检测 ──
  // 用 diff header 里的 gitlink mode 160000 判定(普通文件是 100644/100755/120000);
  // " 160000" 前导空格保证不会误配 blob SHA 子串(SHA 不含空格),比按 "Subproject commit"
  // 行内容判定更稳——避免普通文档文件含该行被误判为子模块。
  function isSubmoduleChange(f: FileDiff): boolean {
    return f.header_raw.includes(" 160000");
  }
  function subRange(f: FileDiff): { old: string; new: string } {
    let oldSha = "";
    let newSha = "";
    for (const h of f.hunks) {
      for (const l of h.lines) {
        const m = l.content.match(/^Subproject commit ([0-9a-f]+)/);
        if (!m) continue;
        if (l.kind === "Removed") oldSha = m[1];
        else if (l.kind === "Added") newSha = m[1];
      }
    }
    return { old: oldSha, new: newSha };
  }
  let submoduleDiffs = $derived(commitDiffs.filter(isSubmoduleChange));
  let normalDiffs = $derived(commitDiffs.filter((f) => !isSubmoduleChange(f)));

  // ── Cherry-pick / Revert 状态 ──
  let operationInProgress = $state(false);
  let operationError = $state("");
  // 重置到此:展开模式选择面板 + 选定模式
  let resetting = $state(false);
  let resetMode = $state<"Soft" | "Mixed" | "Hard">("Mixed");
  // 打 Tag:展开输入面板 + 名称/注释
  let tagging = $state(false);
  let tagName = $state("");
  let tagMessage = $state("");
  // 交互式变基:打开编辑器弹层(从 selectedCommit 起)
  let rebasing = $state(false);
  // reflog:打开 HEAD 历史弹层(查看/恢复)
  let showReflog = $state(false);
  let conflictFiles = $state<string[]>([]);
  let autostash = $state<StashRef | null>(null);
  let inConflictResolution = $state(false);

  // ── 数据加载 ──
  async function load() {
    loading = true;
    error = "";
    try {
      if (hasSub) {
        mergedRows = await invoke<MergedLogEntry[]>("repo_log_merged", {
          path,
          maxCount,
          branch: selectedBranch || null,
          author: authorFilter || null,
          grep: grepFilter || null,
        });
      } else {
        commits = await invoke<GraphCommit[]>("repo_log_topology", {
          path,
          maxCount,
          branch: selectedBranch || null,
          author: authorFilter || null,
          grep: grepFilter || null,
        });
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // 拉本地分支名填充下拉(切到 history tab 会重新挂载,故无需跨组件实时同步)。
  async function loadBranches() {
    try {
      const list = await invoke<BranchInfo[]>("repo_branches", { path });
      branches = list.filter((b) => !b.is_remote).map((b) => b.name);
    } catch {
      branches = [];
    }
  }

  // 切换筛选分支:重置分页并立即重载(下拉是离散选择,无需 debounce)。
  function onBranchChange() {
    maxCount = 50;
    load();
  }

  // 列表总条数(供标题与空态判断,合并/单仓两种来源统一)。
  let rowCount = $derived(hasSub ? mergedRows.length : commits.length);
  // 选中提交所属子仓的相对路径(主仓提交为空串),供详情头部标注操作目标仓库。
  let selectedRepoLabel = $derived(
    selectedRepoPath && selectedRepoPath !== path
      ? selectedRepoPath.slice(path.length).replace(/^\/+/, "")
      : "",
  );

  function onCommitScroll(e: Event) {
    const el = e.target as HTMLElement;
    scrollTop = el.scrollTop;
    viewportH = el.clientHeight;
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 200) {
      loadMore();
    }
  }

  async function loadMore() {
    if (loading || filtering) return;
    maxCount += 50;
    await load();
  }

  // repoPath:该提交所属仓库的绝对路径(合并视图下可能是子仓);单仓视图传主仓 path。
  async function selectCommit(entry: LogEntry, repoPath: string) {
    selectedCommit = entry;
    selectedRepoPath = repoPath;
    detailLoading = true;
    detailError = "";
    commitMsg = "";
    commitDiffs = [];
    submoduleRanges = {};
    copied = false;
    resetting = false;
    tagging = false;
    try {
      const [msg, diffs] = await Promise.all([
        invoke<string>("repo_commit_message", {
          path: repoPath,
          sha: entry.full_sha,
        }),
        invoke<FileDiff[]>("repo_commit_files", {
          path: repoPath,
          sha: entry.full_sha,
        }),
      ]);
      commitMsg = msg;
      commitDiffs = diffs;
      void loadSubmoduleRanges(repoPath, entry.full_sha, diffs);
    } catch (e) {
      detailError = String(e);
    } finally {
      detailLoading = false;
    }
  }

  // 对该提交里的每个子模块指针变化,取其 old..new 区间的子仓提交(失败则只显示指针)。
  // 传入 sha 守卫:慢请求返回时若已切到别的提交,丢弃结果避免写入过期区间。
  async function loadSubmoduleRanges(
    repoPath: string,
    sha: string,
    diffs: FileDiff[],
  ) {
    for (const f of diffs) {
      if (!isSubmoduleChange(f)) continue;
      const { old, new: newSha } = subRange(f);
      if (!old || !newSha) continue; // 仅指针变更(modify)才有区间
      try {
        const commits = await invoke<LogEntry[]>("repo_submodule_commits", {
          path: repoPath,
          subPath: f.path,
          oldSha: old,
          newSha,
        });
        if (selectedCommit?.full_sha !== sha) return; // 已切换,丢弃
        submoduleRanges = { ...submoduleRanges, [f.path]: commits };
      } catch {
        // 子仓未拉取该区间或为回退:存空数组,表示"已尝试但无结果"(区别于尚在加载)
        if (selectedCommit?.full_sha !== sha) return;
        submoduleRanges = { ...submoduleRanges, [f.path]: [] };
      }
    }
  }

  async function copySha() {
    if (!selectedCommit) return;
    try {
      await navigator.clipboard.writeText(selectedCommit.full_sha);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      // 某些环境 clipboard API 不可用,静默失败
    }
  }

  // 键盘激活
  function onActivate(e: KeyboardEvent, fn: () => void) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fn();
    }
  }

  // ── Cherry-pick / Revert 操作 ──
  async function doOperation(command: "repo_cherry_pick" | "repo_revert") {
    if (!selectedCommit || operationInProgress) return;
    operationInProgress = true;
    operationError = "";
    try {
      const outcome = await invoke<UpdateOutcome>(command, {
        path: selectedRepoPath,
        sha: selectedCommit.full_sha,
      });
      if (outcomeVariant(outcome) === "Conflicted") {
        const data = outcomeData<ConflictedData>(outcome);
        if (data) {
          conflictFiles = data.files;
          autostash = data.autostash;
          inConflictResolution = true;
        }
      } else {
        await load();
      }
    } catch (e) {
      operationError = String(e);
    } finally {
      operationInProgress = false;
    }
  }

  // ── 检出该提交(进入 detached HEAD) ──
  async function doCheckout() {
    if (!selectedCommit || operationInProgress) return;
    if (
      !(await ask(
        `检出 ${selectedCommit.sha} 将进入 detached HEAD（游离头指针，不在任何分支上）。确定?`,
        { title: "检出提交", kind: "warning" },
      ))
    )
      return;
    operationInProgress = true;
    operationError = "";
    const sha = selectedCommit.full_sha;
    try {
      await invoke("repo_checkout_commit", {
        path: selectedRepoPath,
        sha,
      });
      await load();
    } catch (e) {
      const msg = String(e);
      // 工作区脏被拒 → 提供 smart checkout(对标 WebStorm:检出被挡时才提示)
      if (
        msg.includes("未提交改动") &&
        (await ask(
          `工作区有未提交改动。暂存后检出 ${sha.slice(0, 7)}(smart checkout)?\n改动会在检出后自动贴回。`,
          { title: "Smart Checkout" },
        ))
      ) {
        await smartCheckoutCommit(sha);
        return;
      }
      operationError = msg;
    } finally {
      operationInProgress = false;
    }
  }

  // smart checkout 提交:自动 stash → checkout → 贴回;贴回冲突时提示去改动列表解决。
  async function smartCheckoutCommit(sha: string) {
    operationInProgress = true;
    operationError = "";
    try {
      const r = await invoke<SwitchOutcome>("repo_checkout_commit_autostash", {
        path: selectedRepoPath,
        sha,
      });
      if (typeof r === "object" && "StashConflict" in r) {
        await message(
          `已暂存改动并检出 ${sha.slice(0, 7)},但贴回时有冲突。\n请在改动列表中解决冲突;原改动仍保留在 stash 中。`,
          { title: "贴回有冲突", kind: "warning" },
        );
      }
      await load();
    } catch (e) {
      operationError = String(e);
    } finally {
      operationInProgress = false;
    }
  }

  // ── 重置到此(Reset Current Branch to Here) ──
  async function doReset() {
    if (!selectedCommit || operationInProgress) return;
    // 硬重置会丢弃工作区改动,额外二次确认。
    if (
      resetMode === "Hard" &&
      !(await ask(
        `硬重置到 ${selectedCommit.sha}：将丢弃工作区与暂存区的所有未提交改动,且当前分支会回退到该提交。此操作不可恢复,确定?`,
        { title: "硬重置", kind: "warning" },
      ))
    ) {
      return;
    }
    operationInProgress = true;
    operationError = "";
    try {
      await invoke("repo_reset", {
        path: selectedRepoPath,
        sha: selectedCommit.full_sha,
        mode: resetMode,
      });
      resetting = false;
      await load();
    } catch (e) {
      operationError = String(e);
    } finally {
      operationInProgress = false;
    }
  }

  // ── 在该提交打 Tag ──
  async function doTag() {
    if (!selectedCommit || operationInProgress) return;
    const name = tagName.trim();
    if (!name) return;
    operationInProgress = true;
    operationError = "";
    try {
      await invoke("repo_create_tag", {
        path: selectedRepoPath,
        name,
        target: selectedCommit.full_sha,
        message: tagMessage.trim() || null,
      });
      tagging = false;
      tagName = "";
      tagMessage = "";
    } catch (e) {
      operationError = String(e);
    } finally {
      operationInProgress = false;
    }
  }

  async function handleContinue() {
    try {
      const outcome = await invoke<UpdateOutcome>("continue_update_cmd", {
        path: selectedRepoPath,
        autostash,
        recurse_submodules: false,
      });
      const variant = outcomeVariant(outcome);
      if (variant === "Conflicted") {
        const data = outcomeData<ConflictedData>(outcome);
        if (data) {
          conflictFiles = data.files;
          autostash = data.autostash;
        }
      } else {
        inConflictResolution = false;
        conflictFiles = [];
        autostash = null;
        await load();
      }
    } catch (e) {
      operationError = String(e);
    }
  }

  async function handleAbort() {
    try {
      await invoke("abort_update_cmd", { path: selectedRepoPath, autostash });
      inConflictResolution = false;
      conflictFiles = [];
      autostash = null;
      operationError = "";
    } catch (e) {
      operationError = String(e);
    }
  }

  // ── SVG 图常量 ──
  const ROW_H = 24;
  const LANE_W = 16;
  const NODE_R = 5;

  const LANE_COLORS = [
    "#56D364", // soft green
    "#58A6FF", // soft blue
    "#BC8CFF", // soft purple
    "#E3B341", // soft amber
    "#F7788B", // soft red
    "#F0883E", // soft orange
    "#79C0FF", // light blue
    "#FFA198", // salmon
    "#A5D6FF", // pale blue
    "#7EE787", // pale green
  ];

  function laneColor(lane: number): string {
    return LANE_COLORS[lane % LANE_COLORS.length];
  }

  function laneX(lane: number): number {
    return lane * LANE_W + LANE_W / 2;
  }

  // ── 派生:最大 lane 号 + SVG 尺寸 ──
  let maxLane = $derived(
    commits.reduce((m, c) => {
      m = Math.max(m, c.lane);
      for (const e of c.edges) {
        m = Math.max(m, e.from_lane, e.to_lane);
      }
      return m;
    }, 0),
  );
  let svgWidth = $derived((maxLane + 1) * LANE_W);
  let svgHeight = $derived(commits.length * ROW_H);

  // ── 虚拟滚动:固定行高,只渲染可视区 ± 缓冲行,避免大历史下 DOM(SVG path/circle + 行)爆炸 ──
  let scrollEl = $state<HTMLElement | null>(null);
  let scrollTop = $state(0);
  let viewportH = $state(600);
  const VBUFFER = 8; // 可视区上下各多渲染的缓冲行,滚动时不露白
  const MERGED_ROW_H = 44; // 合并视图行高(两行布局,固定;须与 CSS .merged-row height 一致)

  // 单仓(SVG 图)视图可视窗口:gi 为全局行索引,SVG 与行均按 gi*ROW_H 定位以保持对齐。
  let visStart = $derived(Math.max(0, Math.floor(scrollTop / ROW_H) - VBUFFER));
  let visEnd = $derived(
    Math.min(
      commits.length,
      Math.ceil((scrollTop + viewportH) / ROW_H) + VBUFFER,
    ),
  );
  let visibleCommits = $derived(
    commits
      .slice(visStart, visEnd)
      .map((c, k) => ({ commit: c, gi: visStart + k })),
  );

  // 合并视图可视窗口。
  let mergedHeight = $derived(mergedRows.length * MERGED_ROW_H);
  let mStart = $derived(
    Math.max(0, Math.floor(scrollTop / MERGED_ROW_H) - VBUFFER),
  );
  let mEnd = $derived(
    Math.min(
      mergedRows.length,
      Math.ceil((scrollTop + viewportH) / MERGED_ROW_H) + VBUFFER,
    ),
  );
  let visibleMerged = $derived(
    mergedRows.slice(mStart, mEnd).map((r, k) => ({ row: r, gi: mStart + k })),
  );

  // 重置滚动到顶(筛选变化 / 切换仓库时,避免 scrollTop 停留在旧列表的越界位置)。
  function resetScroll() {
    scrollTop = 0;
    if (scrollEl) scrollEl.scrollTop = 0;
  }
  // 筛选激活时,过滤后的子集 parent 链断裂会让 topology lane 爆炸(图无意义且会撑爆布局挤掉提交列),
  // 故筛选态下不画拓扑图,退化为线性提交列表。
  let filtering = $derived(authorFilter !== "" || grepFilter !== "");

  // ── Edge → SVG path d ──
  function edgePath(edge: GraphEdge, rowIndex: number): string {
    const x1 = laneX(edge.from_lane);
    const x2 = laneX(edge.to_lane);
    const y1 = rowIndex * ROW_H + ROW_H / 2;
    const y2 = (rowIndex + 1) * ROW_H + ROW_H / 2;

    if (edge.from_lane === edge.to_lane) {
      return `M ${x1} ${y1} L ${x1} ${y2}`;
    }
    // 平滑贝塞尔 S 曲线:水平差越大,控制点偏移越多
    const dx = Math.abs(x2 - x1);
    const dy = Math.min(ROW_H * 0.65, Math.max(dx * 0.5, ROW_H * 0.35));
    return `M ${x1} ${y1} C ${x1} ${y1 + dy}, ${x2} ${y2 - dy}, ${x2} ${y2}`;
  }

  // 筛选变化:debounce 300ms 后重载,并重置 maxCount
  function onFilterChange() {
    if (filterTimeout !== undefined) {
      clearTimeout(filterTimeout);
    }
    filterTimeout = setTimeout(() => {
      maxCount = 50;
      resetScroll();
      load();
    }, 300) as unknown as number;
  }

  // 初始化 / path 变化时重置并重载
  let prevPath = $state("");
  $effect(() => {
    if (path && path !== prevPath) {
      prevPath = path;
      maxCount = 50;
      resetScroll();
      authorFilter = "";
      grepFilter = "";
      selectedBranch = "";
      selectedCommit = null;
      selectedRepoPath = path;
      commitMsg = "";
      commitDiffs = [];
      commits = [];
      mergedRows = [];
      loadBranches();
      load();
    }
  });
</script>

<div class="history">
  {#if error}
    <pre class="error">{error}</pre>
  {/if}

  <div class="split">
    <!-- ── 左侧:提交列表 + SVG 图 ── -->
    <aside class="commit-list">
      <div class="list-header">
        <span class="list-title">提交历史 ({rowCount})</span>
        <button
          class="btn-load-more"
          onclick={() => (showReflog = true)}
          title="查看 HEAD 走过的历史(reflog),可从中恢复被变基/重置丢掉的状态"
          >Reflog</button
        >
      </div>

      <!-- 筛选栏 -->
      <div class="filter-row">
        <select
          class="filter-input branch-select"
          aria-label="按分支筛选"
          title="选择要查看历史的分支(全部=当前 HEAD)"
          bind:value={selectedBranch}
          onchange={onBranchChange}
        >
          <option value="">全部分支</option>
          {#each branches as b (b)}
            <option value={b}>{b}</option>
          {/each}
        </select>
        <input
          type="text"
          class="filter-input"
          placeholder="作者"
          aria-label="按作者筛选"
          bind:value={authorFilter}
          oninput={onFilterChange}
        />
        <input
          type="text"
          class="filter-input"
          placeholder="提交消息关键词"
          aria-label="按提交消息筛选"
          bind:value={grepFilter}
          oninput={onFilterChange}
        />
      </div>

      {#if rowCount === 0 && !loading}
        <p class="muted">无提交记录</p>
      {:else if hasSub}
        <!-- 合并视图:主仓 + 各子仓提交按时间排序,带仓库标识(无拓扑图) -->
        <div
          class="log-scroll merged"
          bind:this={scrollEl}
          bind:clientHeight={viewportH}
          onscroll={onCommitScroll}
        >
          <div
            class="info-rows"
            style="position:relative;height:{mergedHeight}px"
          >
            {#each visibleMerged as { row, gi } (row.repo_path + row.entry.full_sha)}
              {@const entry = row.entry}
              <div
                class="merged-row"
                class:selected={selectedCommit?.full_sha === entry.full_sha &&
                  selectedRepoPath === row.repo_path}
                role="button"
                tabindex="0"
                style="position:absolute;top:{gi *
                  MERGED_ROW_H}px;left:0;right:0;height:{MERGED_ROW_H}px"
                onclick={() => selectCommit(entry, row.repo_path)}
                onkeydown={(e) =>
                  onActivate(e, () => selectCommit(entry, row.repo_path))}
              >
                <div class="mr-top">
                  {#if row.repo_label}
                    <span class="repo-chip" title="子仓库 {row.repo_label}"
                      >{row.repo_label}</span
                    >
                  {:else}
                    <span class="repo-chip repo-main" title="主仓库">主仓</span>
                  {/if}
                  <span class="log-sha">{entry.sha}</span>
                  <span class="mr-msg">{entry.message}</span>
                </div>
                <div class="mr-bot">
                  <span class="mr-author">{entry.author}</span>
                  <span class="mr-dot">·</span>
                  <span class="mr-date">{fmtDate(entry.date)}</span>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {:else}
        <div
          class="log-scroll"
          bind:this={scrollEl}
          bind:clientHeight={viewportH}
          onscroll={onCommitScroll}
        >
          {#if !filtering}
            <!-- SVG 拓扑图 -->
            <svg
              class="graph-svg"
              width={svgWidth}
              height={svgHeight}
              viewBox="0 0 {svgWidth} {svgHeight}"
              aria-hidden="true"
            >
              <!-- edges:先画连线,再画 node 覆盖 -->
              {#each visibleCommits as { commit, gi } (commit.entry.full_sha)}
                {#each commit.edges as edge}
                  {@const fromColor = laneColor(edge.from_lane)}
                  <path
                    d={edgePath(edge, gi)}
                    stroke={fromColor}
                    stroke-width="1.5"
                    fill="none"
                    stroke-linecap="round"
                  />
                {/each}
              {/each}
              <!-- nodes -->
              {#each visibleCommits as { commit, gi } (commit.entry.full_sha)}
                {@const cx = laneX(commit.lane)}
                {@const cy = gi * ROW_H + ROW_H / 2}
                {@const color = laneColor(commit.lane)}
                <circle
                  {cx}
                  {cy}
                  r={NODE_R}
                  fill={color}
                  stroke={color}
                  stroke-width="1"
                />
              {/each}
            </svg>
          {/if}

          <!-- 提交信息列(与 SVG 行对齐) -->
          <div class="info-rows" style="position:relative;height:{svgHeight}px">
            {#each visibleCommits as { commit, gi } (commit.entry.full_sha)}
              {@const entry = commit.entry}
              <div
                class="log-row"
                class:selected={selectedCommit?.full_sha === entry.full_sha}
                role="button"
                tabindex="0"
                style="position:absolute;top:{gi *
                  ROW_H}px;left:0;right:0;height:{ROW_H}px"
                onclick={() => selectCommit(entry, path)}
                onkeydown={(e) =>
                  onActivate(e, () => selectCommit(entry, path))}
              >
                <span class="log-sha">{entry.sha}</span>
                <span class="log-author">{entry.author}</span>
                <span class="log-date">{fmtDate(entry.date)}</span>
                <span class="log-msg">{entry.message}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      {#if loading}
        <p class="muted">加载中…</p>
      {/if}
    </aside>

    <!-- ── 右侧:提交详情 ── -->
    <section class="detail-view">
      {#if inConflictResolution}
        <!-- 冲突解决视图 -->
        {#if operationError}
          <pre class="error">{operationError}</pre>
        {/if}
        <ConflictView
          path={selectedRepoPath}
          {conflictFiles}
          {autostash}
          onContinue={handleContinue}
          onAbort={handleAbort}
        />
      {:else if selectedCommit}
        <!-- 提交信息头部 -->
        <div class="commit-header">
          <div class="commit-title-row">
            <h3 class="commit-title">{selectedCommit.message}</h3>
          </div>
          <div class="commit-meta">
            {#if selectedRepoLabel}
              <div class="meta-row">
                <span class="meta-label">仓库</span>
                <span
                  class="meta-repo"
                  title="该提交属于子仓库,操作将作用于此子仓"
                  >{selectedRepoLabel}</span
                >
              </div>
            {/if}
            <div class="meta-row">
              <span class="meta-label">SHA</span>
              <span class="meta-sha" title={selectedCommit.full_sha}
                >{selectedCommit.full_sha}</span
              >
            </div>
            <div class="meta-row">
              <span class="meta-label">作者</span>
              <span class="meta-author">{selectedCommit.author}</span>
            </div>
            <div class="meta-row">
              <span class="meta-label">日期</span>
              <span class="meta-date">{fmtDate(selectedCommit.date)}</span>
            </div>
          </div>
          <div class="commit-toolbar">
            <button
              class="btn-action"
              disabled={operationInProgress}
              onclick={() => doOperation("repo_cherry_pick")}
              title="把该提交拣选应用到当前分支（git cherry-pick）"
              >Cherry-pick</button
            >
            <button
              class="btn-action"
              disabled={operationInProgress}
              onclick={() => doOperation("repo_revert")}
              title="生成一个撤销该提交改动的新提交（git revert）"
              >Revert</button
            >
            <button
              class="btn-action"
              disabled={operationInProgress}
              onclick={doCheckout}
              title="检出该提交，进入 detached HEAD（git checkout <sha>）"
              >检出</button
            >
            <button
              class="btn-action btn-reset"
              class:active={resetting}
              disabled={operationInProgress}
              onclick={() => (resetting = !resetting)}
              title="把当前分支重置到该提交（git reset）">重置到此</button
            >
            <button
              class="btn-action btn-tag"
              class:active={tagging}
              disabled={operationInProgress}
              onclick={() => (tagging = !tagging)}
              title="在该提交创建 tag（git tag）">打 Tag</button
            >
            <button
              class="btn-action"
              disabled={operationInProgress}
              onclick={() => (rebasing = true)}
              title="从该提交起交互式变基：reword/squash/fixup/drop/重排（git rebase -i）"
              >交互式变基</button
            >
            <button
              class="btn-copy"
              onclick={copySha}
              title="复制该提交的完整 SHA"
              >{copied ? "已复制 ✓" : "复制 SHA"}</button
            >
          </div>
        </div>

        {#if resetting}
          <div class="reset-panel">
            <p class="reset-title">
              把当前分支重置到 <code>{selectedCommit.sha}</code>：
            </p>
            <label class="reset-mode">
              <input
                type="radio"
                name="resetmode"
                value="Mixed"
                bind:group={resetMode}
              />
              <span>
                <b>混合（默认）</b>
                <small>移动分支指针，改动退回工作区（未暂存），不丢失</small>
              </span>
            </label>
            <label class="reset-mode">
              <input
                type="radio"
                name="resetmode"
                value="Soft"
                bind:group={resetMode}
              />
              <span>
                <b>软</b>
                <small>移动分支指针，改动保留在暂存区</small>
              </span>
            </label>
            <label class="reset-mode">
              <input
                type="radio"
                name="resetmode"
                value="Hard"
                bind:group={resetMode}
              />
              <span>
                <b>硬</b>
                <small class="reset-danger"
                  >移动分支指针，丢弃工作区与暂存区改动（不可恢复）</small
                >
              </span>
            </label>
            <div class="reset-actions">
              <button
                class="btn-reset-confirm"
                class:danger={resetMode === "Hard"}
                disabled={operationInProgress}
                onclick={doReset}>确认重置</button
              >
              <button
                class="btn-reset-cancel"
                disabled={operationInProgress}
                onclick={() => (resetting = false)}>取消</button
              >
            </div>
          </div>
        {/if}

        {#if tagging}
          <div class="tag-panel">
            <p class="tag-title">
              在 <code>{selectedCommit.sha}</code> 创建 tag：
            </p>
            <div class="tag-inputs">
              <input
                class="tag-name"
                type="text"
                bind:value={tagName}
                placeholder="tag 名称"
                disabled={operationInProgress}
              />
              <input
                class="tag-msg"
                type="text"
                bind:value={tagMessage}
                placeholder="注释(可选,留空为轻量标签)"
                disabled={operationInProgress}
                onkeydown={(e) => e.key === "Enter" && doTag()}
              />
            </div>
            <div class="tag-actions">
              <button
                class="btn-tag-confirm"
                disabled={operationInProgress || !tagName.trim()}
                onclick={doTag}>创建</button
              >
              <button
                class="btn-tag-cancel"
                disabled={operationInProgress}
                onclick={() => (tagging = false)}>取消</button
              >
            </div>
          </div>
        {/if}

        {#if operationError}
          <pre class="error">{operationError}</pre>
        {/if}

        <!-- 完整提交消息 -->
        {#if commitMsg}
          <pre class="commit-message">{commitMsg}</pre>
        {/if}

        <!-- 文件 diff -->
        {#if detailLoading}
          <p class="muted">加载 diff…</p>
        {:else if detailError}
          <pre class="error">{detailError}</pre>
        {:else if commitDiffs.length === 0}
          <p class="muted">无文件改动</p>
        {:else}
          {#if submoduleDiffs.length > 0}
            <div class="sub-changes">
              {#each submoduleDiffs as f (f.path)}
                {@const r = subRange(f)}
                <div class="sub-change">
                  <div class="sub-change-head">
                    <span class="sub-tag">子模块</span>
                    <span class="sub-path">{f.path}</span>
                    <span class="sub-range">
                      {r.old ? r.old.slice(0, 8) : "（新增）"} → {r.new
                        ? r.new.slice(0, 8)
                        : "（移除）"}
                    </span>
                  </div>
                  {#if submoduleRanges[f.path] === undefined}
                    {#if r.old && r.new}
                      <p class="sub-none">加载区间提交…</p>
                    {/if}
                  {:else if submoduleRanges[f.path].length}
                    <ul class="sub-commit-list">
                      {#each submoduleRanges[f.path] as c (c.full_sha)}
                        <li class="sub-commit">
                          <span class="sub-c-sha">{c.sha}</span>
                          <span class="sub-c-msg">{c.message}</span>
                          <span class="sub-c-meta"
                            >{c.author} · {fmtDate(c.date)}</span
                          >
                        </li>
                      {/each}
                    </ul>
                  {:else}
                    <p class="sub-none">
                      无法列出区间提交（子仓未拉取或为回退）
                    </p>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
          {#if normalDiffs.length > 0}
            <div class="diff-list">
              <DiffView
                files={normalDiffs}
                onFileHistory={(fp) => onFileHistory?.(fp, selectedRepoPath)}
              />
            </div>
          {/if}
        {/if}
      {:else}
        <p class="muted placeholder">← 选择左侧提交查看详情</p>
      {/if}
    </section>
  </div>

  <!-- ── 交互式变基编辑器(冲突/完成都回到本组件已有的 ConflictView/刷新) ── -->
  {#if rebasing && selectedCommit}
    <RebaseTodoView
      path={selectedRepoPath}
      fromSha={selectedCommit.full_sha}
      onClose={() => (rebasing = false)}
      onConflict={(data) => {
        rebasing = false;
        conflictFiles = data.files;
        autostash = data.autostash;
        inConflictResolution = true;
      }}
      onDone={() => {
        rebasing = false;
        void load();
      }}
    />
  {/if}

  <!-- ── Reflog:HEAD 历史查看/恢复 ── -->
  {#if showReflog}
    <ReflogView
      path={selectedRepoPath}
      repoLabel={selectedRepoLabel}
      onChanged={() => void load()}
      onClose={() => (showReflog = false)}
    />
  {/if}
</div>

<style>
  .history {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
  }
  .error {
    background: rgba(247, 120, 139, 0.12);
    border-bottom: 1px solid rgba(247, 120, 139, 0.25);
    padding: 8px 14px;
    color: var(--color-error);
    white-space: pre-wrap;
    font-size: 12px;
    margin: 0;
  }
  .split {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  /* ── 左侧提交列表 ── */
  .commit-list {
    width: 420px;
    flex-shrink: 0;
    border-right: 1px solid var(--border-default);
    overflow-y: auto;
    background: var(--bg-base);
    display: flex;
    flex-direction: column;
  }
  .list-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
  }
  .list-title {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
  }
  .btn-load-more {
    background: var(--bg-hover);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
  }
  .btn-load-more:hover {
    background: var(--bg-hover);
  }
  .btn-load-more:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* ── 筛选栏 ── */
  .filter-row {
    display: flex;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
  }
  .filter-input {
    flex: 1;
    background: var(--bg-surface);
    border: 1px solid var(--bg-hover);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    padding: 4px 8px;
  }
  .filter-input:focus {
    outline: none;
    border-color: var(--accent-cyan);
  }
  .filter-input::placeholder {
    color: var(--text-muted);
  }
  .branch-select {
    flex: 0 0 110px;
    cursor: pointer;
    appearance: auto;
  }

  /* ── SVG + 信息行(同滚容器) ── */
  .log-scroll {
    display: flex;
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .graph-svg {
    flex-shrink: 0;
    display: block;
  }

  .info-rows {
    flex: 1;
    min-width: 0;
  }

  .log-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px;
    box-sizing: border-box;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1;
    white-space: nowrap;
  }
  .log-row:hover {
    background: var(--bg-surface);
    cursor: pointer;
  }
  .log-row.selected {
    background: rgba(88, 166, 255, 0.15);
  }
  .log-sha {
    color: var(--accent-gold);
    flex-shrink: 0;
  }
  .log-author {
    color: var(--text-muted);
    flex-shrink: 0;
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .log-date {
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .log-msg {
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ── 合并视图:仓库标识 chip ── */
  .repo-chip {
    flex-shrink: 0;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    background: rgba(188, 140, 255, 0.14);
    color: #c49ae2;
    font-size: 10px;
    border-radius: 4px;
    padding: 1px 6px;
  }
  .repo-chip.repo-main {
    background: rgba(88, 166, 255, 0.14);
    color: var(--accent-cyan);
  }

  /* ── 合并视图:两行布局(主行 chip+sha+msg / 副行 作者·日期) ── */
  .merged-row {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 2px;
    padding: 5px 8px;
    cursor: pointer;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    border-bottom: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.04));
    /* 固定行高(= JS MERGED_ROW_H),配合虚拟滚动的绝对定位;box-sizing 含 padding/border。 */
    height: 44px;
    box-sizing: border-box;
    overflow: hidden;
  }
  .merged-row:hover {
    background: var(--bg-surface);
  }
  .merged-row.selected {
    background: rgba(88, 166, 255, 0.15);
  }
  .mr-top {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .mr-msg {
    flex: 1;
    min-width: 0;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mr-bot {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-left: 2px;
    color: var(--text-muted);
    font-size: 11px;
  }
  .mr-author {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mr-dot {
    opacity: 0.5;
  }
  .meta-repo {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    color: #c49ae2;
    font-size: 11px;
    word-break: break-all;
  }

  /* ── 右侧详情 ── */
  .detail-view {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px;
  }
  .placeholder {
    margin-top: 40px;
    text-align: center;
  }
  .commit-header {
    margin-bottom: 16px;
  }
  .commit-title-row {
    margin-bottom: 4px;
  }
  .commit-title {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    line-height: 1.4;
    word-break: break-word;
  }
  .commit-toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 10px 0;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--border-default);
  }
  .btn-action {
    background: transparent;
    border: 1px solid rgba(88, 166, 255, 0.3);
    border-radius: 4px;
    color: var(--accent-cyan);
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
    transition: all 0.15s;
  }
  .btn-action:hover:not(:disabled) {
    background: rgba(88, 166, 255, 0.15);
    border-color: var(--accent-cyan);
  }
  .btn-action:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .btn-reset {
    border-color: rgba(227, 179, 65, 0.3);
    color: var(--accent-gold);
  }
  .btn-reset:hover:not(:disabled) {
    background: rgba(227, 179, 65, 0.12);
    border-color: var(--accent-gold);
  }
  .btn-reset.active {
    background: rgba(227, 179, 65, 0.2);
  }

  /* ── 重置面板 ── */
  .reset-panel {
    background: var(--bg-elevated);
    border: 1px solid rgba(227, 179, 65, 0.15);
    border-radius: 6px;
    padding: 12px 14px;
    margin-bottom: 14px;
  }
  .reset-title {
    margin: 0 0 8px;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .reset-title code {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--accent-gold);
  }
  .reset-mode {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 5px 0;
    cursor: pointer;
  }
  .reset-mode input {
    margin-top: 3px;
    flex-shrink: 0;
  }
  .reset-mode span {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .reset-mode b {
    font-size: 12px;
    color: var(--text-primary);
    font-weight: 600;
  }
  .reset-mode small {
    font-size: 11px;
    color: var(--text-muted);
  }
  .reset-mode .reset-danger {
    color: var(--color-error);
  }
  .reset-actions {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }
  .btn-reset-confirm {
    background: rgba(227, 179, 65, 0.12);
    border: 1px solid rgba(227, 179, 65, 0.25);
    border-radius: 4px;
    color: #fff;
    cursor: pointer;
    font-size: 12px;
    padding: 5px 14px;
  }
  .btn-reset-confirm:hover:not(:disabled) {
    background: rgba(227, 179, 65, 0.18);
  }
  .btn-reset-confirm.danger {
    background: rgba(247, 120, 139, 0.25);
    border-color: rgba(247, 120, 139, 0.4);
  }
  .btn-reset-confirm.danger:hover:not(:disabled) {
    background: var(--color-error);
  }
  .btn-reset-confirm:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-reset-cancel {
    background: var(--border-default);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 5px 14px;
  }
  .btn-reset-cancel:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .btn-tag {
    border-color: rgba(86, 211, 100, 0.3);
    color: var(--accent-neon);
  }
  .btn-tag:hover:not(:disabled) {
    background: rgba(86, 211, 100, 0.12);
    border-color: var(--accent-neon);
  }
  .btn-tag.active {
    background: rgba(86, 211, 100, 0.2);
  }

  /* ── 打 Tag 面板 ── */
  .tag-panel {
    background: var(--bg-elevated);
    border: 1px solid rgba(86, 211, 100, 0.15);
    border-radius: 6px;
    padding: 12px 14px;
    margin-bottom: 14px;
  }
  .tag-title {
    margin: 0 0 8px;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .tag-title code {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--accent-gold);
  }
  .tag-inputs {
    display: flex;
    gap: 8px;
  }
  .tag-name,
  .tag-msg {
    background: var(--bg-surface);
    border: 1px solid var(--bg-hover);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    padding: 5px 8px;
    min-width: 0;
  }
  .tag-name {
    flex: 0 0 140px;
  }
  .tag-msg {
    flex: 1;
  }
  .tag-name:disabled,
  .tag-msg:disabled {
    opacity: 0.5;
  }
  .tag-actions {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }
  .btn-tag-confirm {
    background: rgba(86, 211, 100, 0.12);
    border: 1px solid rgba(86, 211, 100, 0.25);
    border-radius: 4px;
    color: #fff;
    cursor: pointer;
    font-size: 12px;
    padding: 5px 14px;
  }
  .btn-tag-confirm:hover:not(:disabled) {
    background: rgba(86, 211, 100, 0.18);
  }
  .btn-tag-confirm:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-tag-cancel {
    background: var(--border-default);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 5px 14px;
  }
  .btn-tag-cancel:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .btn-copy {
    flex-shrink: 0;
    background: transparent;
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
    transition: all 0.15s;
  }
  .btn-copy:hover {
    background: var(--bg-hover);
    border-color: var(--text-secondary);
    color: var(--text-secondary);
  }
  .commit-meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 10px;
  }
  .meta-row {
    display: flex;
    gap: 12px;
    align-items: baseline;
  }
  .meta-label {
    flex-shrink: 0;
    width: 28px;
    color: var(--text-muted);
    font-size: 11px;
  }
  .meta-sha {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--accent-gold);
    font-size: 11px;
    word-break: break-all;
  }
  .meta-author {
    color: var(--text-secondary);
  }
  .meta-date {
    color: var(--text-muted);
  }
  .commit-message {
    margin: 0 0 20px;
    padding: 12px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1.55;
    color: var(--text-secondary);
    white-space: pre-wrap;
    word-break: break-word;
  }

  /* ── 子模块变更(展开区间提交) ── */
  .sub-changes {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 12px;
  }
  .sub-change {
    border: 1px solid var(--border-default);
    border-radius: 6px;
    background: var(--bg-surface);
    overflow: hidden;
  }
  .sub-change-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 12px;
    background: var(--bg-surface);
    border-bottom: 1px solid var(--border-default);
    font-size: 12px;
  }
  .sub-tag {
    background: rgba(188, 140, 255, 0.12);
    color: #c49ae2;
    font-size: 10px;
    border-radius: 4px;
    padding: 1px 6px;
    flex-shrink: 0;
  }
  .sub-path {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--text-primary);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub-range {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--accent-gold);
    flex-shrink: 0;
  }
  .sub-commit-list {
    list-style: none;
    margin: 0;
    padding: 4px 0;
  }
  .sub-commit {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 4px 12px;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
  }
  .sub-c-sha {
    color: var(--accent-gold);
    flex-shrink: 0;
  }
  .sub-c-msg {
    flex: 1;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub-c-meta {
    color: var(--text-muted);
    flex-shrink: 0;
    font-size: 11px;
  }
  .sub-none {
    color: var(--text-muted);
    font-size: 12px;
    padding: 6px 12px;
    margin: 0;
  }

  /* ── Diff 列表 ── */
  .diff-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .muted {
    color: var(--text-muted);
    font-size: 12px;
    padding: 4px 14px;
  }
</style>

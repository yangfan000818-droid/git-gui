<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
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
    onFileHistory,
  }: { path: string; onFileHistory?: (filePath: string) => void } = $props();

  // ── 状态 ──
  let commits = $state<GraphCommit[]>([]);
  let loading = $state(false);
  let error = $state("");
  let maxCount = $state(50);
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
      commits = await invoke<GraphCommit[]>("repo_log_topology", {
        path,
        maxCount,
        branch: null,
        author: authorFilter || null,
        grep: grepFilter || null,
      });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function loadMore() {
    maxCount += 50;
    await load();
  }

  async function selectCommit(entry: LogEntry) {
    selectedCommit = entry;
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
        invoke<string>("repo_commit_message", { path, sha: entry.full_sha }),
        invoke<FileDiff[]>("repo_commit_files", { path, sha: entry.full_sha }),
      ]);
      commitMsg = msg;
      commitDiffs = diffs;
      void loadSubmoduleRanges(entry.full_sha, diffs);
    } catch (e) {
      detailError = String(e);
    } finally {
      detailLoading = false;
    }
  }

  // 对该提交里的每个子模块指针变化,取其 old..new 区间的子仓提交(失败则只显示指针)。
  // 传入 sha 守卫:慢请求返回时若已切到别的提交,丢弃结果避免写入过期区间。
  async function loadSubmoduleRanges(sha: string, diffs: FileDiff[]) {
    for (const f of diffs) {
      if (!isSubmoduleChange(f)) continue;
      const { old, new: newSha } = subRange(f);
      if (!old || !newSha) continue; // 仅指针变更(modify)才有区间
      try {
        const commits = await invoke<LogEntry[]>("repo_submodule_commits", {
          path,
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
        path,
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

  // ── 重置到此(Reset Current Branch to Here) ──
  async function doReset() {
    if (!selectedCommit || operationInProgress) return;
    // 硬重置会丢弃工作区改动,额外二次确认。
    if (
      resetMode === "Hard" &&
      !confirm(
        `硬重置到 ${selectedCommit.sha}：将丢弃工作区与暂存区的所有未提交改动,且当前分支会回退到该提交。此操作不可恢复,确定?`,
      )
    ) {
      return;
    }
    operationInProgress = true;
    operationError = "";
    try {
      await invoke("repo_reset", {
        path,
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
        path,
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
        path,
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
      await invoke("abort_update_cmd", { path, autostash });
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
    "#e2c47a", // gold
    "#7ac4e2", // light blue
    "#7ae2a4", // green
    "#e27ac4", // pink
    "#c4e27a", // lime
    "#7a9fe2", // blue
    "#e29f7a", // orange
    "#b47ae2", // purple
    "#e27a7a", // red
    "#7ae2e2", // cyan
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
      load();
    }, 300) as unknown as number;
  }

  // 初始化 / path 变化时重置并重载
  let prevPath = $state("");
  $effect(() => {
    if (path && path !== prevPath) {
      prevPath = path;
      maxCount = 50;
      authorFilter = "";
      grepFilter = "";
      selectedCommit = null;
      commitMsg = "";
      commitDiffs = [];
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
        <span class="list-title">提交历史 ({commits.length})</span>
        <button
          class="btn-load-more"
          onclick={() => (showReflog = true)}
          title="查看 HEAD 走过的历史(reflog),可从中恢复被变基/重置丢掉的状态"
          >Reflog</button
        >
        <button
          class="btn-load-more"
          disabled={loading}
          onclick={loadMore}
          title="再加载 50 条更早的提交">加载更多</button
        >
      </div>

      <!-- 筛选栏 -->
      <div class="filter-row">
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

      {#if commits.length === 0 && !loading}
        <p class="muted">无提交记录</p>
      {:else}
        <div class="log-scroll">
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
              {#each commits as commit, i}
                {#each commit.edges as edge}
                  {@const fromColor = laneColor(edge.from_lane)}
                  <path
                    d={edgePath(edge, i)}
                    stroke={fromColor}
                    stroke-width="1.5"
                    fill="none"
                    stroke-linecap="round"
                  />
                {/each}
              {/each}
              <!-- nodes -->
              {#each commits as commit, i}
                {@const cx = laneX(commit.lane)}
                {@const cy = i * ROW_H + ROW_H / 2}
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
          <div class="info-rows">
            {#each commits as commit, i}
              {@const entry = commit.entry}
              <div
                class="log-row"
                class:selected={selectedCommit?.full_sha === entry.full_sha}
                role="button"
                tabindex="0"
                style="height:{ROW_H}px"
                onclick={() => selectCommit(entry)}
                onkeydown={(e) => onActivate(e, () => selectCommit(entry))}
              >
                <span class="log-sha">{entry.sha}</span>
                <span class="log-author">{entry.author}</span>
                <span class="log-date">{entry.date}</span>
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
          {path}
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
            <div class="action-buttons">
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
          <div class="commit-meta">
            <span class="meta-sha" title={selectedCommit.full_sha}
              >{selectedCommit.full_sha}</span
            >
            <span class="meta-author">{selectedCommit.author}</span>
            <span class="meta-date">{selectedCommit.date}</span>
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
                          <span class="sub-c-meta">{c.author} · {c.date}</span>
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
              <DiffView files={normalDiffs} {onFileHistory} />
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
      {path}
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
      {path}
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
    background: #3a1d1d;
    border-bottom: 1px solid #6a2b2b;
    padding: 8px 14px;
    color: #f3b4b4;
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
    border-right: 1px solid #383838;
    overflow-y: auto;
    background: #212121;
    display: flex;
    flex-direction: column;
  }
  .list-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid #383838;
    flex-shrink: 0;
  }
  .list-title {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #888;
  }
  .btn-load-more {
    background: #333;
    border: 1px solid #555;
    border-radius: 4px;
    color: #ccc;
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
  }
  .btn-load-more:hover {
    background: #444;
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
    border-bottom: 1px solid #383838;
    flex-shrink: 0;
  }
  .filter-input {
    flex: 1;
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #ddd;
    font-size: 12px;
    padding: 4px 8px;
  }
  .filter-input:focus {
    outline: none;
    border-color: #0e639c;
  }
  .filter-input::placeholder {
    color: #666;
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
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1;
    white-space: nowrap;
  }
  .log-row:hover {
    background: #2a2a2a;
    cursor: pointer;
  }
  .log-row.selected {
    background: #0e639c55;
  }
  .log-sha {
    color: #e2c47a;
    flex-shrink: 0;
  }
  .log-author {
    color: #888;
    flex-shrink: 0;
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .log-date {
    color: #666;
    flex-shrink: 0;
  }
  .log-msg {
    color: #ddd;
    overflow: hidden;
    text-overflow: ellipsis;
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
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 6px;
  }
  .commit-title {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: #e4e4e4;
    line-height: 1.35;
  }
  .action-buttons {
    display: flex;
    gap: 8px;
    flex-shrink: 0;
  }
  .btn-action {
    background: #0e639c;
    border: 1px solid #0e639c;
    border-radius: 4px;
    color: #fff;
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
  }
  .btn-action:hover:not(:disabled) {
    background: #1177b8;
  }
  .btn-action:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .btn-reset {
    background: #5a4a1d;
    border-color: #7a6a3a;
  }
  .btn-reset:hover:not(:disabled) {
    background: #6a5a2d;
  }
  .btn-reset.active {
    background: #7a6a3a;
  }

  /* ── 重置面板 ── */
  .reset-panel {
    background: #252525;
    border: 1px solid #5a4a2a;
    border-radius: 6px;
    padding: 12px 14px;
    margin-bottom: 14px;
  }
  .reset-title {
    margin: 0 0 8px;
    font-size: 12px;
    color: #ccc;
  }
  .reset-title code {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: #e2c47a;
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
    color: #e4e4e4;
    font-weight: 600;
  }
  .reset-mode small {
    font-size: 11px;
    color: #888;
  }
  .reset-mode .reset-danger {
    color: #e0a0a0;
  }
  .reset-actions {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }
  .btn-reset-confirm {
    background: #5a4a1d;
    border: 1px solid #7a6a3a;
    border-radius: 4px;
    color: #fff;
    cursor: pointer;
    font-size: 12px;
    padding: 5px 14px;
  }
  .btn-reset-confirm:hover:not(:disabled) {
    background: #6a5a2d;
  }
  .btn-reset-confirm.danger {
    background: #8b2a2a;
    border-color: #a33;
  }
  .btn-reset-confirm.danger:hover:not(:disabled) {
    background: #a33;
  }
  .btn-reset-confirm:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-reset-cancel {
    background: #383838;
    border: 1px solid #555;
    border-radius: 4px;
    color: #ccc;
    cursor: pointer;
    font-size: 12px;
    padding: 5px 14px;
  }
  .btn-reset-cancel:hover:not(:disabled) {
    background: #444;
  }
  .btn-tag {
    background: #1d4a3a;
    border-color: #2a6a52;
  }
  .btn-tag:hover:not(:disabled) {
    background: #245a48;
  }
  .btn-tag.active {
    background: #2a6a52;
  }

  /* ── 打 Tag 面板 ── */
  .tag-panel {
    background: #252525;
    border: 1px solid #2a5a48;
    border-radius: 6px;
    padding: 12px 14px;
    margin-bottom: 14px;
  }
  .tag-title {
    margin: 0 0 8px;
    font-size: 12px;
    color: #ccc;
  }
  .tag-title code {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: #e2c47a;
  }
  .tag-inputs {
    display: flex;
    gap: 8px;
  }
  .tag-name,
  .tag-msg {
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #e4e4e4;
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
    background: #1d5a1d;
    border: 1px solid #3a7a3a;
    border-radius: 4px;
    color: #fff;
    cursor: pointer;
    font-size: 12px;
    padding: 5px 14px;
  }
  .btn-tag-confirm:hover:not(:disabled) {
    background: #256a25;
  }
  .btn-tag-confirm:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-tag-cancel {
    background: #383838;
    border: 1px solid #555;
    border-radius: 4px;
    color: #ccc;
    cursor: pointer;
    font-size: 12px;
    padding: 5px 14px;
  }
  .btn-tag-cancel:hover:not(:disabled) {
    background: #444;
  }
  .btn-copy {
    flex-shrink: 0;
    background: #333;
    border: 1px solid #555;
    border-radius: 4px;
    color: #ccc;
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
  }
  .btn-copy:hover {
    background: #444;
  }
  .commit-meta {
    display: flex;
    gap: 16px;
    font-size: 12px;
    color: #888;
  }
  .meta-sha {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: #e2c47a;
  }
  .commit-message {
    margin: 0 0 20px;
    padding: 12px;
    background: #252525;
    border: 1px solid #383838;
    border-radius: 6px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1.55;
    color: #ccc;
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
    border: 1px solid #383838;
    border-radius: 6px;
    background: #232323;
    overflow: hidden;
  }
  .sub-change-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 12px;
    background: #2a2a2a;
    border-bottom: 1px solid #333;
    font-size: 12px;
  }
  .sub-tag {
    background: #2d1d3a;
    color: #c49ae2;
    font-size: 10px;
    border-radius: 4px;
    padding: 1px 6px;
    flex-shrink: 0;
  }
  .sub-path {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: #e4e4e4;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub-range {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: #e2c47a;
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
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
  }
  .sub-c-sha {
    color: #e2c47a;
    flex-shrink: 0;
  }
  .sub-c-msg {
    flex: 1;
    color: #ddd;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub-c-meta {
    color: #777;
    flex-shrink: 0;
    font-size: 11px;
  }
  .sub-none {
    color: #666;
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
    color: #666;
    font-size: 12px;
    padding: 4px 14px;
  }
</style>

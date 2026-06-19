<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";

  // ── 类型 ──
  interface UpdateOptions {
    strategy: "Merge" | "Rebase";
    ignore_whitespace: boolean;
    recurse_submodules: boolean;
  }
  interface UpdatePlan {
    upstream: string;
    behind: number;
    ahead: number;
    can_fast_forward: boolean;
    will_autostash: boolean;
  }
  interface Progress {
    phase: string;
    percent: number | null;
    raw: string;
  }
  interface StashRef {
    label: string;
  }
  interface FastForwardedData {
    commits: number;
  }
  interface IntegratedData {
    commits: number;
    strategy: "Merge" | "Rebase";
  }
  interface ConflictedData {
    files: string[];
    autostash: StashRef | null;
  }
  interface StashRestoreConflictData {
    files: string[];
  }
  interface SubmoduleSyncFailedData {
    error: string;
  }
  // UpdateOutcome: externally tagged enum
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
    | { FastForwarded: FastForwardedData }
    | { Integrated: IntegratedData }
    | { Conflicted: ConflictedData }
    | { StashRestoreConflict: StashRestoreConflictData }
    | { SubmoduleSyncFailed: SubmoduleSyncFailedData };

  function outcomeVariant(o: UpdateOutcome): OutcomeVariant {
    if (typeof o === "string") return o as OutcomeVariant;
    return Object.keys(o)[0] as OutcomeVariant;
  }
  function outcomeData<T>(o: UpdateOutcome): T | undefined {
    if (typeof o === "string") return undefined;
    return (o as unknown as Record<string, T>)[Object.keys(o)[0]];
  }

  let { path, onRefresh }: { path: string; onRefresh: () => Promise<void> } =
    $props();

  // ── 策略选项 ──
  let strategy = $state<"Merge" | "Rebase">("Merge");
  let ignoreWhitespace = $state(true);
  let recurseSubmodules = $state(true);

  function buildOptions(): UpdateOptions {
    return {
      strategy,
      ignore_whitespace: ignoreWhitespace,
      recurse_submodules: recurseSubmodules,
    };
  }

  // ── 流程状态机 ──
  type Phase =
    | "idle"
    | "planning"
    | "plan_shown"
    | "executing"
    | "outcome"
    | "conflict_resolution";
  let phase = $state<Phase>("idle");
  let error = $state("");
  let plan = $state<UpdatePlan | null>(null);
  let outcome = $state<UpdateOutcome | null>(null);
  let opId = $state("");
  let progress = $state<Progress | null>(null);

  // ── 冲突解决状态 ──
  let conflictFiles = $state<string[]>([]);
  let autostash = $state<StashRef | null>(null);
  let fileContents = $state<Record<string, string>>({});
  let fileResolving = $state<Set<string>>(new Set());

  let unlisten: UnlistenFn | null = null;

  // 清理事件监听器
  function cleanup() {
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
  }

  // ── Step 1: 计划 ──
  async function doPlan() {
    phase = "planning";
    error = "";
    plan = null;
    try {
      plan = await invoke<UpdatePlan>("plan_update", {
        path,
        options: buildOptions(),
      });
      phase = "plan_shown";
    } catch (e) {
      error = String(e);
      phase = "idle";
    }
  }

  // ── Step 2: 执行 ──
  async function doExecute() {
    phase = "executing";
    error = "";
    outcome = null;
    progress = null;
    opId = crypto.randomUUID();

    // 注册进度事件监听
    try {
      unlisten = await listen<Progress>("update-progress", (event) => {
        progress = event.payload;
      });
    } catch {
      // 监听失败不阻塞(事件通道问题不应阻止更新)
    }

    try {
      const result = await invoke<UpdateOutcome>("execute_update", {
        path,
        opId,
        options: buildOptions(),
      });
      outcome = result;
      phase = "outcome";
      dispatchOutcome(result);
    } catch (e) {
      error = String(e);
      phase = "idle";
    } finally {
      cleanup();
    }
  }

  // ── Step 3: 终态分发 ──
  function dispatchOutcome(o: UpdateOutcome) {
    const v = outcomeVariant(o);
    if (v === "Conflicted") {
      const d = outcomeData<ConflictedData>(o)!;
      conflictFiles = d.files;
      autostash = d.autostash;
      phase = "conflict_resolution";
      // 加载各冲突文件内容
      loadConflictContents(d.files);
    }
    // AlreadyUpToDate / FastForwarded / Integrated / Resolved / StashRestoreConflict / SubmoduleSyncFailed
    // 由 UI 直接展示,用户手动刷新
  }

  async function loadConflictContents(files: string[]) {
    const contents: Record<string, string> = {};
    for (const f of files) {
      try {
        contents[f] = await invoke<string>("read_repo_file", {
          path,
          filePath: f,
        });
      } catch (e) {
        contents[f] = `[读取失败: ${e}]`;
      }
    }
    fileContents = contents;
  }

  // ── 冲突解决 ──
  async function resolveFile(filePath: string, text: string) {
    fileResolving = new Set(fileResolving);
    fileResolving.add(filePath);
    fileResolving = new Set(fileResolving);
    error = "";
    try {
      await invoke("resolve_conflict_file", { path, filePath, text });
    } catch (e) {
      error = String(e);
    } finally {
      fileResolving = new Set(fileResolving);
      fileResolving.delete(filePath);
      fileResolving = new Set(fileResolving);
    }
  }

  async function doContinue() {
    error = "";
    try {
      const result = await invoke<UpdateOutcome>("continue_update_cmd", {
        path,
        autostash,
        recurseSubmodules,
      });
      outcome = result;
      phase = "outcome";
      dispatchOutcome(result);
    } catch (e) {
      error = String(e);
    }
  }

  async function doAbort() {
    if (!confirm("确定放弃本次更新整合？工作区将回到更新前的状态。")) return;
    error = "";
    try {
      await invoke("abort_update_cmd", { path, autostash });
      await onRefresh();
      reset();
    } catch (e) {
      error = String(e);
    }
  }

  // ── 成功后的刷新 ──
  async function finishAndRefresh() {
    await onRefresh();
    reset();
  }

  function reset() {
    cleanup();
    phase = "idle";
    error = "";
    plan = null;
    outcome = null;
    progress = null;
    conflictFiles = [];
    autostash = null;
    fileContents = {};
    fileResolving = new Set();
  }

  function cancelPlan() {
    reset();
  }

  function cancelOp() {
    if (opId) {
      invoke("cancel_op", { opId });
    }
  }

  // ── 辅助 ──
  function onActivate(e: KeyboardEvent, fn: () => void) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fn();
    }
  }

  function variantLabel(v: OutcomeVariant | string): string {
    switch (v) {
      case "AlreadyUpToDate":
        return "已是最新";
      case "FastForwarded":
        return "快进完成";
      case "Integrated":
        return "整合完成";
      case "Conflicted":
        return "发现冲突";
      case "StashRestoreConflict":
        return "Stash 还原冲突";
      case "Resolved":
        return "冲突已解决";
      case "SubmoduleSyncFailed":
        return "子仓库同步失败";
      default:
        return v;
    }
  }
</script>

<div class="update-view">
  {#if error}
    <pre class="update-error">{error}</pre>
  {/if}

  <!-- ── idle: 检查更新入口 ── -->
  {#if phase === "idle"}
    <div class="update-idle">
      <div class="strategy-bar">
        <fieldset class="strategy-group">
          <legend>整合策略</legend>
          <label class="radio-label">
            <input
              type="radio"
              name="strategy"
              value="Merge"
              bind:group={strategy}
            />
            Merge
          </label>
          <label class="radio-label">
            <input
              type="radio"
              name="strategy"
              value="Rebase"
              bind:group={strategy}
            />
            Rebase
          </label>
        </fieldset>
      </div>
      <button class="btn-primary" onclick={doPlan}> 检查更新 </button>
    </div>
  {/if}

  <!-- ── planning: 加载中 ── -->
  {#if phase === "planning"}
    <p class="update-status">联网检查中…</p>
  {/if}

  <!-- ── plan_shown: 展示计划 ── -->
  {#if phase === "plan_shown" && plan}
    <div class="plan-card">
      <h3>更新计划</h3>
      <dl class="plan-fields">
        <dt>upstream</dt>
        <dd>{plan.upstream}</dd>
        <dt>落后</dt>
        <dd>{plan.behind} 个提交</dd>
        <dt>领先</dt>
        <dd>{plan.ahead} 个提交</dd>
        <dt>快进</dt>
        <dd>{plan.can_fast_forward ? "是 (直接快进)" : "否 (需整合)"}</dd>
        <dt>自动 stash</dt>
        <dd>{plan.will_autostash ? "是 (工作区有未保存改动)" : "否"}</dd>
      </dl>
      <div class="plan-actions">
        <button class="btn-primary" onclick={doExecute}> 确认更新 </button>
        <button class="btn-secondary" onclick={cancelPlan}> 取消 </button>
      </div>
    </div>
  {/if}

  <!-- ── executing: 进度条 + 取消 ── -->
  {#if phase === "executing"}
    <div class="executing">
      <div class="progress-bar-wrap">
        <div
          class="progress-bar-fill"
          style="width: {progress?.percent ?? 0}%"
          role="progressbar"
          aria-valuenow={progress?.percent ?? 0}
          aria-valuemin="0"
          aria-valuemax="100"
        ></div>
        <span class="progress-text">
          {progress?.phase ?? "准备中…"}
          {#if progress?.percent != null}
            ({progress?.percent}%){/if}
        </span>
      </div>
      {#if progress?.raw}
        <pre class="progress-raw">{progress.raw}</pre>
      {/if}
      <button class="btn-danger" onclick={cancelOp}> 取消 </button>
    </div>
  {/if}

  <!-- ── outcome: 终态展示 ── -->
  {#if phase === "outcome" && outcome}
    {@const v = outcomeVariant(outcome)}
    <div
      class="outcome-card"
      class:outcome-success={v === "AlreadyUpToDate" ||
        v === "FastForwarded" ||
        v === "Integrated" ||
        v === "Resolved"}
      class:outcome-warn={v === "StashRestoreConflict" ||
        v === "SubmoduleSyncFailed"}
    >
      <h3>{variantLabel(v)}</h3>
      {#if v === "AlreadyUpToDate"}
        <p>仓库已是最新，无需更新。</p>
      {:else if v === "FastForwarded"}
        {@const d = outcomeData<FastForwardedData>(outcome)!}
        <p>已快进 {d.commits} 个提交。</p>
      {:else if v === "Integrated"}
        {@const d = outcomeData<IntegratedData>(outcome)!}
        <p>
          已通过 {d.strategy === "Merge" ? "合并" : "变基"} 整合 {d.commits} 个提交。
        </p>
      {:else if v === "Resolved"}
        <p>冲突已解决，整合完成。</p>
      {:else if v === "StashRestoreConflict"}
        {@const d = outcomeData<StashRestoreConflictData>(outcome)!}
        <p>
          整合已成功，但还原之前的工作区改动时发生冲突。以下文件需要手动处理：
        </p>
        <ul class="file-list">
          {#each d.files as f}
            <li>{f}</li>
          {/each}
        </ul>
        <p class="hint">
          用 git stash pop 取出 stash 中的改动，手动合并后提交。
        </p>
      {:else if v === "SubmoduleSyncFailed"}
        {@const d = outcomeData<SubmoduleSyncFailedData>(outcome)!}
        <p>主仓库已更新，但子仓库同步失败：</p>
        <pre class="update-error">{d.error}</pre>
      {/if}
      <button class="btn-primary" onclick={finishAndRefresh}> 刷新 </button>
    </div>
  {/if}

  <!-- ── conflict_resolution: 冲突解决 ── -->
  {#if phase === "conflict_resolution"}
    <div class="conflict-view">
      <h3>解决冲突 ({conflictFiles.length} 个文件)</h3>
      <p class="hint">
        编辑下方文件移除冲突标记（&lt;&lt;&lt;&lt;&lt;&lt;&lt; / ======= /
        &gt;&gt;&gt;&gt;&gt;&gt;&gt;），然后点击"标记已解决"。
      </p>
      {#each conflictFiles as f}
        <div class="conflict-file">
          <h4 class="conflict-fpath">{f}</h4>
          <textarea
            class="conflict-textarea"
            rows={Math.max(10, (fileContents[f] ?? "").split("\n").length)}
            value={fileContents[f] ?? ""}
            oninput={(e) => (fileContents[f] = e.currentTarget.value)}
            disabled={fileResolving.has(f)}></textarea>
          <button
            class="btn-primary btn-sm"
            disabled={fileResolving.has(f)}
            onclick={() => resolveFile(f, fileContents[f] ?? "")}
          >
            {fileResolving.has(f) ? "解决中…" : "标记已解决: " + f}
          </button>
        </div>
      {/each}
      <div class="conflict-actions">
        <button class="btn-primary" onclick={doContinue}> 继续整合 </button>
        <button class="btn-danger" onclick={doAbort}> 放弃整合 (abort) </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .update-view {
    padding: 16px;
    font-size: 13px;
  }
  .update-error {
    background: #3a1d1d;
    border: 1px solid #6a2b2b;
    border-radius: 6px;
    padding: 8px 12px;
    color: #f3b4b4;
    white-space: pre-wrap;
    font-size: 12px;
    margin: 0 0 12px;
  }
  .update-status {
    color: #888;
  }
  .hint {
    color: #888;
    font-size: 12px;
  }

  /* ── 策略选择 ── */
  .strategy-bar {
    margin-bottom: 12px;
  }
  .strategy-group {
    border: 1px solid #444;
    border-radius: 6px;
    padding: 6px 12px 8px;
    display: inline-flex;
    gap: 12px;
  }
  .strategy-group legend {
    color: #888;
    font-size: 11px;
    text-transform: uppercase;
  }
  .radio-label {
    color: #ccc;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 13px;
  }

  /* ── 按钮 ── */
  .btn-primary {
    background: #0e639c;
    border: none;
    border-radius: 6px;
    color: #fff;
    padding: 6px 16px;
    font-size: 13px;
    cursor: pointer;
  }
  .btn-primary:hover {
    background: #1177bb;
  }
  .btn-primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-secondary {
    background: #383838;
    border: 1px solid #555;
    border-radius: 6px;
    color: #ccc;
    padding: 6px 16px;
    font-size: 13px;
    cursor: pointer;
  }
  .btn-secondary:hover {
    background: #444;
  }
  .btn-danger {
    background: #8b2a2a;
    border: none;
    border-radius: 6px;
    color: #fff;
    padding: 6px 16px;
    font-size: 13px;
    cursor: pointer;
  }
  .btn-danger:hover {
    background: #a33;
  }
  .btn-sm {
    padding: 4px 12px;
    font-size: 12px;
  }

  /* ── 计划卡片 ── */
  .plan-card {
    background: #252525;
    border: 1px solid #383838;
    border-radius: 8px;
    padding: 14px 18px;
    margin-bottom: 12px;
    max-width: 480px;
  }
  .plan-card h3 {
    margin: 0 0 10px;
    font-size: 14px;
  }
  .plan-fields {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 16px;
    margin: 0 0 14px;
  }
  .plan-fields dt {
    color: #888;
    font-size: 12px;
  }
  .plan-fields dd {
    color: #e4e4e4;
    margin: 0;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
  }
  .plan-actions {
    display: flex;
    gap: 8px;
  }

  /* ── 进度条 ── */
  .executing {
    max-width: 480px;
  }
  .progress-bar-wrap {
    position: relative;
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 6px;
    height: 28px;
    overflow: hidden;
    margin-bottom: 8px;
  }
  .progress-bar-fill {
    background: #0e639c;
    height: 100%;
    transition: width 0.3s ease;
    border-radius: 5px 0 0 5px;
    min-width: 0;
  }
  .progress-text {
    position: absolute;
    top: 50%;
    left: 12px;
    transform: translateY(-50%);
    color: #e4e4e4;
    font-size: 12px;
  }
  .progress-raw {
    color: #666;
    font-size: 11px;
    margin: 4px 0 8px;
    white-space: pre-wrap;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }

  /* ── 终态卡片 ── */
  .outcome-card {
    background: #252525;
    border: 1px solid #383838;
    border-radius: 8px;
    padding: 14px 18px;
    max-width: 480px;
  }
  .outcome-success {
    border-color: #2d5a2d;
  }
  .outcome-warn {
    border-color: #5a4a2d;
  }
  .outcome-card h3 {
    margin: 0 0 8px;
    font-size: 14px;
  }
  .outcome-card p {
    margin: 4px 0;
  }
  .outcome-card .file-list {
    color: #f3b4b4;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    padding-left: 20px;
  }

  /* ── 冲突解决 ── */
  .conflict-view {
    max-width: 640px;
  }
  .conflict-view h3 {
    margin: 0 0 4px;
  }
  .conflict-file {
    margin-bottom: 14px;
    padding: 10px;
    background: #212121;
    border: 1px solid #383838;
    border-radius: 6px;
  }
  .conflict-fpath {
    margin: 0 0 6px;
    font-size: 13px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: #f3b4b4;
  }
  .conflict-textarea {
    width: 100%;
    box-sizing: border-box;
    background: #1a1a1a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #e4e4e4;
    padding: 8px 10px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    resize: vertical;
    margin-bottom: 6px;
    line-height: 1.5;
    tab-size: 4;
  }
  .conflict-textarea:disabled {
    opacity: 0.5;
  }
  .conflict-actions {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }
</style>

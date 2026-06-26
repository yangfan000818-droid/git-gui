<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";
  import ConflictView from "$lib/ConflictView.svelte";

  // ── 类型 ──
  interface PendingConflicts {
    files: string[];
    autostash: StashRef | null;
  }
  interface UpdateOptions {
    strategy: "Merge" | "Rebase";
    ignore_whitespace: boolean;
    recurse_submodules: boolean;
  }
  interface AppSettings {
    update_strategy: "Merge" | "Rebase";
    ignore_whitespace: boolean;
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

  let {
    path,
    submodules = [],
    title = "全部更新",
    subsOnly = false,
    pushAfterSuccess = false,
    onRefresh,
    onClose,
    onFinished,
  }: {
    path: string;
    submodules: { name: string; path: string; status: string }[];
    title?: string;
    // 仅子仓模式:跳过主仓库更新,直接逐个在各自当前分支更新子仓库。
    subsOnly?: boolean;
    // 「更新后推送」:整合成功后自动推送本仓(被拒于"远端领先"时由推送入口开启)。
    pushAfterSuccess?: boolean;
    onRefresh: () => Promise<void>;
    // 取消/放弃/手动关闭时调用(队列推送中用它停止整个队列)。
    onClose: () => void;
    // 成功完成(含更新后推送)时调用;提供则取代 onClose,供「全部推送」队列推进到下一个。
    onFinished?: () => void;
  } = $props();

  // 仅子仓模式的范围提示:未初始化的将 init,其余 on-branch 更新。
  let subInitCount = $derived(
    submodules.filter((s) => s.status === "Uninitialized").length,
  );
  let subUpdateCount = $derived(submodules.length - subInitCount);

  // ── 整合选项(onMount 从全局设置载入,不再每次弹选) ──
  let strategy = $state<"Merge" | "Rebase">("Merge");
  let ignoreWhitespace = $state(true);
  // 全部更新:主仓库整合时不自动递归子模块,子仓库改为在各自当前分支上 pull(留在原分支,不 detach)。
  let recurseSubmodules = $state(false);

  function buildOptions(): UpdateOptions {
    return {
      strategy,
      ignore_whitespace: ignoreWhitespace,
      recurse_submodules: recurseSubmodules,
    };
  }

  // ── 流程状态机(一键:打开即执行,无策略选择/计划预览) ──
  // idle 仅作出错重试态;首屏直接进 executing,onMount 自动开跑。
  type Phase =
    | "idle"
    | "executing"
    | "submodules_updating"
    | "outcome"
    | "conflict_resolution";
  let phase = $state<Phase>("executing");
  let error = $state("");
  let cancelled = $state(false);
  let outcome = $state<UpdateOutcome | null>(null);
  let opId = $state("");
  let progress = $state<Progress | null>(null);

  // ── 冲突解决状态 ──
  let conflictFiles = $state<string[]>([]);
  let autostash = $state<StashRef | null>(null);
  // 正在解决冲突的仓库路径(主仓库 path 或子仓绝对路径);
  // resolvingSubIndex 非 null = 正在解决第 N 个子仓的冲突,解决/放弃后从 N+1 续跑。
  let conflictPath = $state("");
  let resolvingSubIndex = $state<number | null>(null);

  // ── 子仓库更新状态(主仓库整合成功后,逐个在各自当前分支 pull) ──
  // SubmoduleUpdate: 外部 tagged 枚举(同 UpdateOutcome)
  type SubConflictData = {
    repo_path: string;
    files: string[];
    autostash: StashRef | null;
  };
  type SubmoduleUpdate =
    | "UpToDate"
    | "SyncedToRecorded"
    | "SkippedNoUpstream"
    | "StashConflict"
    | { Updated: { commits: number } }
    | { Conflicted: SubConflictData };

  interface SubResult {
    label: string;
    status: "ok" | "warn" | "fail";
    detail: string;
  }
  let subResults = $state<SubResult[]>([]);
  let subCurrent = $state(""); // 正在更新的子仓库路径

  function isSubConflicted(
    r: SubmoduleUpdate,
  ): r is { Conflicted: SubConflictData } {
    return typeof r === "object" && "Conflicted" in r;
  }

  // 把(非冲突的)SubmoduleUpdate 结果映射为展示(图标状态 + 文案)。
  function describeSub(r: SubmoduleUpdate): {
    status: SubResult["status"];
    detail: string;
  } {
    if (typeof r === "object") {
      if ("Updated" in r) {
        return { status: "ok", detail: `已更新 ${r.Updated.commits} 个提交` };
      }
      return { status: "warn", detail: "冲突" }; // Conflicted 已在循环里拦截,兜底
    }
    switch (r) {
      case "UpToDate":
        return { status: "ok", detail: "已是最新" };
      case "SyncedToRecorded":
        return { status: "ok", detail: "detached,已同步到记录提交" };
      case "SkippedNoUpstream":
        return { status: "warn", detail: "跳过:无上游分支" };
      case "StashConflict":
        return {
          status: "warn",
          detail: "整合成功但 stash 还原冲突,请手动处理",
        };
    }
  }
  function subIcon(s: SubResult["status"]): string {
    return s === "ok" ? "✓" : s === "warn" ? "⚠" : "✕";
  }

  let unlisten: UnlistenFn | null = null;
  // 成功后延迟自动关闭的定时器(让 outcome 成功提示停留可见)。
  let autoCloseTimer: ReturnType<typeof setTimeout> | null = null;

  // 清理事件监听器。注意:不在此清 autoCloseTimer——doExecute 的 finally 会调
  // cleanup(),那发生在排程自动关闭之后,清掉会导致(全部更新/仅主仓)永不自动关。
  function cleanup() {
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
  }

  // 中断待触发的自动关闭定时器(手动完成/放弃/卸载时调)。
  function clearAutoClose() {
    if (autoCloseTimer) {
      clearTimeout(autoCloseTimer);
      autoCloseTimer = null;
    }
  }

  // ── 一键入口:子仓模式逐个更新子仓,否则直接执行主仓更新(内部已含 fetch) ──
  async function startUpdate() {
    if (subsOnly) {
      await proceedToSubmodules();
    } else {
      await doExecute();
    }
  }

  // ── 执行 ──
  async function doExecute() {
    phase = "executing";
    error = "";
    cancelled = false;
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
      await handleMainOutcome(result);
    } catch (e) {
      if (cancelled) {
        // 取消是主动行为:直接关闭弹层,不停在 idle(避免"取消了还留个弹窗")。
        cancelled = false;
        cleanup();
        onClose();
        return;
      }
      error = String(e);
      phase = "idle";
    } finally {
      cleanup();
    }
  }

  // ── Step 3: 主仓库终态分流 ──
  // 冲突 → 进冲突解决(主仓库);其余终态 → 主仓库已完成,继续逐个更新子仓库。
  async function handleMainOutcome(o: UpdateOutcome) {
    outcome = o;
    const v = outcomeVariant(o);
    if (v === "Conflicted") {
      const d = outcomeData<ConflictedData>(o)!;
      conflictPath = path; // 主仓库
      resolvingSubIndex = null;
      conflictFiles = d.files;
      autostash = d.autostash;
      phase = "conflict_resolution";
      return;
    }
    // AlreadyUpToDate / FastForwarded / Integrated / Resolved / StashRestoreConflict
    await proceedToSubmodules();
  }

  // 主仓库整合完成后,把每个子仓库在**各自当前分支**上更新(pull,留在原分支,不 detach)。
  async function proceedToSubmodules() {
    if (submodules.length === 0) {
      phase = "outcome";
      if (canAutoClose()) scheduleAutoClose();
      return;
    }
    subResults = [];
    await processSubmodulesFrom(0);
  }

  // 从第 start 个子仓起逐个更新;遇冲突则暂停进 ConflictView,解决/放弃后从下一个续跑。
  // 单个失败不阻断,逐个记录结果,全部处理完进 outcome 汇总。
  async function processSubmodulesFrom(start: number) {
    phase = "submodules_updating";
    for (let i = start; i < submodules.length; i++) {
      const sub = submodules[i];
      subCurrent = sub.path;
      // 未初始化子仓:只做 init(无冲突可能),不走 on-branch 更新。
      if (sub.status === "Uninitialized") {
        try {
          await invoke("repo_submodule_update", { path, subPath: sub.path });
          subResults = [
            ...subResults,
            { label: sub.path, status: "ok", detail: "已初始化" },
          ];
        } catch (e) {
          subResults = [
            ...subResults,
            { label: sub.path, status: "fail", detail: String(e) },
          ];
        }
        continue;
      }
      try {
        const r = await invoke<SubmoduleUpdate>(
          "repo_update_submodule_on_branch",
          { path, subPath: sub.path, options: buildOptions() },
        );
        if (isSubConflicted(r)) {
          // 暂停循环:进该子仓的冲突解决(复用 ConflictView),解决/放弃后从 i+1 续跑。
          subCurrent = "";
          resolvingSubIndex = i;
          conflictPath = r.Conflicted.repo_path;
          conflictFiles = r.Conflicted.files;
          autostash = r.Conflicted.autostash;
          phase = "conflict_resolution";
          return;
        }
        const { status, detail } = describeSub(r);
        subResults = [...subResults, { label: sub.path, status, detail }];
      } catch (e) {
        subResults = [
          ...subResults,
          { label: sub.path, status: "fail", detail: String(e) },
        ];
      }
    }
    subCurrent = "";
    phase = "outcome";
    if (canAutoClose()) scheduleAutoClose();
  }

  // 子仓冲突解决/放弃后:记录该子仓结果,从下一个子仓继续。
  async function resumeAfterSub(status: SubResult["status"], detail: string) {
    const idx = resolvingSubIndex!;
    subResults = [
      ...subResults,
      { label: submodules[idx].path, status, detail },
    ];
    resolvingSubIndex = null;
    conflictPath = "";
    conflictFiles = [];
    autostash = null;
    await processSubmodulesFrom(idx + 1);
  }

  async function doContinue() {
    error = "";
    try {
      const result = await invoke<UpdateOutcome>("continue_update_cmd", {
        path: conflictPath,
        autostash,
        recurseSubmodules:
          resolvingSubIndex === null ? recurseSubmodules : false,
      });
      if (resolvingSubIndex === null) {
        await handleMainOutcome(result);
        return;
      }
      // 子仓:仍有冲突 → 留在解决界面;否则记为已解决并续跑剩余子仓。
      if (outcomeVariant(result) === "Conflicted") {
        conflictFiles = outcomeData<ConflictedData>(result)!.files;
        return;
      }
      await resumeAfterSub("ok", "冲突已解决");
    } catch (e) {
      error = String(e);
    }
  }

  async function doAbort() {
    error = "";
    if (resolvingSubIndex === null) {
      // 主仓库:放弃整个更新整合,回到更新前。
      if (
        !(await ask("确定放弃本次更新整合？工作区将回到更新前的状态。", {
          title: "放弃整合",
          kind: "warning",
        }))
      )
        return;
      try {
        await invoke("abort_update_cmd", { path: conflictPath, autostash });
        await onRefresh();
        reset();
        onClose();
      } catch (e) {
        error = String(e);
      }
      return;
    }
    // 子仓:只放弃该子仓的更新(回到更新前),其余子仓继续。
    const subLabel = submodules[resolvingSubIndex].path;
    if (
      !(await ask(
        `确定放弃子仓「${subLabel}」的更新？它将回到更新前;其余子仓继续。`,
        { title: "放弃整合", kind: "warning" },
      ))
    )
      return;
    try {
      await invoke("abort_update_cmd", { path: conflictPath, autostash });
      await resumeAfterSub("warn", "已放弃,保持原状");
    } catch (e) {
      error = String(e);
    }
  }

  // 成功类终态(可自动关闭 / 可在其后推送)。
  function isSuccessOutcome(): boolean {
    if (!outcome) return false;
    const v = outcomeVariant(outcome);
    return (
      v === "AlreadyUpToDate" ||
      v === "FastForwarded" ||
      v === "Integrated" ||
      v === "Resolved"
    );
  }

  // 「更新后推送」:整合成功后自动推送本仓。成功返回 true;失败停在 idle 显示错误(不关闭),返回 false。
  async function doPushAfter(): Promise<boolean> {
    try {
      const r = await invoke<"Success" | "NoUpstream" | "NonFastForward">(
        "repo_push",
        { path },
      );
      if (r === "Success") return true;
      error =
        r === "NonFastForward"
          ? "已更新,但远端又有新提交,推送再次被拒绝,请重试。"
          : "已更新,但当前分支没有 upstream,未能推送。";
    } catch (e) {
      error = String(e);
    }
    phase = "idle";
    return false;
  }

  // ── 成功后刷新并关闭弹层 ──
  async function finishAndRefresh() {
    // 「更新后推送」:整合成功后先推送,推送失败则停在 idle 显示错误、不关闭。
    if (pushAfterSuccess && isSuccessOutcome() && !(await doPushAfter()))
      return;
    await onRefresh();
    reset();
    // 队列推送:成功完成走 onFinished(推进下一个);否则常规关闭。
    if (onFinished) onFinished();
    else onClose();
  }

  // 是否可以自动关闭：子仓无失败/警告,且(仅子仓模式)或(主仓成功类 outcome)。
  function canAutoClose(): boolean {
    // 子仓有失败/警告 → 保留弹窗展示结果,不自动关。
    if (subResults.some((r) => r.status === "fail" || r.status === "warn"))
      return false;
    // 仅子仓模式无主仓 outcome,子仓全部成功即可自动关。
    if (subsOnly) return true;
    // 主仓模式:需成功类 outcome。
    if (!outcome) return false;
    const v = outcomeVariant(outcome);
    return (
      v === "AlreadyUpToDate" ||
      v === "FastForwarded" ||
      v === "Integrated" ||
      v === "Resolved"
    );
  }

  // 成功且可自动关闭:让 outcome 成功提示停留 3s 再刷新关闭(否则一闪而过)。
  function scheduleAutoClose() {
    if (autoCloseTimer) clearTimeout(autoCloseTimer);
    autoCloseTimer = setTimeout(() => {
      autoCloseTimer = null;
      // 期间用户可能已手动完成/进入冲突,再确认仍是可自动关的 outcome 态。
      if (phase === "outcome" && canAutoClose()) void finishAndRefresh();
    }, 3000);
  }

  function reset() {
    cleanup();
    clearAutoClose();
    phase = "idle";
    error = "";
    outcome = null;
    progress = null;
    conflictFiles = [];
    autostash = null;
    conflictPath = "";
    resolvingSubIndex = null;
    subResults = [];
    subCurrent = "";
    cancelled = false;
  }

  // 打开即:载入全局设置 → 检测中断的整合(有则进冲突解决) → 否则一键开跑。
  onMount(async () => {
    try {
      const s = await invoke<AppSettings>("get_settings");
      strategy = s.update_strategy;
      ignoreWhitespace = s.ignore_whitespace;
    } catch {
      // 读设置失败用默认值(Merge / 忽略空白)
    }
    // 仅子仓模式不动主仓库,跳过主仓库未完成整合的检测。
    if (!subsOnly) {
      try {
        const pending = await invoke<PendingConflicts | null>(
          "resume_conflicts",
          { path },
        );
        if (pending && pending.files.length > 0) {
          conflictPath = path; // 中断的整合是主仓库的
          resolvingSubIndex = null;
          conflictFiles = pending.files;
          autostash = pending.autostash;
          phase = "conflict_resolution";
          return; // 先解决中断的冲突,不自动开跑
        }
      } catch {
        // 仓库无未完成整合，正常忽略
      }
    }
    await startUpdate();
  });

  onDestroy(() => {
    cleanup();
    clearAutoClose();
  });

  function cancelOp() {
    // executing 阶段:设标志让 doExecute catch 静默处理
    if (phase === "executing") {
      cancelled = true;
      if (opId) invoke("cancel_op", { opId });
      return;
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
  <div class="update-header">
    <h2 class="update-title">{title}</h2>
    <button
      class="btn-close"
      onclick={onClose}
      disabled={phase === "executing" || phase === "submodules_updating"}
      title="关闭"
    >
      ✕
    </button>
  </div>

  {#if error}
    <pre class="update-error">{error}</pre>
  {/if}

  <!-- ── idle: 出错/重试态(首屏由 onMount 自动进入执行,无需手动点) ── -->
  {#if phase === "idle"}
    <div class="update-idle">
      {#if subsOnly}
        <p class="update-scope">
          {#if subInitCount > 0 && subUpdateCount > 0}
            将初始化 {subInitCount} 个、并把 {subUpdateCount}
            个子仓库在各自当前分支上更新（留在原分支）。
          {:else if subInitCount > 0}
            将初始化 {subInitCount} 个子仓库（拉到父仓库记录的提交）。
          {:else}
            将把 {subUpdateCount} 个子仓库在各自当前分支上更新（留在原分支）。
          {/if}
        </p>
      {:else}
        <p class="update-scope">
          将更新主仓库{#if submodules.length > 0}，并把 {submodules.length}
            个子仓库在各自当前分支上更新（留在原分支）{/if}。
        </p>
      {/if}
      <button
        class="btn-primary"
        onclick={startUpdate}
        title="按全局策略更新:fetch → 整合（autostash 兜底）;有冲突会停下逐个解决"
      >
        {error ? "重试" : "更新"}
      </button>
    </div>
  {/if}

  <!-- ── executing: 进度条 + 取消 ── -->
  {#if phase === "executing"}
    <div class="executing">
      <div class="progress-bar-wrap">
        <div
          class="progress-bar-fill"
          class:indeterminate={progress?.percent == null}
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
      <button
        class="btn-danger"
        onclick={cancelOp}
        title="取消正在进行的更新（fetch/整合）"
      >
        取消
      </button>
    </div>
  {/if}

  <!-- ── submodules_updating: 逐个更新子仓库 ── -->
  {#if phase === "submodules_updating"}
    <div class="sub-updating">
      <p class="sub-updating-title">正在更新子仓库（各自当前分支 pull）…</p>
      <ul class="sub-progress-list">
        {#each subResults as r (r.label)}
          <li
            class:sub-fail={r.status === "fail"}
            class:sub-warn={r.status === "warn"}
          >
            <span class="sub-icon">{subIcon(r.status)}</span>
            {r.label}
          </li>
        {/each}
        {#if subCurrent}
          <li class="sub-current">
            <span class="sub-icon">⋯</span>
            {subCurrent}
          </li>
        {/if}
      </ul>
    </div>
  {/if}

  <!-- ── outcome: 终态展示 ── -->
  {#if phase === "outcome" && (outcome || subsOnly)}
    {@const subProblem = subResults.some(
      (r) => r.status === "fail" || r.status === "warn",
    )}
    <div
      class="outcome-card"
      class:outcome-success={outcome ? isSuccessOutcome() : !subProblem}
      class:outcome-warn={outcome ? !isSuccessOutcome() : subProblem}
    >
      {#if outcome}
        {@const v = outcomeVariant(outcome)}
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
      {:else}
        <h3>子仓库更新完成</h3>
      {/if}

      {#if subResults.length > 0}
        <div class="sub-results">
          <h4>子仓库更新</h4>
          <ul class="sub-result-list">
            {#each subResults as r (r.label)}
              <li
                class:sub-fail={r.status === "fail"}
                class:sub-warn={r.status === "warn"}
              >
                <span class="sub-icon">{subIcon(r.status)}</span>
                <span class="sub-label">{r.label}</span>
                <span class="sub-detail" title={r.detail}>{r.detail}</span>
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      <button
        class="btn-primary"
        onclick={finishAndRefresh}
        title="完成,刷新仓库状态"
      >
        完成
      </button>
    </div>
  {/if}

  <!-- ── conflict_resolution: 冲突解决（三栏视图） ── -->
  {#if phase === "conflict_resolution"}
    {#if resolvingSubIndex !== null}
      <p class="sub-conflict-banner">
        子仓库「{submodules[resolvingSubIndex].path}」更新冲突 —
        解决后将继续更新剩余子仓库
      </p>
    {/if}
    <ConflictView
      path={conflictPath}
      {conflictFiles}
      {autostash}
      onContinue={doContinue}
      onAbort={doAbort}
    />
  {/if}
</div>

<style>
  .update-view {
    padding: 16px;
    font-size: 13px;
  }

  /* ── 弹层头部 ── */
  .update-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 14px;
  }
  .update-title {
    margin: 0;
    font-size: 15px;
    color: var(--text-primary);
  }
  .btn-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    line-height: 1;
    padding: 4px 8px;
    border-radius: 4px;
  }
  .btn-close:hover {
    background: var(--border-default);
    color: var(--text-primary);
  }
  .btn-close:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .update-scope {
    color: var(--text-secondary);
    font-size: 12px;
    margin: 0 0 10px;
  }

  /* ── 子仓库更新进度 / 结果 ── */
  .sub-updating {
    max-width: 480px;
  }
  .sub-updating-title {
    color: var(--text-secondary);
    margin: 0 0 8px;
  }
  .sub-progress-list,
  .sub-result-list {
    list-style: none;
    margin: 0;
    padding: 0;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
  }
  .sub-progress-list li,
  .sub-result-list li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 0;
    color: var(--accent-neon);
  }
  .sub-progress-list li.sub-fail,
  .sub-result-list li.sub-fail {
    color: var(--color-error);
  }
  .sub-progress-list li.sub-warn,
  .sub-result-list li.sub-warn {
    color: var(--accent-gold);
  }
  .sub-current {
    color: var(--text-muted) !important;
  }
  .sub-conflict-banner {
    background: #3a2f1d;
    border: 1px solid #6a542b;
    border-radius: 6px;
    color: var(--accent-gold);
    font-size: 12px;
    padding: 8px 12px;
    margin: 0 0 12px;
  }
  .sub-icon {
    flex-shrink: 0;
    width: 12px;
    text-align: center;
  }
  .sub-results {
    margin: 14px 0 4px;
  }
  .sub-results h4 {
    margin: 0 0 6px;
    font-size: 12px;
    color: var(--text-secondary);
    font-weight: 600;
  }
  .sub-label {
    flex-shrink: 0;
  }
  .sub-detail {
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .update-error {
    background: #3a1d1d;
    border: 1px solid rgba(247, 120, 139, 0.25);
    border-radius: 6px;
    padding: 8px 12px;
    color: var(--color-error);
    white-space: pre-wrap;
    font-size: 12px;
    margin: 0 0 12px;
  }
  .hint {
    color: var(--text-muted);
    font-size: 12px;
  }

  /* ── 按钮 ── */
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
    opacity: 0.5;
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
  /* ── 进度条(executing) ── */
  .executing {
    max-width: 480px;
  }
  .progress-bar-wrap {
    position: relative;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    height: 28px;
    overflow: hidden;
    margin-bottom: 8px;
  }
  .progress-bar-fill {
    background: var(--accent-cyan);
    height: 100%;
    transition: width 0.3s ease;
    border-radius: 5px 0 0 5px;
    min-width: 0;
  }
  /* indeterminate 模式:无百分比阶段(fetch 后的整合/stash/同步等),
     用条纹滚动动画表明仍在工作,不再卡在 100%。 */
  .progress-bar-fill.indeterminate {
    width: 100% !important;
    background-image: linear-gradient(
      -45deg,
      var(--accent-cyan) 0%,
      var(--accent-cyan) 40%,
      rgba(255, 255, 255, 0.25) 50%,
      var(--accent-cyan) 60%,
      var(--accent-cyan) 100%
    );
    background-size: 40px 40px;
    animation: progress-stripes 0.9s linear infinite;
    border-radius: 5px;
  }
  @keyframes progress-stripes {
    from {
      background-position: 0 0;
    }
    to {
      background-position: 40px 0;
    }
  }
  .progress-text {
    position: absolute;
    top: 50%;
    left: 12px;
    transform: translateY(-50%);
    color: var(--text-primary);
    font-size: 12px;
  }
  .progress-raw {
    color: var(--text-muted);
    font-size: 11px;
    margin: 4px 0 8px;
    white-space: pre-wrap;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }

  /* ── 终态卡片 ── */
  .outcome-card {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    padding: 14px 18px;
    max-width: 480px;
  }
  .outcome-success {
    border-color: rgba(86, 211, 100, 0.2);
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
    color: var(--color-error);
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    padding-left: 20px;
  }

  /* ── 冲突解决（由 ConflictView 组件自行管理样式） ── */
</style>

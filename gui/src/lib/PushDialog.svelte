<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";

  interface LogEntry {
    sha: string;
    full_sha: string;
    message: string;
    author: string;
    date: string;
  }
  interface PushPreview {
    upstream: string | null;
    commits: LogEntry[];
  }
  interface Progress {
    phase: string;
    percent: number | null;
    raw: string;
  }
  type PushOutcome = "Success" | "NoUpstream" | "NonFastForward";

  let {
    path,
    label,
    onClose,
  }: {
    path: string;
    label: string;
    // pushed=true → 推送成功,调用方刷新仓库状态。
    onClose: (pushed: boolean) => void;
  } = $props();

  type Phase = "loading" | "preview" | "pushing" | "done";
  let phase = $state<Phase>("loading");
  let preview = $state<PushPreview | null>(null);
  let error = $state("");
  let forceWithLease = $state(false);
  let progress = $state<Progress | null>(null);
  let opId = $state("");
  let cancelled = $state(false);
  let result = $state<PushOutcome | null>(null);
  let unlisten: UnlistenFn | null = null;

  async function loadPreview() {
    phase = "loading";
    error = "";
    try {
      preview = await invoke<PushPreview>("repo_push_preview", { path });
      phase = "preview";
    } catch (e) {
      error = String(e);
      phase = "preview";
    }
  }

  async function doPush() {
    if (!preview?.upstream) return;
    // 强制推送是危险操作:二次确认。
    if (
      forceWithLease &&
      !(await ask(
        `确定对「${label}」强制推送(--force-with-lease)?\n` +
          `它会用本地分支覆盖远端 ${preview.upstream};仅当远端未被他人推进时才会成功。`,
        { title: "强制推送", kind: "warning" },
      ))
    )
      return;

    phase = "pushing";
    error = "";
    result = null;
    cancelled = false;
    progress = null;
    opId = crypto.randomUUID();
    try {
      unlisten = await listen<Progress>("push-progress", (e) => {
        progress = e.payload;
      });
    } catch {
      // 监听失败不阻塞推送
    }
    try {
      const r = await invoke<PushOutcome>("repo_push_streaming", {
        path,
        opId,
        forceWithLease,
      });
      result = r;
      if (r === "Success") {
        phase = "done";
      } else if (r === "NonFastForward") {
        error = forceWithLease
          ? "强制推送被拒:远端在你拉取后又有新提交(--force-with-lease 保护)。请先更新再推送。"
          : "推送被拒:远端领先。可勾选强制推送(--force-with-lease)覆盖,或先更新后再推。";
        phase = "preview";
      } else {
        error = "当前分支没有 upstream,无法推送。请先设置上游分支。";
        phase = "preview";
      }
    } catch (e) {
      if (cancelled) {
        onClose(false);
        return;
      }
      error = String(e);
      phase = "preview";
    } finally {
      cleanup();
    }
  }

  function cancelPush() {
    if (phase !== "pushing") return;
    cancelled = true;
    if (opId) invoke("cancel_op", { opId });
  }

  function cleanup() {
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && phase !== "pushing")
      onClose(result === "Success");
  }

  function fmtDate(s: string): string {
    return s.replace(/(\d{2}:\d{2}):\d{2}.*/, "$1");
  }

  onMount(loadPreview);
  onDestroy(cleanup);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="pd-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={() => phase !== "pushing" && onClose(result === "Success")}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="pd-panel" onclick={(e) => e.stopPropagation()}>
    <div class="pd-header">
      <h3>推送 · {label}</h3>
      <button
        class="pd-close"
        onclick={() => onClose(result === "Success")}
        disabled={phase === "pushing"}
        aria-label="关闭">×</button
      >
    </div>

    {#if error}
      <div class="pd-error">{error}</div>
    {/if}

    {#if phase === "loading"}
      <p class="pd-muted">读取待推送提交…</p>
    {:else if phase === "done"}
      <div class="pd-done">
        <p class="pd-ok">✓ 推送成功</p>
        <div class="pd-actions">
          <button class="pd-btn pd-btn-primary" onclick={() => onClose(true)}
            >完成</button
          >
        </div>
      </div>
    {:else if phase === "pushing"}
      <div class="pd-pushing">
        <div class="pd-progress-wrap">
          <div
            class="pd-progress-fill"
            style="width: {progress?.percent ?? 0}%"
            role="progressbar"
            aria-valuenow={progress?.percent ?? 0}
            aria-valuemin="0"
            aria-valuemax="100"
          ></div>
          <span class="pd-progress-text">
            {progress?.phase ?? "推送中…"}
            {#if progress?.percent != null}({progress.percent}%){/if}
          </span>
        </div>
        {#if progress?.raw}
          <pre class="pd-progress-raw">{progress.raw}</pre>
        {/if}
        <div class="pd-actions">
          <button class="pd-btn pd-btn-danger" onclick={cancelPush}>取消</button
          >
        </div>
      </div>
    {:else if !preview?.upstream}
      <p class="pd-muted">
        当前分支没有 upstream,无法推送。请先在分支管理里设置上游分支。
      </p>
      <div class="pd-actions">
        <button class="pd-btn" onclick={() => onClose(false)}>关闭</button>
      </div>
    {:else}
      <div class="pd-target">
        推送到 <code>{preview.upstream}</code>
      </div>
      {#if preview.commits.length === 0}
        <p class="pd-muted">没有待推送的提交(本地与远端一致)。</p>
      {:else}
        <p class="pd-count">{preview.commits.length} 个待推送提交：</p>
        <ul class="pd-commits">
          {#each preview.commits as c (c.full_sha)}
            <li class="pd-commit">
              <span class="pd-sha">{c.sha}</span>
              <span class="pd-msg">{c.message}</span>
              <span class="pd-meta">{c.author} · {fmtDate(c.date)}</span>
            </li>
          {/each}
        </ul>
      {/if}

      <label class="pd-force" title="安全强制推送:仅当远端未被他人推进时才覆盖">
        <input type="checkbox" bind:checked={forceWithLease} />
        强制推送(--force-with-lease)
      </label>

      <div class="pd-actions">
        <button class="pd-btn" onclick={() => onClose(false)}>取消</button>
        <button
          class="pd-btn pd-btn-primary"
          class:pd-btn-danger={forceWithLease}
          disabled={preview.commits.length === 0 && !forceWithLease}
          onclick={doPush}
        >
          {forceWithLease ? "强制推送" : "推送"}
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .pd-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .pd-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 520px;
    max-width: 92%;
    max-height: 82%;
    overflow-y: auto;
  }
  .pd-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 10px;
  }
  .pd-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
  .pd-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .pd-close:hover:not(:disabled) {
    color: var(--text-primary);
  }
  .pd-close:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .pd-error {
    background: #3a1d1d;
    border-top: 1px solid rgba(247, 120, 139, 0.25);
    border-bottom: 1px solid rgba(247, 120, 139, 0.25);
    padding: 8px 18px;
    color: var(--color-error);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .pd-muted {
    color: var(--text-muted);
    font-size: 12px;
    padding: 16px 18px;
    margin: 0;
  }
  .pd-target {
    padding: 8px 18px 4px;
    font-size: 13px;
    color: var(--text-secondary);
  }
  .pd-target code,
  .pd-sha {
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--accent-gold);
  }
  .pd-count {
    padding: 4px 18px 0;
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }
  .pd-commits {
    list-style: none;
    margin: 6px 0;
    padding: 0 18px;
    max-height: 260px;
    overflow-y: auto;
  }
  .pd-commit {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 4px 0;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    border-bottom: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.04));
  }
  .pd-sha {
    flex-shrink: 0;
  }
  .pd-msg {
    flex: 1;
    min-width: 0;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pd-meta {
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: 11px;
  }
  .pd-force {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 18px;
    margin-top: 4px;
    font-size: 12px;
    color: var(--accent-gold);
    cursor: pointer;
    border-top: 1px solid var(--border-default);
  }
  .pd-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 18px 16px;
  }
  .pd-btn {
    background: var(--border-default);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 6px 16px;
  }
  .pd-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .pd-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .pd-btn-primary {
    background: rgba(86, 211, 100, 0.12);
    border-color: var(--accent-neon);
    color: var(--accent-neon);
  }
  .pd-btn-primary:hover:not(:disabled) {
    background: var(--accent-neon);
    color: #000;
  }
  .pd-btn-danger {
    background: rgba(247, 120, 139, 0.18);
    border-color: rgba(247, 120, 139, 0.4);
    color: #fff;
  }
  .pd-btn-danger:hover:not(:disabled) {
    background: var(--color-error);
    color: #fff;
  }
  .pd-ok {
    color: var(--accent-neon);
    font-size: 14px;
    padding: 16px 18px 0;
    margin: 0;
  }
  /* ── 推送进度 ── */
  .pd-pushing {
    padding: 14px 18px;
  }
  .pd-progress-wrap {
    position: relative;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    height: 28px;
    overflow: hidden;
    margin-bottom: 8px;
  }
  .pd-progress-fill {
    background: var(--accent-cyan);
    height: 100%;
    transition: width 0.3s ease;
    border-radius: 5px 0 0 5px;
    min-width: 0;
  }
  .pd-progress-text {
    position: absolute;
    top: 50%;
    left: 12px;
    transform: translateY(-50%);
    color: var(--text-primary);
    font-size: 12px;
  }
  .pd-progress-raw {
    color: var(--text-muted);
    font-size: 11px;
    margin: 4px 0 8px;
    white-space: pre-wrap;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  }
</style>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  interface LogEntry {
    sha: string;
    full_sha: string;
    message: string;
    author: string;
    date: string;
  }
  interface StashRef {
    label: string;
  }
  interface ConflictedData {
    files: string[];
    autostash: StashRef | null;
  }

  type ActionKind = "pick" | "reword" | "squash" | "fixup" | "drop";

  interface Row {
    full_sha: string;
    sha: string;
    subject: string;
    action: ActionKind;
    message: string; // reword/squash 用
  }

  let {
    path,
    fromSha,
    onClose,
    onConflict,
    onDone,
  }: {
    path: string;
    fromSha: string;
    onClose: () => void;
    onConflict: (data: ConflictedData) => void;
    onDone: () => void;
  } = $props();

  let rows = $state<Row[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state("");

  async function load() {
    loading = true;
    error = "";
    try {
      const plan = await invoke<LogEntry[]>("repo_rebase_plan", {
        path,
        fromSha,
      });
      rows = plan.map((e) => ({
        full_sha: e.full_sha,
        sha: e.sha,
        subject: e.message,
        action: "pick" as ActionKind,
        message: e.message,
      }));
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function move(i: number, dir: -1 | 1) {
    const j = i + dir;
    if (j < 0 || j >= rows.length) return;
    const next = [...rows];
    [next[i], next[j]] = [next[j], next[i]];
    rows = next;
  }

  // ── 校验(与后端一致,但更早给反馈) ──
  let validation = $derived.by(() => {
    const kept = rows.filter((r) => r.action !== "drop");
    if (kept.length === 0) return "不能丢弃全部提交";
    if (kept[0].action === "fixup" || kept[0].action === "squash")
      return "第一个保留的提交不能是 squash/fixup(没有可折叠的上一个)";
    for (const r of rows)
      if ((r.action === "reword" || r.action === "squash") && !r.message.trim())
        return "reword/squash 的提交信息不能为空";
    return "";
  });

  // 摘要:保留 N、丢弃 M、折叠 K
  let summary = $derived.by(() => {
    const drop = rows.filter((r) => r.action === "drop").length;
    const fold = rows.filter(
      (r) => r.action === "squash" || r.action === "fixup",
    ).length;
    const keep = rows.length - drop - fold;
    return { keep, fold, drop };
  });

  function buildItems() {
    return rows.map((r) => {
      switch (r.action) {
        case "pick":
          return { sha: r.full_sha, action: "Pick" };
        case "fixup":
          return { sha: r.full_sha, action: "Fixup" };
        case "drop":
          return { sha: r.full_sha, action: "Drop" };
        case "reword":
          return { sha: r.full_sha, action: { Reword: r.message } };
        case "squash":
          return { sha: r.full_sha, action: { Squash: r.message } };
      }
    });
  }

  async function start() {
    if (validation || busy) return;
    busy = true;
    error = "";
    try {
      const outcome = await invoke<unknown>("repo_rebase_interactive", {
        path,
        fromSha,
        items: buildItems(),
      });
      // Conflicted → 交回 HistoryView 的 ConflictView;其余视为完成。
      if (
        outcome &&
        typeof outcome === "object" &&
        "Conflicted" in (outcome as Record<string, unknown>)
      ) {
        onConflict((outcome as { Conflicted: ConflictedData }).Conflicted);
      } else {
        onDone();
      }
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !busy) onClose();
  }

  $effect(() => {
    load();
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="rb-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={() => !busy && onClose()}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="rb-panel" onclick={(e) => e.stopPropagation()}>
    <div class="rb-header">
      <h3>交互式变基</h3>
      <button
        class="rb-close"
        onclick={onClose}
        disabled={busy}
        aria-label="关闭">×</button
      >
    </div>

    <p class="rb-hint">
      从上到下依次应用(顶部为较早的提交)。变基会改写提交历史 ——
      已推送的提交请谨慎。
    </p>

    {#if error}
      <pre class="rb-error">{error}</pre>
    {/if}

    {#if loading}
      <p class="rb-muted">加载中…</p>
    {:else if rows.length === 0}
      <p class="rb-muted">没有可变基的提交</p>
    {:else}
      <ul class="rb-list">
        {#each rows as r, i (r.full_sha)}
          <li class="rb-item" class:rb-drop={r.action === "drop"}>
            <span class="rb-reorder">
              <button
                class="rb-mv"
                disabled={i === 0 || busy}
                onclick={() => move(i, -1)}
                aria-label="上移">↑</button
              >
              <button
                class="rb-mv"
                disabled={i === rows.length - 1 || busy}
                onclick={() => move(i, 1)}
                aria-label="下移">↓</button
              >
            </span>
            <select class="rb-action" bind:value={r.action} disabled={busy}>
              <option value="pick">保留 (pick)</option>
              <option value="reword">改写信息 (reword)</option>
              <option value="squash">合并到上一个 (squash)</option>
              <option value="fixup">合并·丢信息 (fixup)</option>
              <option value="drop">丢弃 (drop)</option>
            </select>
            <span class="rb-sha">{r.sha}</span>
            {#if r.action === "reword" || r.action === "squash"}
              <input
                class="rb-msg"
                type="text"
                bind:value={r.message}
                placeholder="新的提交信息"
                disabled={busy}
              />
            {:else}
              <span class="rb-subject">{r.subject}</span>
            {/if}
          </li>
        {/each}
      </ul>

      <div class="rb-summary">
        保留 {summary.keep} · 折叠 {summary.fold} · 丢弃 {summary.drop}
        {#if validation}<span class="rb-invalid">— {validation}</span>{/if}
      </div>

      <div class="rb-actions">
        <button
          class="rb-start"
          disabled={busy || !!validation}
          onclick={start}
          title="按上面的计划开始变基(git rebase -i)"
        >
          {busy ? "变基中…" : "开始变基"}
        </button>
        <button class="rb-cancel" disabled={busy} onclick={onClose}>取消</button
        >
      </div>
    {/if}
  </div>
</div>

<style>
  .rb-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .rb-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg);
    width: 640px;
    max-width: 94%;
    max-height: 86%;
    display: flex;
    flex-direction: column;
  }
  .rb-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px var(--space-sm);
  }
  .rb-header h3 {
    font-size: var(--fs-lg);
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
  .rb-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: var(--fs-xl);
    cursor: pointer;
    padding: 0 var(--space-xs);
    line-height: 1;
  }
  .rb-close:hover:not(:disabled) {
    color: var(--text-primary);
  }
  .rb-close:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .rb-hint {
    color: var(--text-muted);
    font-size: var(--fs-sm);
    margin: 0;
    padding: 0 18px var(--space-sm);
  }
  .rb-error {
    background: rgba(255, 51, 102, 0.12);
    border-top: 1px solid rgba(255, 51, 102, 0.3);
    border-bottom: 1px solid rgba(255, 51, 102, 0.3);
    padding: var(--space-sm) 18px;
    color: var(--color-error);
    font-size: var(--fs-sm);
    white-space: pre-wrap;
    margin: 0;
  }
  .rb-muted {
    color: var(--text-muted);
    font-size: var(--fs-sm);
    text-align: center;
    padding: 22px 18px;
    margin: 0;
  }
  .rb-list {
    list-style: none;
    margin: 0;
    padding: var(--space-sm) var(--space-md);
    overflow-y: auto;
  }
  .rb-item {
    display: flex;
    align-items: center;
    gap: var(--space-sm);
    padding: var(--space-xs) var(--space-sm);
    border-radius: var(--radius-sm);
  }
  .rb-item:hover {
    background: var(--bg-elevated);
  }
  .rb-drop {
    opacity: 0.5;
  }
  .rb-reorder {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .rb-mv {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 9px;
    line-height: 1;
    padding: 1px var(--space-xs);
  }
  .rb-mv:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .rb-mv:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .rb-action {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font-size: var(--fs-sm);
    padding: var(--space-xs) var(--space-sm);
    flex-shrink: 0;
  }
  .rb-sha {
    color: var(--text-muted);
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--fs-code);
    flex-shrink: 0;
  }
  .rb-subject {
    color: var(--text-secondary);
    font-size: var(--fs-base);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }
  .rb-msg {
    flex: 1;
    min-width: 0;
    background: var(--bg-surface);
    border: 1px solid rgba(0, 255, 136, 0.2);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font-size: var(--fs-sm);
    padding: var(--space-xs) var(--space-sm);
  }
  .rb-summary {
    color: var(--text-muted);
    font-size: var(--fs-sm);
    padding: var(--space-sm) 18px 0;
    border-top: 1px solid var(--border-default);
  }
  .rb-invalid {
    color: var(--accent-gold);
  }
  .rb-actions {
    display: flex;
    gap: var(--space-sm);
    padding: var(--space-md) 18px var(--space-lg);
  }
  .rb-start {
    background: rgba(0, 255, 136, 0.12);
    border: 1px solid rgba(0, 255, 136, 0.3);
    border-radius: var(--radius-sm);
    color: #fff;
    cursor: pointer;
    font-size: var(--fs-base);
    padding: var(--space-sm) 18px;
  }
  .rb-start:hover:not(:disabled) {
    background: rgba(0, 255, 136, 0.2);
  }
  .rb-start:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .rb-cancel {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: var(--fs-base);
    padding: var(--space-sm) var(--space-lg);
  }
  .rb-cancel:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .rb-cancel:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>

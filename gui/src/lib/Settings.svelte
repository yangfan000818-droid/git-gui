<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  interface AppSettings {
    update_strategy: "Merge" | "Rebase";
    ignore_whitespace: boolean;
  }

  let { onClose }: { onClose: () => void } = $props();

  let strategy = $state<"Merge" | "Rebase">("Merge");
  let ignoreWhitespace = $state(true);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state("");

  onMount(async () => {
    try {
      const s = await invoke<AppSettings>("get_settings");
      strategy = s.update_strategy;
      ignoreWhitespace = s.ignore_whitespace;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  async function save() {
    saving = true;
    error = "";
    try {
      await invoke("save_settings", {
        settings: {
          update_strategy: strategy,
          ignore_whitespace: ignoreWhitespace,
        },
      });
      onClose();
    } catch (e) {
      error = String(e);
      saving = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="st-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={onClose}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="st-panel" onclick={(e) => e.stopPropagation()}>
    <div class="st-header">
      <h3>设置</h3>
      <button class="st-close" onclick={onClose} aria-label="关闭">×</button>
    </div>

    {#if error}
      <div class="st-error">{error}</div>
    {/if}

    {#if loading}
      <p class="st-muted">加载中…</p>
    {:else}
      <div class="st-body">
        <fieldset class="st-group">
          <legend>更新整合策略</legend>
          <p class="st-hint">
            「更新」时全局采用,不再每次弹出选择;有冲突仍会停下逐个解决。
          </p>
          <label class="st-radio">
            <input
              type="radio"
              name="strategy"
              value="Merge"
              bind:group={strategy}
            />
            <span>
              <b>Merge（合并）</b>
              <small>保留分支历史,落后时产生一个合并提交</small>
            </span>
          </label>
          <label class="st-radio">
            <input
              type="radio"
              name="strategy"
              value="Rebase"
              bind:group={strategy}
            />
            <span>
              <b>Rebase（变基）</b>
              <small>线性历史,把本地提交移到上游最新之后</small>
            </span>
          </label>
        </fieldset>

        <label class="st-check">
          <input type="checkbox" bind:checked={ignoreWhitespace} />
          <span>
            <b>忽略空白差异</b>
            <small>整合时用 -Xignore-space-change,减少纯空白引起的伪冲突</small>
          </span>
        </label>
      </div>

      <div class="st-actions">
        <button class="st-save" disabled={saving} onclick={save}>
          {saving ? "保存中…" : "保存"}
        </button>
        <button class="st-cancel" disabled={saving} onclick={onClose}
          >取消</button
        >
      </div>
    {/if}
  </div>
</div>

<style>
  .st-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .st-panel {
    background: #1e1e1e;
    border: 1px solid #444;
    border-radius: 8px;
    width: 440px;
    max-width: 92%;
    max-height: 85%;
    overflow-y: auto;
  }
  .st-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 10px;
  }
  .st-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: #e4e4e4;
    margin: 0;
  }
  .st-close {
    background: transparent;
    border: none;
    color: #888;
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .st-close:hover {
    color: #e4e4e4;
  }
  .st-error {
    background: #3a1d1d;
    border-top: 1px solid #6a2b2b;
    border-bottom: 1px solid #6a2b2b;
    padding: 8px 18px;
    color: #f3b4b4;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .st-muted {
    color: #666;
    font-size: 12px;
    text-align: center;
    padding: 24px 18px;
    margin: 0;
  }
  .st-body {
    padding: 6px 18px 4px;
  }
  .st-group {
    border: 1px solid #383838;
    border-radius: 6px;
    padding: 8px 12px 12px;
    margin: 0 0 14px;
  }
  .st-group legend {
    font-size: 12px;
    font-weight: 600;
    color: #bbb;
    padding: 0 6px;
  }
  .st-hint {
    font-size: 11px;
    color: #888;
    margin: 0 0 10px;
    line-height: 1.5;
  }
  .st-radio,
  .st-check {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 6px 0;
    cursor: pointer;
  }
  .st-radio input,
  .st-check input {
    margin-top: 3px;
    flex-shrink: 0;
  }
  .st-radio span,
  .st-check span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .st-radio b,
  .st-check b {
    font-size: 13px;
    color: #e4e4e4;
    font-weight: 600;
  }
  .st-radio small,
  .st-check small {
    font-size: 11px;
    color: #888;
    line-height: 1.4;
  }
  .st-check {
    padding: 4px 0 12px;
  }
  .st-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    padding: 4px 18px 16px;
  }
  .st-save {
    background: #1d5a1d;
    border: 1px solid #3a7a3a;
    border-radius: 4px;
    color: #fff;
    cursor: pointer;
    font-size: 12px;
    padding: 6px 16px;
  }
  .st-save:hover:not(:disabled) {
    background: #256a25;
  }
  .st-save:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .st-cancel {
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #ccc;
    cursor: pointer;
    font-size: 12px;
    padding: 6px 16px;
  }
  .st-cancel:hover:not(:disabled) {
    background: #333;
  }
</style>

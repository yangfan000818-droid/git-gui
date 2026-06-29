<script lang="ts">
  // 提交前检查结果对话框(对标 WebStorm commit checks failed):
  // 汇总警告数 + 按类别分组列详情 + [取消][仍然提交]。纯展示,决策交给调用方。

  type WarningKind =
    | "SensitiveInfo"
    | "ConflictMarker"
    | "LargeFile"
    | "DebugResidue"
    | "Todo";

  interface PrecommitWarning {
    kind: WarningKind;
    file: string;
    line: number | null;
    detail: string;
  }

  let {
    warnings,
    onDecide,
  }: {
    warnings: PrecommitWarning[];
    // proceed=false → 取消(不提交);true → 仍然提交。
    onDecide: (proceed: boolean) => void;
  } = $props();

  const KIND_META: Record<
    WarningKind,
    { label: string; icon: string; cls: string }
  > = {
    SensitiveInfo: { label: "敏感信息泄露", icon: "🔓", cls: "pc-c-danger" },
    ConflictMarker: { label: "冲突标记残留", icon: "⚠️", cls: "pc-c-danger" },
    LargeFile: { label: "大文件", icon: "📦", cls: "pc-c-warn" },
    DebugResidue: { label: "调试残留", icon: "🐛", cls: "pc-c-warn" },
    Todo: { label: "TODO 待办", icon: "📝", cls: "pc-c-info" },
  };

  // 按固定类别顺序分组(空组跳过)。
  const ORDER: WarningKind[] = [
    "SensitiveInfo",
    "ConflictMarker",
    "LargeFile",
    "DebugResidue",
    "Todo",
  ];

  let groups = $derived(
    ORDER.map((k) => ({
      kind: k,
      items: warnings.filter((w) => w.kind === k),
    })).filter((g) => g.items.length > 0),
  );

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onDecide(false);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="pc-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={() => onDecide(false)}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="pc-panel" onclick={(e) => e.stopPropagation()}>
    <div class="pc-header">
      <h3>⚠️ 提交前检查</h3>
      <button class="pc-close" onclick={() => onDecide(false)} aria-label="关闭"
        >×</button
      >
    </div>

    <p class="pc-summary">
      发现 <strong>{warnings.length}</strong> 个潜在问题(扫描的是本次将提交的新增内容)。
    </p>

    {#each groups as g (g.kind)}
      <div class="pc-group">
        <div class="pc-group-head {KIND_META[g.kind].cls}">
          <span class="pc-icon">{KIND_META[g.kind].icon}</span>
          <span class="pc-group-label">{KIND_META[g.kind].label}</span>
          <span class="pc-group-count">{g.items.length}</span>
        </div>
        <ul class="pc-items">
          {#each g.items as w, i (g.kind + ":" + i)}
            <li class="pc-item">
              <code class="pc-file"
                >{w.file}{#if w.line != null}:{w.line}{/if}</code
              >
              <span class="pc-detail">{w.detail}</span>
            </li>
          {/each}
        </ul>
      </div>
    {/each}

    <div class="pc-hint">这些只是提醒,不阻止提交。确认无误可「仍然提交」。</div>

    <div class="pc-actions">
      <button class="pc-btn" onclick={() => onDecide(false)}>取消</button>
      <button class="pc-btn pc-btn-primary" onclick={() => onDecide(true)}
        >仍然提交</button
      >
    </div>
  </div>
</div>

<style>
  .pc-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .pc-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 560px;
    max-width: 92%;
    max-height: 82%;
    overflow-y: auto;
  }
  .pc-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 10px;
  }
  .pc-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
  .pc-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .pc-close:hover {
    color: var(--text-primary);
  }
  .pc-summary {
    padding: 0 18px 10px;
    margin: 0;
    font-size: 13px;
    color: var(--text-secondary);
  }
  .pc-summary strong {
    color: var(--color-error);
  }
  .pc-group {
    border-top: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.04));
  }
  .pc-group-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 18px;
    font-size: 12px;
    font-weight: 600;
  }
  .pc-icon {
    font-size: 13px;
  }
  .pc-group-count {
    margin-left: auto;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 10px;
    padding: 0 8px;
    font-size: 11px;
    color: var(--text-muted);
  }
  .pc-c-danger {
    color: var(--color-error);
  }
  .pc-c-warn {
    color: var(--accent-gold);
  }
  .pc-c-info {
    color: var(--text-secondary);
  }
  .pc-items {
    list-style: none;
    margin: 0;
    padding: 0 18px 8px;
  }
  .pc-item {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 3px 0;
    font-size: 12px;
    border-bottom: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.03));
  }
  .pc-file {
    flex-shrink: 0;
    max-width: 46%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--accent-gold);
  }
  .pc-detail {
    flex: 1;
    min-width: 0;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pc-hint {
    padding: 8px 18px;
    margin: 4px 0 0;
    font-size: 11px;
    color: var(--text-muted);
    border-top: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.04));
  }
  .pc-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 18px 16px;
  }
  .pc-btn {
    background: var(--border-default);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 6px 16px;
  }
  .pc-btn:hover {
    background: var(--bg-hover);
  }
  .pc-btn-primary {
    background: rgba(86, 211, 100, 0.12);
    border-color: var(--accent-neon);
    color: var(--accent-neon);
  }
  .pc-btn-primary:hover {
    background: var(--accent-neon);
    color: #000;
  }
</style>

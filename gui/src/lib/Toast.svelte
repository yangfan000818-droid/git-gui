<script lang="ts">
  import { onMount, onDestroy } from "svelte";

  let {
    message,
    kind = "info",
    duration = 3500,
    onClose,
  }: {
    message: string;
    kind?: "success" | "error" | "info";
    // <= 0 表示不自动消失(用于持久的"更新中"提示,由外部替换/关闭)。
    duration?: number;
    onClose: () => void;
  } = $props();

  let timer: ReturnType<typeof setTimeout> | null = null;

  onMount(() => {
    if (duration > 0) {
      timer = setTimeout(() => onClose(), duration);
    }
  });

  onDestroy(() => {
    if (timer) clearTimeout(timer);
  });
</script>

<div class="toast toast-{kind}" role="status" aria-live="polite">
  <span class="toast-icon">
    {kind === "success" ? "✓" : kind === "error" ? "✕" : "⋯"}
  </span>
  <span class="toast-msg">{message}</span>
  <button class="toast-close" onclick={onClose} aria-label="关闭">×</button>
</div>

<style>
  .toast {
    position: fixed;
    right: 20px;
    bottom: 20px;
    z-index: 2000;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    min-width: 240px;
    max-width: 420px;
    padding: 10px 12px;
    border-radius: 8px;
    border: 1px solid var(--border-default);
    background: var(--bg-elevated);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    font-size: 13px;
    color: var(--text-primary);
    animation: toast-in 0.18s ease;
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  .toast-success {
    border-color: rgba(86, 211, 100, 0.35);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5), 0 0 12px rgba(86, 211, 100, 0.15);
  }
  .toast-error {
    border-color: rgba(247, 120, 139, 0.35);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5), 0 0 12px rgba(247, 120, 139, 0.15);
  }
  .toast-info {
    border-color: rgba(88, 166, 255, 0.35);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5), 0 0 12px rgba(88, 166, 255, 0.15);
  }
  .toast-icon {
    flex-shrink: 0;
    font-weight: 700;
    line-height: 1.2;
  }
  .toast-success .toast-icon {
    color: #56d364;
  }
  .toast-error .toast-icon {
    color: var(--color-error);
  }
  .toast-info .toast-icon {
    color: var(--accent-cyan);
  }
  .toast-msg {
    flex: 1;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.4;
  }
  .toast-close {
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 0 2px;
    border-radius: 4px;
  }
  .toast-close:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }
</style>

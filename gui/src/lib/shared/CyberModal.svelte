<script lang="ts">
  interface Props {
    open: boolean;
    title?: string;
    variant?: "default" | "terminal" | "holographic";
    width?: string;
    maxHeight?: string;
    showClose?: boolean;
    onclose: () => void;
    children: import("svelte").Snippet;
    footer?: import("svelte").Snippet;
  }

  let {
    open = false,
    title = "",
    variant = "default",
    width = "520px",
    maxHeight = "85%",
    showClose = true,
    onclose,
    children,
    footer,
  }: Props = $props();
</script>

{#if open}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="cyber-overlay"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={onclose}
    onkeydown={(e) => e.key === "Escape" && onclose()}
  >
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="cyber-modal"
      data-variant={variant}
      style="width: {width}; max-height: {maxHeight};"
      onclick={(e) => e.stopPropagation()}
    >
      {#if variant === "terminal"}
        <div class="modal-traffic">
          <span class="traffic-dot dot-red"></span>
          <span class="traffic-dot dot-yellow"></span>
          <span class="traffic-dot dot-green"></span>
          <span class="terminal-title">{title || "terminal"}</span>
          {#if showClose}
            <button class="modal-close" onclick={onclose} aria-label="关闭"
              >×</button
            >
          {/if}
        </div>
      {:else if title || showClose}
        <div class="modal-header">
          {#if title}
            <h2 class="modal-title">{title}</h2>
          {/if}
          {#if showClose}
            <button class="modal-close" onclick={onclose} aria-label="关闭"
              >×</button
            >
          {/if}
        </div>
      {/if}
      <div class="modal-body">
        {@render children()}
      </div>
      {#if footer}
        <div class="modal-footer">
          {@render footer()}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .cyber-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.8);
    backdrop-filter: blur(6px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    animation: cyber-overlay-in 150ms steps(5);
  }
  @keyframes cyber-overlay-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  .cyber-modal {
    background: var(--bg-elevated);
    border: 1px solid var(--accent);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow:
      0 0 15px rgba(0, 255, 136, 0.2),
      0 0 30px rgba(0, 255, 136, 0.05),
      0 8px 40px rgba(0, 0, 0, 0.6);
    animation: cyber-modal-in 200ms steps(6);
    clip-path: polygon(
      0 var(--corner-cut, 6px),
      var(--corner-cut, 6px) 0,
      calc(100% - var(--corner-cut, 6px)) 0,
      100% var(--corner-cut, 6px),
      100% calc(100% - var(--corner-cut, 6px)),
      calc(100% - var(--corner-cut, 6px)) 100%,
      var(--corner-cut, 6px) 100%,
      0 calc(100% - var(--corner-cut, 6px))
    );
  }
  @keyframes cyber-modal-in {
    from {
      opacity: 0;
      transform: scale(0.96) translateY(8px);
    }
    to {
      opacity: 1;
      transform: scale(1) translateY(0);
    }
  }
  .cyber-modal[data-variant="holographic"] {
    background: rgba(28, 28, 46, 0.85);
    backdrop-filter: blur(12px);
    border: 1px solid rgba(0, 255, 136, 0.3);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .modal-title {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--fs-sm, 12px);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--accent);
  }

  /* Terminal variant: traffic light header */
  .modal-traffic {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    background: var(--bg-void);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .traffic-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .dot-red {
    background: #ff5f57;
  }
  .dot-yellow {
    background: #febc2e;
  }
  .dot-green {
    background: #28c840;
  }
  .terminal-title {
    flex: 1;
    text-align: center;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
  }
  .modal-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 18px;
    line-height: 1;
    padding: 4px 8px;
    transition: all 100ms steps(4);
  }
  .modal-close:hover {
    color: var(--destructive);
    text-shadow: var(--glow-error);
  }

  .modal-body {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
  }
  .modal-footer {
    flex-shrink: 0;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>

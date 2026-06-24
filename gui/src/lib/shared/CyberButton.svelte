<script lang="ts">
  interface Props {
    variant?: "primary" | "ghost" | "destructive" | "glow";
    size?: "sm" | "md" | "lg";
    disabled?: boolean;
    title?: string;
    type?: "button" | "submit";
    onclick?: (e: MouseEvent) => void;
    children: import("svelte").Snippet;
  }

  let {
    variant = "ghost",
    size = "md",
    disabled = false,
    title = "",
    type = "button",
    onclick,
    children,
  }: Props = $props();
</script>

<button
  class="cyber-btn"
  data-variant={variant}
  data-size={size}
  {disabled}
  {title}
  {type}
  {onclick}
>
  {@render children()}
</button>

<style>
  .cyber-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    border: 1px solid var(--border);
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    white-space: nowrap;
    transition: all 100ms steps(4);
    position: relative;
  }
  /* Chamfered corners */
  .cyber-btn {
    clip-path: polygon(
      0 3px,
      3px 0,
      calc(100% - 3px) 0,
      100% 3px,
      100% calc(100% - 3px),
      calc(100% - 3px) 100%,
      3px 100%,
      0 calc(100% - 3px)
    );
  }

  /* Sizes */
  .cyber-btn[data-size="sm"] {
    padding: 3px 10px;
    font-size: 10px;
  }
  .cyber-btn[data-size="md"] {
    padding: 5px 14px;
    font-size: 11px;
  }
  .cyber-btn[data-size="lg"] {
    padding: 7px 20px;
    font-size: 12px;
  }

  /* Variants */
  .cyber-btn[data-variant="primary"] {
    border-color: var(--accent);
    color: var(--accent);
    background: rgba(0, 255, 136, 0.08);
    box-shadow: var(--glow-neon);
  }
  .cyber-btn[data-variant="primary"]:hover:not(:disabled) {
    background: var(--accent);
    color: #000;
    box-shadow:
      0 0 20px rgba(0, 255, 136, 0.5),
      0 0 40px rgba(0, 255, 136, 0.15);
  }

  .cyber-btn[data-variant="destructive"] {
    border-color: var(--destructive);
    color: var(--destructive);
    background: rgba(255, 51, 102, 0.08);
    box-shadow: var(--glow-error);
  }
  .cyber-btn[data-variant="destructive"]:hover:not(:disabled) {
    background: var(--destructive);
    color: #000;
    box-shadow:
      0 0 20px rgba(255, 51, 102, 0.5),
      0 0 40px rgba(255, 51, 102, 0.15);
  }

  .cyber-btn[data-variant="glow"] {
    border-color: var(--accent-secondary);
    color: var(--accent-secondary);
    box-shadow: var(--glow-magenta);
  }
  .cyber-btn[data-variant="glow"]:hover:not(:disabled) {
    background: var(--accent-secondary);
    color: #000;
    box-shadow:
      0 0 20px rgba(255, 0, 255, 0.5),
      0 0 40px rgba(255, 0, 255, 0.15);
  }

  .cyber-btn[data-variant="ghost"]:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--accent);
    color: var(--accent);
    text-shadow: 0 0 6px rgba(0, 255, 136, 0.3);
  }

  .cyber-btn:active:not(:disabled) {
    transform: scale(0.95);
    transition: transform 50ms steps(2);
  }
  .cyber-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }
</style>

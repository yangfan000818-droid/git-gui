<script lang="ts">
  interface Props {
    value?: string;
    placeholder?: string;
    disabled?: boolean;
    type?: "text" | "textarea";
    rows?: number;
    monospace?: boolean;
    title?: string;
    oninput?: (e: Event) => void;
    onchange?: (e: Event) => void;
  }

  let {
    value = "",
    placeholder = "",
    disabled = false,
    type = "text",
    rows = 3,
    monospace = true,
    title = "",
    oninput,
    onchange,
  }: Props = $props();
</script>

<div class="cyber-input-wrap">
  <span class="prompt" aria-hidden="true">&gt;</span>
  {#if type === "textarea"}
    <textarea
      class="cyber-input"
      bind:value
      {placeholder}
      {disabled}
      {rows}
      {title}
      {oninput}
      {onchange}></textarea>
  {:else}
    <input
      class="cyber-input"
      type="text"
      bind:value
      {placeholder}
      {disabled}
      {title}
      {oninput}
      {onchange}
    />
  {/if}
</div>

<style>
  .cyber-input-wrap {
    position: relative;
    display: flex;
    align-items: flex-start;
    background: var(--bg-void);
    border: 1px solid var(--border);
    transition:
      border-color 100ms steps(4),
      box-shadow 100ms steps(4);
    clip-path: polygon(
      0 4px,
      4px 0,
      calc(100% - 4px) 0,
      100% 4px,
      100% calc(100% - 4px),
      calc(100% - 4px) 100%,
      4px 100%,
      0 calc(100% - 4px)
    );
  }
  .cyber-input-wrap:focus-within {
    border-color: var(--accent);
    box-shadow:
      0 0 12px rgba(0, 255, 136, 0.2),
      inset 0 0 8px rgba(0, 255, 136, 0.05);
  }
  .prompt {
    color: var(--accent);
    font-family: var(--font-mono);
    font-size: var(--fs-code, 12px);
    padding: 8px 0 8px 10px;
    user-select: none;
    text-shadow: 0 0 6px var(--accent);
    flex-shrink: 0;
  }
  .cyber-input {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: var(--fs-code, 12px);
    padding: 8px 10px;
    outline: none;
    resize: vertical;
    min-width: 0;
  }
  .cyber-input::placeholder {
    color: var(--text-muted);
  }
  .cyber-input:disabled {
    opacity: 0.4;
  }
</style>

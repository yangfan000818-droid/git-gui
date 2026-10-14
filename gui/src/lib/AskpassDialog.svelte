<script lang="ts">
  // git / ssh 凭据询问弹框(对标 IntelliJ 的 askpass 对话框):后端经 askpass-request
  // 推来提示语,用户填写后经 askpass_respond 回传;取消回传 null,git 随即以认证失败退出。
  // 多个询问排队,一次只显示一个;后端超时作废时经 askpass-expired 撤下。
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  interface AskpassRequest {
    id: number;
    prompt: string;
    secret: boolean;
  }

  let queue = $state<AskpassRequest[]>([]);
  let value = $state("");
  let input = $state<HTMLInputElement | null>(null);
  let current = $derived(queue[0] ?? null);
  let currentId = $derived(current?.id);

  onMount(() => {
    const unlisteners = [
      listen<AskpassRequest>("askpass-request", (e) => {
        queue = [...queue, e.payload];
      }),
      listen<number>("askpass-expired", (e) => drop(e.payload)),
    ];
    return () => {
      for (const u of unlisteners) u.then((off) => off());
    };
  });

  // 轮到下一条询问:清空输入并聚焦。
  $effect(() => {
    if (currentId === undefined) return;
    value = "";
    tick().then(() => input?.focus());
  });

  function drop(id: number) {
    queue = queue.filter((r) => r.id !== id);
  }

  async function answer(text: string | null) {
    const req = current;
    if (!req) return;
    drop(req.id);
    await invoke("askpass_respond", { id: req.id, answer: text });
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") answer(null);
  }
</script>

{#if current}
  <div
    class="ap-overlay"
    role="dialog"
    aria-modal="true"
    aria-labelledby="ap-title"
    tabindex="-1"
    onkeydown={handleKeydown}
  >
    <form
      class="ap-panel"
      onsubmit={(e) => {
        e.preventDefault();
        answer(value);
      }}
    >
      <h3 id="ap-title">🔐 Git 需要身份验证</h3>
      <pre class="ap-prompt">{current.prompt.trimEnd()}</pre>
      <input
        bind:this={input}
        bind:value
        class="ap-input"
        type={current.secret ? "password" : "text"}
        autocomplete="off"
        spellcheck="false"
      />
      <div class="ap-hint">
        已配置凭据助手(如 macOS 钥匙串)时,验证成功后 git 会自动记住。
      </div>
      <div class="ap-actions">
        <button type="button" class="ap-btn" onclick={() => answer(null)}
          >取消</button
        >
        <button type="submit" class="ap-btn ap-btn-primary">确定</button>
      </div>
    </form>
  </div>
{/if}

<style>
  .ap-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1100;
  }
  .ap-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 480px;
    max-width: 92%;
    padding: 14px 18px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .ap-panel h3 {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
  .ap-prompt {
    margin: 0;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    color: var(--text-secondary);
    white-space: pre-wrap;
    word-break: break-all;
  }
  .ap-input {
    background: var(--bg-surface);
    border: 1px solid var(--bg-hover);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    padding: 7px 10px;
  }
  .ap-input:focus {
    outline: none;
    border-color: var(--accent-cyan);
  }
  .ap-hint {
    font-size: 11px;
    color: var(--text-muted);
  }
  .ap-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .ap-btn {
    background: var(--border-default);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 6px 16px;
  }
  .ap-btn:hover {
    background: var(--bg-hover);
  }
  .ap-btn-primary {
    background: rgba(86, 211, 100, 0.12);
    border-color: var(--accent-neon);
    color: var(--accent-neon);
  }
  .ap-btn-primary:hover {
    background: var(--accent-neon);
    color: #000;
  }
</style>

<script lang="ts">
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { checkForUpdate, type UpdateInfo } from "$lib/updateCheck";

  let info = $state<UpdateInfo | null>(null);

  // 记住"已忽略"的版本,该版本不再于启动时打扰(下个更新版本仍会提示)。
  const DISMISS_KEY = "dismissed_update_version";

  onMount(async () => {
    try {
      const u = await checkForUpdate();
      if (!u) return;
      if (localStorage.getItem(DISMISS_KEY) === u.latest) return;
      info = u;
    } catch {
      // 网络/API 失败:静默,不打扰用户。
    }
  });

  function dismiss() {
    if (info) localStorage.setItem(DISMISS_KEY, info.latest);
    info = null;
  }

  async function view() {
    if (info?.url) await openUrl(info.url);
  }
</script>

{#if info}
  <div class="update-banner" role="status">
    <span class="ub-text">
      🎉 新版本 <b>v{info.latest}</b> 可用（当前 v{info.current}）
    </span>
    <button class="ub-view" onclick={view}>查看更新</button>
    <button class="ub-dismiss" onclick={dismiss} aria-label="忽略此版本"
      >×</button
    >
  </div>
{/if}

<style>
  .update-banner {
    display: flex;
    align-items: center;
    gap: var(--space-md);
    padding: var(--space-sm) 14px;
    background: rgba(0, 255, 136, 0.12);
    border-bottom: 1px solid rgba(0, 255, 136, 0.3);
    font-size: var(--fs-sm);
    color: var(--text-primary);
    flex-shrink: 0;
  }
  .ub-text {
    flex: 1;
    min-width: 0;
  }
  .ub-text b {
    color: var(--accent-neon, #56d364);
  }
  .ub-view {
    flex-shrink: 0;
    background: rgba(0, 255, 136, 0.2);
    border: 1px solid rgba(0, 255, 136, 0.3);
    border-radius: var(--radius-sm);
    color: var(--accent-neon, #56d364);
    cursor: pointer;
    font-size: var(--fs-xs);
    padding: 3px var(--space-md);
  }
  .ub-view:hover {
    background: rgba(0, 255, 136, 0.3);
  }
  .ub-dismiss {
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: var(--fs-xl);
    line-height: 1;
    padding: 0 var(--space-xs);
  }
  .ub-dismiss:hover {
    color: var(--text-primary);
  }
</style>

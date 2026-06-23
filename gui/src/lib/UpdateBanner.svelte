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
    gap: 10px;
    padding: 6px 14px;
    background: rgba(86, 211, 100, 0.12);
    border-bottom: 1px solid rgba(86, 211, 100, 0.25);
    font-size: 12px;
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
    background: rgba(86, 211, 100, 0.16);
    border: 1px solid rgba(86, 211, 100, 0.3);
    border-radius: 4px;
    color: var(--accent-neon, #56d364);
    cursor: pointer;
    font-size: 11px;
    padding: 3px 12px;
  }
  .ub-view:hover {
    background: rgba(86, 211, 100, 0.24);
  }
  .ub-dismiss {
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    line-height: 1;
    padding: 0 4px;
  }
  .ub-dismiss:hover {
    color: var(--text-primary);
  }
</style>

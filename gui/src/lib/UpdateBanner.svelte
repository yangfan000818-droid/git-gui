<script lang="ts">
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { check } from "@tauri-apps/plugin-updater";
  import { checkForUpdate, type UpdateInfo } from "$lib/updateCheck";

  let update = $state<UpdateInfo | null>(null);
  let downloading = $state(false);
  let downloadError = $state("");

  const DISMISS_KEY = "dismissed_update_version";

  onMount(async () => {
    // 优先走 updater 插件;macOS 上插件能 check 但不能 install(未签名),
    // 此时回退到手动 fetch 展示。
    try {
      const u = await check();
      if (u?.available) {
        update = {
          latest: u.version,
          current: u.currentVersion,
          url: "", // updater 自带下载,不需 URL
          notes: u.body ?? "",
        };
        if (localStorage.getItem(DISMISS_KEY) === u.version) {
          update = null;
        }
        return;
      }
    } catch {
      // 插件 check 失败 → 走旧的手动模式。
    }

    // macOS / 插件不可用时的手动检查回退。
    try {
      const u = await checkForUpdate();
      if (!u) return;
      if (localStorage.getItem(DISMISS_KEY) === u.latest) return;
      update = u;
    } catch {
      // 静默
    }
  });

  function dismiss() {
    if (update) localStorage.setItem(DISMISS_KEY, update.latest);
    update = null;
  }

  async function downloadAndInstall() {
    downloading = true;
    downloadError = "";
    try {
      // 用 updater 插件下载并安装;成功后自动重启 app。
      const u = await check();
      if (u?.available) {
        await u.downloadAndInstall();
      }
    } catch (e) {
      downloadError = String(e);
    } finally {
      downloading = false;
    }
  }

  async function viewInBrowser() {
    if (update?.url) await openUrl(update.url);
  }
</script>

{#if update}
  <div class="update-banner" role="status">
    <span class="ub-text">
      🎉 新版本 <b>v{update.latest}</b> 可用（当前 v{update.current}）
    </span>
    {#if downloadError}
      <span class="ub-err">{downloadError}</span>
    {/if}
    <button
      class="ub-install"
      disabled={downloading}
      onclick={downloadAndInstall}
    >
      {downloading ? "下载中…" : "下载并安装"}
    </button>
    <button class="ub-view" onclick={viewInBrowser}>查看</button>
    <button
      class="ub-dismiss"
      disabled={downloading}
      onclick={dismiss}
      aria-label="忽略此版本">×</button
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
  .ub-err {
    color: var(--color-error);
    font-size: 11px;
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ub-install {
    flex-shrink: 0;
    background: rgba(86, 211, 100, 0.16);
    border: 1px solid rgba(86, 211, 100, 0.3);
    border-radius: 4px;
    color: var(--accent-neon, #56d364);
    cursor: pointer;
    font-size: 11px;
    padding: 4px 14px;
    font-weight: 600;
  }
  .ub-install:hover:not(:disabled) {
    background: rgba(86, 211, 100, 0.28);
  }
  .ub-install:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .ub-view {
    flex-shrink: 0;
    background: transparent;
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 11px;
    padding: 3px 10px;
  }
  .ub-view:hover {
    color: var(--text-primary);
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
  .ub-dismiss:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .ub-dismiss:hover:not(:disabled) {
    color: var(--text-primary);
  }
</style>

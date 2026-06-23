<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  interface AppSettings {
    update_strategy: "Merge" | "Rebase";
    ignore_whitespace: boolean;
    skip_hooks: boolean;
    theme: string;
    density: string;
    font_size: string;
    animations_enabled: boolean;
    scanline_enabled: boolean;
    glow_intensity: string;
  }

  let { onClose, onAppearanceChanged }: {
    onClose: () => void;
    onAppearanceChanged?: (s: AppSettings) => void;
  } = $props();

  // ── Git 设置 ──
  let strategy = $state<"Merge" | "Rebase">("Merge");
  let ignoreWhitespace = $state(true);
  let skipHooks = $state(false);

  // ── 外观设置 ──
  let theme = $state("neon-dark");
  let density = $state("comfortable");
  let font_size = $state("medium");
  let animationsEnabled = $state(true);
  let scanlineEnabled = $state(true);
  let glowIntensity = $state("medium");

  let loading = $state(true);
  let saving = $state(false);
  let error = $state("");

  // ── 主题预设定义 ──
  const THEMES: { id: string; name: string; preview: string[]; kind: string }[] = [
    {
      id: "neon-dark",
      name: "Neon Dark",
      kind: "暗色",
      preview: ["#0D1117", "#56D364", "#58A6FF", "#BC8CFF"],
    },
    {
      id: "light",
      name: "Light",
      kind: "亮色",
      preview: ["#F6F8FA", "#1A7F37", "#0550AE", "#8250DF"],
    },
    {
      id: "dracula",
      name: "Dracula",
      kind: "暗色",
      preview: ["#282A36", "#50FA7B", "#BD93F9", "#FF79C6"],
    },
    {
      id: "nord",
      name: "Nord",
      kind: "暗色",
      preview: ["#2E3440", "#A3BE8C", "#88C0D0", "#B48EAD"],
    },
    {
      id: "solarized-dark",
      name: "Solarized Dark",
      kind: "暗色",
      preview: ["#002B36", "#859900", "#2AA198", "#B58900"],
    },
    {
      id: "monokai",
      name: "Monokai",
      kind: "暗色",
      preview: ["#272822", "#A6E22E", "#66D9EF", "#F92672"],
    },
    {
      id: "tokyo-night",
      name: "Tokyo Night",
      kind: "暗色",
      preview: ["#1A1B26", "#9ECE6A", "#7DCFFF", "#BB9AF7"],
    },
    {
      id: "github-dark",
      name: "GitHub Dark",
      kind: "暗色",
      preview: ["#0D1117", "#3FB950", "#58A6FF", "#F85149"],
    },
  ];

  onMount(async () => {
    try {
      const s = await invoke<AppSettings>("get_settings");
      strategy = s.update_strategy;
      ignoreWhitespace = s.ignore_whitespace;
      skipHooks = s.skip_hooks;
      theme = s.theme || "neon-dark";
      density = s.density || "comfortable";
      font_size = s.font_size || "medium";
      animationsEnabled = s.animations_enabled ?? true;
      scanlineEnabled = s.scanline_enabled ?? true;
      glowIntensity = s.glow_intensity || "medium";
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
      const settings: AppSettings = {
        update_strategy: strategy,
        ignore_whitespace: ignoreWhitespace,
        skip_hooks: skipHooks,
        theme,
        density,
        font_size,
        animations_enabled: animationsEnabled,
        scanline_enabled: scanlineEnabled,
        glow_intensity: glowIntensity,
      };
      await invoke("save_settings", { settings });
      onAppearanceChanged?.(settings);
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
        <!-- ─── 外观：主题 ─── -->
        <fieldset class="st-group">
          <legend>🎨 主题配色</legend>
          <div class="theme-grid">
            {#each THEMES as t}
              <button
                class="theme-card"
                class:theme-active={theme === t.id}
                onclick={() => (theme = t.id)}
                aria-pressed={theme === t.id}
                title={t.name}
              >
                <span class="theme-swatches">
                  {#each t.preview as color}
                    <span class="theme-swatch" style="background:{color}"></span>
                  {/each}
                </span>
                <span class="theme-name">{t.name}</span>
              </button>
            {/each}
          </div>
        </fieldset>

        <!-- ─── 外观：布局密度 ─── -->
        <fieldset class="st-group">
          <legend>📐 布局密度</legend>
          <div class="segmented">
            <button
              class="seg-btn"
              class:seg-active={density === "compact"}
              onclick={() => (density = "compact")}
            >紧凑</button>
            <button
              class="seg-btn"
              class:seg-active={density === "comfortable"}
              onclick={() => (density = "comfortable")}
            >舒适</button>
            <button
              class="seg-btn"
              class:seg-active={density === "spacious"}
              onclick={() => (density = "spacious")}
            >宽松</button>
          </div>
        </fieldset>

        <!-- ─── 外观：字号 ─── -->
        <fieldset class="st-group">
          <legend>🔤 字号大小</legend>
          <div class="segmented">
            <button
              class="seg-btn"
              class:seg-active={font_size === "small"}
              onclick={() => (font_size = "small")}
            >小</button>
            <button
              class="seg-btn"
              class:seg-active={font_size === "medium"}
              onclick={() => (font_size = "medium")}
            >中</button>
            <button
              class="seg-btn"
              class:seg-active={font_size === "large"}
              onclick={() => (font_size = "large")}
            >大</button>
          </div>
        </fieldset>

        <!-- ─── 外观：效果 ─── -->
        <fieldset class="st-group">
          <legend>✨ 视觉特效</legend>

          <label class="st-check">
            <input type="checkbox" bind:checked={animationsEnabled} />
            <span>
              <b>过渡动画</b>
              <small>按钮悬停、面板切换等过渡效果</small>
            </span>
          </label>

          <label class="st-check">
            <input type="checkbox" bind:checked={scanlineEnabled} />
            <span>
              <b>扫描线纹理</b>
              <small>给界面添加微妙的 CRT 扫描线纹理</small>
            </span>
          </label>

          <div class="glow-row">
            <span class="glow-label">Glow 强度</span>
            <div class="segmented">
              <button
                class="seg-btn seg-sm"
                class:seg-active={glowIntensity === "off"}
                onclick={() => (glowIntensity = "off")}
              >关闭</button>
              <button
                class="seg-btn seg-sm"
                class:seg-active={glowIntensity === "subtle"}
                onclick={() => (glowIntensity = "subtle")}
              >微光</button>
              <button
                class="seg-btn seg-sm"
                class:seg-active={glowIntensity === "medium"}
                onclick={() => (glowIntensity = "medium")}
              >适中</button>
              <button
                class="seg-btn seg-sm"
                class:seg-active={glowIntensity === "strong"}
                onclick={() => (glowIntensity = "strong")}
              >强光</button>
            </div>
          </div>
        </fieldset>

        <!-- ─── Git 设置 ─── -->
        <fieldset class="st-group">
          <legend>⚙️ 更新整合策略</legend>
          <p class="st-hint">
            「更新」时全局采用，不再每次弹出选择；有冲突仍会停下逐个解决。
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
              <small>保留分支历史，落后时产生一个合并提交</small>
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
              <small>线性历史，把本地提交移到上游最新之后</small>
            </span>
          </label>
        </fieldset>

        <label class="st-check">
          <input type="checkbox" bind:checked={ignoreWhitespace} />
          <span>
            <b>忽略空白差异</b>
            <small>整合时用 -Xignore-space-change，减少纯空白引起的伪冲突</small>
          </span>
        </label>

        <label class="st-check">
          <input type="checkbox" bind:checked={skipHooks} />
          <span>
            <b>提交时跳过 git 钩子</b>
            <small>勾选则提交用 --no-verify，不运行 pre-commit / commit-msg 钩子。默认不跳过。</small>
          </span>
        </label>
      </div>

      <div class="st-actions">
        <button class="st-save" disabled={saving} onclick={save}>
          {saving ? "保存中…" : "保存"}
        </button>
        <button class="st-cancel" disabled={saving} onclick={onClose}>取消</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .st-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .st-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 640px;
    max-width: 94%;
    max-height: 88%;
    overflow-y: auto;
    box-shadow: 0 12px 60px rgba(0,0,0,0.5);
  }
  .st-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px 12px;
    border-bottom: 1px solid var(--border-dim);
    position: sticky;
    top: 0;
    background: var(--bg-elevated);
    z-index: 2;
  }
  .st-header h3 {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
  .st-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 20px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
    border-radius: var(--radius-sm);
    transition: all 0.15s;
  }
  .st-close:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }
  .st-error {
    background: rgba(247, 120, 139, 0.12);
    border-top: 1px solid rgba(247, 120, 139, 0.25);
    border-bottom: 1px solid rgba(247, 120, 139, 0.25);
    padding: 8px 20px;
    color: var(--color-error);
    font-size: var(--fs-sm, 12px);
    white-space: pre-wrap;
  }
  .st-muted {
    color: var(--text-muted);
    font-size: var(--fs-sm, 12px);
    text-align: center;
    padding: 28px 20px;
    margin: 0;
  }
  .st-body {
    padding: 10px 20px 4px;
  }
  .st-group {
    border: 1px solid var(--border-default);
    border-radius: 6px;
    padding: 10px 14px 14px;
    margin: 0 0 16px;
  }
  .st-group legend {
    font-size: var(--fs-sm, 12px);
    font-weight: 600;
    color: var(--text-secondary);
    padding: 0 6px;
  }
  .st-hint {
    font-size: var(--fs-xs, 11px);
    color: var(--text-muted);
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
    accent-color: var(--accent-neon);
  }
  .st-radio span,
  .st-check span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .st-radio b,
  .st-check b {
    font-size: var(--fs-base, 13px);
    color: var(--text-primary);
    font-weight: 600;
  }
  .st-radio small,
  .st-check small {
    font-size: var(--fs-xs, 11px);
    color: var(--text-muted);
    line-height: 1.4;
  }
  .st-check {
    padding: 4px 0 12px;
  }
  .st-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    padding: 6px 20px 18px;
  }
  .st-save {
    background: var(--accent-neon);
    border: none;
    border-radius: 4px;
    color: #000;
    cursor: pointer;
    font-size: var(--fs-sm, 12px);
    font-weight: 600;
    padding: 7px 20px;
    transition: all 0.2s;
  }
  .st-save:hover:not(:disabled) {
    filter: brightness(1.1);
    box-shadow: var(--glow-neon);
  }
  .st-save:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .st-cancel {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: var(--fs-sm, 12px);
    padding: 7px 16px;
    transition: all 0.15s;
  }
  .st-cancel:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  /* ── 主题卡片网格 ── */
  .theme-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
  }
  .theme-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    background: var(--bg-surface);
    border: 2px solid var(--border-default);
    border-radius: var(--radius-md);
    padding: 10px 6px 8px;
    cursor: pointer;
    transition: all 0.2s;
  }
  .theme-card:hover {
    border-color: var(--text-muted);
    background: var(--bg-hover);
  }
  .theme-active {
    border-color: var(--accent-cyan) !important;
    background: var(--bg-hover);
    box-shadow: var(--glow-cyan);
  }
  .theme-swatches {
    display: flex;
    gap: 4px;
  }
  .theme-swatch {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 1px solid var(--border-dim);
    flex-shrink: 0;
  }
  .theme-name {
    font-size: var(--fs-xs, 11px);
    color: var(--text-secondary);
    text-align: center;
    font-weight: 500;
  }

  /* ── 分段按钮 ── */
  .segmented {
    display: inline-flex;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    overflow: hidden;
  }
  .seg-btn {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: var(--fs-sm, 12px);
    padding: 5px 14px;
    transition: all 0.15s;
  }
  .seg-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .seg-active {
    background: var(--bg-active);
    color: var(--text-primary);
    font-weight: 600;
    box-shadow: inset 0 1px 0 rgba(0,0,0,0.1);
  }
  .seg-sm {
    padding: 4px 10px;
    font-size: var(--fs-xs, 11px);
  }

  /* ── Glow 行 ── */
  .glow-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 0 4px;
  }
  .glow-label {
    font-size: var(--fs-base, 13px);
    font-weight: 600;
    color: var(--text-primary);
    flex-shrink: 0;
  }
</style>

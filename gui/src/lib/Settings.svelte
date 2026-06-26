<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { checkForUpdate } from "$lib/updateCheck";
  import { check } from "@tauri-apps/plugin-updater";

  interface AppSettings {
    update_strategy: "Merge" | "Rebase";
    ignore_whitespace: boolean;
    skip_hooks: boolean;
    silent_update: boolean;
    theme: string;
    density: string;
    font_size: string;
    animations_enabled: boolean;
    scanline_enabled: boolean;
    glow_intensity: string;
    ai_enabled: boolean;
    ai_base_url: string;
    ai_api_key: string;
    ai_model: string;
    ai_language: string;
    ai_max_diff_chars: number;
    ai_generate_body: boolean;
  }

  let {
    onClose,
    onAppearanceChanged,
  }: {
    onClose: () => void;
    onAppearanceChanged?: (s: AppSettings) => void;
  } = $props();

  // ── Git 设置 ──
  let strategy = $state<"Merge" | "Rebase">("Merge");
  let ignoreWhitespace = $state(true);
  let skipHooks = $state(false);
  let silentUpdate = $state(true);

  // ── 外观设置 ──
  let theme = $state("neon-dark");
  let density = $state("comfortable");
  let font_size = $state("medium");
  let animationsEnabled = $state(true);
  let scanlineEnabled = $state(true);
  let glowIntensity = $state("medium");

  // ── AI 提交助手 ──
  let aiEnabled = $state(false);
  let aiBaseUrl = $state("https://api.openai.com/v1");
  let aiApiKey = $state("");
  let aiModel = $state("gpt-4o-mini");
  let aiLanguage = $state("zh");
  let aiMaxDiffChars = $state(30000);
  let aiGenerateBody = $state(false);

  let loading = $state(true);
  let saving = $state(false);
  let error = $state("");

  // ── Windows Defender 排除引导 ──
  const isWindows =
    typeof navigator !== "undefined" && /win/i.test(navigator.platform);
  let copied = $state(false);

  function copyDefenderCmd() {
    const cmd = 'Add-MpPreference -ExclusionPath "C:\\你的仓库根目录"';
    navigator.clipboard.writeText(cmd).then(() => {
      copied = true;
      setTimeout(() => (copied = false), 2000);
    });
  }

  // ── 主题预设定义 ──
  const THEMES: {
    id: string;
    name: string;
    preview: string[];
    kind: string;
  }[] = [
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
      silentUpdate = s.silent_update ?? false;
      theme = s.theme || "neon-dark";
      density = s.density || "comfortable";
      font_size = s.font_size || "medium";
      animationsEnabled = s.animations_enabled ?? true;
      scanlineEnabled = s.scanline_enabled ?? true;
      glowIntensity = s.glow_intensity || "medium";
      aiEnabled = s.ai_enabled ?? false;
      aiBaseUrl = s.ai_base_url || "https://api.openai.com/v1";
      aiApiKey = s.ai_api_key || "";
      aiModel = s.ai_model || "gpt-4o-mini";
      aiLanguage = s.ai_language || "zh";
      aiMaxDiffChars = s.ai_max_diff_chars ?? 30000;
      aiGenerateBody = s.ai_generate_body ?? false;
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
        silent_update: silentUpdate,
        theme,
        density,
        font_size,
        animations_enabled: animationsEnabled,
        scanline_enabled: scanlineEnabled,
        glow_intensity: glowIntensity,
        ai_enabled: aiEnabled,
        ai_base_url: aiBaseUrl,
        ai_api_key: aiApiKey,
        ai_model: aiModel,
        ai_language: aiLanguage,
        ai_max_diff_chars: aiMaxDiffChars,
        ai_generate_body: aiGenerateBody,
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

  // ── 手动检查更新 ──
  let checking = $state(false);
  let checkMsg = $state("");
  let updateUrl = $state("");

  async function manualCheck() {
    checking = true;
    checkMsg = "";
    updateUrl = "";
    try {
      // 优先走 updater 插件（含签名校验）,不可用时回退到 GitHub API。
      const upd = await check();
      if (upd?.available) {
        checkMsg = `发现新版本 v${upd.version}（当前 v${upd.currentVersion}）`;
        // 有 Banner 按钮下载，这里不显 URL
        checking = false;
        return;
      }
      // 无更新或插件不可用，回退手动检查
      if (upd && !upd.available) {
        checkMsg = "已是最新版本";
        checking = false;
        return;
      }
    } catch {
      // 回退到手动 fetch
    }
    try {
      const u = await checkForUpdate();
      if (u) {
        checkMsg = `发现新版本 v${u.latest}（当前 v${u.current}）`;
        updateUrl = u.url;
      } else {
        checkMsg = "已是最新版本";
      }
    } catch (e) {
      checkMsg = "检查失败：" + String(e);
    } finally {
      checking = false;
    }
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
                    <span class="theme-swatch" style="background:{color}"
                    ></span>
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
              onclick={() => (density = "compact")}>紧凑</button
            >
            <button
              class="seg-btn"
              class:seg-active={density === "comfortable"}
              onclick={() => (density = "comfortable")}>舒适</button
            >
            <button
              class="seg-btn"
              class:seg-active={density === "spacious"}
              onclick={() => (density = "spacious")}>宽松</button
            >
          </div>
        </fieldset>

        <!-- ─── 外观：字号 ─── -->
        <fieldset class="st-group">
          <legend>🔤 字号大小</legend>
          <div class="segmented">
            <button
              class="seg-btn"
              class:seg-active={font_size === "small"}
              onclick={() => (font_size = "small")}>小</button
            >
            <button
              class="seg-btn"
              class:seg-active={font_size === "medium"}
              onclick={() => (font_size = "medium")}>中</button
            >
            <button
              class="seg-btn"
              class:seg-active={font_size === "large"}
              onclick={() => (font_size = "large")}>大</button
            >
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
                onclick={() => (glowIntensity = "off")}>关闭</button
              >
              <button
                class="seg-btn seg-sm"
                class:seg-active={glowIntensity === "subtle"}
                onclick={() => (glowIntensity = "subtle")}>微光</button
              >
              <button
                class="seg-btn seg-sm"
                class:seg-active={glowIntensity === "medium"}
                onclick={() => (glowIntensity = "medium")}>适中</button
              >
              <button
                class="seg-btn seg-sm"
                class:seg-active={glowIntensity === "strong"}
                onclick={() => (glowIntensity = "strong")}>强光</button
              >
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
            <small>整合时用 -Xignore-space-change，减少纯空白引起的伪冲突</small
            >
          </span>
        </label>

        <label class="st-check">
          <input type="checkbox" bind:checked={skipHooks} />
          <span>
            <b>提交时跳过 git 钩子</b>
            <small
              >勾选则提交用 --no-verify，不运行 pre-commit / commit-msg
              钩子。默认不跳过。</small
            >
          </span>
        </label>

        <label class="st-check">
          <input type="checkbox" bind:checked={silentUpdate} />
          <span>
            <b>静默更新</b>
            <small>
              点「更新」/「全部更新」时不弹进度窗口，后台执行；成功显示通知，冲突仍弹出解决窗口。
            </small>
          </span>
        </label>

        <!-- ─── AI 提交助手 ─── -->
        <fieldset class="st-group">
          <legend>🤖 AI 提交助手</legend>
          <p class="st-hint">
            根据「暂存的改动」自动生成 Conventional Commits
            风格的提交信息草稿;生成后可再编辑。仅支持 OpenAI 兼容协议(智谱 /
            DeepSeek / Kimi / 通义 / OpenAI 等均可填)。Key 明文保存在本地
            settings.json。
          </p>

          <label class="st-check">
            <input type="checkbox" bind:checked={aiEnabled} />
            <span>
              <b>启用 AI 生成提交信息</b>
              <small
                >开启后,提交区会为每个有暂存改动的仓库显示独立输入框 + 生成按钮</small
              >
            </span>
          </label>

          <label class="st-check">
            <input type="checkbox" bind:checked={aiGenerateBody} />
            <span>
              <b>生成提交信息正文</b>
              <small
                >开启后生成「标题 + 空行 + 简短正文」;关闭则只生成一行标题</small
              >
            </span>
          </label>

          <label class="st-radio" style="display:block;padding:6px 0">
            <span>
              <b>API Base URL</b>
              <input
                class="ai-input"
                type="text"
                bind:value={aiBaseUrl}
                placeholder="https://api.openai.com/v1"
              />
            </span>
          </label>

          <label class="st-radio" style="display:block;padding:6px 0">
            <span>
              <b>API Key</b>
              <input
                class="ai-input"
                type="password"
                bind:value={aiApiKey}
                placeholder="sk-..."
              />
            </span>
          </label>

          <label class="st-radio" style="display:block;padding:6px 0">
            <span>
              <b>模型</b>
              <input
                class="ai-input"
                type="text"
                bind:value={aiModel}
                placeholder="gpt-4o-mini"
              />
            </span>
          </label>

          <div class="glow-row">
            <span class="glow-label">提交信息语言</span>
            <div class="segmented">
              <button
                class="seg-btn seg-sm"
                class:seg-active={aiLanguage === "zh"}
                onclick={() => (aiLanguage = "zh")}>中文</button
              >
              <button
                class="seg-btn seg-sm"
                class:seg-active={aiLanguage === "en"}
                onclick={() => (aiLanguage = "en")}>English</button
              >
            </div>
          </div>

          <label class="st-radio" style="display:block;padding:6px 0">
            <span>
              <b>diff 截断字符数</b>
              <input
                class="ai-input"
                type="number"
                min="1000"
                bind:value={aiMaxDiffChars}
              />
            </span>
          </label>
        </fieldset>

        <!-- ─── 软件更新 ─── -->
        <fieldset class="st-group">
          <legend>🔄 软件更新</legend>
          <div class="update-row">
            <button class="st-cancel" disabled={checking} onclick={manualCheck}>
              {checking ? "检查中…" : "检查更新"}
            </button>
            {#if checkMsg}
              <span class="update-msg">{checkMsg}</span>
            {/if}
            {#if updateUrl}
              <button class="seg-btn" onclick={() => openUrl(updateUrl)}
                >打开下载页</button
              >
            {/if}
          </div>
        </fieldset>

        <!-- ─── Windows Defender 性能提示(仅 Windows 可见) ─── -->
        {#if isWindows}
          <fieldset class="st-group">
            <legend>🛡️ Windows Defender 性能建议</legend>
            <p class="st-hint">
              Windows 自带的实时防病毒扫描会拦截每次 <code>git.exe</code>
              启动和文件写入，导致大仓库操作和刷新风暴时卡顿显著。将仓库目录加入排除项可大幅缓解，且不影响其余目录的安全防护。
            </p>
            <p class="st-hint">
              以<b>管理员身份</b>打开 PowerShell 执行以下命令（将
              <code>你的仓库根目录</code> 替换为实际路径）：
            </p>
            <button
              class="def-cmd"
              title="点击复制"
              onclick={() => copyDefenderCmd()}
              >Add-MpPreference -ExclusionPath "C:\你的仓库根目录"</button
            >
            {#if copied}
              <span class="copied-msg">已复制!</span>
            {/if}
          </fieldset>
        {/if}
      </div>

      <div class="st-actions">
        <button class="st-save" disabled={saving} onclick={save}>
          {saving ? "保存中…" : "保存"}
        </button>
        <button class="st-cancel" disabled={saving} onclick={onClose}
          >取消</button
        >
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
    box-shadow: 0 12px 60px rgba(0, 0, 0, 0.5);
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
    box-shadow: inset 0 1px 0 rgba(0, 0, 0, 0.1);
  }
  .seg-sm {
    padding: 4px 10px;
    font-size: var(--fs-xs, 11px);
  }

  /* ── 软件更新行 ── */
  .update-row {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .update-msg {
    font-size: var(--fs-sm, 12px);
    color: var(--text-secondary);
  }

  /* ── Windows Defender 提示 ── */
  .st-hint {
    font-size: 12px;
    color: var(--text-secondary);
    line-height: 1.55;
    margin: 4px 0;
  }
  .st-hint code {
    background: var(--bg-surface);
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 11px;
  }
  .def-cmd {
    display: block;
    margin: 8px 0 4px;
    padding: 10px 14px;
    background: #1a1a2e;
    border: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.1));
    border-radius: 6px;
    color: var(--accent-cyan, #58a6ff);
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-all;
    cursor: pointer;
    user-select: all;
    transition: border-color 0.15s;
  }
  .def-cmd:hover {
    border-color: var(--accent-cyan, rgba(88, 166, 255, 0.5));
  }
  .copied-msg {
    font-size: 11px;
    color: #56d364;
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
  .ai-input {
    width: 100%;
    margin-top: 4px;
    padding: 6px 8px;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: var(--fs-sm, 12px);
    font-family: inherit;
  }
  .ai-input:focus {
    outline: none;
    border-color: var(--accent-cyan, #58a6ff);
  }
</style>

<script lang="ts">
  // WebStorm 式三栏合并编辑器(单个内容文件):整文件 ours | 结果 | theirs,
  // 冲突与单边更改都对齐高亮、可逐区域取左/取右/两者/忽略/编辑;结果按决策实时拼接。
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { wordDiffLines, type WordTok } from "$lib/wordDiff";

  type RegionKind =
    | "Unchanged"
    | "OursOnly"
    | "TheirsOnly"
    | "BothSame"
    | "Conflict";
  interface MergeRegion {
    kind: RegionKind;
    ours: string[];
    base: string[];
    theirs: string[];
  }
  // 单边:applied|ignored;冲突:undecided|left|right|both|edited。
  type Decision =
    | "applied"
    | "ignored"
    | "undecided"
    | "left"
    | "right"
    | "both"
    | "edited";

  let {
    path,
    file,
    onWritten,
  }: { path: string; file: string; onWritten: () => void } = $props();

  let regions = $state<MergeRegion[]>([]);
  let decisions = $state<Decision[]>([]);
  let edited = $state<string[]>([]); // 冲突 edited 模式的文本(每区域)
  let loading = $state(true);
  let error = $state("");
  // 整文件手动编辑(无区域可解析时的退回 / 逃生口)。
  let manualMode = $state(false);
  let manualText = $state("");

  onMount(async () => {
    try {
      const rs = await invoke<MergeRegion[]>("merge_regions_cmd", {
        path,
        filePath: file,
      });
      if (rs.length === 0) {
        // stage 已清(已解决)或无可解析区域 → 退回整文件手动编辑。
        await enterManual();
      } else {
        regions = rs;
        decisions = rs.map((r) =>
          r.kind === "Conflict" ? "undecided" : "applied",
        );
        edited = rs.map(() => "");
      }
    } catch (e) {
      error = String(e);
    }
    loading = false;
  });

  // ── 结果文本(按决策拼接) ──
  function regionResultText(i: number): string {
    const r = regions[i];
    const d = decisions[i];
    switch (r.kind) {
      case "Unchanged":
        return r.ours.join("");
      case "OursOnly":
        return (d === "ignored" ? r.base : r.ours).join("");
      case "TheirsOnly":
        return (d === "ignored" ? r.base : r.theirs).join("");
      case "BothSame":
        return (d === "ignored" ? r.base : r.ours).join("");
      case "Conflict":
        if (d === "left") return r.ours.join("");
        if (d === "right") return r.theirs.join("");
        if (d === "both") return r.ours.join("") + r.theirs.join("");
        if (d === "edited") return edited[i];
        return ""; // undecided
    }
  }
  let fullText = $derived(regions.map((_, i) => regionResultText(i)).join(""));
  let conflictCount = $derived(
    regions.filter((r) => r.kind === "Conflict").length,
  );
  let undecided = $derived(
    regions.filter(
      (r, i) => r.kind === "Conflict" && decisions[i] === "undecided",
    ).length,
  );
  let changeCount = $derived(
    regions.filter((r) => r.kind !== "Unchanged").length,
  );

  // ── 词级 token(高亮差异) ──
  function plainToks(lines: string[]): WordTok[][] {
    return lines.map((l) => [{ text: l.replace(/\n$/, ""), changed: false }]);
  }
  function oursTok(r: MergeRegion): WordTok[][] {
    if (r.kind === "Conflict")
      return wordDiffLines(r.ours.join(""), r.theirs.join(""));
    if (r.kind === "OursOnly" || r.kind === "BothSame")
      return wordDiffLines(r.ours.join(""), r.base.join(""));
    return plainToks(r.ours);
  }
  function theirsTok(r: MergeRegion): WordTok[][] {
    if (r.kind === "Conflict")
      return wordDiffLines(r.theirs.join(""), r.ours.join(""));
    if (r.kind === "TheirsOnly" || r.kind === "BothSame")
      return wordDiffLines(r.theirs.join(""), r.base.join(""));
    return plainToks(r.theirs);
  }
  function splitLines(text: string): string[] {
    if (text === "") return [];
    return text.replace(/\n$/, "").split("\n");
  }

  // 带各列行号的对齐 band(决策/edited 变化时重算)。
  let bands = $derived.by(() => {
    let oN = 0;
    let tN = 0;
    let rN = 0;
    return regions.map((r, i) => {
      const ours = oursTok(r).map((toks) => ({ no: ++oN, toks }));
      const theirs = theirsTok(r).map((toks) => ({ no: ++tN, toks }));
      const result = splitLines(regionResultText(i)).map((text) => ({
        no: ++rN,
        text,
      }));
      return { i, kind: r.kind, decision: decisions[i], ours, theirs, result };
    });
  });

  // ── 决策操作 ──
  function setDecision(i: number, d: Decision) {
    decisions[i] = d;
  }
  function startEdit(i: number) {
    const r = regions[i];
    edited[i] = r.ours.join("") + r.theirs.join("");
    decisions[i] = "edited";
  }
  function takeAll(side: "left" | "right") {
    decisions = decisions.map((d, i) =>
      regions[i].kind === "Conflict" ? side : d,
    );
  }
  function applyAllNonConflict() {
    decisions = decisions.map((d, i) =>
      regions[i].kind === "Conflict" ? d : "applied",
    );
  }

  // ── 写回 ──
  let writing = $state(false);
  async function write() {
    writing = true;
    error = "";
    try {
      const text = manualMode ? manualText : fullText;
      if (/^<{7}/m.test(text) || /^>{7}/m.test(text)) {
        error = "文本仍含冲突标记（<<<<<<< / >>>>>>>),请先清除再写入";
        writing = false;
        return;
      }
      await invoke("resolve_conflict_file", { path, filePath: file, text });
      onWritten();
    } catch (e) {
      error = String(e);
    }
    writing = false;
  }

  async function enterManual() {
    try {
      manualText = await invoke<string>("read_repo_file", {
        path,
        filePath: file,
      });
      manualMode = true;
    } catch (e) {
      error = String(e);
    }
  }
  function exitManual() {
    manualMode = false;
  }

  // 决策态 → 列着色类。
  function bandClass(kind: RegionKind, decision: Decision): string {
    if (kind === "Unchanged") return "";
    if (kind === "Conflict")
      return decision === "undecided" ? "b-conflict" : "b-resolved";
    return decision === "ignored" ? "b-ignored" : "b-change";
  }
</script>

<div class="merge-editor">
  {#if error}
    <pre class="me-error">{error}</pre>
  {/if}

  {#if loading}
    <p class="me-status">加载合并视图…</p>
  {:else if manualMode}
    <!-- ── 整文件手动编辑 ── -->
    <div class="me-toolbar">
      <span class="me-info">手动编辑整文件</span>
      <span class="me-spacer"></span>
      {#if regions.length > 0}
        <button class="me-btn" onclick={exitManual}>返回三栏</button>
      {/if}
      <button class="me-btn me-primary" disabled={writing} onclick={write}>
        写入
      </button>
    </div>
    <textarea class="me-manual" bind:value={manualText} spellcheck="false"
    ></textarea>
  {:else}
    <!-- ── 工具条 ── -->
    <div class="me-toolbar">
      <span class="me-info">
        {changeCount} 个更改 · {conflictCount} 个冲突{#if undecided > 0}<span
            class="me-undecided"
          >
            · {undecided} 未决</span
          >{/if}
      </span>
      <span class="me-spacer"></span>
      <button class="me-btn" onclick={() => takeAll("left")}>全部取左</button>
      <button class="me-btn" onclick={() => takeAll("right")}>全部取右</button>
      <button
        class="me-btn"
        onclick={applyAllNonConflict}
        title="把所有非冲突更改并入结果">应用不冲突</button
      >
      <button class="me-btn" onclick={enterManual}>手动编辑</button>
      <button
        class="me-btn me-primary"
        disabled={undecided > 0 || writing}
        title={undecided > 0
          ? `还有 ${undecided} 个冲突未决`
          : "写入并标记已解决"}
        onclick={write}
      >
        写入
      </button>
    </div>

    <!-- ── 主体:粘顶表头 + 每区域一行 5 列网格(ours │中缝│ 结果 │中缝│ theirs) ── -->
    <div class="me-body">
      <div class="me-heads">
        <div class="me-head head-ours">OURS · 本地</div>
        <div class="me-gut"></div>
        <div class="me-head head-result">结果</div>
        <div class="me-gut"></div>
        <div class="me-head head-theirs">THEIRS · 对方</div>
      </div>
      {#each bands as band (band.i)}
        <div
          class="region region-{band.kind.toLowerCase()} {bandClass(
            band.kind,
            band.decision,
          )}"
        >
          <!-- ours -->
          <div class="col col-ours">
            {#each band.ours as row (row.no)}
              <div class="ln">
                <span class="lno">{row.no}</span>
                <span class="lt"
                  >{#each row.toks as tk}{#if tk.changed}<span class="wc"
                        >{tk.text}</span
                      >{:else}{tk.text}{/if}{/each}</span
                >
              </div>
            {/each}
          </div>

          <!-- 左中缝:取左 / 两者(或单边应用/忽略) -->
          <div class="gut gut-l">
            {#if band.kind === "Conflict"}
              <button
                class="chev chev-l"
                class:on={band.decision === "left"}
                title="采用左侧(ours)"
                onclick={() => setDecision(band.i, "left")}>»</button
              >
              <button
                class="chev chev-mini"
                class:on={band.decision === "both"}
                title="两者都要(左+右)"
                onclick={() => setDecision(band.i, "both")}>±</button
              >
            {:else if band.kind === "OursOnly" || band.kind === "BothSame"}
              <button
                class="chev"
                class:on={band.decision !== "ignored"}
                title={band.decision !== "ignored"
                  ? "已并入结果 — 点击忽略此更改"
                  : "已忽略 — 点击并入结果"}
                onclick={() =>
                  setDecision(
                    band.i,
                    band.decision === "ignored" ? "applied" : "ignored",
                  )}>{band.decision !== "ignored" ? "✓" : "○"}</button
              >
            {/if}
          </div>

          <!-- result -->
          <div class="col col-result">
            {#if band.kind === "Conflict" && band.decision === "edited"}
              <textarea
                class="me-edit"
                bind:value={edited[band.i]}
                spellcheck="false"></textarea>
            {:else if band.kind === "Conflict" && band.decision === "undecided"}
              <div class="ln me-unresolved">⚠ 未解决</div>
            {:else}
              {#each band.result as row (row.no)}
                <div class="ln">
                  <span class="lno">{row.no}</span>
                  <span class="lt">{row.text}</span>
                </div>
              {/each}
            {/if}
          </div>

          <!-- 右中缝:取右 / 编辑(或单边应用/忽略) -->
          <div class="gut gut-r">
            {#if band.kind === "Conflict"}
              <button
                class="chev chev-r"
                class:on={band.decision === "right"}
                title="采用右侧(theirs)"
                onclick={() => setDecision(band.i, "right")}>«</button
              >
              <button
                class="chev chev-mini"
                class:on={band.decision === "edited"}
                title="手动编辑此块"
                onclick={() => startEdit(band.i)}>✎</button
              >
            {:else if band.kind === "TheirsOnly"}
              <button
                class="chev"
                class:on={band.decision !== "ignored"}
                title={band.decision !== "ignored"
                  ? "已并入结果 — 点击忽略此更改"
                  : "已忽略 — 点击并入结果"}
                onclick={() =>
                  setDecision(
                    band.i,
                    band.decision === "ignored" ? "applied" : "ignored",
                  )}>{band.decision !== "ignored" ? "✓" : "○"}</button
              >
            {/if}
          </div>

          <!-- theirs -->
          <div class="col col-theirs">
            {#each band.theirs as row (row.no)}
              <div class="ln">
                <span class="lno">{row.no}</span>
                <span class="lt"
                  >{#each row.toks as tk}{#if tk.changed}<span class="wc"
                        >{tk.text}</span
                      >{:else}{tk.text}{/if}{/each}</span
                >
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .merge-editor {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }
  .me-error {
    background: #3a1d1d;
    border: 1px solid rgba(247, 120, 139, 0.25);
    border-radius: 6px;
    padding: 8px 12px;
    color: var(--color-error);
    white-space: pre-wrap;
    font-size: 12px;
    margin: 0 0 8px;
  }
  .me-status {
    color: var(--text-muted);
  }

  /* ── 工具条 ── */
  .me-toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 2px 8px;
    flex-shrink: 0;
  }
  .me-info {
    font-size: 12px;
    color: var(--text-secondary);
  }
  .me-undecided {
    color: var(--accent-gold);
    font-weight: 600;
  }
  .me-spacer {
    flex: 1;
  }
  .me-btn {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    padding: 4px 10px;
  }
  .me-btn:hover {
    background: var(--border-default);
  }
  .me-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .me-primary {
    background: var(--accent-cyan);
    border-color: var(--accent-cyan);
    color: #fff;
  }
  .me-primary:hover:not(:disabled) {
    background: #58a6ff;
  }

  /* 三栏 + 两条中缝的共用列模板 */
  .me-heads,
  .region {
    display: grid;
    grid-template-columns: 1fr 44px 1fr 44px 1fr;
  }

  /* ── 三栏标题(容器内粘顶,与内容列严格对齐) ── */
  .me-heads {
    position: sticky;
    top: 0;
    z-index: 2;
  }
  .me-head {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-secondary);
    padding: 5px 12px;
    background: var(--bg-surface);
    border-bottom: 2px solid var(--border-default);
  }
  .head-ours {
    border-bottom-color: var(--accent-neon, #56d364);
  }
  .head-theirs {
    border-bottom-color: var(--accent-cyan, #58a6ff);
  }
  .head-result {
    color: var(--text-primary);
    background: var(--bg-void, #0d1117);
  }
  .me-gut {
    background: var(--bg-surface);
    border-bottom: 2px solid var(--border-default);
  }

  /* ── 主体:单一滚动容器,内含逐区域 5 列网格 ── */
  .me-body {
    overflow: auto;
    flex: 1;
    min-height: 0;
    border: 1px solid var(--border-default);
    border-radius: 6px;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    line-height: 1.6;
  }
  .region + .region {
    border-top: 1px solid var(--bg-hover);
  }

  /* 列:ours/theirs 抬升底,结果列用更深底当"画布" */
  .col {
    padding: 2px 0;
    overflow-x: auto;
    min-width: 0;
  }
  .col-ours,
  .col-theirs {
    background: var(--bg-elevated);
  }
  .col-result {
    background: var(--bg-void, #0d1117);
  }

  /* 中缝:不同底色,既是列分隔又承载取舍按钮 */
  .gut {
    background: var(--bg-surface);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 4px 0;
  }
  .chev {
    width: 26px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
    padding: 0;
  }
  .chev:hover {
    background: var(--accent-cyan);
    border-color: var(--accent-cyan);
    color: #fff;
  }
  .chev.on {
    background: var(--accent-cyan);
    border-color: var(--accent-cyan);
    color: #fff;
  }
  .chev-mini {
    width: 22px;
    height: 18px;
    font-size: 11px;
  }

  /* ── 区域着色(只染变化的一侧,结果列保持画布) ── */
  .region-conflict.b-conflict .col-ours,
  .region-conflict.b-conflict .col-theirs {
    background: #2c1719;
  }
  .region-conflict.b-resolved .col-result {
    box-shadow: inset 3px 0 0 var(--accent-neon, #56d364);
  }
  .region-oursonly .col-ours,
  .region-bothsame .col-ours,
  .region-bothsame .col-theirs {
    background: #14202e;
  }
  .region-theirsonly .col-theirs {
    background: #14202e;
  }
  .b-ignored .col {
    opacity: 0.45;
  }

  .ln {
    display: flex;
    white-space: pre;
    min-height: 1.6em;
  }
  .lno {
    flex-shrink: 0;
    width: 38px;
    text-align: right;
    padding-right: 10px;
    color: var(--text-muted);
    opacity: 0.5;
    user-select: none;
  }
  .lt {
    flex: 1;
    padding-right: 10px;
  }
  .wc {
    background: rgba(247, 120, 139, 0.3);
    border-radius: 2px;
    color: #ffd0d8;
  }
  .me-unresolved {
    color: var(--accent-gold);
    padding: 2px 10px;
    font-weight: 600;
  }
  .me-edit {
    width: 100%;
    box-sizing: border-box;
    min-height: 60px;
    resize: vertical;
    background: var(--bg-surface);
    border: 1px solid var(--accent-cyan);
    border-radius: 4px;
    color: var(--text-primary, #e6e6e6);
    padding: 4px 8px;
    font-family: inherit;
    font-size: 13px;
    line-height: 1.6;
    white-space: pre;
  }

  /* ── 整文件手动编辑 ── */
  .me-manual {
    width: 100%;
    box-sizing: border-box;
    flex: 1;
    min-height: 300px;
    resize: none;
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    color: var(--text-primary, #e6e6e6);
    padding: 8px 10px;
    font-family:
      "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    line-height: 1.55;
    white-space: pre;
    tab-size: 4;
  }
</style>

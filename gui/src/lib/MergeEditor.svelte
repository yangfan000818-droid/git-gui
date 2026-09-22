<script lang="ts">
  // WebStorm 式三栏合并编辑器(单个内容文件):整文件 ours | 结果 | theirs,
  // 冲突与单边更改都对齐高亮、可逐区域取左/取右/两者/忽略/编辑;结果按决策实时拼接。
  import { invoke } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";
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
    onDirtyChange,
  }: {
    path: string;
    file: string;
    onWritten: () => void;
    onDirtyChange?: (dirty: boolean) => void;
  } = $props();

  let regions = $state<MergeRegion[]>([]);
  let decisions = $state<Decision[]>([]);
  let edited = $state<string[]>([]); // 冲突 edited 模式的文本(每区域)
  let loading = $state(true);
  let error = $state("");
  // 整文件手动编辑(无区域可解析时的退回 / 逃生口)。
  let manualMode = $state(false);
  let manualText = $state("");
  // 有未写入的取舍变更(用于离开前确认,防丢决策状态)。
  let dirty = $state(false);
  // 各 region 的 DOM 元素,供冲突导航滚动定位。
  let regionEls: HTMLElement[] = [];
  let focusIdx = $state(-1);

  function markDirty() {
    dirty = true;
  }
  // 上报 dirty 给父组件(返回列表 / 关窗前确认)。
  $effect(() => {
    onDirtyChange?.(dirty);
  });

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
      error = `加载合并视图失败:${String(e)}`;
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
        // ours 末行无换行(文件末尾)时补一个,否则 theirs 会粘到同一行。
        if (d === "both") return nlTerm(r.ours.join("")) + r.theirs.join("");
        // 同样要补换行:startEdit 预填的文本末尾有换行,但用户删掉它是最自然不过的
        // 事(手工敲完最后一个字符就收手),不补的话下一段会整段粘到这一行末尾。
        if (d === "edited") return nlTerm(edited[i]);
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

  // 左右两列的词级 token 只由 regions 决定,与决策无关 —— 必须单独 derive。
  // (原来和结果列算在同一个 $derived 里:改一次取舍、在编辑框里敲一个字符,
  //  就把整份文件所有区域的词级 LCS 全部重跑一遍,大文件明显卡顿。)
  let sideBands = $derived.by(() => {
    let oN = 0;
    let tN = 0;
    return regions.map((r) => ({
      ours: oursTok(r).map((toks) => ({ no: ++oN, toks })),
      theirs: theirsTok(r).map((toks) => ({ no: ++tN, toks })),
    }));
  });

  // 带各列行号的对齐 band;只有结果列随决策 / 编辑内容重算。
  let bands = $derived.by(() => {
    let rN = 0;
    return regions.map((r, i) => ({
      i,
      kind: r.kind,
      decision: decisions[i],
      ours: sideBands[i].ours,
      theirs: sideBands[i].theirs,
      result: splitLines(regionResultText(i)).map((text) => ({
        no: ++rN,
        text,
      })),
    }));
  });

  // ── 决策操作 ──
  function setDecision(i: number, d: Decision) {
    decisions[i] = d;
    markDirty();
  }
  // 文本段末尾补换行:拼接时少一个换行会把下一段粘到同一行。
  function nlTerm(t: string): string {
    return t === "" || t.endsWith("\n") ? t : t + "\n";
  }
  function startEdit(i: number) {
    const r = regions[i];
    edited[i] = nlTerm(r.ours.join("")) + nlTerm(r.theirs.join(""));
    decisions[i] = "edited";
    markDirty();
  }
  // 点结果列:已决的块以当前结果为起点进自由编辑;未决的块只定位,不替你做决定。
  // (以前点一下就把 ours+theirs 拼起来写进结果并标记已决,误点一次且无法退回。)
  function editResult(i: number) {
    const r = regions[i];
    if (r.kind !== "Conflict") return; // 单边更改维持 ✓/○
    if (decisions[i] === "undecided") {
      focusIdx = i;
      return;
    }
    edited[i] = regionResultText(i);
    decisions[i] = "edited";
    markDirty();
  }
  // 重置一个冲突块回未决:误点 / 改主意时的退路。手工编辑过的先确认。
  async function resetDecision(i: number) {
    if (regions[i].kind !== "Conflict") return;
    if (decisions[i] === "edited" && edited[i].trim() !== "") {
      const ok = await ask("重置会丢弃这一块手工编辑的内容。确定?", {
        title: "重置本块",
        kind: "warning",
      });
      if (!ok) return;
    }
    decisions[i] = "undecided";
    edited[i] = "";
    focusIdx = i;
    markDirty();
  }
  // 结果列(冲突区)作为可点击元素:Enter / Space 进入编辑。
  function handleResultKey(i: number) {
    return (e: KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        editResult(i);
      }
    };
  }
  async function takeAll(side: "left" | "right") {
    const edits = regions.filter(
      (r, i) => r.kind === "Conflict" && decisions[i] === "edited",
    ).length;
    if (edits > 0) {
      const ok = await ask(
        `有 ${edits} 个块是手工编辑过的,「全部取${side === "left" ? "左" : "右"}」会覆盖掉它们。确定?`,
        { title: "覆盖手工编辑", kind: "warning" },
      );
      if (!ok) return;
    }
    decisions = decisions.map((d, i) =>
      regions[i].kind === "Conflict" ? side : d,
    );
    markDirty();
  }
  function applyAllNonConflict() {
    decisions = decisions.map((d, i) =>
      regions[i].kind === "Conflict" ? d : "applied",
    );
    markDirty();
  }

  // ── 冲突导航:跳到上一个 / 下一个未决冲突 ──
  function undecidedList(): number[] {
    const r: number[] = [];
    for (let i = 0; i < regions.length; i++) {
      if (regions[i].kind === "Conflict" && decisions[i] === "undecided") {
        r.push(i);
      }
    }
    return r;
  }
  function gotoConf(dir: 1 | -1) {
    const list = undecidedList();
    if (list.length === 0) return;
    let target: number;
    if (dir === 1) {
      target = list.find((i) => i > focusIdx) ?? list[0];
    } else {
      target =
        [...list].reverse().find((i) => i < focusIdx) ?? list[list.length - 1];
    }
    focusIdx = target;
    regionEls[target]?.scrollIntoView({ block: "center", behavior: "smooth" });
  }

  // 当前焦点冲突:已导航过的 focusIdx,否则第一个未决。
  function currentConflictIdx(): number {
    if (focusIdx >= 0 && regions[focusIdx]?.kind === "Conflict")
      return focusIdx;
    const list = undecidedList();
    return list.length ? list[0] : -1;
  }
  function decideFocused(d: Decision) {
    const i = currentConflictIdx();
    if (i < 0) return;
    setDecision(i, d);
    gotoConf(1); // 决定后跳到下一个未决冲突
  }
  function editFocused() {
    const i = currentConflictIdx();
    if (i < 0) return;
    // 键盘 e 是明确的"我要编辑"意图:未决块直接以 ours+theirs 起手,
    // 不走结果列点击那条"只定位"的路。
    if (decisions[i] === "undecided") startEdit(i);
    else editResult(i);
  }

  // 键盘:j/k 跳冲突 · 1/2 取左右 · b 两者 · e 编辑(在文本框内输入时不触发)。
  function handleKey(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const t = e.target as HTMLElement | null;
    if (
      t &&
      (t.tagName === "TEXTAREA" || t.tagName === "INPUT" || t.isContentEditable)
    )
      return;
    let acted = true;
    switch (e.key) {
      case "j":
        gotoConf(1);
        break;
      case "k":
        gotoConf(-1);
        break;
      case "1":
        decideFocused("left");
        break;
      case "2":
        decideFocused("right");
        break;
      case "b":
        decideFocused("both");
        break;
      case "e":
        editFocused();
        break;
      default:
        acted = false;
    }
    if (acted) e.preventDefault();
  }

  // ── Unchanged 上下文折叠:行数过多时只显首尾几行 + 可点开占位 ──
  let expanded = $state<Record<number, boolean>>({});
  type DisplayItem = { gap: false; idx: number } | { gap: true; count: number };
  const FOLD_CTX = 3;
  const FOLD_THRESHOLD = 10;
  function displayItems(
    kind: RegionKind,
    total: number,
    i: number,
  ): DisplayItem[] {
    const fold = kind === "Unchanged" && total > FOLD_THRESHOLD && !expanded[i];
    if (!fold) {
      return Array.from({ length: total }, (_, k) => ({
        gap: false as const,
        idx: k,
      }));
    }
    const items: DisplayItem[] = [];
    for (let k = 0; k < FOLD_CTX; k++) items.push({ gap: false, idx: k });
    items.push({ gap: true, count: total - FOLD_CTX * 2 });
    for (let k = total - FOLD_CTX; k < total; k++)
      items.push({ gap: false, idx: k });
    return items;
  }
  function toggleExpand(i: number) {
    expanded[i] = !expanded[i];
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
      dirty = false;
      onWritten();
    } catch (e) {
      error = String(e);
    }
    writing = false;
  }

  // 未决冲突块回写成 zdiff3 标记(write 会拦住带标记的文本,逼你处理完再写)。
  function manualDraft(): string {
    return regions
      .map((r, i) => {
        if (r.kind === "Conflict" && decisions[i] === "undecided") {
          return (
            "<<<<<<< ours\n" +
            nlTerm(r.ours.join("")) +
            "||||||| base\n" +
            nlTerm(r.base.join("")) +
            "=======\n" +
            nlTerm(r.theirs.join("")) +
            ">>>>>>> theirs\n"
          );
        }
        return regionResultText(i);
      })
      .join("");
  }

  // 进入手动编辑时的初始文本,用于判断要不要拦"返回三栏"。
  let manualBase = $state("");

  async function enterManual() {
    // 有三栏数据时以当前取舍拼出的结果为起点:以前直接读磁盘上的原始标记文件,
    // 已经做完的取舍全部白做。
    if (regions.length > 0) {
      manualText = manualDraft();
      manualBase = manualText;
      manualMode = true;
      return;
    }
    try {
      manualText = await invoke<string>("read_repo_file", {
        path,
        filePath: file,
      });
      manualBase = manualText;
      manualMode = true;
    } catch (e) {
      error = String(e);
    }
  }
  async function exitManual() {
    if (manualText !== manualBase) {
      const ok = await ask(
        "返回三栏会丢弃手动编辑的内容(回到按取舍拼出的结果)。确定?",
        { title: "放弃手动编辑", kind: "warning" },
      );
      if (!ok) return;
    }
    manualMode = false;
  }

  // ── 横向滚动同步 ──
  // 每个区域的每一列都是独立滚动容器(列宽 1fr,长行只能自己横滚),
  // 不同步的话长行一滚就三列错位、区域之间也各滚各的。按列归组,组内跟随。
  type HCol = "ours" | "result" | "theirs";
  const hGroups: Record<HCol, Set<HTMLElement>> = {
    ours: new Set(),
    result: new Set(),
    theirs: new Set(),
  };
  let hSyncing = false;
  function hsync(node: HTMLElement, col: HCol) {
    const group = hGroups[col];
    group.add(node);
    const onScroll = () => {
      if (hSyncing) return;
      hSyncing = true;
      const left = node.scrollLeft;
      for (const el of group) {
        if (el !== node) el.scrollLeft = left;
      }
      // 赋值触发的 scroll 事件在下一帧才到,用帧边界解锁避免来回抖。
      requestAnimationFrame(() => {
        hSyncing = false;
      });
    };
    node.addEventListener("scroll", onScroll, { passive: true });
    return {
      destroy() {
        node.removeEventListener("scroll", onScroll);
        group.delete(node);
      },
    };
  }

  // 冲突块编辑框:挂载即聚焦 + 高度随内容自适应(免再点一次、免固定高度)。
  function editArea(node: HTMLTextAreaElement) {
    const grow = () => {
      node.style.height = "auto";
      node.style.height = `${Math.max(60, node.scrollHeight)}px`;
    };
    grow();
    node.addEventListener("input", grow);
    node.focus();
    return {
      destroy() {
        node.removeEventListener("input", grow);
      },
    };
  }

  // 决策态 → 列着色类。
  function bandClass(kind: RegionKind, decision: Decision): string {
    if (kind === "Unchanged") return "";
    if (kind === "Conflict")
      return decision === "undecided" ? "b-conflict" : "b-resolved";
    return decision === "ignored" ? "b-ignored" : "b-change";
  }
</script>

<svelte:window onkeydown={handleKey} />

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
        {writing ? "写入中…" : "写入"}
      </button>
    </div>
    <textarea
      class="me-manual"
      bind:value={manualText}
      oninput={markDirty}
      spellcheck="false"></textarea>
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
      {#if conflictCount > 0}
        <button
          class="me-btn me-icon"
          disabled={undecided === 0}
          onclick={() => gotoConf(-1)}
          title="上一个未决冲突">↑</button
        >
        <button
          class="me-btn me-icon"
          disabled={undecided === 0}
          onclick={() => gotoConf(1)}
          title="下一个未决冲突">↓</button
        >
      {/if}
      <span
        class="me-keys"
        title="键盘:j/k 跳冲突 · 1/2 取左右 · b 两者 · e 编辑">⌨</span
      >
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
        {writing ? "写入中…" : "写入"}
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
        {@const editable =
          band.kind === "Conflict" && band.decision !== "edited"}
        {@const focused = band.i === focusIdx}
        {@const oursItems = displayItems(band.kind, band.ours.length, band.i)}
        {@const theirsItems = displayItems(
          band.kind,
          band.theirs.length,
          band.i,
        )}
        {@const resultItems = displayItems(
          band.kind,
          band.result.length,
          band.i,
        )}
        <div
          bind:this={regionEls[band.i]}
          class="region region-{band.kind.toLowerCase()} {bandClass(
            band.kind,
            band.decision,
          )}"
          class:region-focus={focused}
        >
          <!-- ours -->
          <div class="col col-ours" use:hsync={"ours"}>
            {#each oursItems as it (it.gap ? "g" : "r" + it.idx)}
              {#if it.gap}
                <div class="ln-gap" aria-hidden="true">⋯</div>
              {:else}
                {@const row = band.ours[it.idx]}
                <div class="ln">
                  <span class="lno">{row.no}</span>
                  <span class="lt"
                    >{#each row.toks as tk}{#if tk.changed}<span class="wc"
                          >{tk.text}</span
                        >{:else}{tk.text}{/if}{/each}</span
                  >
                </div>
              {/if}
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
          <div class="col col-result" use:hsync={"result"}>
            {#if band.kind === "Conflict" && band.decision === "edited"}
              <textarea
                class="me-edit"
                bind:value={edited[band.i]}
                oninput={markDirty}
                use:editArea
                spellcheck="false"></textarea>
            {:else if editable}
              <div
                class="result-editable"
                class:result-undecided={band.decision === "undecided"}
                role="button"
                tabindex={0}
                aria-label={band.decision === "undecided"
                  ? "定位到此冲突"
                  : "编辑此冲突的结果"}
                onclick={() => editResult(band.i)}
                onkeydown={handleResultKey(band.i)}
              >
                {#if band.decision === "undecided"}
                  <div class="ln me-unresolved">
                    ⚠ 未解决 — 用两侧 » « 选边,或 ✎ 手动编辑
                  </div>
                {:else}
                  {#each band.result as row (row.no)}
                    <div class="ln">
                      <span class="lno">{row.no}</span>
                      <span class="lt">{row.text}</span>
                    </div>
                  {/each}
                {/if}
              </div>
            {:else}
              {#each resultItems as it (it.gap ? "g" : "r" + it.idx)}
                {#if it.gap}
                  <div
                    class="ln-gap ln-gap-action"
                    role="button"
                    tabindex={0}
                    title="点击展开"
                    aria-label="展开折叠的未改动行"
                    onclick={() => toggleExpand(band.i)}
                    onkeydown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        toggleExpand(band.i);
                      }
                    }}
                  >
                    ⋯ {it.count} 行
                  </div>
                {:else}
                  {@const row = band.result[it.idx]}
                  <div class="ln">
                    <span class="lno">{row.no}</span>
                    <span class="lt">{row.text}</span>
                  </div>
                {/if}
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
              {#if band.decision !== "undecided"}
                <button
                  class="chev chev-mini"
                  title="重置本块 — 回到未决,重新选"
                  onclick={() => resetDecision(band.i)}>↺</button
                >
              {/if}
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
          <div class="col col-theirs" use:hsync={"theirs"}>
            {#each theirsItems as it (it.gap ? "g" : "r" + it.idx)}
              {#if it.gap}
                <div class="ln-gap" aria-hidden="true">⋯</div>
              {:else}
                {@const row = band.theirs[it.idx]}
                <div class="ln">
                  <span class="lno">{row.no}</span>
                  <span class="lt"
                    >{#each row.toks as tk}{#if tk.changed}<span class="wc"
                          >{tk.text}</span
                        >{:else}{tk.text}{/if}{/each}</span
                  >
                </div>
              {/if}
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
    flex-wrap: wrap;
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
  .me-icon {
    width: 28px;
    padding: 4px 0;
    text-align: center;
    font-size: 13px;
  }
  .me-keys {
    font-size: 12px;
    color: var(--text-muted);
    cursor: help;
    user-select: none;
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
    /* 对标 WebStorm:三栏等宽,中缝承载接受箭头 */
    grid-template-columns: 1fr 40px 1fr 40px 1fr;
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
  /* 表头色点:对应词级配色,一眼区分 ours(绿)/ theirs(青) */
  .head-ours::before {
    content: "● ";
    color: var(--accent-neon, #56d364);
  }
  .head-theirs::before {
    content: "● ";
    color: var(--accent-cyan, #58a6ff);
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
  /* 冲突区的结果列可点击编辑:左侧淡青竖线 + hover 提示可交互 */
  .result-editable {
    cursor: pointer;
    box-shadow: inset 2px 0 0 rgba(88, 166, 255, 0.35);
  }
  .result-editable:hover {
    background: rgba(88, 166, 255, 0.1);
  }
  /* 未决块点了只定位不做决定,别用编辑光标误导 */
  .result-undecided {
    cursor: default;
  }
  /* 冲突导航跳到的 region:短暂高亮提示 */
  .region-focus {
    outline: 2px solid var(--accent-cyan, #58a6ff);
    outline-offset: -2px;
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
  /* 接受箭头:对标 WebStorm,默认低调半透明,hover/激活才提亮 */
  .chev {
    width: 26px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
    padding: 0;
    transition:
      background 0.12s,
      border-color 0.12s,
      color 0.12s;
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

  /* ── 区域着色(对标 WebStorm:改动侧整列柔和底色 + 内缘 gutter 竖条标记) ── */
  /* 冲突:两侧 + 中缝暖红,改动侧内缘红竖条(指向中缝),冲突块连成一片;
     红调比单边蓝更浓,拉开"冲突 vs 单边更改"的视觉层次(对标 WebStorm)。 */
  .region-conflict.b-conflict .col-ours,
  .region-conflict.b-conflict .col-theirs,
  .region-conflict.b-conflict .gut {
    background: rgba(229, 83, 75, 0.2);
  }
  .region-conflict.b-conflict .col-ours {
    box-shadow: inset -4px 0 0 rgba(229, 83, 75, 0.9);
  }
  .region-conflict.b-conflict .col-theirs {
    box-shadow: inset 4px 0 0 rgba(229, 83, 75, 0.9);
  }
  /* 冲突已决:结果列左侧绿条 */
  .region-conflict.b-resolved .col-result {
    box-shadow: inset 3px 0 0 var(--accent-neon, #56d364);
  }
  /* 单边改动:对标 WebStorm,统一蓝色(不分 ours/theirs);竖条朝中缝 */
  .region-oursonly .col-ours,
  .region-bothsame .col-ours {
    background: rgba(88, 166, 255, 0.1);
    box-shadow: inset -3px 0 0 rgba(88, 166, 255, 0.55);
  }
  .region-theirsonly .col-theirs,
  .region-bothsame .col-theirs {
    background: rgba(88, 166, 255, 0.1);
    box-shadow: inset 3px 0 0 rgba(88, 166, 255, 0.55);
  }
  .b-ignored .col {
    opacity: 0.4;
  }

  .ln {
    display: flex;
    white-space: pre;
    min-height: 1.6em;
  }
  /* 折叠的未改动段占位行(三列同高对齐;result 列那行可点展开) */
  .ln-gap {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 1.6em;
    color: var(--text-muted);
    background: var(--bg-surface);
    font-size: 12px;
    letter-spacing: 0.05em;
    white-space: nowrap;
  }
  .ln-gap-action {
    cursor: pointer;
  }
  .ln-gap-action:hover {
    color: var(--accent-cyan);
    background: var(--bg-hover);
  }
  /* 行号 gutter:对标 WebStorm,右对齐 + 与正文间细分隔线 */
  .lno {
    flex-shrink: 0;
    width: 40px;
    text-align: right;
    padding-right: 8px;
    margin-right: 8px;
    border-right: 1px solid var(--border-default);
    color: var(--text-muted);
    opacity: 0.55;
    user-select: none;
  }
  .lt {
    flex: 1;
    padding-right: 10px;
  }
  /* 词级高亮:对标 WebStorm,与所在改动块同色系更深一档
     —— 冲突区(两侧暖红)用更深红,单边改动(蓝)用更深蓝。 */
  .wc {
    border-radius: 2px;
  }
  .region-conflict .wc {
    background: rgba(229, 83, 75, 0.32);
    color: #ffd0cc;
  }
  .region-oursonly .wc,
  .region-theirsonly .wc,
  .region-bothsame .wc {
    background: rgba(88, 166, 255, 0.3);
    color: #cfe5ff;
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
    resize: none;
    background: var(--bg-surface);
    border: 1px solid var(--accent-cyan);
    border-radius: 4px;
    color: var(--text-primary, #e6e6e6);
    padding: 4px 8px;
    font-family: inherit;
    font-size: 13px;
    line-height: 1.6;
    white-space: pre;
    overflow: hidden;
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

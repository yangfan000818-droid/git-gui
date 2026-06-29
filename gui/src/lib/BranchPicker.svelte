<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask, message } from "@tauri-apps/plugin-dialog";

  interface BranchInfo {
    name: string;
    is_current: boolean;
    upstream: string | null;
    ahead: number;
    behind: number;
    is_remote: boolean;
  }

  // 合并/变基到当前分支需要的类型
  interface StashRef {
    label: string;
  }
  interface UpdateOptions {
    strategy: "Merge" | "Rebase";
    ignore_whitespace: boolean;
    recurse_submodules: boolean;
  }
  // UpdateOutcome: externally tagged
  type UpdateOutcome =
    | "AlreadyUpToDate"
    | "Resolved"
    | { FastForwarded: { commits: number } }
    | { Integrated: { commits: number; strategy: "Merge" | "Rebase" } }
    | { Conflicted: { files: string[]; autostash: StashRef | null } }
    | { StashRestoreConflict: { files: string[] } }
    | { SubmoduleSyncFailed: { error: string } };

  // Show Diff with Working Tree:选定分支与工作区差异(只读,交给 +page 用 DiffView 展示)
  type LineKind = "Context" | "Added" | "Removed";
  interface DiffLine {
    kind: LineKind;
    content: string;
  }
  interface Hunk {
    old_start: number;
    new_start: number;
    heading: string;
    lines: DiffLine[];
    raw: string;
  }
  interface FileDiff {
    path: string;
    binary: boolean;
    hunks: Hunk[];
    header_raw: string;
  }

  // Compare with Current:选定分支与当前 HEAD 的双向独有提交
  interface LogEntry {
    sha: string;
    full_sha: string;
    message: string;
    author: string;
    date: string;
  }

  // smart checkout 结果(externally tagged)
  type SwitchOutcome = "Switched" | { StashConflict: { files: string[] } };

  interface Props {
    repoPath: string;
    onClose: () => void;
    onSwitched: () => void;
    onConflict?: (details: {
      repoPath: string;
      files: string[];
      autostash: StashRef | null;
    }) => void;
    onShowDiff?: (details: { branch: string; files: FileDiff[] }) => void;
    onCompare?: (details: {
      branch: string;
      incoming: LogEntry[];
      outgoing: LogEntry[];
    }) => void;
  }

  let {
    repoPath,
    onClose,
    onSwitched,
    onConflict,
    onShowDiff,
    onCompare,
  }: Props = $props();

  let branches = $state<BranchInfo[]>([]);
  let remoteBranches = $state<BranchInfo[]>([]);
  let loading = $state(true);
  let error = $state("");
  let switching = $state(false);
  let fetching = $state(false);

  // 新建分支("" = 从当前 HEAD;否则从所选分支 = New Branch from Selected)
  let newBranchName = $state("");
  let newBranchStart = $state("");

  // 重命名分支(null = 没有在重命名)
  let renamingBranch = $state<string | null>(null);
  let renameValue = $state("");

  // ⋯ 更多操作菜单:当前展开的分支名(null = 全部收起)
  let openMenu = $state<string | null>(null);

  // 分支搜索(纯前端过滤,本地/远程分别 filter;空搜索显示全部)。
  let search = $state("");
  let filteredLocal = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return q === ""
      ? branches
      : branches.filter((b) => b.name.toLowerCase().includes(q));
  });
  let filteredRemote = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return q === ""
      ? remoteBranches
      : remoteBranches.filter((b) => b.name.toLowerCase().includes(q));
  });

  function toggleMenu(name: string) {
    openMenu = openMenu === name ? null : name;
  }

  async function load() {
    loading = true;
    error = "";
    try {
      branches = await invoke<BranchInfo[]>("repo_branches", {
        path: repoPath,
      });
      remoteBranches = await invoke<BranchInfo[]>("repo_remote_branches", {
        path: repoPath,
      });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function doFetch() {
    fetching = true;
    error = "";
    try {
      await invoke("repo_fetch", { path: repoPath });
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      fetching = false;
    }
  }

  async function checkoutRemote(remoteBranch: string) {
    switching = true;
    error = "";
    try {
      await invoke("repo_checkout_remote", {
        path: repoPath,
        remoteBranch,
      });
      onSwitched();
      onClose();
    } catch (e) {
      const msg = String(e);
      // 工作区脏被拒 → 提供 smart checkout(对标 WebStorm:检出被挡时才提示)
      if (
        msg.includes("未提交改动") &&
        (await ask(
          `工作区有未提交改动。暂存后检出远程分支 "${remoteBranch}"(smart checkout)?\n改动会在检出后自动贴回新分支。`,
          { title: "Smart Checkout" },
        ))
      ) {
        await smartCheckoutRemote(remoteBranch);
        return;
      }
      error = msg;
    } finally {
      switching = false;
    }
  }

  // smart checkout 远程分支:自动 stash → checkout -b --track → 贴回;贴回冲突时提示去改动列表解决。
  async function smartCheckoutRemote(remoteBranch: string) {
    switching = true;
    error = "";
    try {
      const r = await invoke<SwitchOutcome>("repo_checkout_remote_autostash", {
        path: repoPath,
        remoteBranch,
      });
      if (typeof r === "object" && "StashConflict" in r) {
        await message(
          `已暂存改动并检出 "${remoteBranch}",但贴回时有冲突。\n请在改动列表中解决冲突;原改动仍保留在 stash 中。`,
          { title: "贴回有冲突", kind: "warning" },
        );
      }
      onSwitched();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  function mergeOptions(strategy: "Merge" | "Rebase"): UpdateOptions {
    return { strategy, ignore_whitespace: true, recurse_submodules: false };
  }

  function outcomeVariant(o: UpdateOutcome): string {
    if (typeof o === "string") return o;
    return Object.keys(o)[0];
  }
  function outcomeData<T>(o: UpdateOutcome, key: string): T | undefined {
    if (typeof o === "string") return undefined;
    return (o as unknown as Record<string, T>)[key];
  }

  async function mergeInto(branch: string) {
    switching = true;
    error = "";
    try {
      const r = await invoke<UpdateOutcome>("repo_merge_branch", {
        path: repoPath,
        branch,
        options: mergeOptions("Merge"),
      });
      if (outcomeVariant(r) === "Conflicted") {
        const d = outcomeData<{ files: string[]; autostash: StashRef | null }>(
          r,
          "Conflicted",
        )!;
        onConflict?.({ repoPath, files: d.files, autostash: d.autostash });
        onClose();
        return;
      }
      onSwitched();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  async function rebaseOnto(branch: string) {
    switching = true;
    error = "";
    try {
      const r = await invoke<UpdateOutcome>("repo_rebase_branch", {
        path: repoPath,
        branch,
        options: mergeOptions("Rebase"),
      });
      if (outcomeVariant(r) === "Conflicted") {
        const d = outcomeData<{ files: string[]; autostash: StashRef | null }>(
          r,
          "Conflicted",
        )!;
        onConflict?.({ repoPath, files: d.files, autostash: d.autostash });
        onClose();
        return;
      }
      onSwitched();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  async function showDiff(branch: string) {
    switching = true;
    error = "";
    try {
      const files = await invoke<FileDiff[]>("repo_diff_with_workdir", {
        path: repoPath,
        rev: branch,
      });
      onShowDiff?.({ branch, files });
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  async function compareWith(branch: string) {
    switching = true;
    error = "";
    try {
      const cmp = await invoke<{ incoming: LogEntry[]; outgoing: LogEntry[] }>(
        "repo_compare_commits",
        { path: repoPath, other: branch },
      );
      onCompare?.({ branch, incoming: cmp.incoming, outgoing: cmp.outgoing });
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  async function switchTo(name: string) {
    switching = true;
    error = "";
    try {
      await invoke("repo_switch_branch", { path: repoPath, name });
      onSwitched();
      onClose();
    } catch (e) {
      const msg = String(e);
      // 工作区脏被拒 → 提供 smart checkout(对标 WebStorm:切换被挡时才提示)
      if (
        msg.includes("未提交改动") &&
        (await ask(
          `工作区有未提交改动。暂存后切换到 "${name}"(smart checkout)?\n改动会在切换后自动贴回新分支。`,
          { title: "Smart Checkout" },
        ))
      ) {
        await smartSwitch(name);
        return;
      }
      error = msg;
    } finally {
      switching = false;
    }
  }

  // smart checkout:自动 stash → checkout → 贴回;贴回冲突时提示去改动列表解决。
  async function smartSwitch(name: string) {
    switching = true;
    error = "";
    try {
      const r = await invoke<SwitchOutcome>("repo_switch_branch_autostash", {
        path: repoPath,
        name,
      });
      if (typeof r === "object" && "StashConflict" in r) {
        await message(
          `已暂存改动并切换到 "${name}",但贴回时有冲突。\n请在改动列表中解决冲突;原改动仍保留在 stash 中。`,
          { title: "贴回有冲突", kind: "warning" },
        );
      }
      onSwitched();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  async function createAndSwitch() {
    const name = newBranchName.trim();
    if (!name) return;
    switching = true;
    error = "";
    try {
      await invoke("repo_create_branch", {
        path: repoPath,
        name,
        startPoint: newBranchStart || null,
      });
      await invoke("repo_switch_branch", { path: repoPath, name });
      newBranchName = "";
      onSwitched();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  async function deleteBranch(name: string) {
    if (
      !(await ask(
        `确定删除分支 "${name}"?（仅安全删除：已合并 + 非当前分支）`,
        {
          title: "删除分支",
          kind: "warning",
        },
      ))
    )
      return;
    switching = true;
    error = "";
    try {
      await invoke("repo_delete_branch", { path: repoPath, name });
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  // 删除远程分支:从远程分支名按第一个 `/` 拆出 remote / branch(如
  // origin/feature/x → origin + feature/x),二次确认后 git push --delete。
  async function deleteRemoteBranch(remoteBranch: string) {
    const idx = remoteBranch.indexOf("/");
    if (idx < 0) return;
    const remote = remoteBranch.slice(0, idx);
    const branch = remoteBranch.slice(idx + 1);
    if (
      !(await ask(
        `确定删除远程分支 "${remoteBranch}"?\n这将在远程执行 git push ${remote} --delete ${branch},不可恢复。`,
        {
          title: "删除远程分支",
          kind: "warning",
        },
      ))
    )
      return;
    switching = true;
    error = "";
    try {
      await invoke("repo_delete_remote_branch", {
        path: repoPath,
        remote,
        branch,
      });
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }

  function startRename(name: string) {
    renamingBranch = name;
    renameValue = name;
    error = "";
    openMenu = null;
  }
  function cancelRename() {
    renamingBranch = null;
  }
  async function confirmRename() {
    const oldName = renamingBranch;
    const newName = renameValue.trim();
    if (!oldName || !newName || newName === oldName) {
      cancelRename();
      return;
    }
    switching = true;
    error = "";
    try {
      await invoke("repo_rename_branch", { path: repoPath, oldName, newName });
      renamingBranch = null;
      await load();
      onSwitched(); // 重命名当前分支会改主仓显示的分支名,刷新外部
    } catch (e) {
      error = String(e);
    } finally {
      switching = false;
    }
  }
  function handleRenameKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") confirmRename();
    else if (e.key === "Escape") {
      e.stopPropagation();
      cancelRename();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      // 先收菜单,再关面板
      if (openMenu !== null) openMenu = null;
      else onClose();
    }
  }

  function handleNewKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") createAndSwitch();
  }

  $effect(() => {
    load();
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="bp-overlay"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onclick={onClose}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="bp-panel" onclick={(e) => e.stopPropagation()}>
    <div class="bp-header">
      <h3>分支</h3>
      <button
        class="bp-fetch"
        disabled={fetching || switching}
        onclick={doFetch}
        title="git fetch --prune,刷新远程分支"
      >
        {fetching ? "拉取中…" : "⟳ Fetch"}
      </button>
      <button class="bp-close" onclick={onClose} aria-label="关闭">×</button>
    </div>

    {#if error}
      <div class="bp-error">{error}</div>
    {/if}

    <!-- 新建分支(可选起点 = New Branch from Selected) -->
    <div class="bp-create">
      <input
        class="bp-input"
        type="text"
        bind:value={newBranchName}
        placeholder="+ 新建分支名称"
        disabled={switching}
        onkeydown={handleNewKeydown}
      />
      <select
        class="bp-start"
        bind:value={newBranchStart}
        disabled={switching}
        title="新分支起点"
      >
        <option value="">起点:当前 HEAD</option>
        {#each branches as b}
          <option value={b.name}>起点:{b.name}</option>
        {/each}
      </select>
      <button
        class="bp-create-btn"
        disabled={switching || !newBranchName.trim()}
        onclick={createAndSwitch}
      >
        新建并切换
      </button>
    </div>

    {#if loading}
      <p class="bp-muted">加载中…</p>
    {:else}
      <input
        class="bp-search"
        type="text"
        bind:value={search}
        placeholder="搜索分支..."
        disabled={switching}
      />
      <p class="bp-group-label">本地分支</p>
      {#if branches.length === 0}
        <p class="bp-muted">没有本地分支</p>
      {:else if filteredLocal.length === 0}
        <p class="bp-muted">无匹配的本地分支</p>
      {:else}
        <ul class="bp-list">
          {#each filteredLocal as b}
            <li class="bp-item" class:bp-current={b.is_current}>
              {#if renamingBranch === b.name}
                <div class="bp-row">
                  <!-- svelte-ignore a11y_autofocus -->
                  <input
                    class="bp-rename-input"
                    type="text"
                    bind:value={renameValue}
                    disabled={switching}
                    onkeydown={handleRenameKeydown}
                    autofocus
                  />
                  <button
                    class="bp-rename-ok"
                    disabled={switching || !renameValue.trim()}
                    onclick={confirmRename}
                    aria-label="确认重命名">✓</button
                  >
                  <button
                    class="bp-rename-cancel"
                    disabled={switching}
                    onclick={cancelRename}
                    aria-label="取消重命名">×</button
                  >
                </div>
              {:else}
                <div class="bp-row">
                  <button
                    class="bp-btn"
                    disabled={switching || b.is_current}
                    onclick={() => switchTo(b.name)}
                  >
                    <span class="bp-name">{b.name}</span>
                    {#if b.is_current}
                      <span class="bp-check">✓</span>
                    {/if}
                    {#if b.upstream}
                      <span class="bp-upstream">{b.upstream}</span>
                    {/if}
                    <span class="bp-stats">
                      {#if b.ahead > 0}<span class="badge ahead"
                          >↑{b.ahead}</span
                        >{/if}
                      {#if b.behind > 0}<span class="badge behind"
                          >↓{b.behind}</span
                        >{/if}
                    </span>
                  </button>
                  <button
                    class="bp-more"
                    disabled={switching}
                    onclick={() => toggleMenu(b.name)}
                    aria-haspopup="true"
                    aria-expanded={openMenu === b.name}
                    aria-label="更多操作 {b.name}"
                    title="更多操作">⋯</button
                  >
                </div>
                {#if openMenu === b.name}
                  <div class="bp-menu">
                    <button
                      class="bp-menu-item"
                      onclick={() => startRename(b.name)}>重命名…</button
                    >
                    {#if !b.is_current}
                      <button
                        class="bp-menu-item"
                        onclick={() => {
                          openMenu = null;
                          mergeInto(b.name);
                        }}>合并到当前分支</button
                      >
                      <button
                        class="bp-menu-item"
                        onclick={() => {
                          openMenu = null;
                          rebaseOnto(b.name);
                        }}>变基到当前分支</button
                      >
                      <button
                        class="bp-menu-item"
                        onclick={() => {
                          openMenu = null;
                          showDiff(b.name);
                        }}>与工作区比较</button
                      >
                      <button
                        class="bp-menu-item"
                        onclick={() => {
                          openMenu = null;
                          compareWith(b.name);
                        }}>与当前分支比较</button
                      >
                      <button
                        class="bp-menu-item bp-menu-danger"
                        onclick={() => {
                          openMenu = null;
                          deleteBranch(b.name);
                        }}>删除分支</button
                      >
                    {/if}
                  </div>
                {/if}
              {/if}
            </li>
          {/each}
        </ul>
      {/if}

      {#if filteredRemote.length > 0}
        <p class="bp-group-label">远程分支</p>
        <ul class="bp-list">
          {#each filteredRemote as b}
            <li class="bp-item">
              <div class="bp-row">
                <button
                  class="bp-btn"
                  disabled={switching}
                  onclick={() => checkoutRemote(b.name)}
                  title="检出为本地跟踪分支"
                >
                  <span class="bp-name">{b.name}</span>
                  <span class="bp-checkout">检出</span>
                </button>
                <button
                  class="bp-more"
                  disabled={switching}
                  onclick={() => toggleMenu(b.name)}
                  aria-haspopup="true"
                  aria-expanded={openMenu === b.name}
                  aria-label="更多操作 {b.name}"
                  title="更多操作">⋯</button
                >
              </div>
              {#if openMenu === b.name}
                <div class="bp-menu">
                  <button
                    class="bp-menu-item"
                    onclick={() => {
                      openMenu = null;
                      showDiff(b.name);
                    }}>与工作区比较</button
                  >
                  <button
                    class="bp-menu-item"
                    onclick={() => {
                      openMenu = null;
                      compareWith(b.name);
                    }}>与当前分支比较</button
                  >
                  <button
                    class="bp-menu-item bp-menu-danger"
                    onclick={() => {
                      openMenu = null;
                      deleteRemoteBranch(b.name);
                    }}>删除远程分支</button
                  >
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  </div>
</div>

<style>
  .bp-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.75);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }
  .bp-panel {
    background: var(--bg-elevated);
    border: 1px solid var(--border-default);
    border-radius: 8px;
    width: 400px;
    max-width: 92%;
    max-height: 80%;
    overflow-y: auto;
  }
  .bp-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 10px;
  }
  .bp-header h3 {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
  .bp-fetch {
    margin-left: auto;
    margin-right: 8px;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 11px;
    padding: 3px 9px;
    white-space: nowrap;
  }
  .bp-fetch:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .bp-fetch:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .bp-search {
    margin: 8px 14px 4px;
    padding: 6px 10px;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 12px;
    width: calc(100% - 28px);
    box-sizing: border-box;
  }
  .bp-search:focus {
    outline: none;
    border-color: var(--accent-neon);
  }
  .bp-search::placeholder {
    color: var(--text-muted);
  }
  .bp-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
  }
  .bp-close:hover {
    color: var(--text-primary);
  }
  .bp-group-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin: 8px 0 2px;
    padding: 0 18px;
  }
  .bp-checkout {
    font-size: 11px;
    color: var(--accent-cyan);
    flex-shrink: 0;
    margin-left: auto;
  }
  .bp-error {
    background: rgba(247, 120, 139, 0.12);
    border-top: 1px solid rgba(247, 120, 139, 0.25);
    border-bottom: 1px solid rgba(247, 120, 139, 0.25);
    padding: 8px 18px;
    color: var(--color-error);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .bp-muted {
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
    padding: 24px 18px;
    margin: 0;
  }

  /* 新建分支 */
  .bp-create {
    display: flex;
    gap: 6px;
    padding: 8px 18px 2px;
  }
  .bp-input {
    flex: 1;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-primary);
    font-family:
      ui-monospace, SFMono-Regular, "JetBrains Mono", Menlo, monospace;
    font-size: 12px;
    padding: 5px 8px;
    min-width: 0;
  }
  .bp-input:disabled {
    opacity: 0.4;
  }
  .bp-start {
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 11px;
    padding: 5px 4px;
    max-width: 130px;
    flex-shrink: 0;
  }
  .bp-start:disabled {
    opacity: 0.4;
  }
  .bp-create-btn {
    background: rgba(86, 211, 100, 0.12);
    border: 1px solid rgba(86, 211, 100, 0.25);
    border-radius: 4px;
    color: var(--text-primary);
    cursor: pointer;
    font-size: 11px;
    padding: 5px 10px;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .bp-create-btn:hover:not(:disabled) {
    background: rgba(86, 211, 100, 0.18);
  }
  .bp-create-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* 分支列表 */
  .bp-list {
    list-style: none;
    margin: 0;
    padding: 6px 0 12px;
  }
  .bp-item {
    margin: 0;
  }
  .bp-row {
    display: flex;
    align-items: center;
  }
  .bp-more {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    line-height: 1;
    padding: 7px 14px;
    flex-shrink: 0;
  }
  .bp-more:hover:not(:disabled),
  .bp-more[aria-expanded="true"] {
    color: var(--text-primary);
    background: var(--bg-surface);
  }
  .bp-more:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .bp-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    background: transparent;
    border: none;
    border-radius: 0;
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    padding: 7px 0 7px 18px;
    text-align: left;
    font-family:
      ui-monospace, SFMono-Regular, "JetBrains Mono", Menlo, monospace;
    min-width: 0;
  }
  .bp-btn:hover:not(:disabled) {
    background: var(--bg-surface);
  }
  .bp-btn:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .bp-current .bp-btn {
    background: rgba(88, 166, 255, 0.1);
  }
  .bp-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bp-check {
    color: var(--accent-neon);
    font-weight: 700;
    flex-shrink: 0;
  }
  .bp-upstream {
    font-size: 11px;
    color: var(--text-muted);
    flex-shrink: 0;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bp-stats {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .badge {
    font-size: 10px;
    border-radius: 10px;
    padding: 1px 7px;
  }
  .ahead {
    background: rgba(86, 211, 100, 0.12);
    color: var(--accent-neon);
  }
  .behind {
    background: rgba(88, 166, 255, 0.12);
    color: var(--accent-cyan);
  }

  /* 重命名输入 */
  .bp-rename-input {
    flex: 1;
    background: var(--bg-surface);
    border: 1px solid var(--accent-cyan);
    border-radius: 4px;
    color: var(--text-primary);
    font-family:
      ui-monospace, SFMono-Regular, "JetBrains Mono", Menlo, monospace;
    font-size: 13px;
    padding: 5px 8px;
    margin: 2px 0 2px 16px;
    min-width: 0;
  }
  .bp-rename-ok,
  .bp-rename-cancel {
    background: transparent;
    border: none;
    cursor: pointer;
    font-size: 15px;
    padding: 7px 8px;
    line-height: 1;
    flex-shrink: 0;
  }
  .bp-rename-ok {
    color: var(--accent-neon);
  }
  .bp-rename-cancel {
    color: var(--text-muted);
  }
  .bp-rename-ok:disabled {
    opacity: 0.3;
    cursor: default;
  }

  /* ⋯ 更多操作菜单 */
  .bp-menu {
    display: flex;
    flex-direction: column;
    background: var(--bg-elevated);
    border-top: 1px solid var(--border-default);
    border-bottom: 1px solid var(--border-default);
    padding: 4px 0;
  }
  .bp-menu-item {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    text-align: left;
    padding: 7px 18px 7px 30px;
  }
  .bp-menu-item:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .bp-menu-danger:hover {
    background: rgba(247, 120, 139, 0.15);
    color: var(--color-error);
  }
</style>

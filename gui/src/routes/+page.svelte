<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  // 与 gitcore 序列化结构对应(serde 默认:enum 单元变体 → 字符串,PathBuf → 字符串)。
  type FileState = "Staged" | "Modified" | "Untracked" | "StagedAndModified";
  interface FileStatus {
    path: string;
    state: FileState;
  }
  interface Submodule {
    name: string;
    path: string;
    status: string;
  }
  interface RepoStatus {
    branch: string | null;
    upstream: string | null;
    behind: number;
    ahead: number;
    dirty: boolean;
    conflicted: string[];
    files: FileStatus[];
    submodules: Submodule[];
  }

  let path = $state("/Users/yfan/work/git-gui");
  let status = $state<RepoStatus | null>(null);
  let error = $state("");
  let loading = $state(false);

  async function load() {
    loading = true;
    error = "";
    status = null;
    try {
      status = await invoke<RepoStatus>("repo_status", { path });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  const stateLabel: Record<FileState, string> = {
    Staged: "已暂存",
    Modified: "已修改",
    Untracked: "未跟踪",
    StagedAndModified: "暂存+修改",
  };
</script>

<main>
  <header>
    <h1>git-gui</h1>
    <span class="skeleton">walking skeleton</span>
  </header>
  <p class="hint">链路验证:前端 invoke → Tauri command → gitcore → serde → 渲染</p>

  <div class="bar">
    <input bind:value={path} placeholder="仓库路径" spellcheck="false" />
    <button onclick={load} disabled={loading}>{loading ? "读取中…" : "打开"}</button>
  </div>

  {#if error}
    <pre class="error">{error}</pre>
  {/if}

  {#if status}
    <section class="head">
      <span class="branch">{status.branch ?? "(detached)"}</span>
      {#if status.upstream}<span class="up">→ {status.upstream}</span>{/if}
      {#if status.ahead > 0}<span class="badge ahead">↑{status.ahead}</span>{/if}
      {#if status.behind > 0}<span class="badge behind">↓{status.behind}</span>{/if}
      <span class="badge dirty" class:clean={!status.dirty}>
        {status.dirty ? "有改动" : "干净"}
      </span>
    </section>

    {#if status.conflicted.length}
      <section>
        <h2>冲突 ({status.conflicted.length})</h2>
        <ul class="files">
          {#each status.conflicted as c}<li><span class="fpath conflict">{c}</span></li>{/each}
        </ul>
      </section>
    {/if}

    <section>
      <h2>变更文件 ({status.files.length})</h2>
      {#if status.files.length === 0}
        <p class="muted">无变更</p>
      {:else}
        <ul class="files">
          {#each status.files as f}
            <li>
              <span class="tag tag-{f.state}">{stateLabel[f.state] ?? f.state}</span>
              <span class="fpath">{f.path}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    {#if status.submodules.length}
      <section>
        <h2>子仓库 ({status.submodules.length})</h2>
        <ul class="files">
          {#each status.submodules as s}<li><span class="fpath">{s.path}</span> · {s.status}</li>{/each}
        </ul>
      </section>
    {/if}
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    background: #1e1e1e;
    color: #e4e4e4;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }
  main {
    max-width: 760px;
    margin: 0 auto;
    padding: 28px 24px 60px;
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  h1 {
    font-size: 22px;
    margin: 0;
  }
  .skeleton {
    font-size: 12px;
    color: #888;
    border: 1px solid #444;
    border-radius: 10px;
    padding: 1px 8px;
  }
  .hint {
    color: #888;
    font-size: 12px;
    margin: 4px 0 18px;
  }
  .bar {
    display: flex;
    gap: 8px;
    margin-bottom: 18px;
  }
  input {
    flex: 1;
    background: #2a2a2a;
    border: 1px solid #444;
    border-radius: 6px;
    color: #e4e4e4;
    padding: 8px 10px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
  }
  button {
    background: #0e639c;
    border: none;
    border-radius: 6px;
    color: #fff;
    padding: 8px 18px;
    font-size: 13px;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .error {
    background: #3a1d1d;
    border: 1px solid #6a2b2b;
    border-radius: 6px;
    padding: 10px 12px;
    color: #f3b4b4;
    white-space: pre-wrap;
    font-size: 13px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    padding: 12px 14px;
    background: #252525;
    border: 1px solid #383838;
    border-radius: 8px;
    margin-bottom: 18px;
  }
  .branch {
    font-weight: 600;
    font-size: 15px;
  }
  .up {
    color: #888;
    font-size: 13px;
  }
  .badge {
    font-size: 12px;
    border-radius: 10px;
    padding: 1px 9px;
  }
  .ahead {
    background: #1d3a24;
    color: #7ee29a;
  }
  .behind {
    background: #1d2b3a;
    color: #7ab8e2;
  }
  .dirty {
    background: #3a311d;
    color: #e2c47a;
  }
  .dirty.clean {
    background: #1d3a24;
    color: #7ee29a;
  }
  h2 {
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #999;
    margin: 0 0 8px;
  }
  .muted {
    color: #666;
    font-size: 13px;
  }
  .files {
    list-style: none;
    margin: 0 0 4px;
    padding: 0;
  }
  .files li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 0;
  }
  .fpath {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
  }
  .conflict {
    color: #f3b4b4;
  }
  .tag {
    font-size: 11px;
    border-radius: 4px;
    padding: 1px 7px;
    flex-shrink: 0;
    min-width: 64px;
    text-align: center;
  }
  .tag-Staged {
    background: #1d3a24;
    color: #7ee29a;
  }
  .tag-Modified {
    background: #3a311d;
    color: #e2c47a;
  }
  .tag-Untracked {
    background: #2f2f2f;
    color: #999;
  }
  .tag-StagedAndModified {
    background: #3a2f1d;
    color: #e2b47a;
  }
</style>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";

  interface Props {
    onselect: (path: string) => void;
  }

  let { onselect }: Props = $props();

  let recentProjects = $state<string[]>([]);
  let loading = $state(false);
  let error = $state("");

  async function loadRecent() {
    try {
      recentProjects = await invoke<string[]>("get_recent_projects");
    } catch (e) {
      error = String(e);
    }
  }

  async function selectProject(path: string) {
    loading = true;
    error = "";
    try {
      // 验证路径是否为 git 仓库
      await invoke("repo_status", { path });
      onselect(path);
    } catch (e) {
      error = `无法打开项目: ${e}`;
    } finally {
      loading = false;
    }
  }

  async function browseDirectory() {
    const result = await open({
      directory: true,
      multiple: false,
      title: "选择 Git 仓库",
    });
    if (result) {
      await selectProject(result);
    }
  }

  async function removeProject(path: string, e: Event) {
    e.stopPropagation();
    try {
      await invoke("remove_recent_project", { path });
      await loadRecent();
    } catch (err) {
      error = String(err);
    }
  }

  $effect(() => {
    loadRecent();
  });
</script>

<div class="picker-container">
  <div class="header">
    <h2>选择项目</h2>
    <button class="btn-browse" onclick={browseDirectory} disabled={loading}>
      {loading ? "加载中..." : "选择目录..."}
    </button>
  </div>

  {#if error}
    <div class="error">{error}</div>
  {/if}

  {#if recentProjects.length > 0}
    <div class="recent-section">
      <h3>最近打开</h3>
      <ul class="project-list">
        {#each recentProjects as project}
          <li class="project-item">
            <button
              class="project-btn"
              onclick={() => selectProject(project)}
              disabled={loading}
            >
              <span class="project-path">{project}</span>
            </button>
            <button
              class="btn-remove"
              onclick={(e) => removeProject(project, e)}
              disabled={loading}
              aria-label="移除项目"
            >
              ×
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {:else}
    <div class="empty-state">
      <p>暂无最近打开的项目</p>
      <p class="hint">点击上方"选择目录"按钮打开 Git 仓库</p>
    </div>
  {/if}
</div>

<style>
  .picker-container {
    max-width: 800px;
    margin: 60px auto;
    padding: 0 20px;
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 24px;
  }

  .header h2 {
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .btn-browse {
    background: var(--accent-cyan);
    border: 1px solid #58a6ff;
    border-radius: 6px;
    color: var(--text-primary);
    padding: 8px 16px;
    font-size: 13px;
    cursor: pointer;
    transition: background 0.15s;
  }

  .btn-browse:hover:not(:disabled) {
    background: #58a6ff;
  }

  .btn-browse:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .error {
    background: rgba(247, 120, 139, 0.12);
    border: 1px solid rgba(247, 120, 139, 0.25);
    border-radius: 6px;
    color: var(--color-error);
    padding: 12px;
    margin-bottom: 16px;
    font-size: 13px;
  }

  .recent-section h3 {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-secondary);
    margin: 0 0 12px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .project-list {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .project-item {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
  }

  .project-btn {
    flex: 1;
    background: var(--bg-surface);
    border: 1px solid var(--border-default);
    border-radius: 6px;
    color: var(--text-primary);
    padding: 12px 16px;
    text-align: left;
    cursor: pointer;
    transition: all 0.15s;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
  }

  .project-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--accent-cyan);
  }

  .project-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .project-path {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .btn-remove {
    background: rgba(247, 120, 139, 0.15);
    border: 1px solid rgba(247, 120, 139, 0.25);
    border-radius: 6px;
    color: var(--color-error);
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    font-size: 20px;
    line-height: 1;
    transition: all 0.15s;
    flex-shrink: 0;
  }

  .btn-remove:hover:not(:disabled) {
    background: rgba(247, 120, 139, 0.18);
    border-color: rgba(247, 120, 139, 0.25);
  }

  .btn-remove:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .empty-state {
    text-align: center;
    padding: 60px 20px;
    color: var(--text-muted);
  }

  .empty-state p {
    margin: 0 0 8px;
    font-size: 14px;
  }

  .empty-state .hint {
    font-size: 12px;
    color: var(--text-muted);
  }
</style>

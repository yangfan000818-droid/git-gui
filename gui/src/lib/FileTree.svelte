<script lang="ts" generics="F extends { path: string; binary: boolean }">
  // 可折叠目录树:把扁平文件列表按目录分组,目录节点支持批量操作。
  // 连续单子目录自动合并为一行(如 src/lib/components)。折叠状态按目录路径记忆。
  interface Props {
    files: F[];
    selectedPath: string | null;
    kind: "unstaged" | "staged";
    operating: boolean;
    onSelect: (file: F) => void;
    onStage: (paths: string[]) => void;
    onUnstage: (paths: string[]) => void;
    onDiscard: (paths: string[]) => void;
  }
  let {
    files,
    selectedPath,
    kind,
    operating,
    onSelect,
    onStage,
    onUnstage,
    onDiscard,
  }: Props = $props();

  interface FileNode {
    type: "file";
    name: string;
    file: F;
  }
  interface DirNode {
    type: "dir";
    name: string;
    path: string;
    dirs: DirNode[];
    files: FileNode[];
    allPaths: string[]; // 子树下所有文件路径,供目录级批量操作
  }

  function buildTree(list: F[]): DirNode {
    const root: DirNode = {
      type: "dir",
      name: "",
      path: "",
      dirs: [],
      files: [],
      allPaths: [],
    };
    for (const file of list) {
      const parts = file.path.split("/");
      const fileName = parts.pop() ?? file.path;
      let cur = root;
      let prefix = "";
      for (const part of parts) {
        prefix = prefix ? `${prefix}/${part}` : part;
        let next = cur.dirs.find((d) => d.name === part);
        if (!next) {
          next = {
            type: "dir",
            name: part,
            path: prefix,
            dirs: [],
            files: [],
            allPaths: [],
          };
          cur.dirs.push(next);
        }
        next.allPaths.push(file.path);
        cur = next;
      }
      cur.files.push({ type: "file", name: fileName, file });
    }
    compress(root);
    sortDir(root);
    return root;
  }

  // 连续单子目录(自身无文件)合并成一行。root(name="")保留为容器不参与合并。
  function compress(dir: DirNode): void {
    for (const d of dir.dirs) compress(d);
    while (dir.name !== "" && dir.dirs.length === 1 && dir.files.length === 0) {
      const child = dir.dirs[0];
      dir.name = `${dir.name}/${child.name}`;
      dir.path = child.path;
      dir.dirs = child.dirs;
      dir.files = child.files;
    }
  }

  function sortDir(dir: DirNode): void {
    dir.dirs.sort((a, b) => a.name.localeCompare(b.name));
    dir.files.sort((a, b) => a.name.localeCompare(b.name));
    for (const d of dir.dirs) sortDir(d);
  }

  let tree = $derived(buildTree(files));
  // 默认空集 = 所有目录收起;展开的目录路径加入此集合(每级目录默认收起)。
  let expanded = $state(new Set<string>());

  function toggle(path: string) {
    const next = new Set(expanded);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    expanded = next;
  }

  function onActivate(e: KeyboardEvent, fn: () => void) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      fn();
    }
  }
</script>

{#snippet treeDir(dir: DirNode, depth: number)}
  {@const open = expanded.has(dir.path)}
  <div
    class="row dir-row"
    role="button"
    tabindex="0"
    style="padding-left: {14 + depth * 12}px"
    onclick={() => toggle(dir.path)}
    onkeydown={(e) => onActivate(e, () => toggle(dir.path))}
  >
    <span class="caret">{open ? "▾" : "▸"}</span>
    <span class="dname">{dir.name}</span>
    <span class="count">{dir.allPaths.length}</span>
    <span class="actions">
      {#if kind === "unstaged"}
        <button
          class="act stage"
          disabled={operating}
          title="暂存该目录全部"
          onclick={(e) => {
            e.stopPropagation();
            onStage(dir.allPaths);
          }}>+</button
        >
        <button
          class="act discard"
          disabled={operating}
          title="丢弃该目录全部改动（stash 可找回）"
          onclick={(e) => {
            e.stopPropagation();
            onDiscard(dir.allPaths);
          }}>↺</button
        >
      {:else}
        <button
          class="act unstage"
          disabled={operating}
          title="取消暂存该目录全部"
          onclick={(e) => {
            e.stopPropagation();
            onUnstage(dir.allPaths);
          }}>−</button
        >
      {/if}
    </span>
  </div>
  {#if open}
    {#each dir.dirs as d (d.path)}
      {@render treeDir(d, depth + 1)}
    {/each}
    {#each dir.files as f (f.file.path)}
      {@render treeFile(f, depth + 1)}
    {/each}
  {/if}
{/snippet}

{#snippet treeFile(node: FileNode, depth: number)}
  <div
    class="row file-row"
    class:selected={selectedPath === node.file.path}
    role="button"
    tabindex="0"
    style="padding-left: {14 + depth * 12}px"
    onclick={() => onSelect(node.file)}
    onkeydown={(e) => onActivate(e, () => onSelect(node.file))}
  >
    <span class="fname">{node.name}</span>
    {#if node.file.binary}<span class="tag">二进制</span>{/if}
    <span class="actions">
      {#if kind === "unstaged"}
        <button
          class="act stage"
          disabled={operating}
          title="暂存"
          onclick={(e) => {
            e.stopPropagation();
            onStage([node.file.path]);
          }}>+</button
        >
        <button
          class="act discard"
          disabled={operating}
          title="丢弃改动（stash 可找回）"
          onclick={(e) => {
            e.stopPropagation();
            onDiscard([node.file.path]);
          }}>↺</button
        >
      {:else}
        <button
          class="act unstage"
          disabled={operating}
          title="取消暂存"
          onclick={(e) => {
            e.stopPropagation();
            onUnstage([node.file.path]);
          }}>−</button
        >
      {/if}
    </span>
  </div>
{/snippet}

<div class="tree">
  {#each tree.dirs as d (d.path)}
    {@render treeDir(d, 0)}
  {/each}
  {#each tree.files as f (f.file.path)}
    {@render treeFile(f, 0)}
  {/each}
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 26px;
    padding-top: 3px;
    padding-bottom: 3px;
    padding-right: 10px;
    cursor: pointer;
    user-select: none;
  }
  .row:hover {
    background: #2a2a2a;
  }
  .file-row.selected {
    background: #0e639c55;
  }
  .caret {
    color: #aaa;
    font-size: 14px;
    width: 18px;
    flex-shrink: 0;
    text-align: center;
  }
  .dir-row:hover .caret {
    color: #fff;
  }
  .dname,
  .fname {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .dname {
    color: #cfcfcf;
  }
  .count {
    font-size: 10px;
    color: #666;
    flex-shrink: 0;
  }
  .tag {
    font-size: 10px;
    border-radius: 4px;
    padding: 1px 5px;
    background: #2f2f2f;
    color: #999;
    flex-shrink: 0;
  }
  .actions {
    display: none;
    gap: 2px;
    margin-left: auto;
    flex-shrink: 0;
  }
  .row:hover .actions {
    display: flex;
  }
  .act {
    background: transparent;
    border: 1px solid #555;
    border-radius: 3px;
    color: #ccc;
    cursor: pointer;
    font-size: 13px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    width: 22px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    line-height: 1;
  }
  .act:hover {
    background: #444;
  }
  .act:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .stage {
    color: #7ee29a;
    border-color: #3a5a3a;
  }
  .stage:hover {
    background: #1d3a24;
  }
  .unstage {
    color: #e2c47a;
    border-color: #5a4a3a;
  }
  .unstage:hover {
    background: #3a311d;
  }
  .discard {
    color: #f3b4b4;
    border-color: #5a3a3a;
  }
  .discard:hover {
    background: #3a1d1d;
  }
</style>

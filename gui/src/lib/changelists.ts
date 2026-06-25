// Changelist(命名变更集)纯前端状态:每个仓库一组命名分组 + 活跃分组 + 文件→分组分配。
// 非 git 概念,不入 .git;存 localStorage,跨会话保留。每个仓库独立。
// 暂存区仍是 git 概念,changelist 只是工作区文件(未暂存)的视图分组。

export interface Changelist {
  id: string;
  name: string;
}
export interface RepoChangelists {
  lists: Changelist[];
  activeId: string;
  // 文件路径 → changelist id;未出现的文件按 activeId 归类(由同步逻辑补齐)。
  assign: Record<string, string>;
}
export type ChangelistStore = Record<string, RepoChangelists>;

export const CL_KEY = "git-gui:changelists";
export const CL_DEFAULT = "default";
const CL_DEFAULT_NAME = "默认";

export function loadStore(): ChangelistStore {
  try {
    const raw = localStorage.getItem(CL_KEY);
    return raw ? (JSON.parse(raw) as ChangelistStore) : {};
  } catch {
    return {};
  }
}

export function saveStore(store: ChangelistStore) {
  try {
    localStorage.setItem(CL_KEY, JSON.stringify(store));
  } catch {
    // localStorage 不可用:仅本会话生效
  }
}

// 取仓库的 changelist 数据,不存在则就地创建只含「默认」分组的初始结构(直接写入 store)。
export function ensureRepo(
  store: ChangelistStore,
  repoPath: string,
): RepoChangelists {
  let r = store[repoPath];
  if (!r) {
    r = {
      lists: [{ id: CL_DEFAULT, name: CL_DEFAULT_NAME }],
      activeId: CL_DEFAULT,
      assign: {},
    };
    store[repoPath] = r;
  }
  return r;
}

// 把当前未暂存文件同步进 assign:新文件归入活跃分组,已消失的文件清掉(幂等,稳定后无写入)。
export function syncFiles(r: RepoChangelists, unstagedPaths: string[]) {
  const present = new Set(unstagedPaths);
  for (const p of unstagedPaths) {
    if (!(p in r.assign)) r.assign[p] = r.activeId;
  }
  for (const p of Object.keys(r.assign)) {
    if (!present.has(p)) delete r.assign[p];
  }
}

// 某文件当前所属分组(无分配则归活跃分组)。
export function clOf(r: RepoChangelists, filePath: string): string {
  return r.assign[filePath] ?? r.activeId;
}

let seq = 0;
export function newId(): string {
  seq += 1;
  return `cl-${Date.now().toString(36)}-${seq}`;
}

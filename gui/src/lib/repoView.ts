import type {
  FileDiff,
  FileStatus,
  IntegrationKind,
  PushOutcome,
  RepoView,
  SubStatus,
} from "./gitTypes";

export const SUB_STATE_LABEL: Record<SubStatus, string> = {
  Clean: "干净",
  Dirty: "有改动",
  Detached: "游离 HEAD",
  Uninitialized: "未初始化",
};

export const INTEGRATION_LABEL: Record<IntegrationKind, string> = {
  Merge: "合并",
  Rebase: "变基",
  CherryPick: "拣选",
  Revert: "回退",
  None: "整合",
};

export const COMMIT_PUSH_KEY = "git-gui:commit-then-push";

export function joinPath(base: string, rel: string): string {
  return `${base.replace(/[/\\]+$/, "")}/${rel}`;
}

export function repoLabel(p: string): string {
  return (
    p
      .replace(/[/\\]+$/, "")
      .split(/[/\\]/)
      .pop() || p
  );
}

export function buildRepoView(
  repoPath: string,
  label: string,
  isMain: boolean,
  subRelPath: string | null,
  subStatus: SubStatus | null,
  branch: string | null,
  ahead: number,
  behind: number,
  files: FileStatus[],
): RepoView {
  const unstaged = files
    .filter(
      (f) =>
        f.state === "Modified" ||
        f.state === "Untracked" ||
        f.state === "StagedAndModified",
    )
    .map((f) => ({ path: f.path, state: f.state }));
  const staged = files
    .filter((f) => f.state === "Staged" || f.state === "StagedAndModified")
    .map((f) => ({ path: f.path, state: f.state }));
  return {
    path: repoPath,
    label,
    isMain,
    subRelPath,
    subStatus,
    branch,
    ahead,
    behind,
    unstaged,
    staged,
  };
}

export function sameDiff(a: FileDiff | null, b: FileDiff | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  if (a.hunks.length !== b.hunks.length) return false;
  return a.hunks.every((h, i) => h.raw === b.hunks[i].raw);
}

export function pushMsg(r: PushOutcome): string {
  if (r === "Success") return "推送成功";
  if (r === "NoUpstream") return "跳过:无 upstream";
  return "远端领先,待更新后推送";
}

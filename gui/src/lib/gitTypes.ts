export interface AppearanceSettings {
  theme: string;
  density: string;
  font_size: string;
  animations_enabled: boolean;
  scanline_enabled: boolean;
  glow_intensity: string;
}

export type FileState =
  | "Staged"
  | "Modified"
  | "Untracked"
  | "StagedAndModified";
export type LineKind = "Context" | "Added" | "Removed";

export interface DiffLine {
  kind: LineKind;
  content: string;
}

export interface Hunk {
  old_start: number;
  new_start: number;
  heading: string;
  lines: DiffLine[];
  raw: string;
}

export interface FileDiff {
  path: string;
  binary: boolean;
  hunks: Hunk[];
  header_raw: string;
}

export interface FileStatus {
  path: string;
  state: FileState;
}

export type SubStatus = "Clean" | "Dirty" | "Detached" | "Uninitialized";

export interface RepoStatus {
  branch: string | null;
  upstream: string | null;
  behind: number;
  ahead: number;
  dirty: boolean;
  conflicted: string[];
  files: FileStatus[];
  submodules: { name: string; path: string; status: SubStatus }[];
}

export interface FileEntry {
  path: string;
  state: FileState;
}

export interface RepoView {
  path: string;
  label: string;
  isMain: boolean;
  subRelPath: string | null;
  subStatus: SubStatus | null;
  branch: string | null;
  ahead: number;
  behind: number;
  unstaged: FileEntry[];
  staged: FileEntry[];
}

export interface PrecommitWarning {
  kind:
    | "SensitiveInfo"
    | "ConflictMarker"
    | "LargeFile"
    | "DebugResidue"
    | "Todo";
  file: string;
  line: number | null;
  detail: string;
}

export interface PrecommitReport {
  warnings: PrecommitWarning[];
}

export type ToastKind = "success" | "error" | "info";
export type UndoAction = { kind: string; mode?: string } & Record<
  string,
  unknown
>;

export interface StashRef {
  label: string;
}

export type ConflictKind =
  | "BothModified"
  | "ModifyDelete"
  | "DeleteModify"
  | "AddAdd"
  | "Binary";
export type IntegrationKind =
  | "Merge"
  | "Rebase"
  | "CherryPick"
  | "Revert"
  | "None";

export interface ConflictFile {
  path: string;
  kind: ConflictKind;
}

export interface ConflictState {
  kind: IntegrationKind;
  files: ConflictFile[];
  autostash: StashRef | null;
}

export interface BranchDiff {
  branch: string;
  files: FileDiff[];
}

export interface LogEntry {
  sha: string;
  full_sha: string;
  message: string;
  author: string;
  date: string;
}

export interface CompareResult {
  branch: string;
  incoming: LogEntry[];
  outgoing: LogEntry[];
}

export type PushOutcome = "Success" | "NoUpstream" | "NonFastForward";

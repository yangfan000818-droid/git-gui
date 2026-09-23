// 三栏合并编辑器的结果文本拼接:按每个区域的决策拼出写回文件的文本。
// 从 MergeEditor 抽出(纯函数,便于单测)。

export type RegionKind =
  | "Unchanged"
  | "OursOnly"
  | "TheirsOnly"
  | "BothSame"
  | "Conflict";
export interface MergeRegion {
  kind: RegionKind;
  ours: string[];
  base: string[];
  theirs: string[];
}
// 单边:applied|ignored;冲突:undecided|left|right|both|edited。
export type Decision =
  | "applied"
  | "ignored"
  | "undecided"
  | "left"
  | "right"
  | "both"
  | "edited";

// 文本段末尾补换行:拼接时少一个换行会把下一段粘到同一行。
export function nlTerm(t: string): string {
  return t === "" || t.endsWith("\n") ? t : t + "\n";
}

// 单个区域按决策得到的结果文本;editedText 仅 edited 决策时使用。
export function regionResultText(
  r: MergeRegion,
  d: Decision,
  editedText: string,
): string {
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
      if (d === "edited") return nlTerm(editedText);
      return ""; // undecided
  }
}

// 整份结果文本。
export function mergeResult(
  regions: MergeRegion[],
  decisions: Decision[],
  edited: string[],
): string {
  return regions
    .map((r, i) => regionResultText(r, decisions[i], edited[i]))
    .join("");
}

// 手动编辑的起始文本:未决冲突块回写成 zdiff3 标记(写回时会被拦下,逼你处理完)。
export function manualDraft(
  regions: MergeRegion[],
  decisions: Decision[],
  edited: string[],
): string {
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
      return regionResultText(r, decisions[i], edited[i]);
    })
    .join("");
}

// 文本是否仍含冲突标记(行首 <<<<<<< 或 >>>>>>>)。
export function hasConflictMarkers(text: string): boolean {
  return /^<{7}/m.test(text) || /^>{7}/m.test(text);
}

// 结果文本按行切分展示(末尾换行不产生额外空行)。
export function splitLines(text: string): string[] {
  if (text === "") return [];
  return text.replace(/\n$/, "").split("\n");
}

import { describe, expect, it } from "vitest";
import {
  hasConflictMarkers,
  manualDraft,
  mergeResult,
  nlTerm,
  regionResultText,
  splitLines,
  type MergeRegion,
} from "./mergeText";

const conflict = (
  ours: string[],
  theirs: string[],
  base: string[] = [],
): MergeRegion => ({ kind: "Conflict", ours, base, theirs });
const unchanged = (lines: string[]): MergeRegion => ({
  kind: "Unchanged",
  ours: lines,
  base: lines,
  theirs: lines,
});

describe("nlTerm", () => {
  it("空串原样返回", () => expect(nlTerm("")).toBe(""));
  it("已有换行不重复补", () => expect(nlTerm("a\n")).toBe("a\n"));
  it("缺换行补一个", () => expect(nlTerm("a")).toBe("a\n"));
});

describe("regionResultText", () => {
  const oursOnly: MergeRegion = {
    kind: "OursOnly",
    ours: ["new\n"],
    base: ["old\n"],
    theirs: ["old\n"],
  };
  const theirsOnly: MergeRegion = {
    kind: "TheirsOnly",
    ours: ["old\n"],
    base: ["old\n"],
    theirs: ["new\n"],
  };
  const bothSame: MergeRegion = {
    kind: "BothSame",
    ours: ["same\n"],
    base: ["old\n"],
    theirs: ["same\n"],
  };

  it("单边更改:applied 取改动侧,ignored 回到 base", () => {
    expect(regionResultText(oursOnly, "applied", "")).toBe("new\n");
    expect(regionResultText(oursOnly, "ignored", "")).toBe("old\n");
    expect(regionResultText(theirsOnly, "applied", "")).toBe("new\n");
    expect(regionResultText(theirsOnly, "ignored", "")).toBe("old\n");
    expect(regionResultText(bothSame, "applied", "")).toBe("same\n");
    expect(regionResultText(bothSame, "ignored", "")).toBe("old\n");
  });

  it("冲突:取左 / 取右 / 未决", () => {
    const r = conflict(["o\n"], ["t\n"]);
    expect(regionResultText(r, "left", "")).toBe("o\n");
    expect(regionResultText(r, "right", "")).toBe("t\n");
    expect(regionResultText(r, "undecided", "")).toBe("");
  });

  it("冲突取两者:ours 末行无换行(文件末尾)时补换行", () => {
    const r = conflict(["o"], ["t"]);
    expect(regionResultText(r, "both", "")).toBe("o\nt");
  });

  it("冲突 edited 使用编辑文本", () => {
    const r = conflict(["o\n"], ["t\n"]);
    expect(regionResultText(r, "edited", "x\n")).toBe("x\n");
  });
});

describe("mergeResult", () => {
  // 回归 cdb5187:块内手动编辑删掉末尾换行后,下一段整段粘到这一行末尾。
  it("编辑块末尾缺换行时,下一段不粘到同一行", () => {
    const regions = [
      conflict(["const base = a;\n"], ["const base = b;\n"]),
      unchanged(["return base;\n"]),
    ];
    const text = mergeResult(
      regions,
      ["edited", "applied"],
      ["const base = (a - b) * 2; // 手动合并", ""],
    );
    expect(text).toBe("const base = (a - b) * 2; // 手动合并\nreturn base;\n");
  });

  it("按区域顺序拼接各自的结果", () => {
    const regions = [
      unchanged(["head\n"]),
      conflict(["o\n"], ["t\n"]),
      unchanged(["tail\n"]),
    ];
    expect(mergeResult(regions, ["applied", "right", "applied"], [])).toBe(
      "head\nt\ntail\n",
    );
  });
});

describe("manualDraft", () => {
  it("未决冲突写成 zdiff3 标记,已决区域用结果文本", () => {
    const regions = [
      unchanged(["head\n"]),
      conflict(["o"], ["t"], ["b"]),
      conflict(["o2\n"], ["t2\n"]),
    ];
    const draft = manualDraft(regions, ["applied", "undecided", "left"], []);
    expect(draft).toBe(
      "head\n" +
        "<<<<<<< ours\no\n||||||| base\nb\n=======\nt\n>>>>>>> theirs\n" +
        "o2\n",
    );
    expect(hasConflictMarkers(draft)).toBe(true);
  });
});

describe("hasConflictMarkers", () => {
  it("只认行首的 7 个标记符", () => {
    expect(hasConflictMarkers("a\n<<<<<<< ours\n")).toBe(true);
    expect(hasConflictMarkers(">>>>>>> theirs")).toBe(true);
    expect(hasConflictMarkers("x <<<<<<< y\n")).toBe(false);
    expect(hasConflictMarkers("<<<<<< six\n")).toBe(false);
  });
});

describe("splitLines", () => {
  it("空串无行", () => expect(splitLines("")).toEqual([]));
  it("末尾换行不产生额外空行", () =>
    expect(splitLines("a\nb\n")).toEqual(["a", "b"]));
  it("保留中间空行", () =>
    expect(splitLines("a\n\nb")).toEqual(["a", "", "b"]));
});

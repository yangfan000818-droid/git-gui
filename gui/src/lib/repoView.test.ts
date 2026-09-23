import { describe, expect, it } from "vitest";
import type { FileDiff, FileStatus } from "./gitTypes";
import {
  buildRepoView,
  joinPath,
  pushMsg,
  repoLabel,
  sameDiff,
} from "./repoView";

describe("joinPath", () => {
  it("去掉 base 末尾分隔符再拼接", () => {
    expect(joinPath("/repo/", "sub")).toBe("/repo/sub");
    expect(joinPath("C:\\repo\\", "sub")).toBe("C:\\repo/sub");
  });
});

describe("repoLabel", () => {
  it("取路径最后一段,兼容两种分隔符与末尾分隔符", () => {
    expect(repoLabel("/a/b/repo")).toBe("repo");
    expect(repoLabel("/a/b/repo/")).toBe("repo");
    expect(repoLabel("C:\\work\\项目")).toBe("项目");
  });
});

describe("buildRepoView", () => {
  it("按状态把文件分到未暂存 / 已暂存,部分暂存两边都有", () => {
    const files: FileStatus[] = [
      { path: "m.txt", state: "Modified" },
      { path: "u.txt", state: "Untracked" },
      { path: "s.txt", state: "Staged" },
      { path: "sm.txt", state: "StagedAndModified" },
    ];
    const v = buildRepoView(
      "/r",
      "r",
      true,
      null,
      null,
      "main",
      0,
      0,
      files,
      [],
      "None",
    );
    expect(v.unstaged.map((f) => f.path)).toEqual(["m.txt", "u.txt", "sm.txt"]);
    expect(v.staged.map((f) => f.path)).toEqual(["s.txt", "sm.txt"]);
  });
});

describe("sameDiff", () => {
  const d = (...raws: string[]): FileDiff => ({
    path: "f",
    binary: false,
    header_raw: "",
    hunks: raws.map((raw) => ({
      old_start: 1,
      new_start: 1,
      heading: "",
      lines: [],
      raw,
    })),
  });
  it("按 hunk 原文比较", () => {
    expect(sameDiff(d("a", "b"), d("a", "b"))).toBe(true);
    expect(sameDiff(d("a"), d("b"))).toBe(false);
    expect(sameDiff(d("a"), d("a", "b"))).toBe(false);
  });
  it("处理 null", () => {
    expect(sameDiff(null, null)).toBe(true);
    expect(sameDiff(d("a"), null)).toBe(false);
  });
});

describe("pushMsg", () => {
  it("每种结果对应一条提示", () => {
    expect(pushMsg("Success")).toBe("推送成功");
    expect(pushMsg("NoUpstream")).toBe("跳过:无 upstream");
    expect(pushMsg("NonFastForward")).toBe("远端领先,待更新后推送");
  });
});

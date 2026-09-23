import { describe, expect, it } from "vitest";
import { lcsChanged, tokenize, wordDiffLines } from "./wordDiff";

describe("tokenize", () => {
  it("按换行 / 空白串 / 非空白串切分", () => {
    expect(tokenize("a  b\nc")).toEqual(["a", "  ", "b", "\n", "c"]);
  });
  it("空串无 token", () => expect(tokenize("")).toEqual([]));
});

describe("lcsChanged", () => {
  it("标出不在 LCS 中的 token", () => {
    expect(lcsChanged(["a", "b", "c"], ["a", "c"])).toEqual([
      false,
      true,
      false,
    ]);
  });
  it("任一侧为空时全部视为未改", () => {
    expect(lcsChanged(["a"], [])).toEqual([false]);
  });
  it("超大块跳过计算,全部视为未改", () => {
    const a = new Array(700).fill("x");
    const b = new Array(700).fill("y");
    expect(lcsChanged(a, b).every((c) => !c)).toBe(true);
  });
});

describe("wordDiffLines", () => {
  it("按行分组,只高亮改动的非空白词", () => {
    expect(wordDiffLines("let a = 1\nfoo\n", "let a = 2\nfoo\n")).toEqual([
      [
        { text: "let", changed: false },
        { text: " ", changed: false },
        { text: "a", changed: false },
        { text: " ", changed: false },
        { text: "=", changed: false },
        { text: " ", changed: false },
        { text: "1", changed: true },
      ],
      [{ text: "foo", changed: false }],
    ]);
  });
  it("行数与原文一致(末尾换行不多出空行)", () => {
    expect(wordDiffLines("a\nb\n", "a\nb\n")).toHaveLength(2);
  });
});

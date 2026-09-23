import { describe, expect, it } from "vitest";
import { parseInline, parseNotes } from "./releaseNotes";

describe("parseInline", () => {
  it("拆出加粗与代码,其余原样", () => {
    expect(parseInline("**merge**: 运行 `xattr -cr` 即可")).toEqual([
      { text: "merge", bold: true },
      { text: ": 运行 " },
      { text: "xattr -cr", code: true },
      { text: " 即可" },
    ]);
  });
  it("无标记时整段为一个片段", () => {
    expect(parseInline("plain")).toEqual([{ text: "plain" }]);
  });
  it("未闭合的标记按普通文本", () => {
    expect(parseInline("a ** b ` c")).toEqual([{ text: "a ** b ` c" }]);
  });
});

describe("parseNotes", () => {
  // v1.2.1 的 Release 正文(scripts/release-notes.mjs 生成 + macOS 提示)。
  const body = [
    "### 修复",
    "",
    "- **merge**: 块内手动编辑末尾缺换行会把下一段粘到同一行 (cdb5187)",
    "- **conflict**: 修整合无法收尾 (4462595)",
    "",
    "**完整变更**: https://github.com/o/r/compare/v1.2.0...v1.2.1",
    "",
    "---",
    "",
    "> **macOS 用户**：首次打开提示「已损坏无法打开」是因为应用未签名。",
    "> 解决方法：运行 `xattr -cr /Applications/GitSail.app`。",
  ].join("\n");

  it("按行识别标题 / 条目 / 段落 / 引用,跳过空行与分隔线", () => {
    expect(parseNotes(body).map((b) => b.kind)).toEqual([
      "heading",
      "item",
      "item",
      "para",
      "quote",
      "quote",
    ]);
  });

  it("条目去掉末尾的提交 hash", () => {
    const item = parseNotes(body)[1];
    expect(item.segs.map((s) => s.text).join("")).toBe(
      "merge: 块内手动编辑末尾缺换行会把下一段粘到同一行",
    );
  });

  it("兼容 CRLF 与旧版的纯链接说明", () => {
    expect(parseNotes("### 修复\r\n- a\r\n")).toHaveLength(2);
    expect(parseNotes("See https://github.com/o/r/releases")).toEqual([
      { kind: "para", segs: [{ text: "See https://github.com/o/r/releases" }] },
    ]);
  });

  it("空说明无内容", () => {
    expect(parseNotes("")).toEqual([]);
    expect(parseNotes("\n---\n")).toEqual([]);
  });
});

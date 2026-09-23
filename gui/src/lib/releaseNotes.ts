// 更新说明(Release 正文 / latest.json notes)的轻量解析,供更新提示展示。
// 只认 scripts/release-notes.mjs 产出与 Release 正文用到的几种 Markdown:
// ### 标题、- 列表、> 引用、--- 分隔、**加粗**、`代码`。其余按普通段落。
// 结构化输出由模板渲染,不走 {@html}(正文来自网络,避免注入)。

export interface InlineSeg {
  text: string;
  bold?: boolean;
  code?: boolean;
}
export type NoteBlock =
  | { kind: "heading"; segs: InlineSeg[] }
  | { kind: "item"; segs: InlineSeg[] }
  | { kind: "quote"; segs: InlineSeg[] }
  | { kind: "para"; segs: InlineSeg[] };

// 行内:**加粗** 与 `代码`,其余原样。
export function parseInline(s: string): InlineSeg[] {
  const segs: InlineSeg[] = [];
  const re = /\*\*(.+?)\*\*|`([^`]+)`/g;
  let last = 0;
  for (let m = re.exec(s); m; m = re.exec(s)) {
    if (m.index > last) segs.push({ text: s.slice(last, m.index) });
    if (m[1] !== undefined) segs.push({ text: m[1], bold: true });
    else segs.push({ text: m[2], code: true });
    last = re.lastIndex;
  }
  if (last < s.length) segs.push({ text: s.slice(last) });
  return segs;
}

export function parseNotes(md: string): NoteBlock[] {
  const blocks: NoteBlock[] = [];
  for (const raw of md.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "" || /^-{3,}$/.test(line)) continue;
    let m: RegExpMatchArray | null;
    if ((m = line.match(/^#{1,6}\s+(.*)$/))) {
      blocks.push({ kind: "heading", segs: parseInline(m[1]) });
    } else if ((m = line.match(/^[-*]\s+(.*)$/))) {
      // 条目末尾的提交短 hash 对用户是噪音,去掉。
      const text = m[1].replace(/\s*\([0-9a-f]{7,40}\)$/, "");
      blocks.push({ kind: "item", segs: parseInline(text) });
    } else if ((m = line.match(/^>\s?(.*)$/))) {
      blocks.push({ kind: "quote", segs: parseInline(m[1]) });
    } else {
      blocks.push({ kind: "para", segs: parseInline(line) });
    }
  }
  return blocks;
}

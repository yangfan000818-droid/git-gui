// 词级 diff:把一段文本相对另一段做按词 LCS,标出差异的词,供三栏冲突高亮。
// 从 ConflictView 抽出,MergeEditor / ConflictView 共用。

export interface WordTok {
  text: string;
  changed: boolean;
}

// 切 token:换行 / 空白串 / 非空白串,既能按词比较又保留布局。
export function tokenize(s: string): string[] {
  return s.match(/\n|[^\S\n]+|\S+/g) ?? [];
}

// 标记 a 中每个 token 是否相对 b 改变(不在 LCS 中 = 改变)。
export function lcsChanged(a: string[], b: string[]): boolean[] {
  const n = a.length;
  const m = b.length;
  // 超大块跳过 O(nm),全部按未改处理(不高亮)。
  if (n === 0 || m === 0 || n * m > 400000) return new Array(n).fill(false);
  const dp: number[][] = Array.from({ length: n + 1 }, () =>
    new Array(m + 1).fill(0),
  );
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] =
        a[i] === b[j]
          ? dp[i + 1][j + 1] + 1
          : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const changed = new Array(n).fill(true);
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      changed[i] = false;
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      i++;
    } else {
      j++;
    }
  }
  return changed;
}

// 把 text 相对 other 做词级 diff,按行分组返回 token(供模板高亮)。
// 行数与 text 的行数一致(末尾换行不产生额外空行)。
export function wordDiffLines(text: string, other: string): WordTok[][] {
  const toks = tokenize(text);
  const changed = lcsChanged(toks, tokenize(other));
  const lines: WordTok[][] = [[]];
  toks.forEach((t, idx) => {
    if (t === "\n") {
      lines.push([]);
    } else {
      // 仅高亮含非空白的改动 token,避免空白噪声。
      lines[lines.length - 1].push({
        text: t,
        changed: changed[idx] && /\S/.test(t),
      });
    }
  });
  // 文本以换行结尾会多出一个空行,去掉,使行数与原文一致。
  if (lines.length > 1 && lines[lines.length - 1].length === 0) lines.pop();
  return lines;
}

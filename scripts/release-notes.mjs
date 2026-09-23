// 生成 Release 更新说明:取上一个 tag 到 TAG 之间的提交,按 feat / fix / perf 分组。
// 用法: node scripts/release-notes.mjs <tag>   (输出 Markdown 到 stdout)
// 需要完整历史与 tag(CI 中 checkout 要 fetch-depth: 0)。
import { execFileSync } from "node:child_process";

const tag = process.argv[2];
if (!tag) {
  console.error("usage: node scripts/release-notes.mjs <tag>");
  process.exit(1);
}

const git = (...args) =>
  execFileSync("git", args, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();

// 上一个 tag;首个 tag 没有前驱时取全部历史。
let prevTag = "";
try {
  prevTag = git("describe", "--tags", "--abbrev=0", `${tag}^`);
} catch {
  prevTag = "";
}
const range = prevTag ? `${prevTag}..${tag}` : tag;

const SECTIONS = [
  ["feat", "新功能"],
  ["fix", "修复"],
  ["perf", "性能"],
];
const groups = Object.fromEntries(SECTIONS.map(([type]) => [type, []]));

// %x1f 分隔 hash 与标题;--no-merges 去掉 PR 合并提交本身。
const log = git("log", "--no-merges", "--format=%h%x1f%s", range);
for (const line of log ? log.split("\n") : []) {
  const [hash, subject] = line.split("\x1f");
  const m = subject.match(/^(\w+)(?:\(([^)]+)\))?!?:\s*(.+)$/);
  if (!m || !groups[m[1]]) continue;
  const [, type, scope, desc] = m;
  groups[type].push(`- ${scope ? `**${scope}**: ` : ""}${desc} (${hash})`);
}

const parts = [];
for (const [type, title] of SECTIONS) {
  if (groups[type].length)
    parts.push(`### ${title}\n\n${groups[type].join("\n")}`);
}
if (!parts.length) parts.push("本次发布无功能或修复类变更。");

const repo = process.env.GITHUB_REPOSITORY;
if (repo && prevTag) {
  parts.push(
    `**完整变更**: https://github.com/${repo}/compare/${prevTag}...${tag}`,
  );
}

process.stdout.write(parts.join("\n\n") + "\n");

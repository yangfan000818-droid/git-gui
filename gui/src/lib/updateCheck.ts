// 新版本检查:查 GitHub Releases 最新版本,与当前 app 版本比对。
// 纯前端实现(fetch + getVersion),不引入后端命令/插件。
import { getVersion } from "@tauri-apps/api/app";

// 发布仓库(GitHub owner/repo)。
const REPO = "yangfan000818-droid/git-gui";

export interface UpdateInfo {
  latest: string; // 最新版本号(去掉 tag 前缀 v),如 "0.9.1"
  current: string; // 当前 app 版本,如 "0.9.0"
  url: string; // 该 release 的页面 URL(供打开下载)
  notes: string; // release 说明(可空)
}

// "v1.2.3" / "1.2.3" → [1,2,3];非数字段按 0。
function parseVer(v: string): number[] {
  return v
    .replace(/^v/, "")
    .split(".")
    .map((n) => parseInt(n, 10) || 0);
}

// a 是否严格比 b 新。
function isNewer(a: string, b: string): boolean {
  const pa = parseVer(a);
  const pb = parseVer(b);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x !== y) return x > y;
  }
  return false;
}

// 查询最新 release;有更新返回 UpdateInfo,已是最新返回 null;
// 网络/API 失败抛错,由调用方决定静默(启动检查)或提示(手动检查)。
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  const current = await getVersion();
  const resp = await fetch(
    `https://api.github.com/repos/${REPO}/releases/latest`,
    { headers: { Accept: "application/vnd.github+json" } },
  );
  if (!resp.ok) throw new Error(`GitHub API ${resp.status}`);
  const data = await resp.json();
  const tag: string = data.tag_name ?? "";
  const latest = tag.replace(/^v/, "");
  if (!latest || !isNewer(latest, current)) return null;
  return {
    latest,
    current,
    url: data.html_url ?? "",
    notes: data.body ?? "",
  };
}

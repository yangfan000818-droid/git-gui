// 生成 Tauri updater 更新 manifest (latest.json)。
// 环境变量由 CI 注入: VERSION, WIN_URL, WIN_SIG (签名), NOTES_FILE (更新说明,可选).
import { readFileSync, writeFileSync } from "fs";

const version = process.env.VERSION || "0.0.0";
const winSig = (process.env.WIN_SIG || "").trim();
const winUrl = process.env.WIN_URL || "";
const notesFile = process.env.NOTES_FILE;

const manifest = {
  version,
  notes: notesFile
    ? readFileSync(notesFile, "utf8").trim()
    : `See https://github.com/${process.env.GITHUB_REPOSITORY || "yangfan000818-droid/git-gui"}/releases`,
  pub_date: new Date().toISOString(),
  platforms: {},
};

if (winSig.length > 0) {
  manifest.platforms["windows-x86_64"] = {
    signature: winSig,
    url: winUrl,
  };
}

writeFileSync("latest.json", JSON.stringify(manifest, null, 2) + "\n");
console.log("latest.json written");

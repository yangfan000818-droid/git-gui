import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("..", import.meta.url));
const packageJsonPath = path.join(repoRoot, "gui/package.json");
const packageLockPath = path.join(repoRoot, "gui/package-lock.json");
const tauriConfigPath = path.join(repoRoot, "gui/src-tauri/tauri.conf.json");
const cargoManifestPath = path.join(repoRoot, "gui/src-tauri/Cargo.toml");

const readJson = (filePath) => JSON.parse(readFileSync(filePath, "utf8"));
const packageVersion = readJson(packageJsonPath).version;
const packageLockVersion = readJson(packageLockPath).packages[""].version;
const tauriVersion = readJson(tauriConfigPath).version;
const cargoMetadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    cwd: repoRoot,
    encoding: "utf8",
  }),
);
const cargoPackage = cargoMetadata.packages.find(
  (pkg) => path.resolve(pkg.manifest_path) === cargoManifestPath,
);

if (!cargoPackage) {
  throw new Error(`Cargo package not found for ${cargoManifestPath}`);
}

const versions = {
  "gui/package.json": packageVersion,
  "gui/package-lock.json": packageLockVersion,
  "gui/src-tauri/Cargo.toml": cargoPackage.version,
  "gui/src-tauri/tauri.conf.json": tauriVersion,
};
const uniqueVersions = new Set(Object.values(versions));

if (uniqueVersions.size !== 1) {
  console.error("Application versions do not match:");
  for (const [file, version] of Object.entries(versions)) {
    console.error(`  ${file}: ${version}`);
  }
  process.exit(1);
}

const version = packageVersion;
const tag = process.argv[2];

if (tag) {
  const expectedTag = `v${version}`;
  if (tag !== expectedTag) {
    console.error(
      `Release tag ${tag} does not match application version ${version}.`,
    );
    console.error(`Expected tag: ${expectedTag}`);
    process.exit(1);
  }
}

console.log(
  `Application version ${version}${tag ? ` matches tag ${tag}` : " is consistent"}.`,
);

#!/usr/bin/env node
// Propagates a single version string to package.json, Cargo.toml and
// tauri.conf.json. `version.txt` is left alone — build.rs already rewrites
// it from tauri.conf.json on every Windows build.
//
// Usage:
//   npm run set-version 2026.6.1
//   node scripts/set-version.mjs 2026.6.1

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");

const version = process.argv[2];
if (!version) {
  console.error("Usage: npm run set-version <version>");
  console.error("Example: npm run set-version 2026.6.1");
  process.exit(1);
}

// Reuse tauri-action / semver-compatible shape: digits, dots, optional
// pre-release suffix (e.g. 2026.6.1-rc.1). Reject anything else so a typo
// like "v2026.6.1" or "2026/6/1" fails loudly instead of silently shipping.
if (!/^\d+(\.\d+){2,3}(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(
    `Invalid version "${version}". Expected e.g. "2026.6.1" or "2026.6.1-rc.1".`
  );
  process.exit(1);
}

function updateJson(path, mutate) {
  const raw = readFileSync(path, "utf8");
  const trailingNewline = raw.endsWith("\n");
  const indent = raw.match(/^\s*\n*\{\n( +)"/)?.[1] ?? "  ";
  const obj = JSON.parse(raw);
  mutate(obj);
  const out = JSON.stringify(obj, null, indent) + (trailingNewline ? "\n" : "");
  writeFileSync(path, out);
}

function updateCargoToml(path, newVersion) {
  const lines = readFileSync(path, "utf8").split(/\r?\n/);
  let inPackage = false;
  let changed = false;
  for (let i = 0; i < lines.length; i++) {
    const header = lines[i].match(/^\s*\[([^\]]+)\]\s*$/);
    if (header) {
      inPackage = header[1].trim() === "package";
      continue;
    }
    if (!inPackage) continue;
    if (/^\s*version\s*=\s*"[^"]*"/.test(lines[i])) {
      lines[i] = lines[i].replace(
        /^(\s*version\s*=\s*")[^"]*(".*)$/,
        `$1${newVersion}$2`
      );
      changed = true;
      break;
    }
  }
  if (!changed) {
    throw new Error(`Could not find [package].version in ${path}`);
  }
  writeFileSync(path, lines.join("\n"));
}

const pkgPath = resolve(repo, "package.json");
const tauriPath = resolve(repo, "src-tauri/tauri.conf.json");
const cargoPath = resolve(repo, "src-tauri/Cargo.toml");

updateJson(pkgPath, (o) => {
  o.version = version;
});
updateJson(tauriPath, (o) => {
  o.version = version;
});
updateCargoToml(cargoPath, version);

console.log(`Set version to ${version} in:`);
console.log(`  package.json`);
console.log(`  src-tauri/Cargo.toml`);
console.log(`  src-tauri/tauri.conf.json`);
console.log(`\nNext: \`cargo build\` will refresh Cargo.lock and version.txt.`);

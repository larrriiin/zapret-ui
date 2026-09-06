#!/usr/bin/env node
// Propagates a single version string to every application-version source.
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

function prepareJson(path, mutate) {
  const raw = readFileSync(path, "utf8");
  const trailingNewline = raw.endsWith("\n");
  const indent = raw.match(/^\s*\n*\{\n( +)"/)?.[1] ?? "  ";
  const obj = JSON.parse(raw);
  mutate(obj);
  return JSON.stringify(obj, null, indent) + (trailingNewline ? "\n" : "");
}

function prepareCargoToml(path, newVersion) {
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
  return lines.join("\n");
}

function prepareCargoLock(path, newVersion) {
  const raw = readFileSync(path, "utf8");
  const pattern = /(\[\[package\]\]\r?\nname = "zapret-ui"\r?\nversion = ")[^"]+(".*)/;
  if (!pattern.test(raw)) {
    throw new Error(`Could not find the zapret-ui package version in ${path}`);
  }
  return raw.replace(pattern, `$1${newVersion}$2`);
}

const pkgPath = resolve(repo, "package.json");
const pkgLockPath = resolve(repo, "package-lock.json");
const tauriPath = resolve(repo, "src-tauri/tauri.conf.json");
const cargoPath = resolve(repo, "src-tauri/Cargo.toml");
const cargoLockPath = resolve(repo, "src-tauri/Cargo.lock");
const versionTxtPath = resolve(repo, "version.txt");

const updates = [
  [pkgPath, prepareJson(pkgPath, (o) => { o.version = version; })],
  [pkgLockPath, prepareJson(pkgLockPath, (o) => {
    o.version = version;
    if (!o.packages?.[""]) throw new Error(`Could not find the root package in ${pkgLockPath}`);
    o.packages[""].version = version;
  })],
  [tauriPath, prepareJson(tauriPath, (o) => { o.version = version; })],
  [cargoPath, prepareCargoToml(cargoPath, version)],
  [cargoLockPath, prepareCargoLock(cargoLockPath, version)],
  [versionTxtPath, version],
];

// Validate every input before writing any output, so malformed metadata cannot
// leave the repository with only part of the version bump applied.
for (const [path, contents] of updates) writeFileSync(path, contents);

console.log(`Set version to ${version} in:`);
console.log(`  package.json`);
console.log(`  package-lock.json`);
console.log(`  src-tauri/Cargo.toml`);
console.log(`  src-tauri/Cargo.lock`);
console.log(`  src-tauri/tauri.conf.json`);
console.log(`  version.txt`);

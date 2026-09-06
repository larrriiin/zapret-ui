#!/usr/bin/env node
// Fails CI when the app version drifts between package.json, Cargo.toml and
// tauri.conf.json. tauri-action publishes the version from tauri.conf.json,
// so anything else is a silent mismatch waiting to happen on the next tag.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function readCargoVersion(path) {
  const txt = readFileSync(path, "utf8");
  // Only consider the first `version = "..."` in the `[package]` table.
  const lines = txt.split(/\r?\n/);
  let inPackage = false;
  for (const line of lines) {
    const header = line.match(/^\s*\[([^\]]+)\]\s*$/);
    if (header) {
      inPackage = header[1].trim() === "package";
      continue;
    }
    if (!inPackage) continue;
    const m = line.match(/^\s*version\s*=\s*"([^"]+)"/);
    if (m) return m[1];
  }
  throw new Error(`Could not find [package].version in ${path}`);
}

function readCargoLockVersion(path) {
  const txt = readFileSync(path, "utf8");
  const match = txt.match(/\[\[package\]\]\r?\nname = "zapret-ui"\r?\nversion = "([^"]+)"/);
  if (match) return match[1];
  throw new Error(`Could not find the zapret-ui package version in ${path}`);
}

const pkg = readJson(resolve(repo, "package.json"));
const pkgLock = readJson(resolve(repo, "package-lock.json"));
const tauri = readJson(resolve(repo, "src-tauri/tauri.conf.json"));
const cargoVersion = readCargoVersion(resolve(repo, "src-tauri/Cargo.toml"));
const cargoLockVersion = readCargoLockVersion(resolve(repo, "src-tauri/Cargo.lock"));
const versionTxt = readFileSync(resolve(repo, "version.txt"), "utf8").trim();

const versions = {
  "package.json": pkg.version,
  "package-lock.json": pkgLock.version,
  'package-lock.json packages[""]': pkgLock.packages?.[""]?.version,
  "src-tauri/Cargo.toml": cargoVersion,
  "src-tauri/Cargo.lock": cargoLockVersion,
  "src-tauri/tauri.conf.json": tauri.version,
  "version.txt": versionTxt,
};

const unique = new Set(Object.values(versions));
if (unique.size !== 1) {
  console.error("Version mismatch detected:");
  for (const [file, version] of Object.entries(versions)) {
    console.error(`  ${file}: ${version}`);
  }
  console.error(
    "\nAll application-version files must carry the same version string. Run npm run set-version -- <version>."
  );
  process.exit(1);
}

console.log(`OK: all files report version ${[...unique][0]}`);

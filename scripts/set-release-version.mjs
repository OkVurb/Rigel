#!/usr/bin/env node
/**
 * Set the version a release build reports.
 *
 * A tag is the source of truth: `v0.1.1` produces binaries whose `CARGO_PKG_VERSION` is 0.1.1,
 * so the installed app can tell the next release apart from itself. Only the working tree is
 * changed; the workflow does not commit the bump.
 *
 * Usage: node scripts/set-release-version.mjs 0.1.1
 */

import { readFileSync, writeFileSync } from "node:fs";

const version = (process.argv[2] ?? "").trim().replace(/^v/, "");
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error("usage: node scripts/set-release-version.mjs <semver>");
}

const cargoPath = "Cargo.toml";
const cargo = readFileSync(cargoPath, "utf8");
const cargoVersion = /(\[workspace\.package\][\s\S]*?\nversion = ")[^"]+(")/;
if (!cargoVersion.test(cargo)) {
  throw new Error("Cargo.toml has no [workspace.package] version");
}
writeFileSync(cargoPath, cargo.replace(cargoVersion, `$1${version}$2`), "utf8");

const tauriPath = "crates/rigel-app/tauri.conf.json";
const tauri = readFileSync(tauriPath, "utf8");
if (!/"version":\s*"[^"]+"/.test(tauri)) {
  throw new Error("tauri.conf.json has no version");
}
writeFileSync(tauriPath, tauri.replace(/("version":\s*")[^"]+(")/, `$1${version}$2`), "utf8");

const uiPath = "ui/package.json";
const ui = readFileSync(uiPath, "utf8");
if (!/"version":\s*"[^"]+"/.test(ui)) {
  throw new Error("ui/package.json has no version");
}
writeFileSync(uiPath, ui.replace(/("version":\s*")[^"]+(")/, `$1${version}$2`), "utf8");

console.log(`set release version ${version}`);

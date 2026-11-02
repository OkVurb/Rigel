#!/usr/bin/env node
// One version, three files. A release whose page advertises a different build than the installer
// is a support ticket, so CI fails when they drift instead of a human noticing weeks later.

import fs from "node:fs";

const read = (path) => fs.readFileSync(path, "utf8");

const cargo = read("Cargo.toml");
const workspace = cargo.match(/^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m);
const app = JSON.parse(read("crates/rigel-app/tauri.conf.json"));
const html = read("web/index.html");
const site = html.match(/data-version="([^"]+)"/);

if (!workspace) throw new Error("Cargo.toml has no [workspace.package] version");
if (!site) throw new Error("web/index.html has no data-version on the download button");

const found = {
  "Cargo.toml": workspace[1],
  "crates/rigel-app/tauri.conf.json": app.version,
  "web/index.html": site[1],
};

const unique = new Set(Object.values(found));
if (unique.size !== 1) {
  console.error("version mismatch:");
  for (const [file, version] of Object.entries(found)) console.error(`  ${version.padEnd(8)} ${file}`);
  process.exit(1);
}

const [version] = unique;
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`version ${version} is not MAJOR.MINOR.PATCH`);
  process.exit(1);
}

console.log(`version ${version} agrees across ${Object.keys(found).length} files`);

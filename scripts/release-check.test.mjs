// The release gate has to fail on a stale version and name the file that is stale, not just
// print a diff. These fixtures build a minimal tree, run the real script, and assert the exit
// code and the message.

import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import assert from "node:assert/strict";

const script = path.join(process.cwd(), "scripts", "release-check.mjs");

function fixture({ cargo = "0.2.0", tauri = "0.2.0", ui = "0.2.0", site = "0.1.7", label = "0.1.7", receipts = ["0.1.7"] } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), "rigel-release-"));
  mkdirSync(path.join(root, "crates", "rigel-app"), { recursive: true });
  mkdirSync(path.join(root, "ui"), { recursive: true });
  mkdirSync(path.join(root, "web"), { recursive: true });
  mkdirSync(path.join(root, "docs", "release-receipts"), { recursive: true });
  writeFileSync(path.join(root, "Cargo.toml"), `[workspace.package]\nversion = "${cargo}"\n`);
  writeFileSync(path.join(root, "crates", "rigel-app", "tauri.conf.json"), JSON.stringify({ version: tauri }));
  writeFileSync(path.join(root, "ui", "package.json"), JSON.stringify({ version: ui }));
  writeFileSync(
    path.join(root, "web", "index.html"),
    `<a id="download" data-version="${site}" href="/releases/latest">Get Rigel ${label}</a>`,
  );
  writeFileSync(
    path.join(root, "docs", "release-checklist.md"),
    [
      "Version is checked.",
      "cargo fmt --all -- --check",
      "cargo clippy --workspace --all-targets -- -D warnings",
      "cargo test --workspace",
      "npm --prefix ui run build",
      "node scripts/check-docs.mjs",
    ].join("\n"),
  );
  for (const receipt of receipts) {
    writeFileSync(path.join(root, "docs", "release-receipts", `${receipt}.md`), `# ${receipt}\n`);
  }
  return root;
}

function run(root) {
  return spawnSync(process.execPath, [script, "--root", root, "--only", "versions"], {
    encoding: "utf8",
  });
}

test("a consistent tree passes, with the site on the published build", () => {
  const result = run(fixture());
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /0\.2\.0/);
  assert.match(result.stdout, /advertises the published 0\.1\.7/);
});

test("a stale build file fails and names the file", () => {
  const result = run(fixture({ ui: "0.2.1" }));
  assert.equal(result.status, 1);
  assert.match(result.stderr, /ui\/package\.json/);
  assert.match(result.stderr, /0\.2\.1/);
});

test("a website newer than the tree fails and names the file", () => {
  const result = run(fixture({ site: "0.3.0", label: "0.3.0" }));
  assert.equal(result.status, 1);
  assert.match(result.stderr, /web\/index\.html/);
});

test("a label that disagrees with its own data-version fails", () => {
  const result = run(fixture({ site: "0.1.7", label: "0.2.0" }));
  assert.equal(result.status, 1);
  assert.match(result.stderr, /label/);
});

test("a version with no receipt cannot sit behind the tree", () => {
  const result = run(fixture({ receipts: [] }));
  assert.equal(result.status, 1);
  assert.match(result.stderr, /no receipt/);
});

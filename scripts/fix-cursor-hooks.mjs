#!/usr/bin/env node
/**
 * ECC writes Cursor hook commands as project-relative paths, e.g.
 *
 *   "node .cursor/hooks/session-start.js"
 *
 * into the *user-level* ~/.cursor/hooks.json. Cursor runs hook commands from the
 * opened workspace root, so that path only resolves when the workspace happens to
 * be the home directory — in every real project the hook fails silently.
 *
 * This rewrites those commands to absolute paths and verifies each target exists.
 * Safe to re-run; a backup is written next to hooks.json.
 *
 * Usage: node scripts/fix-cursor-hooks.mjs [--dry-run]
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const HOME = os.homedir();
const FILE = path.join(HOME, ".cursor", "hooks.json");
const NEEDLE = ".cursor/hooks/";
const DRY = process.argv.includes("--dry-run");

if (!fs.existsSync(FILE)) {
  console.log("no ~/.cursor/hooks.json — nothing to do");
  process.exit(0);
}

const json = JSON.parse(fs.readFileSync(FILE, "utf8"));
const hooks = json.hooks ?? json;
const base = path.join(HOME, ".cursor").split(path.sep).join("/");

let rewritten = 0;
let alreadyAbsolute = 0;
const missing = [];

for (const event of Object.keys(hooks)) {
  for (const group of [].concat(hooks[event])) {
    for (const hook of group?.hooks ?? [group]) {
      const cmd = hook?.command;
      if (typeof cmd !== "string" || !cmd.includes(NEEDLE)) continue;

      // Already resolved by a previous run?
      if (cmd.includes(base)) {
        alreadyAbsolute++;
        const found = cmd.slice(cmd.indexOf(base), cmd.indexOf(".js", cmd.indexOf(base)) + 3);
        if (!fs.existsSync(found)) missing.push(found);
        continue;
      }

      let next = cmd.split(NEEDLE).join(base + "/hooks/");

      // Quote the script token if the rewrite left it bare.
      const at = next.indexOf(base);
      if (at > -1 && next[at - 1] !== '"') {
        const end = next.indexOf(".js", at) + 3;
        next = next.slice(0, at) + '"' + next.slice(at, end) + '"' + next.slice(end);
      }

      const target = next.slice(next.indexOf(base), next.indexOf(".js", next.indexOf(base)) + 3);
      if (!fs.existsSync(target)) missing.push(target);

      hook.command = next;
      rewritten++;
    }
  }
}

console.log(
  `${DRY ? "[dry-run] " : ""}rewritten: ${rewritten} | already absolute: ${alreadyAbsolute} | missing targets: ${missing.length}`,
);
for (const m of missing.slice(0, 6)) console.log("  missing: " + m);

if (!DRY && rewritten > 0) {
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  fs.copyFileSync(FILE, `${FILE}.bak-${stamp}`);
  fs.writeFileSync(FILE, JSON.stringify(json, null, 2) + "\n");
  console.log(`wrote ${FILE} (backup: hooks.json.bak-${stamp})`);
}

process.exit(missing.length ? 1 : 0);

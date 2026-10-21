#!/usr/bin/env node
/**
 * Snapshot the skill names that actually exist on this machine, for the
 * prototype's GitHub discovery view to diff against.
 *
 *   node scripts/export-installed-skills.mjs
 *
 * Writes prototype/src/installed.json. Re-run whenever skills are installed.
 */
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const HOME = process.env.USERPROFILE || os.homedir();

const STORES = {
  claude: ".claude/skills",
  codex: ".codex/skills",
  agents: ".agents/skills",
  cursor: ".cursor/skills",
  // Cline has no store of its own; it reads the Claude path.
};

const names = new Set();
const stores = {};

for (const [id, rel] of Object.entries(STORES)) {
  const dir = path.join(HOME, rel);
  let count = 0;
  try {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (!entry.isDirectory() && !entry.isSymbolicLink()) continue;
      if (!fs.existsSync(path.join(dir, entry.name, "SKILL.md"))) continue;
      names.add(entry.name.toLowerCase());
      count += 1;
    }
  } catch {
    /* store absent */
  }
  stores[id] = count;
}

const sorted = [...names].sort();
const out = {
  generatedAt: new Date().toISOString().slice(0, 10),
  machine: `${os.platform()}-${os.arch()}`,
  stores,
  count: sorted.length,
  names: sorted,
};

const target = path.join(process.cwd(), "prototype/src/installed.json");
fs.writeFileSync(target, `${JSON.stringify(out)}\n`);
console.log(
  `wrote ${target}\n${sorted.length} distinct skills · ${Object.entries(stores)
    .map(([k, v]) => `${k} ${v}`)
    .join(" · ")}`,
);

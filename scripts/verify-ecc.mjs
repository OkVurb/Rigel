#!/usr/bin/env node
/**
 * Verify that ECC is installed and loadable for every agent harness on this machine.
 *
 * Checks the filesystem contract each harness actually uses (skills dir + valid
 * SKILL.md frontmatter + hook wiring), because that is what decides whether a
 * harness can see a skill. It does NOT call a model — see the printed
 * "finish the live check" commands for that.
 *
 * Usage:  node scripts/verify-ecc.mjs
 * Exit:   0 all structural checks pass, 1 one or more failed.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const HOME = os.homedir();
const p = (...parts) => path.join(HOME, ...parts);
const exists = (f) => fs.existsSync(f);

// Skill dirs are often symlinked/junctioned (cloud sync, per-tool sharing), and
// Dirent.isDirectory() reports false for those. Resolve before deciding.
const isDir = (f) => {
  try {
    return fs.statSync(f).isDirectory();
  } catch {
    return false;
  }
};

function readJson(f) {
  try {
    return JSON.parse(fs.readFileSync(f, "utf8"));
  } catch {
    return null;
  }
}

/** Parse the required Agent Skills frontmatter. */
function inspectSkill(dir) {
  const file = path.join(dir, "SKILL.md");
  if (!exists(file)) return null;
  const head = fs.readFileSync(file, "utf8").slice(0, 4000);
  const fm = head.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  if (!fm) return { valid: false, reason: "no frontmatter" };
  const body = fm[1];
  const name = (body.match(/^name:\s*(.+)$/m) || [])[1]?.trim();
  const desc = (body.match(/^description:\s*(.+)$/m) || [])[1]?.trim();
  if (!name) return { valid: false, reason: "no name" };
  if (name !== path.basename(dir)) return { valid: false, reason: `name≠dir (${name})` };
  if (!desc || desc.length < 20) return { valid: false, reason: "thin description" };
  return { valid: true };
}

const isEcc = (name) => /^(ecc-|continuous-learning|tdd-workflow|orchestrat)/.test(name);

function scanRoot(root) {
  if (!exists(root)) return { root, present: false, total: 0, valid: 0, ecc: 0, eccBad: 0, bad: [] };
  let total = 0, valid = 0, ecc = 0, eccBad = 0;
  const bad = [];
  for (const name of fs.readdirSync(root)) {
    const dir = path.join(root, name);
    if (!isDir(dir)) continue;
    const info = inspectSkill(dir);
    if (!info) continue;
    total++;
    if (info.valid) valid++;
    else {
      bad.push(`${name}: ${info.reason}`);
      if (isEcc(name)) eccBad++;
    }
    if (isEcc(name)) ecc++;
  }
  return { root, present: true, total, valid, ecc, eccBad, bad };
}

function hookEvents(kind) {
  if (kind === "claude") {
    const s = readJson(p(".claude", "settings.json"));
    return s?.hooks ? Object.keys(s.hooks) : [];
  }
  if (kind === "cursor") {
    const h = readJson(p(".cursor", "hooks.json"));
    const obj = h?.hooks ?? h ?? {};
    return Object.keys(obj);
  }
  return [];
}

function hookCommands(kind) {
  const file = kind === "claude" ? p(".claude", "settings.json") : p(".cursor", "hooks.json");
  const json = readJson(file);
  if (!json) return [];
  const hooks = json.hooks ?? json;
  const out = [];
  for (const ev of Object.keys(hooks)) {
    for (const group of [].concat(hooks[ev])) {
      for (const h of group?.hooks ?? [group]) {
        if (h?.command) out.push(h.command);
      }
    }
  }
  return out;
}

/**
 * Hooks fail silently when their script path is wrong — check the literal ones.
 * ECC's Claude hooks are `node -e "..."` expressions that resolve their own
 * paths at runtime, so those are counted separately instead of misread as paths.
 */
function checkHookTargets(kind) {
  const cmds = hookCommands(kind);
  let checked = 0, dynamic = 0;
  const missing = [];
  for (const c of cmds) {
    if (/\bnode\s+-e\b|--eval\b|\$\{/.test(c)) {
      dynamic++;
      continue;
    }
    const matches = c.match(/"[A-Za-z]:\\[^"]+\.(?:js|mjs|cjs|py|ps1|sh)"|"[^"\s]+\.(?:js|mjs|cjs|py|ps1|sh)"|\/[^\s"]+\.(?:js|mjs|cjs|py|ps1|sh)/g) ?? [];
    for (const raw of matches) {
      const file = raw.replace(/"/g, "");
      if (/[()'`,;=]/.test(file)) continue; // fragment of an expression, not a path
      const abs = path.isAbsolute(file) ? file : path.resolve(HOME, file);
      checked++;
      if (!exists(abs)) missing.push(abs);
    }
  }
  return { events: hookEvents(kind).length, commands: cmds.length, checked, dynamic, missing };
}

const HARNESSES = [
  {
    name: "Claude Code",
    roots: [p(".claude", "skills")],
    hooks: "claude",
    agentCount: () => fs.existsSync(p(".claude", "agents")) ? fs.readdirSync(p(".claude", "agents")).length : 0,
  },
  {
    name: "Codex",
    roots: [p(".agents", "skills")],
    hooks: null, // ECC's codex adapter skips hooks-runtime
    agentCount: () => fs.existsSync(p(".codex", "agents")) ? fs.readdirSync(p(".codex", "agents")).length : 0,
  },
  {
    name: "Cursor",
    roots: [p(".cursor", "skills"), p(".agents", "skills")],
    hooks: "cursor",
    agentCount: () => fs.existsSync(p(".cursor", "agents")) ? fs.readdirSync(p(".cursor", "agents")).length : 0,
  },
  {
    name: "Cline",
    roots: [p(".claude", "skills"), p(".agents", "skills")],
    hooks: null,
    agentCount: () => 0,
  },
];

const pad = (s, n) => String(s).padEnd(n);
const rows = [];
let failures = 0;

for (const h of HARNESSES) {
  const scans = h.roots.map(scanRoot).filter((s) => s.present);
  if (scans.length === 0) {
    rows.push([h.name, "not installed", "-", "-", "-", "-", "MISSING"]);
    continue;
  }
  const primary = scans[0];
  const eccAny = scans.some((s) => s.ecc > 0);
  // Only ECC's own skills decide the verdict; pre-existing third-party skills
  // with thin frontmatter are reported but must not fail the install.
  const eccBad = scans.reduce((n, s) => n + s.eccBad, 0);
  const hooks = h.hooks ? hookEvents(h.hooks) : [];
  const hooksLabel = h.hooks ? (hooks.length ? `${hooks.length} events` : "none") : "not supported";
  const verdict = eccAny && eccBad === 0 ? "PASS" : "FAIL";
  if (verdict === "FAIL") failures++;
  rows.push([
    h.name,
    primary.total,
    primary.valid,
    eccAny ? `${primary.ecc}` : "0",
    h.agentCount() || "-",
    hooksLabel,
    verdict,
  ]);
}

const header = ["HARNESS", "SKILLS", "VALID", "ECC", "AGENTS", "HOOKS", "VERDICT"];
const widths = [12, 7, 6, 5, 7, 15, 8];
console.log("\nECC verification — " + new Date().toISOString().slice(0, 10) + "\n");
console.log(header.map((c, i) => pad(c, widths[i])).join(""));
console.log(widths.map((w) => "-".repeat(w)).join(""));
for (const r of rows) console.log(r.map((c, i) => pad(c, widths[i])).join(""));

// Roots overview
console.log("\nRoots");
for (const root of [p(".claude", "skills"), p(".agents", "skills"), p(".cursor", "skills")]) {
  const s = scanRoot(root);
  console.log(
    "  " + pad(root.replace(HOME, "~"), 26) +
    (s.present ? `${s.total} skills, ${s.ecc} ECC` : "absent"),
  );
  if (s.bad.length) console.log("    non-ECC issues: " + s.bad.slice(0, 5).join("; "));
}

console.log("\nHook targets");
for (const kind of ["claude", "cursor"]) {
  const r = checkHookTargets(kind);
  if (!r.events) {
    console.log(`  ${pad(kind, 8)} no hooks wired`);
    continue;
  }
  console.log(
    `  ${pad(kind, 8)} ${r.events} events, ${r.checked} literal paths, ` +
    `${r.dynamic} runtime-resolved, ` +
    (r.missing.length ? `${r.missing.length} MISSING` : "all present"),
  );
  for (const m of r.missing.slice(0, 4)) console.log("    missing: " + m);
  if (r.missing.length) failures++;
}

// Live checks are separate on purpose: they need network + a logged-in CLI.
console.log("\nFinish the live check (needs a logged-in harness):");
console.log('  claude -p "list 3 ECC skill names you can see" --output-format text');
console.log('  codex exec --skip-git-repo-check "list 3 ECC skill names you can see" < /dev/null');
console.log("  Cursor / Cline: open the app, type / and look for ecc-* entries.");

console.log(`\n${failures === 0 ? "Structural checks: PASS" : `Structural checks: ${failures} FAILED`}\n`);
process.exit(failures === 0 ? 0 : 1);

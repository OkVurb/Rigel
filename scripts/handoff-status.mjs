#!/usr/bin/env node
/**
 * Read docs/handoff.md and report checklist completion per phase.
 *
 * The handoff's §11 summary table is hand-maintained, so this script treats the
 * checkbox list as the source of truth and fails when the table disagrees with it.
 *
 * Usage:
 *   node scripts/handoff-status.mjs           # human table
 *   node scripts/handoff-status.mjs --json    # machine-readable
 *   node scripts/handoff-status.mjs --quiet   # exit code only
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const docPath = join(root, "docs", "handoff.md");
const source = readFileSync(docPath, "utf8").split(/\r?\n/);

const ITEM = /^- \[(x| )\] \*\*(P(\d+)\.(\d+))\*\*/;
const PHASE = /^### Phase (\d+)/;
const SUMMARY = /^\|\s*(\d+)\s*\|[^|]+\|\s*\*{0,2}([^|*]+?)\*{0,2}\s*\|\s*(\d+)\/(\d+)\s*\|$/;

const phases = new Map();
const seen = new Set();
const duplicates = [];
let current = null;

for (const line of source) {
  const phase = line.match(PHASE);
  if (phase) {
    current = Number(phase[1]);
    if (!phases.has(current)) phases.set(current, { done: 0, total: 0, ids: [] });
    continue;
  }
  if (line.startsWith("## ")) current = null;

  const item = line.match(ITEM);
  if (!item) continue;
  const [, mark, id, phaseNo] = item;
  if (seen.has(id)) duplicates.push(id);
  seen.add(id);

  const bucket = phases.get(Number(phaseNo)) ?? { done: 0, total: 0, ids: [] };
  bucket.total += 1;
  if (mark === "x") bucket.done += 1;
  bucket.ids.push({ id, done: mark === "x" });
  phases.set(Number(phaseNo), bucket);
}

const order = [...phases.keys()].sort((a, b) => a - b);
const rows = order.map((n) => {
  const p = phases.get(n);
  const state = p.done === p.total ? "complete" : p.done === 0 ? "not started" : "in progress";
  return { phase: n, done: p.done, total: p.total, state, pct: p.total ? Math.round((p.done / p.total) * 100) : 0 };
});

const totalDone = rows.reduce((n, r) => n + r.done, 0);
const totalItems = rows.reduce((n, r) => n + r.total, 0);

// Cross-check the hand-written summary table in §11.
const declared = new Map();
for (const line of source) {
  const m = line.match(SUMMARY);
  if (m) declared.set(Number(m[1]), { done: Number(m[3]), total: Number(m[4]) });
}
const mismatches = [];
for (const r of rows) {
  const d = declared.get(r.phase);
  if (!d) {
    mismatches.push(`phase ${r.phase}: missing from the §11 summary table`);
    continue;
  }
  if (d.done !== r.done || d.total !== r.total) {
    mismatches.push(`phase ${r.phase}: table says ${d.done}/${d.total}, checklist has ${r.done}/${r.total}`);
  }
}

if (process.argv.includes("--json")) {
  console.log(JSON.stringify({ phases: rows, total: { done: totalDone, items: totalItems }, duplicates, mismatches }, null, 2));
} else if (!process.argv.includes("--quiet")) {
  const width = Math.max(...rows.map((r) => r.state.length), 11);
  console.log("");
  console.log("  Phase  Done   State");
  console.log("  ─────  ─────  " + "─".repeat(width));
  for (const r of rows) {
    console.log(`  ${String(r.phase).padEnd(5)}  ${`${r.done}/${r.total}`.padEnd(5)}  ${r.state.padEnd(width)}  ${r.pct}%`);
  }
  console.log("  ─────  ─────  " + "─".repeat(width));
  console.log(`  all    ${`${totalDone}/${totalItems}`.padEnd(5)}  ${totalItems ? Math.round((totalDone / totalItems) * 100) : 0}% of the handoff`);
  console.log("");
}

if (duplicates.length) {
  console.error(`duplicate item ids: ${duplicates.join(", ")}`);
}
if (mismatches.length) {
  console.error("the §11 summary table is stale:");
  for (const m of mismatches) console.error(`  - ${m}`);
}
process.exit(duplicates.length || mismatches.length ? 1 : 0);

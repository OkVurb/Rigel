#!/usr/bin/env node
/**
 * Item 2's release wrapper: run the every-prefix rehearsal and print the versions each prefix
 * reached. The tests are the source of truth; this only makes their answer citable in a release
 * checklist without opening a database by hand.
 *
 * Usage: node scripts/rehearse-migrations.mjs
 */

import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

let output;
try {
  output = execFileSync(
    "cargo",
    ["test", "-p", "rigel-core", "--test", "schema", "every_", "--", "--nocapture"],
    { cwd: ROOT, encoding: "utf8" },
  );
} catch (error) {
  process.stdout.write(error.stdout ?? "");
  process.stderr.write(error.stderr ?? "");
  console.error("migration rehearsal: a prefix failed; see the test output above");
  process.exit(1);
}

process.stdout.write(output);
const lines = output.split(/\r?\n/).filter((line) => /^(project|index) prefix \d+ -> /.test(line));
if (lines.length === 0) {
  console.error("migration rehearsal: the tests printed no prefix versions");
  process.exit(1);
}
for (const line of lines) {
  console.log(line);
}
console.log(`migration rehearsal: ${lines.length} prefix runs upgraded to the current schema`);

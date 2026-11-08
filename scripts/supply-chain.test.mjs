import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { checkDenyConfig, checkWorkflow, scanText } from "./supply-chain.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => readFileSync(path.join(ROOT, relative), "utf8");

test("the repository's deny.toml names every section cargo-deny enforces", () => {
  assert.deepEqual(checkDenyConfig(read("deny.toml")), []);
});

test("a deny.toml without an advisories section is refused", () => {
  const problems = checkDenyConfig('[licenses]\nallow = ["MIT", "Apache-2.0"]\n');
  assert.ok(
    problems.some((problem) => problem.includes("[advisories]")),
    `expected an advisories problem, got ${JSON.stringify(problems)}`,
  );
});

test("the repository's CI lane runs audit, secrets, SBOM and its own test", () => {
  assert.deepEqual(checkWorkflow(read(".github/workflows/ci.yml")), []);
});

test("a CI lane that forgot the secret scan is refused", () => {
  const problems = checkWorkflow(
    "jobs:\n  supply-chain:\n    steps:\n      - run: node scripts/make-sbom.mjs\n",
  );
  assert.ok(
    problems.some((problem) => problem.includes("committed-secret")),
    `expected a secret-scan problem, got ${JSON.stringify(problems)}`,
  );
});

test("the scanner finds a planted cloud key and leaves prose alone", () => {
  assert.deepEqual(scanText("the quick brown fox"), []);
  assert.deepEqual(scanText(`aws key AKIA${"ABCDEFGHIJKLMNOP"}`), ["aws access key"]);
  assert.deepEqual(scanText(`groq ${"gsk_"}${"a".repeat(24)}`), ["groq key"]);
  assert.deepEqual(scanText(`xai ${"xai-"}${"b".repeat(24)}`), ["xai key"]);
});

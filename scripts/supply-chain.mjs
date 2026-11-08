#!/usr/bin/env node
/**
 * Item 9: the supply-chain lane's local half.
 *
 * CI runs the four hosted tools (RustSec audit, cargo-deny, gitleaks, SBOM artifact). This script
 * is what a developer can run before pushing and what CI runs as its self-test: it validates the
 * policy files, runs every tool that is installed, says plainly which ones were skipped, and
 * proves the lane can still build an SBOM. `--require-tools` turns a missing tool into a failure,
 * which is how CI can pin the full lane.
 *
 * Usage: node scripts/supply-chain.mjs [--require-tools]
 */

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const DENY_SECTIONS = ["advisories", "licenses", "bans", "sources"];
const WORKFLOW_MARKERS = [
  ["a supply-chain job", /\bsupply-chain:/],
  ["an advisory check", /rustsec\/audit-check|cargo audit/],
  ["a licence and ban check", /cargo-deny-action|cargo deny check/],
  ["a committed-secret scan", /gitleaks/],
  ["SBOM generation", /make-sbom\.mjs/],
  ["its own self-test", /node --test scripts\/supply-chain\.test\.mjs/],
];

/** Problems with `deny.toml`, as sentences. Empty means the policy is complete. */
export function checkDenyConfig(text) {
  const problems = [];
  for (const section of DENY_SECTIONS) {
    if (!new RegExp(`^\\[${section}\\]`, "m").test(text)) {
      problems.push(`deny.toml is missing [${section}]`);
    }
  }
  if (!/allow\s*=\s*\[[^\]]*"MIT"/s.test(text)) {
    problems.push("deny.toml does not allow the workspace's MIT licence");
  }
  if (!/allow\s*=\s*\[[^\]]*"Apache-2\.0"/s.test(text)) {
    problems.push("deny.toml does not allow Apache-2.0");
  }
  return problems;
}

/** Problems with the CI lane, as sentences. Empty means every step is present. */
export function checkWorkflow(text) {
  return WORKFLOW_MARKERS.filter(([, pattern]) => !pattern.test(text)).map(
    ([name]) => `ci.yml's supply-chain job has no ${name}`,
  );
}

const RULES = [
  ["aws access key", /\bAKIA[0-9A-Z]{16}\b/],
  ["github token", /\bgh[pousr]_[A-Za-z0-9]{30,}\b/],
  ["github fine-grained token", /\bgithub_pat_[A-Za-z0-9_]{40,}\b/],
  ["anthropic key", /\bsk-ant-[A-Za-z0-9_-]{20,}\b/],
  ["openrouter key", /\bsk-or-v1-[A-Za-z0-9]{32,}\b/],
  ["groq key", /\bgsk_[A-Za-z0-9]{20,}\b/],
  ["xai key", /\bxai-[A-Za-z0-9]{20,}\b/],
  ["google api key", /\bAIza[0-9A-Za-z_-]{35}\b/],
  ["private key", /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/],
];

/** Names of the credential shapes found in `text`. Empty means nothing looked like a secret. */
export function scanText(text) {
  return RULES.filter(([, pattern]) => pattern.test(text)).map(([name]) => name);
}

function installed(command) {
  try {
    execFileSync(command, ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function run(command, args) {
  return execFileSync(command, args, {
    cwd: ROOT,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
}

export function main({ requireTools = process.argv.includes("--require-tools") } = {}) {
  const problems = [];
  const skipped = [];

  problems.push(...checkDenyConfig(readFileSync(path.join(ROOT, "deny.toml"), "utf8")));
  problems.push(
    ...checkWorkflow(readFileSync(path.join(ROOT, ".github", "workflows", "ci.yml"), "utf8")),
  );

  if (installed("cargo")) {
    try {
      run("cargo", ["audit", "--quiet"]);
      console.log("audit:     cargo audit found no vulnerability");
    } catch (error) {
      problems.push(`cargo audit failed: ${(error.stdout || error.message).trim()}`);
    }
  } else if (requireTools) {
    problems.push("cargo is not installed, so the advisory database was not checked");
  } else {
    skipped.push("cargo audit (cargo not on PATH)");
  }

  if (installed("cargo-deny")) {
    try {
      run("cargo", ["deny", "check"]);
      console.log("policy:    cargo deny accepted licences, bans and sources");
    } catch (error) {
      problems.push(`cargo deny check failed: ${(error.stdout || error.message).trim()}`);
    }
  } else if (requireTools) {
    problems.push("cargo-deny is not installed");
  } else {
    skipped.push("cargo deny check (cargo-deny not installed)");
  }

  if (installed("gitleaks")) {
    try {
      run("gitleaks", ["detect", "--no-banner", "--redact"]);
      console.log("secrets:   gitleaks found no committed credential");
    } catch (error) {
      problems.push(`gitleaks found a committed credential: ${(error.stdout || error.message).trim()}`);
    }
  } else if (requireTools) {
    problems.push("gitleaks is not installed");
  } else {
    skipped.push("gitleaks (not installed)");
  }

  const sbom = path.join(ROOT, "sbom.local.cdx.json");
  try {
    run("node", [path.join(ROOT, "scripts", "make-sbom.mjs"), sbom]);
    const text = readFileSync(sbom, "utf8");
    if (!text.includes("rigel-core")) {
      problems.push("the SBOM does not mention rigel-core");
    } else {
      console.log("sbom:      generated and names the workspace crates");
    }
  } catch (error) {
    problems.push(`SBOM generation failed: ${error.message}`);
  } finally {
    rmSync(sbom, { force: true });
  }

  for (const item of skipped) {
    console.log(`skipped:   ${item}`);
  }
  if (problems.length > 0) {
    for (const problem of problems) {
      console.error(`problem:   ${problem}`);
    }
    return 1;
  }
  console.log("supply-chain lane: policy complete, every installed tool clean");
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}

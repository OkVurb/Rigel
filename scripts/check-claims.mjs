#!/usr/bin/env node
// No invented metrics (roadmap 130).
//
// Every percentage in public copy (`web/`) or visible product copy (`ui/src`) must have a
// row in `docs/claims.json` with a source and a basis. CSS geometry (`width: 100%`) and code
// comments are not claims. `--self-test` plants a fabricated figure and asserts the checker
// rejects it, so the check cannot silently stop working.

import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CLAIM = /\b\d+(?:\.\d+)?%/g;

function walk(dir, extensions, files = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "node_modules" && entry.name !== "dist") {
        walk(full, extensions, files);
      }
    } else if (extensions.some((extension) => entry.name.endsWith(extension))) {
      files.push(full);
    }
  }
  return files;
}

function htmlText(source) {
  return source
    .replace(/<script[\s\S]*?<\/script>/gi, " ")
    .replace(/<style[\s\S]*?<\/style>/gi, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&[a-z#0-9]+;/gi, " ");
}

function tsxText(source) {
  return source
    .split(/\r?\n/)
    .map((line) => {
      if (/\b(?:style|width|height|left|top|right|bottom|color-mix)\s*[:=]/.test(line)) return "";
      if (/^\s*(?:\/\/|\*|\/\*)/.test(line)) return "";
      return [...line.matchAll(/"([^"]*)"/g), ...line.matchAll(/'>([^<]*)<'/g)]
        .map((match) => match[1] ?? "")
        .join(" ");
    })
    .join("\n");
}

function claims(root) {
  const registry = JSON.parse(readFileSync(path.join(root, "docs", "claims.json"), "utf8"));
  return registry.claims ?? [];
}

/** Return the claims in `text` that the registry does not cover. */
export function findUnsourced(text, registered) {
  const missing = [];
  for (const match of text.matchAll(CLAIM)) {
    const figure = match[0];
    const covered = registered.some(
      (claim) => claim.text === figure || claim.text.includes(figure),
    );
    if (!covered) missing.push(figure);
  }
  return missing;
}

export function checkClaims({ root = ROOT } = {}) {
  const problems = [];
  const registered = claims(root);
  const sources = [
    ...walk(path.join(root, "web"), [".html"]).map((file) => [file, htmlText]),
    ...walk(path.join(root, "ui", "src"), [".tsx"]).map((file) => [file, tsxText]),
  ];
  for (const [file, extract] of sources) {
    const text = extract(readFileSync(file, "utf8"));
    for (const figure of findUnsourced(text, registered)) {
      problems.push(
        `${path.relative(root, file).replaceAll("\\\\", "/")}: unsourced figure ${figure} (add docs/claims.json row)`,
      );
    }
  }
  return { problems, claims: registered.length, files: sources.length };
}

function selfTest() {
  const planted = "99% of teams ship faster with Rigel.";
  const missing = findUnsourced(planted, []);
  if (!missing.includes("99%")) {
    console.error("claims self-test: a planted fabricated figure was not detected");
    process.exit(1);
  }
  console.log("claims self-test: a planted fabricated percentage is rejected");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.includes("--self-test")) {
    selfTest();
  } else {
    const { problems, claims: count, files } = checkClaims();
    if (problems.length > 0) {
      console.error(`claims check: ${problems.length} problem${problems.length === 1 ? "" : "s"}\n`);
      for (const problem of problems) console.error(`  ${problem}`);
      process.exitCode = 1;
    } else {
      console.log(`claims check: ${count} sourced claims across ${files} copy files`);
    }
  }
}

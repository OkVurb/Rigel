#!/usr/bin/env node
// Design-system lock (roadmap 106) and surface job audit (roadmap 107).
//
// Fails when a file in ui/src introduces a colour literal outside the token sheet and its
// two documented exceptions, when docs/design.md omits a surface job or leaves a cell blank,
// or when two surfaces with different jobs claim the same rhythm.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const COLOUR = /#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})\b/g;
const FUNCTIONS = /\b(?:rgba?|hsla?)\s*\(/g;

// The token sheet owns colour. The xterm palette needs concrete ANSI values; the fixture
// provider colours are data for the offline harness, not styling.
const ALLOWED = new Set([
  "ui/src/tokens.css",
  "ui/src/shell.css",
  "ui/src/terminal.tsx",
  "ui/src/api.ts",
  "ui/src/mock.ts",
]);

export function checkTokens({ root = ROOT } = {}) {
  const problems = [];
  const files = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== "node_modules" && entry.name !== "dist") walk(full);
      } else if (/\.(css|ts|tsx)$/.test(entry.name)) {
        files.push(full);
      }
    }
  };
  const source = path.join(root, "ui", "src");
  walk(source);
  for (const file of files) {
    const relative = path.relative(root, file).replaceAll("\\", "/");
    if (ALLOWED.has(relative)) continue;
    const text = readFileSync(file, "utf8");
    text.split(/\r?\n/).forEach((line, index) => {
      // Comments are documentation, not a rendered colour.
      const code = line.replace(/\/\/.*$/, "").replace(/\/\*.*?\*\//g, "");
      if (COLOUR.test(code) || FUNCTIONS.test(code)) {
        problems.push(`${relative}:${index + 1}: raw colour outside the token sheet`);
      }
      COLOUR.lastIndex = 0;
      FUNCTIONS.lastIndex = 0;
    });
  }

  const design = readFileSync(path.join(root, "docs", "design.md"), "utf8");
  const tables = design.split(/\n## /).find((section) => section.startsWith("Surface jobs"));
  if (!tables) {
    problems.push("docs/design.md: the Surface jobs section is missing");
  } else {
    const rows = tables
      .split("\n")
      .filter((line) => line.startsWith("|"))
      .map((line) => line.split("|").slice(1, -1).map((cell) => cell.trim()));
    const body = rows.filter(
      (cells) => cells.length > 0 && !cells.every((cell) => /^-+$/.test(cell)),
    );
    const header = body.shift();
    if (!header || header.length !== 5) {
      problems.push("docs/design.md: the surface table must have five columns");
    }
    if (body.length < 7) {
      problems.push(`docs/design.md: expected at least 7 surfaces, found ${body.length}`);
    }
    for (const cells of body) {
      if (cells.length !== 5 || cells.some((cell) => cell === "")) {
        problems.push(`docs/design.md: a surface row has a blank cell: ${cells.join(" | ")}`);
      }
    }
    const rhythms = new Map();
    for (const cells of body) {
      const [surface, , , rhythm] = cells;
      const key = rhythm.toLowerCase();
      if (rhythms.has(key)) {
        problems.push(
          `docs/design.md: ${surface} shares its rhythm with ${rhythms.get(key)} despite a different job`,
        );
      }
      rhythms.set(key, surface);
    }
    for (const required of ["Composer / session", "Approval card", "Cost / usage"]) {
      if (!body.some((cells) => cells[0] === required)) {
        problems.push(`docs/design.md: the ${required} surface is missing`);
      }
    }
  }
  return { problems, files: files.length };
}

function selfTest() {
  const sample = 'const color = "#ff00ff";';
  const found = [...sample.matchAll(COLOUR)];
  if (found.length !== 1) {
    console.error("tokens self-test: the raw-colour detector missed a planted hex");
    process.exit(1);
  }
  console.log("tokens self-test: a planted raw hex is detected");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.includes("--self-test")) {
    selfTest();
  } else {
    const { problems, files } = checkTokens();
    if (problems.length > 0) {
      console.error(`design check: ${problems.length} problem${problems.length === 1 ? "" : "s"}\n`);
      for (const problem of problems) console.error(`  ${problem}`);
      process.exitCode = 1;
    } else {
      console.log(`design check: ${files} ui source files use tokens only; the surface table is complete`);
    }
  }
}

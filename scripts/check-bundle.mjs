#!/usr/bin/env node
// Inspect a support bundle without a GUI (roadmap 12's acceptance check).
//
// Usage: node scripts/check-bundle.mjs out.zip [--secret <value>]
// Prints every entry, fails when a required entry is missing, and fails when a secret value
// appears in any decompressed entry.

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const args = process.argv.slice(2);
const bundle = args.find((arg) => !arg.startsWith("--"));
const secretIndex = args.indexOf("--secret");
const secret = secretIndex >= 0 ? args[secretIndex + 1] : null;
if (!bundle) {
  console.error("usage: node scripts/check-bundle.mjs <bundle.zip> [--secret <value>]");
  process.exit(2);
}

const required = ["README.txt", "manifest.json", "doctor.json", "logs/events.log", "omitted.txt"];
const dir = mkdtempSync(path.join(tmpdir(), "rigel-bundle-"));
try {
  if (process.platform === "win32") {
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-Command",
        `Expand-Archive -LiteralPath ${quote(path.resolve(bundle))} -DestinationPath ${quote(dir)} -Force`,
      ],
      { stdio: "inherit" },
    );
  } else {
    execFileSync("unzip", ["-q", bundle, "-d", dir], { stdio: "inherit" });
  }

  const names = [];
  const walk = (current, prefix = "") => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) walk(path.join(current, entry.name), rel);
      else names.push(rel);
    }
  };
  walk(dir);
  names.sort();
  console.log(`entries: ${names.join(", ")}`);
  for (const wanted of required) {
    if (!names.includes(wanted)) {
      console.error(`missing required entry: ${wanted}`);
      process.exit(1);
    }
  }

  const manifest = JSON.parse(readFileSync(path.join(dir, "manifest.json"), "utf8"));
  console.log(
    `manifest: rigel ${manifest.rigel}, ${manifest.os}/${manifest.arch}, ${manifest.diagnostic_ring_events} ring events`,
  );
  const events = readFileSync(path.join(dir, "logs/events.log"), "utf8");
  console.log(`events: ${events.split("\n").filter(Boolean).length} lines`);
  const omitted = readFileSync(path.join(dir, "omitted.txt"), "utf8");
  console.log(`omitted: ${omitted.split("\n").filter((line) => line.startsWith("- ")).length} entries`);

  if (secret) {
    for (const name of names) {
      if (readFileSync(path.join(dir, name)).includes(Buffer.from(secret))) {
        console.error(`secret found in ${name}`);
        process.exit(1);
      }
    }
    console.log("secret: absent from every entry");
  }
} finally {
  rmSync(dir, { recursive: true, force: true });
}

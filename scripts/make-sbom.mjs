#!/usr/bin/env node
/**
 * Generate a CycloneDX 1.5 SBOM from the two real dependency graphs.
 *
 * Cargo metadata is authoritative for the Rust workspace; `ui/package-lock.json` is authoritative
 * for the UI. This is deliberately a generator, not a committed snapshot: the release workflow
 * builds it from the tagged tree and attaches it to the release.
 *
 * Usage: node scripts/make-sbom.mjs [output]
 */

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.resolve(ROOT, process.argv[2] ?? "sbom.cdx.json");

function workspaceVersion() {
  const cargo = readFileSync(path.join(ROOT, "Cargo.toml"), "utf8");
  return /\[workspace\.package\][\s\S]*?\nversion = "([^"]+)"/.exec(cargo)?.[1] ?? "0.0.0";
}

function licenseEntry(value) {
  if (!value || typeof value !== "string") return undefined;
  return [{ license: { name: value } }];
}

const components = new Map();

function add(name, version, purl, license, scope) {
  if (!name || !version || components.has(purl)) return;
  const component = {
    type: "library",
    name,
    version,
    purl,
    scope: scope ?? "required",
  };
  const licenses = licenseEntry(license);
  if (licenses) component.licenses = licenses;
  components.set(purl, component);
}

const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--format-version", "1", "--locked"], {
    cwd: ROOT,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  }),
);

for (const pkg of metadata.packages ?? []) {
  add(
    pkg.name,
    pkg.version,
    `pkg:cargo/${encodeURIComponent(pkg.name)}@${encodeURIComponent(pkg.version)}`,
    pkg.license,
    "required",
  );
}

const npmLock = path.join(ROOT, "ui", "package-lock.json");
if (existsSync(npmLock)) {
  const lock = JSON.parse(readFileSync(npmLock, "utf8"));
  for (const [key, meta] of Object.entries(lock.packages ?? {})) {
    if (!key.startsWith("node_modules/") || !meta?.version || meta.link) continue;
    const name = meta.name ?? key.split("node_modules/").filter(Boolean).pop();
    add(
      name,
      meta.version,
      `pkg:npm/${encodeURIComponent(name)}@${encodeURIComponent(meta.version)}`,
      meta.license,
      meta.dev ? "excluded" : "required",
    );
  }
}

const version = workspaceVersion();
const bom = {
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  version: 1,
  metadata: {
    timestamp: new Date().toISOString(),
    tools: {
      components: [
        {
          type: "application",
          name: "rigel-sbom",
          version,
        },
      ],
    },
    component: {
      type: "application",
      name: "Rigel",
      version,
    },
  },
  components: [...components.values()].sort((left, right) => left.purl.localeCompare(right.purl)),
};

writeFileSync(output, `${JSON.stringify(bom, null, 2)}\n`, "utf8");
console.log(`sbom: ${bom.components.length} components -> ${path.relative(ROOT, output)}`);

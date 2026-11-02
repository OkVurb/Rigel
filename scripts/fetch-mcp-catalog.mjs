#!/usr/bin/env node
// Build one MCP catalogue from the registries that publish machine-readable data.
//
// Three sources, merged and de-duplicated:
//
//   1. The official MCP registry  — keyless, authoritative, but no grand total and no repository
//      URL on remote-only servers.
//   2. Smithery                   — keyless, ~20k servers, description and homepage but no repo.
//   3. punkpeye/awesome-mcp-servers (MIT) — repo URLs on essentially every row, which is exactly
//      what the other two lack.
//
// Nothing here is invented: a row only exists because one of those sources listed it. Each row
// keeps the source it came from so the app can say where a listing originated and link back.
//
// usage:
//   node scripts/fetch-mcp-catalog.mjs                # full sweep
//   node scripts/fetch-mcp-catalog.mjs --max-pages 5  # quick, for a smoke test
//   node scripts/fetch-mcp-catalog.mjs --out tmp/x.json

import fs from "node:fs";
import path from "node:path";

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i === -1 ? fallback : args[i + 1];
};

const OUT = flag("out", "data/mcp-catalog.json");
const MAX_PAGES = Number(flag("max-pages", "400"));
const DELAY_MS = Number(flag("delay", "120"));

const UA = "rigel-catalog/1.0 (+https://github.com/OkVurb/Rigel)";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function getJson(url, tries = 2) {
  for (let attempt = 1; attempt <= tries; attempt += 1) {
    try {
      const response = await fetch(url, {
        headers: { Accept: "application/json", "User-Agent": UA },
        signal: AbortSignal.timeout(30000),
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return await response.json();
    } catch (error) {
      if (attempt === tries) throw error;
      await sleep(600 * attempt);
    }
  }
}

async function getText(url, tries = 2) {
  for (let attempt = 1; attempt <= tries; attempt += 1) {
    try {
      const response = await fetch(url, {
        headers: { "User-Agent": UA },
        signal: AbortSignal.timeout(30000),
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return await response.text();
    } catch (error) {
      if (attempt === tries) throw error;
      await sleep(600 * attempt);
    }
  }
}

// Trim the long tail of whitespace/markdown so the catalogue stays readable in a UI row.
const clean = (value, max = 320) => {
  if (!value) return "";
  const text = String(value)
    .replace(/<[^>]+>/g, " ")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/\*\*|__|`/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text;
};

const repoFromUrl = (url) => {
  if (!url) return "";
  const match = String(url).match(/github\.com\/([^/\s]+)\/([^/\s#?]+)/i);
  return match ? `${match[1]}/${match[2].replace(/\.git$/i, "")}`.toLowerCase() : "";
};

// The awesome list pads every row with a Glama score badge: `[![alt](badge.svg)](link)`. Left
// alone, its alt text bleeds into the description, so badges and stray markers come out first and
// the real description is whatever follows the final ` - ` separator.
function awesomeDescription(rest) {
  let text = rest
    .replace(/\[!\[[^\]]*\]\([^)]*\)\]\([^)]*\)/g, " ")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[\p{Extended_Pictographic}\u{FE0F}\u{200D}]/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
  const cut = text.lastIndexOf(" - ");
  if (cut !== -1) text = text.slice(cut + 3);
  return clean(text.replace(/^[\s\-–—:|]*/, ""));
}

const rows = new Map(); // dedupe key -> row

function add(row) {
  const repo = row.repo ? row.repo.toLowerCase() : "";
  const key = repo || `${row.source}:${row.name.toLowerCase()}`;
  const existing = rows.get(key);
  if (!existing) {
    rows.set(key, { ...row, repo });
    return;
  }
  // Merge: keep the earliest source's identity, fill blanks from the newcomer.
  for (const field of ["description", "homepage", "repo", "transport", "install", "verified"]) {
    if (!existing[field] && row[field]) existing[field] = row[field];
  }
  existing.sources = [...new Set([...(existing.sources || [existing.source]), row.source])];
}

// ---------------------------------------------------------------- official registry

async function officialRegistry() {
  let cursor;
  let pages = 0;
  let seen = 0;
  while (pages < MAX_PAGES) {
    const url = `https://registry.modelcontextprotocol.io/v0/servers?version=latest&limit=100${
      cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""
    }`;
    const page = await getJson(url);
    const servers = page?.servers ?? [];
    for (const entry of servers) {
      const server = entry.server ?? entry;
      const name = server.name || server.title;
      if (!name) continue;
      const remote = (server.remotes ?? [])[0];
      const pkg = (server.packages ?? [])[0];
      add({
        id: `registry:${name}`,
        name: clean(server.title || name, 120),
        qualified: name,
        description: clean(server.description),
        repo: repoFromUrl(server.repository?.url),
        homepage: server.repository?.url || remote?.url || "",
        source: "registry",
        transport: remote?.type || (pkg ? "package" : ""),
        install: pkg ? [pkg.registryType, pkg.identifier].filter(Boolean).join(":") : "",
        version: server.version || "",
      });
      seen += 1;
    }
    pages += 1;
    cursor = page?.metadata?.nextCursor;
    if (!cursor || servers.length === 0) break;
    await sleep(DELAY_MS);
  }
  return seen;
}

// ---------------------------------------------------------------- smithery

async function smithery() {
  let page = 1;
  let seen = 0;
  let totalPages = 1;
  while (page <= totalPages && page <= MAX_PAGES) {
    const body = await getJson(
      `https://registry.smithery.ai/servers?pageSize=100&page=${page}`,
    );
    const list = body?.servers ?? [];
    totalPages = Number(body?.pagination?.totalPages ?? 1);
    for (const server of list) {
      const name = server.displayName || server.qualifiedName;
      if (!name) continue;
      add({
        id: `smithery:${server.qualifiedName || server.id}`,
        name: clean(name, 120),
        qualified: server.qualifiedName || "",
        description: clean(server.description),
        repo: repoFromUrl(server.homepage || ""),
        homepage: server.homepage || "",
        source: "smithery",
        transport: server.remote ? "remote" : "",
        install: "",
        version: "",
        verified: Boolean(server.verified),
        uses: Number(server.useCount || 0) || 0,
      });
      seen += 1;
    }
    if (list.length === 0) break;
    page += 1;
    await sleep(DELAY_MS);
  }
  return seen;
}

// ---------------------------------------------------------------- awesome list (MIT)

async function awesomeList() {
  const text = await getText(
    "https://raw.githubusercontent.com/punkpeye/awesome-mcp-servers/main/README.md",
  );
  let seen = 0;
  for (const line of text.split(/\r?\n/)) {
    // "- [owner/repo](https://github.com/owner/repo) 🐍 ☁️ - description"
    const match = line.match(/^\s*[-*]\s*\[([^\]]+)\]\((https?:\/\/[^)]+)\)\s*(.*)$/);
    if (!match) continue;
    const [, label, url, rest] = match;
    const repo = repoFromUrl(url);
    if (!repo) continue;
    const description = awesomeDescription(rest);
    add({
      id: `awesome:${repo}`,
      name: clean(label, 120),
      qualified: repo,
      description,
      repo,
      homepage: url,
      source: "awesome",
      transport: "",
      install: "",
      version: "",
    });
    seen += 1;
  }
  return seen;
}

// ---------------------------------------------------------------- run

const report = {};
const sources = [
  ["registry", officialRegistry],
  ["smithery", smithery],
  ["awesome", awesomeList],
];

for (const [name, run] of sources) {
  const started = Date.now();
  try {
    report[name] = { rows: await run(), seconds: 0 };
    report[name].seconds = Math.round((Date.now() - started) / 1000);
    console.log(`${name.padEnd(9)} ${String(report[name].rows).padStart(6)} rows in ${report[name].seconds}s`);
  } catch (error) {
    report[name] = { rows: 0, error: String(error.message || error) };
    console.error(`${name.padEnd(9)} failed: ${error.message || error}`);
  }
}

const catalog = [...rows.values()]
  .map((row) => ({
    id: row.id,
    name: row.name,
    qualified: row.qualified || "",
    description: row.description,
    repo: row.repo || "",
    homepage: row.homepage || "",
    source: row.source,
    sources: row.sources || [row.source],
    transport: row.transport || "",
    install: row.install || "",
    version: row.version || "",
    ...(row.verified ? { verified: true } : {}),
    ...(row.uses ? { uses: row.uses } : {}),
  }))
  .sort((a, b) => a.name.localeCompare(b.name));

const payload = {
  version: 1,
  generatedAt: new Date().toISOString(),
  counts: {
    unique: catalog.length,
    withRepo: catalog.filter((row) => row.repo).length,
    withDescription: catalog.filter((row) => row.description).length,
    bySource: Object.fromEntries(
      sources.map(([name]) => [name, catalog.filter((row) => row.sources.includes(name)).length]),
    ),
  },
  servers: catalog,
};

fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, `${JSON.stringify(payload, null, 0)}\n`, "utf8");

const size = fs.statSync(OUT).size;
console.log(
  `\nwrote ${OUT} — ${catalog.length} unique servers (${payload.counts.withRepo} with a repo), ${(size / 1048576).toFixed(2)} MB`,
);

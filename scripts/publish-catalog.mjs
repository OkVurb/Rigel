#!/usr/bin/env node
// Publish the local shelf (and the MCP catalogue) to the project's Realtime Database.
//
// Why a shared catalogue at all: every user currently has to crawl GitHub themselves to fill
// index.db. The bodies are the expensive part and they stay local — what goes up is metadata only,
// a few hundred bytes per row, so a fresh install can read the catalogue instead of rebuilding it.
//
// Writes go through `firebase database:update` because the CLI already holds the admin session;
// the database rules allow public reads of /catalog and no public writes.
//
// usage:
//   node scripts/publish-catalog.mjs --dry-run
//   node scripts/publish-catalog.mjs                     # skills + mcps
//   node scripts/publish-catalog.mjs --skills-only
//   node scripts/publish-catalog.mjs --chunk 500

import fs from "node:fs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";

const args = process.argv.slice(2);
const has = (name) => args.includes(`--${name}`);
const value = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i === -1 ? fallback : args[i + 1];
};

const PROJECT = value("project", "rigel-25128");
const INSTANCE = value("instance", `${PROJECT}-default-rtdb`);
const CHUNK = Number(value("chunk", "800"));
const DRY = has("dry-run");
const SKILLS_ONLY = has("skills-only");
const MCPS_ONLY = has("mcps-only");
const LIMIT = Number(value("limit", "0")) || Infinity;

const DATA_DIR = process.env.RIGEL_INDEX_DIR || `${process.env.APPDATA}/Rigel`;
const INDEX_DB = `${DATA_DIR}/index.db`;
const MCP_FILE = value("mcps", "data/mcp-catalog.json");

// RTDB keys forbid . # $ [ ] / and control characters; a hash keeps them short, stable and unique.
const key = (id) => createHash("sha256").update(String(id)).digest("hex").slice(0, 20);
const clip = (text, max) => {
  if (text === null || text === undefined) return "";
  const s = String(text).replace(/\s+/g, " ").trim();
  return s.length > max ? `${s.slice(0, max - 1)}…` : s;
};

function readSkills() {
  if (!fs.existsSync(INDEX_DB)) {
    console.error(`no index at ${INDEX_DB}; build one with scripts/refresh-skills.sh`);
    return [];
  }
  const db = new DatabaseSync(INDEX_DB, { readOnly: true });
  const rows = db
    .prepare(
      `select id, name, description, source, repo, path, url, grade, stars, installs, tokens,
              cluster_id, last_seen_at
         from skills
        order by name collate nocase`,
    )
    .all();
  db.close();

  // Short field names: 11k rows of long keys is megabytes of nothing.
  return rows.slice(0, LIMIT).map((row) => ({
    k: key(row.id),
    v: {
      i: row.id,
      n: clip(row.name, 120),
      d: clip(row.description, 300),
      s: row.source || "",
      r: clip(row.repo, 140),
      p: clip(row.path, 240),
      u: clip(row.url, 300) || "",
      g: row.grade || "",
      st: row.stars || 0,
      in: row.installs || 0,
      tk: row.tokens || 0,
      c: row.cluster_id || "",
      t: row.last_seen_at || 0,
    },
  }));
}

function readMcps() {
  if (!fs.existsSync(MCP_FILE)) {
    console.error(`no MCP catalogue at ${MCP_FILE}; run scripts/fetch-mcp-catalog.mjs`);
    return { rows: [], meta: null };
  }
  const payload = JSON.parse(fs.readFileSync(MCP_FILE, "utf8"));
  const rows = (payload.servers || []).slice(0, LIMIT).map((server) => ({
    k: key(server.id),
    v: {
      i: server.id,
      n: clip(server.name, 120),
      d: clip(server.description, 300),
      r: clip(server.repo, 140),
      h: clip(server.homepage, 300),
      s: server.source,
      src: (server.sources || []).join(","),
      tr: server.transport || "",
      ins: clip(server.install, 160),
      v: server.version || "",
      ...(server.verified ? { ver: 1 } : {}),
      ...(server.uses ? { uses: server.uses } : {}),
    },
  }));
  return { rows, meta: payload.counts || null };
}

function writeChunk(path, entries) {
  const object = {};
  for (const entry of entries) object[`${path}/${entry.k}`] = entry.v;
  const file = `tmp/catalog-chunk-${process.pid}.json`;
  fs.mkdirSync("tmp", { recursive: true });
  fs.writeFileSync(file, JSON.stringify(object), "utf8");
  const result = spawnSync(
    process.platform === "win32" ? "npx.cmd" : "npx",
    [
      "--yes",
      "firebase-tools",
      "database:update",
      "/",
      file,
      "--force",
      "--instance",
      INSTANCE,
      "--project",
      PROJECT,
    ],
    { encoding: "utf8", shell: process.platform === "win32" },
  );
  fs.rmSync(file, { force: true });
  if (result.status !== 0) {
    throw new Error((result.stderr || result.stdout || "database:update failed").trim().slice(0, 400));
  }
}

async function publish(label, path, entries) {
  if (entries.length === 0) {
    console.log(`${label}: nothing to publish`);
    return 0;
  }
  console.log(`${label}: ${entries.length} rows in ${Math.ceil(entries.length / CHUNK)} chunks`);
  if (DRY) return entries.length;

  let sent = 0;
  for (let i = 0; i < entries.length; i += CHUNK) {
    const slice = entries.slice(i, i + CHUNK);
    writeChunk(path, slice);
    sent += slice.length;
    process.stdout.write(`\r  ${label}: ${sent}/${entries.length}`);
  }
  process.stdout.write("\n");
  return sent;
}

const skillEntries = MCPS_ONLY ? [] : readSkills();
const mcpEntries = SKILLS_ONLY ? [] : readMcps().rows;

const published = {
  skills: await publish("skills", "catalog/v1/skills", skillEntries),
  mcps: await publish("mcps", "catalog/v1/mcps", mcpEntries),
};

// A partial run (`--skills-only`, `--mcps-only`) must not zero the half it did not touch, so the
// published counts are read back and merged rather than assumed.
async function readPublishedMeta() {
  try {
    const response = await fetch(`https://${INSTANCE}.firebaseio.com/catalog/v1/meta.json`, {
      signal: AbortSignal.timeout(15000),
    });
    if (!response.ok) return {};
    return (await response.json()) || {};
  } catch {
    return {};
  }
}

const existing = await readPublishedMeta();
const meta = {
  schema: 1,
  generatedAt: new Date().toISOString(),
  skills: published.skills || Number(existing.skills || 0),
  mcps: published.mcps || Number(existing.mcps || 0),
  skillSources:
    skillEntries.length > 0
      ? [...new Set(skillEntries.map((e) => e.v.s))].filter(Boolean).sort()
      : existing.skillSources || [],
  note: "Metadata only. Bodies stay in each user's local index.",
};

if (!DRY) {
  const file = "tmp/catalog-meta.json";
  fs.writeFileSync(file, JSON.stringify({ "catalog/v1/meta": meta }), "utf8");
  const result = spawnSync(
    process.platform === "win32" ? "npx.cmd" : "npx",
    ["--yes", "firebase-tools", "database:update", "/", file, "--force", "--instance", INSTANCE, "--project", PROJECT],
    { encoding: "utf8", shell: process.platform === "win32" },
  );
  fs.rmSync(file, { force: true });
  if (result.status !== 0) console.error((result.stderr || result.stdout || "").trim().slice(0, 300));
}

console.log(
  `\n${DRY ? "would publish" : "published"}: ${published.skills} skills, ${published.mcps} MCP servers`,
);
console.log(`read it at https://${INSTANCE}.firebaseio.com/catalog/v1/meta.json`);

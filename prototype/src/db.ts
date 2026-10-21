import type { Grade, Package } from "./data";

/**
 * Local metadata index. Bodies are deliberately not stored — only what the list
 * needs to render and what an agent needs to decide whether to look closer:
 * name, description, source URL, grade, stats.
 *
 * localStorage, v1. Phase 1 replaces this with SQLite in the Tauri shell; the
 * record shape is the migration contract.
 */

const ENTRIES_KEY = "skilldb:entries:v1";
const CATALOGUE_KEY = "skilldb:catalogue:v1";
const MAX_ENTRIES = 2000;

export interface DbEntry {
  key: string;
  name: string;
  description: string;
  source: string;
  url: string;
  grade?: Grade;
  stars?: number;
  installs?: string;
  path?: string;
  savedAt: number;
}

function read<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
    return true;
  } catch {
    return false;
  }
}

export function saveEntries(incoming: DbEntry[]): { added: number; total: number } {
  if (incoming.length === 0) return { added: 0, total: allEntries().length };
  const existing = new Map(allEntries().map((e) => [e.key, e]));
  let added = 0;
  for (const entry of incoming) {
    if (!existing.has(entry.key)) added += 1;
    existing.set(entry.key, entry);
  }
  const merged = [...existing.values()]
    .sort((a, b) => b.savedAt - a.savedAt)
    .slice(0, MAX_ENTRIES);
  write(ENTRIES_KEY, merged);
  return { added, total: merged.length };
}

export function allEntries(): DbEntry[] {
  return read<DbEntry[]>(ENTRIES_KEY, []);
}

export function entryCount(): number {
  return allEntries().length;
}

/** Plain-text lookup for an agent: "what do I have that mentions pdf?" */
export function queryEntries(query: string, limit = 20): DbEntry[] {
  const q = query.trim().toLowerCase();
  const all = allEntries();
  if (!q) return all.slice(0, limit);
  return all
    .filter((e) => e.name.toLowerCase().includes(q) || e.description.toLowerCase().includes(q) || e.source.toLowerCase().includes(q))
    .slice(0, limit);
}

const pad = (s: string, n: number) => (s.length >= n ? s : s + " ".repeat(n - s.length));

/** The whole index as text, for an agent to read or diff. */
export function entriesText(entries: DbEntry[] = allEntries()): string {
  const header = [
    "# Skill index",
    `generated: ${new Date().toISOString()}`,
    `entries: ${entries.length}`,
    "#",
    "# grade  name  source  description",
  ].join("\n");
  const rows = entries.map(
    (e) => `${pad(e.grade ?? "?", 5)} ${pad(e.name, 28)} ${pad(e.source, 24)} ${e.description.replace(/\s+/g, " ").slice(0, 140)}`,
  );
  return `${header}\n${rows.join("\n")}\n`;
}

export function saveCatalogue(packages: Package[]) {
  write(CATALOGUE_KEY, packages.filter((p) => p.source));
}

export function loadCatalogue(): Package[] {
  return read<Package[]>(CATALOGUE_KEY, []);
}

export function dbStats(lastSync: string | null) {
  const entries = allEntries();
  const raw = (() => {
    try {
      return (localStorage.getItem(ENTRIES_KEY)?.length ?? 0) + (localStorage.getItem(CATALOGUE_KEY)?.length ?? 0);
    } catch {
      return 0;
    }
  })();
  return {
    entries: entries.length,
    lastSync: lastSync ?? (entries[0] ? new Date(entries[0].savedAt).toISOString().slice(0, 16).replace("T", " ") : null),
    kb: Math.round(raw / 1024),
  };
}

export function clearDb() {
  try {
    localStorage.removeItem(ENTRIES_KEY);
    localStorage.removeItem(CATALOGUE_KEY);
  } catch {
    /* ignore */
  }
}

/** Trigger a browser download for a text payload. */
export function downloadText(filename: string, text: string, type = "text/plain;charset=utf-8") {
  const blob = new Blob([text], { type });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

const bullets = (items: string[]) => items.map((i) => `- ${i}`).join("\n");

/** A self-contained brief an agent can act on, no UI required. */
export function agentBrief(pkg: Package): string {
  const lines: string[] = [
    `# ${pkg.name}`,
    `${pkg.kind} · ${pkg.id}`,
    "",
    pkg.description,
    "",
    "## Safety",
    `grade: ${pkg.grade}`,
    pkg.capabilities.length
      ? bullets(
          pkg.capabilities.map(
            (c) =>
              `[${c.severity}] ${c.label}\n${c.evidence.map((e) => `    ${e.file}:${e.line}  ${e.code}`).join("\n")}`,
          ),
        )
      : "- no capabilities recorded",
    "",
  ];

  if (pkg.evidence?.length) {
    lines.push("## Measured vs advertised", pkg.evidenceSource ?? "", "");
    for (const e of pkg.evidence) {
      lines.push(`- ${e.metric}: measured ${e.measured}, advertised ${e.advertised}`);
    }
    lines.push("");
  }

  lines.push("## Install", `- source: ${pkg.source?.url ?? pkg.id}`, "- reviewed file: SKILL.md only", "", "## Summary");
  for (const block of pkg.doc) {
    if ("h" in block) lines.push(`### ${block.h}`);
    else if ("p" in block) lines.push(block.p);
    else lines.push(...block.ul.map((li) => `- ${li}`));
  }
  return `${lines.join("\n")}\n`;
}

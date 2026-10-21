import type { Capability, DocBlock, Grade, Package } from "./data";

/**
 * Real GitHub import. Everything here talks to the public API from the browser:
 *   api.github.com          repo metadata + one recursive tree
 *   raw.githubusercontent   SKILL.md bodies (not counted against API quota)
 *
 * Unauthenticated GitHub allows 60 requests/hour per IP; the tree call is 1,
 * every SKILL.md body is 1 raw fetch. We cap a repo import at 12 skills.
 */

const API = "https://api.github.com";
const RAW = "https://raw.githubusercontent.com";
const MAX_SKILLS = 12;

export interface RepoRef {
  owner: string;
  repo: string;
  ref?: string;
  path?: string;
}

export interface RepoMeta {
  fullName: string;
  branch: string;
  stars: number;
  license: string | null;
  pushedAt: string | null;
  htmlUrl: string;
}

export interface ImportedSkill {
  key: string;
  name: string;
  description: string;
  path: string;
  body: string;
  allowedTools: string[] | null;
  siblings: { path: string; size: number }[];
  url: string;
  grade: Grade;
  capabilities: Capability[];
  tokens: number;
  doc: DocBlock[];
}

export interface ImportResult {
  repo: RepoMeta;
  skills: ImportedSkill[];
  totalFound: number;
  truncatedTree: boolean;
}

export class GitHubError extends Error {
  status: number;
  rateLimited: boolean;
  resetAt: Date | null;
  constructor(message: string, status: number, resetAt: Date | null = null) {
    super(message);
    this.status = status;
    this.rateLimited = status === 403 || status === 429;
    this.resetAt = resetAt;
  }
}

export function parseRepoInput(raw: string): RepoRef | null {
  const input = raw.trim().replace(/\.git$/, "").replace(/\/+$/, "");
  if (!input) return null;

  // bare owner/repo, optionally with a path
  const bare = input.match(/^([\w.-]+)\/([\w.-]+)(?:\/(?:tree|blob)\/([^/]+)\/(.+))?$/);
  if (bare && !/^https?:/i.test(input)) {
    return { owner: bare[1], repo: bare[2], ref: bare[3], path: bare[4] };
  }

  let url: URL;
  try {
    url = new URL(input);
  } catch {
    return null;
  }
  if (!/(^|\.)github\.com$/i.test(url.hostname)) return null;

  const parts = url.pathname.split("/").filter(Boolean);
  if (parts.length < 2) return null;
  const [owner, repo, kind, ref, ...rest] = parts;

  if ((kind === "tree" || kind === "blob") && ref && rest.length) {
    let path = rest.join("/");
    // a pasted file URL should collapse to its directory
    if (kind === "blob" || /\.mdx?$/i.test(path)) path = path.replace(/\/[^/]*\.mdx?$/i, "");
    return { owner, repo, ref, path: path || undefined };
  }

  return { owner, repo };
}

async function ghFetch(url: string, accept = "application/vnd.github+json") {
  let res: Response;
  try {
    res = await fetch(url, { headers: { Accept: accept } });
  } catch {
    throw new GitHubError("Could not reach api.github.com — check the network.", 0);
  }
  if (!res.ok) {
    const reset = res.headers.get("x-ratelimit-reset");
    const detail =
      res.status === 404
        ? "Repository not found (or it is private)."
        : res.status === 403 || res.status === 429
          ? "GitHub rate limit reached for this browser (60 requests/hour unauthenticated)."
          : `GitHub responded ${res.status}.`;
    throw new GitHubError(detail, res.status, reset ? new Date(Number(reset) * 1000) : null);
  }
  return res;
}

function unquote(v: string) {
  const m = v.match(/^(['"])([\s\S]*)\1$/);
  return m ? m[2] : v;
}

export function parseFrontmatter(src: string): { data: Record<string, string>; body: string } {
  const m = src.replace(/^\uFEFF/, "").match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/);
  if (!m) return { data: {}, body: src };

  const data: Record<string, string> = {};
  let key: string | null = null;
  let buf: string[] = [];

  const flush = () => {
    if (key) data[key] = buf.join(" ").replace(/\s+/g, " ").trim();
    key = null;
    buf = [];
  };

  for (const line of m[1].split(/\r?\n/)) {
    const kv = line.match(/^([A-Za-z0-9_-]+):\s*(.*)$/);
    if (kv) {
      flush();
      const [, k, rawValue] = kv;
      const v = rawValue.trim();
      if (v === ">" || v === "|" || v === "") key = k;
      else data[k] = unquote(v);
      continue;
    }
    if (key && /^\s*-\s+/.test(line)) {
      buf.push(line.replace(/^\s*-\s+/, ""));
      continue;
    }
    if (key && /^\s+\S/.test(line)) {
      buf.push(line.trim());
      continue;
    }
    if (/^\s*#/.test(line) || line.trim() === "") continue;
    flush();
  }
  flush();

  return { data, body: m[2] };
}

/* ------------------------------------------------------------------ *
 * static capability scan — same rules as the indexer, single file only
 * ------------------------------------------------------------------ */

const RULES: { label: string; severity: "critical" | "high" | "low"; re: RegExp }[] = [
  {
    label: "Reads secrets and env files",
    severity: "critical",
    re: /(\.aws\/credentials|\.ssh\/id_|\.npmrc|process\.env\.[A-Z0-9_]+|\bcat\s+\.env\b|\.env\.local)/g,
  },
  {
    label: "Destructive command",
    severity: "critical",
    re: /\brm\s+-rf?\b|\bgit\s+reset\s+--hard\b|\bmkfs\b|\bDROP\s+TABLE\b/gi,
  },
  {
    label: "Privilege escalation",
    severity: "critical",
    re: /\bsudo\b|\bchmod\s+777\b|\bsetcap\b|\bdoas\b/gi,
  },
  {
    label: "Writes outside the project",
    severity: "high",
    re: /(>>\s*~\/|>\s*\/etc\/|~\/\.(bashrc|zshrc|profile)|>>\s*\/usr\/)/g,
  },
  {
    label: "Spawns processes",
    severity: "high",
    re: /\b(execSync|exec\(|spawn\(|child_process|subprocess\.(run|Popen)|os\.system)/g,
  },
  {
    label: "Network access",
    severity: "high",
    re: /\b(fetch\(|axios\.|curl\s|wget\s|requests\.(get|post)|https?:\/\/[a-z0-9.-]+\.[a-z]{2,})/gi,
  },
];

export function scanSkill(body: string, file: string): Capability[] {
  const lines = body.split(/\r?\n/);
  const caps: Capability[] = [];

  for (const rule of RULES) {
    const evidence: { file: string; line: number; code: string }[] = [];
    for (let i = 0; i < lines.length && evidence.length < 3; i++) {
      rule.re.lastIndex = 0;
      if (rule.re.test(lines[i])) {
        evidence.push({ file, line: i + 1, code: lines[i].trim().slice(0, 96) });
      }
    }
    if (evidence.length) caps.push({ label: rule.label, severity: rule.severity, evidence });
  }

  if (caps.length === 0) {
    caps.push({
      label: "No dangerous capabilities detected",
      severity: "low",
      evidence: [{ file, line: 1, code: "single markdown file — no scripts bundled" }],
    });
  }

  return caps;
}

export function gradeFor(caps: Capability[]): Grade {
  const critical = caps.filter((c) => c.severity === "critical").length;
  const high = caps.filter((c) => c.severity === "high").length;
  if (critical > 0) return "F";
  if (high >= 2) return "D";
  if (high === 1) return "C";
  return "B";
}

/* ------------------------------------------------------------------ *
 * markdown -> detail blocks
 * ------------------------------------------------------------------ */

export function toDocBlocks(body: string): DocBlock[] {
  const blocks: DocBlock[] = [];
  let list: string[] = [];
  const flush = () => {
    if (list.length) blocks.push({ ul: list });
    list = [];
  };

  for (const raw of body.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line === "---") {
      flush();
      continue;
    }
    const h = line.match(/^#{1,4}\s+(.*)$/);
    if (h) {
      flush();
      blocks.push({ h: h[1] });
      continue;
    }
    const li = line.match(/^[-*]\s+(.*)$/);
    if (li) {
      list.push(li[1].replace(/[*`]/g, ""));
      continue;
    }
    if (line.startsWith("```") || line.startsWith("|")) continue;
    flush();
    blocks.push({ p: line.replace(/[*`]/g, "") });
  }
  flush();

  // long prose is cut at paragraph granularity to keep the pane readable
  return blocks.slice(0, 14);
}

/* ------------------------------------------------------------------ *
 * repo import
 * ------------------------------------------------------------------ */

interface TreeEntry {
  path: string;
  type: string;
  size?: number;
}

export interface IndexEntry {
  key: string;
  repo: string;
  owner: string;
  name: string;
  path: string;
  branch: string;
  rawUrl: string;
  url: string;
  size: number;
  siblings: { path: string; size: number }[];
}

export interface RepoIndex {
  repo: RepoMeta;
  entries: IndexEntry[];
  totalFound: number;
  truncatedTree: boolean;
}

async function repoTree(ref: RepoRef) {
  const metaRes = await ghFetch(`${API}/repos/${ref.owner}/${ref.repo}`);
  const meta = await metaRes.json();
  const branch: string = ref.ref ?? meta.default_branch;
  const treeRes = await ghFetch(
    `${API}/repos/${ref.owner}/${ref.repo}/git/trees/${encodeURIComponent(branch)}?recursive=1`,
  );
  const treeJson = await treeRes.json();
  const entries: TreeEntry[] = Array.isArray(treeJson.tree) ? treeJson.tree : [];
  return { meta, branch, entries, truncated: Boolean(treeJson.truncated) };
}

function skillDirs(entries: TreeEntry[], ref: RepoRef) {
  let candidates = entries.filter(
    (e) => e.type === "blob" && (e.path === "SKILL.md" || e.path.endsWith("/SKILL.md")),
  );
  if (ref.path) {
    const prefix = ref.path.replace(/^\/+|\/+$/g, "");
    const scoped = candidates.filter((e) => e.path.startsWith(prefix));
    if (scoped.length) candidates = scoped;
  }
  return candidates;
}

/**
 * Structure only: one repo call + one tree call. No SKILL.md bodies, so a
 * browse list costs 2 requests per repo instead of 2 + N.
 */
export async function fetchRepoIndex(input: string, limit = 40): Promise<RepoIndex> {
  const ref = parseRepoInput(input);
  if (!ref) throw new GitHubError("That does not look like a GitHub repo or URL.", 0);

  const { meta, branch, entries, truncated } = await repoTree(ref);
  const candidates = skillDirs(entries, ref);
  if (candidates.length === 0) throw new GitHubError("No SKILL.md found in that repo or directory.", 0);

  const picked = candidates.slice(0, limit).map((entry) => {
    const dir = entry.path.includes("/") ? entry.path.slice(0, entry.path.lastIndexOf("/")) : "";
    const siblings = entries
      .filter((e) => e.type === "blob" && (dir === "" ? !e.path.includes("/") : e.path.startsWith(`${dir}/`)))
      .slice(0, 8)
      .map((e) => ({ path: dir === "" ? e.path : e.path.slice(dir.length + 1), size: e.size ?? 0 }));

    return {
      key: `${ref.owner}/${ref.repo}:${entry.path}`,
      repo: meta.full_name,
      owner: ref.owner,
      name: (dir.split("/").pop() || ref.repo).toLowerCase(),
      path: entry.path,
      branch,
      rawUrl: `${RAW}/${ref.owner}/${ref.repo}/${encodeURI(branch)}/${entry.path}`,
      url: `https://github.com/${ref.owner}/${ref.repo}/blob/${branch}/${entry.path.split("/").map(encodeURIComponent).join("/")}`,
      size: entry.size ?? 0,
      siblings,
    } satisfies IndexEntry;
  });

  return {
    repo: {
      fullName: meta.full_name,
      branch,
      stars: meta.stargazers_count ?? 0,
      license: meta.license?.spdx_id ?? null,
      pushedAt: meta.pushed_at ?? null,
      htmlUrl: meta.html_url,
    },
    entries: picked,
    totalFound: candidates.length,
    truncatedTree: truncated,
  };
}

/** Body fetch for a single indexed skill — one raw request, no API quota. */
export async function loadSkill(entry: IndexEntry): Promise<ImportedSkill> {
  const res = await fetch(entry.rawUrl);
  if (!res.ok) throw new GitHubError(`Could not load ${entry.path} (${res.status}).`, res.status);
  const text = await res.text();
  const { data, body } = parseFrontmatter(text);
  const capabilities = scanSkill(body, entry.path);
  return {
    key: entry.key,
    name: (data.name || entry.name).trim(),
    description: (data.description || "No description in frontmatter.").slice(0, 240),
    path: entry.path,
    body,
    allowedTools: data["allowed-tools"] ? data["allowed-tools"].split(/[,\s]+/).filter(Boolean) : null,
    siblings: entry.siblings,
    url: entry.url,
    grade: gradeFor(capabilities),
    capabilities,
    tokens: Math.max(120, Math.round(body.length / 4)),
    doc: toDocBlocks(body),
  };
}

export async function fetchRepoSkills(input: string): Promise<ImportResult> {
  const ref = parseRepoInput(input);
  if (!ref) throw new GitHubError("That does not look like a GitHub repo or URL.", 0);

  const metaRes = await ghFetch(`${API}/repos/${ref.owner}/${ref.repo}`);
  const meta = await metaRes.json();
  const branch: string = ref.ref ?? meta.default_branch;

  const treeRes = await ghFetch(`${API}/repos/${ref.owner}/${ref.repo}/git/trees/${encodeURIComponent(branch)}?recursive=1`);
  const treeJson = await treeRes.json();
  const entries: TreeEntry[] = Array.isArray(treeJson.tree) ? treeJson.tree : [];

  let candidates = entries.filter(
    (e) => e.type === "blob" && (e.path === "SKILL.md" || e.path.endsWith("/SKILL.md")),
  );

  if (ref.path) {
    const prefix = ref.path.replace(/^\/+|\/+$/g, "");
    const scoped = candidates.filter((e) => e.path.startsWith(prefix));
    if (scoped.length) candidates = scoped;
  }

  if (candidates.length === 0) {
    throw new GitHubError("No SKILL.md found in that repo or directory.", 0);
  }

  const totalFound = candidates.length;
  const picked = candidates.slice(0, MAX_SKILLS);

  const fetched = await Promise.all(
    picked.map(async (entry) => {
      const rawUrl = `${RAW}/${ref.owner}/${ref.repo}/${encodeURI(branch)}/${entry.path}`;
      const res = await fetch(rawUrl);
      if (!res.ok) return null;
      const text = await res.text();
      const { data, body } = parseFrontmatter(text);
      const dir = entry.path.includes("/") ? entry.path.slice(0, entry.path.lastIndexOf("/")) : "";
      const siblings = entries
        .filter((e) => e.type === "blob" && (dir === "" ? !e.path.includes("/") : e.path.startsWith(`${dir}/`)))
        .slice(0, 8)
        .map((e) => ({
          path: dir === "" ? e.path : e.path.slice(dir.length + 1),
          size: e.size ?? 0,
        }));
      const capabilities = scanSkill(body, entry.path);
      const name = (data.name || dir.split("/").pop() || ref.repo).trim();
      return {
        key: `${ref.owner}/${ref.repo}:${entry.path}`,
        name,
        description: (data.description || "No description in frontmatter.").slice(0, 240),
        path: entry.path,
        body,
        allowedTools: data["allowed-tools"]
          ? data["allowed-tools"].split(/[,\s]+/).filter(Boolean)
          : null,
        siblings,
        url: `https://github.com/${ref.owner}/${ref.repo}/blob/${branch}/${entry.path.split("/").map(encodeURIComponent).join("/")}`,
        grade: gradeFor(capabilities),
        capabilities,
        tokens: Math.max(120, Math.round(body.length / 4)),
        doc: toDocBlocks(body),
      } satisfies ImportedSkill;
    }),
  );

  const skills = fetched.filter((s): s is ImportedSkill => s !== null);
  if (skills.length === 0) throw new GitHubError("Found SKILL.md files but could not load any of them.", 0);

  return {
    repo: {
      fullName: meta.full_name,
      branch,
      stars: meta.stargazers_count ?? 0,
      license: meta.license?.spdx_id ?? null,
      pushedAt: meta.pushed_at ?? null,
      htmlUrl: meta.html_url,
    },
    skills,
    totalFound,
    truncatedTree: Boolean(treeJson.truncated),
  };
}

const fmtBytes = (n: number) => (n >= 1024 ? `${(n / 1024).toFixed(1)} KB` : `${n} B`);

export function toPackage(skill: ImportedSkill, repo: RepoMeta): Package {
  return {
    id: `gh:${skill.key}`,
    name: skill.name,
    kind: "skill",
    author: repo.fullName.split("/")[0],
    description: skill.description,
    grade: skill.grade,
    installs: 0,
    stars: repo.stars,
    lastCommit: repo.pushedAt ? `pushed ${repo.pushedAt.slice(0, 10)}` : "unknown",
    license: repo.license,
    tokens: skill.tokens,
    capabilities: skill.capabilities,
    files: skill.siblings.map((s) => ({ path: s.path, size: fmtBytes(s.size) })),
    doc: [
      { p: "Imported from GitHub and scanned in the browser. The only file reviewed is SKILL.md — anything it tells the agent to run later is outside this review." },
      ...skill.doc,
    ],
    source: { repo: repo.fullName, branch: repo.branch, path: skill.path, url: skill.url },
    unverified: true,
  };
}

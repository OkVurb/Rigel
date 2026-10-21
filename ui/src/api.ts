/**
 * The only place the renderer talks to Rust.
 *
 * In a browser (`npm run dev`) there is no Tauri host, so these fall back to empty data and
 * the shell says so instead of pretending. In the app every call goes through `invoke`, and
 * the Rust core remains the single owner of state (handoff §3.2, S2).
 *
 * Field names are snake_case because they are the Rust structs verbatim: no mapping layer to
 * drift out of sync, and the same JSON the CLI prints under `--json`.
 */

export interface Session {
  id: string;
  project_root: string;
  title: string | null;
  status: "active" | "idle" | "interrupted" | "ended";
  budget_cents: number | null;
  spend_cents: number;
  interventions: number;
  tasks_done: number;
  created_at: number;
  ended_at: number | null;
}

export interface ProjectInfo {
  root: string;
  name: string;
  db: string;
}

export interface Settings {
  theme: string;
  default_budget_cents: number;
  unattended_default: boolean;
}

export type Grade = "A" | "B" | "C" | "D" | "F";

export interface Capability {
  label: string;
  severity: "critical" | "high" | "low";
  evidence: { file: string; line: number; code: string }[];
}

export interface SkillRow {
  id: string;
  name: string;
  description: string;
  source: string;
  repo: string | null;
  path: string | null;
  url: string | null;
  /** `null` means not scanned. The shelf prints that, never a guessed letter. */
  grade: Grade | null;
  grade_scope: string | null;
  stars: number | null;
  installs: number | null;
  tokens: number | null;
  cluster_id: string | null;
  /** Rows in this row's cluster, including itself. Never used to hide a row. */
  variants: number;
  last_seen_at: number | null;
  /** The description advertises a number nobody has measured (P2.7). */
  unmeasured: boolean;
  has_body: boolean;
}

export interface EvidenceRow {
  metric: string;
  measured: number | null;
  advertised: number | null;
  source: string;
  source_url: string | null;
  observed_at: number;
  /** "measured" or "unmeasured", derived in Rust from the row itself. */
  label: "measured" | "unmeasured";
}

export interface VariantRow {
  id: string;
  name: string;
  repo: string | null;
  grade: Grade | null;
  license: string | null;
  last_seen_at: number | null;
  delta: string;
}

export interface SkillDetail {
  skill: SkillRow;
  capabilities: Capability[];
  evidence: EvidenceRow[];
  variants: VariantRow[];
  first_seen_at: number | null;
}

export interface SourceStatus {
  id: string;
  base_url: string | null;
  state: "ok" | "stale" | "broken";
  last_sync_at: number | null;
  error: string | null;
}

export interface IngestReport {
  source: string;
  repo: string | null;
  branch: string | null;
  found: number;
  added: number;
  updated: number;
  unchanged: number;
  with_body: number;
  http_requests: number;
  truncated: boolean;
  clusters: number;
}

/** What the shelf needs from a source. `mock.ts` implements the same shape for previews. */
export interface SkillSource {
  readonly label: string;
  search(query: string, limit: number): Promise<SkillRow[]>;
  load(id: string): Promise<SkillDetail>;
  status(): Promise<SourceStatus[]>;
  indexRepo?(repo: string): Promise<IngestReport>;
}

export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

export const api = {
  isTauri,

  async openProject(path: string): Promise<ProjectInfo> {
    if (!isTauri()) {
      return { root: path, name: path.split(/[\\/]/).pop() || path, db: "(browser preview)" };
    }
    return call<ProjectInfo>("project_open", { path });
  },

  async listSessions(project: string): Promise<Session[]> {
    if (!isTauri()) return [];
    return call<Session[]>("session_list", { project });
  },

  async startSession(project: string, title?: string): Promise<Session> {
    if (!isTauri()) throw new Error("Start a session from the desktop app, not the browser preview.");
    return call<Session>("session_start", { project, title: title ?? null, budgetCents: null });
  },

  async getSettings(): Promise<Settings> {
    if (!isTauri()) {
      return { theme: "dark", default_budget_cents: 500, unattended_default: false };
    }
    return call<Settings>("settings_get");
  },
};

/** The real shelf: SQLite FTS5 and the blob store, through the Rust core. */
export class TauriSkillSource implements SkillSource {
  readonly label = "index.db";

  async search(query: string, limit = 100): Promise<SkillRow[]> {
    return call<SkillRow[]>("skills_search", { query, limit });
  }

  async load(id: string): Promise<SkillDetail> {
    return call<SkillDetail>("skills_load", { id });
  }

  async status(): Promise<SourceStatus[]> {
    return call<SourceStatus[]>("skills_status");
  }

  async indexRepo(repo: string): Promise<IngestReport> {
    return call<IngestReport>("skills_index", { repo, force: false });
  }
}

/** Timestamps are epoch milliseconds everywhere in Rigel; render them as UTC minutes. */
export function when(ms: number | null | undefined): string {
  if (!ms) return "never";
  return new Date(ms).toISOString().slice(0, 16).replace("T", " ");
}

export function compact(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}m`;
  if (value >= 1_000) return `${Math.round(value / 1_000)}k`;
  return String(value);
}

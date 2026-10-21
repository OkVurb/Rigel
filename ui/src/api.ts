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
  spend_micros: number;
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
  /** `omniroute` (local router) or `openrouter`. */
  provider: string;
  model: string;
}

/** Live state of the local OmniRoute router. `origin` is where it listens. */
export interface OmnirouteStatus {
  running: boolean;
  origin: string;
  base_url: string;
}

export interface Agent {
  id: string;
  session_id: string;
  harness: string;
  role: string | null;
  model: string | null;
  status: "starting" | "running" | "idle" | "exited" | "failed" | string;
  started_at: number;
  spend_cents: number;
  spend_micros: number;
  tokens_in: number;
  tokens_out: number;
}

export interface Usage {
  input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  /** Micro-dollars. This is the bill; cents are only for display. */
  cost_micros: number;
  provider_micros: number | null;
  source: "provider" | "estimated";
}

export interface Turn {
  id: string;
  agent_id: string;
  /** Set when a subagent ran this turn on behalf of a parent turn. */
  parent_turn_id: string | null;
  seq: number;
  status: "running" | "completed" | "failed" | "cancelled" | string;
  prompt: string | null;
  started_at: number;
  completed_at: number | null;
  usage: Usage | null;
  stop_reason: string | null;
}

export interface Item {
  id: string;
  turn_id: string;
  seq: number;
  kind: "message" | "reasoning" | "command" | "file_change" | "mcp_call" | "web_search" | "plan" | "approval" | "usage" | "error" | string;
  status: string;
  body: Record<string, unknown>;
  created_at: number;
  raw?: unknown;
}

export interface TurnTranscript {
  turn: Turn;
  items: Item[];
}

export interface BudgetSnapshot {
  session_id: string;
  agent_id: string;
  session_micros: number;
  agent_micros: number;
  cap_micros: number | null;
  remaining_micros: number | null;
  session_cents: number;
  agent_cents: number;
  cap_cents: number | null;
  remaining_cents: number | null;
}

export interface Patch {
  id: string;
  session_id: string;
  agent_id: string;
  path: string;
  base_hash: string;
  result_hash: string;
  diff: string;
  status: "proposed" | "applied" | "reverted" | "conflicted" | string;
  created_at: number;
  applied_at: number | null;
  reverted_at: number | null;
}

export interface Recovery {
  patch_id: string;
  path: string;
  state: "applied" | "pending" | "conflicted";
}

export interface ResumeOutcome {
  session: Session;
  clean: boolean;
  tree_hash: string;
  checkpoint: { id: string; tree_hash: string; created_at: number } | null;
  closed_turns: Turn[];
  patches: Recovery[];
  reconcile: {
    session_id: string;
    session_recorded_micros: number;
    session_summed_micros: number;
    ok: boolean;
  };
}

export interface TurnReply {
  turn: Turn;
  budget_cents: number | null;
}

/**
 * A pre-turn estimate from Rust: what the first model call should cost, and the headroom.
 * Advisory — `reserve` and the cap remain the authority.
 */
export interface TurnQuote {
  model: string;
  input_tokens: number;
  output_tokens: number;
  first_step_micros: number;
  spend_micros: number;
  cap_micros: number | null;
  remaining_micros: number | null;
  over_remaining: boolean;
  reserve_micros: number;
}

/**
 * Browser-preview prices only. The desktop app asks Rust, where `agent/pricing.rs` owns the
 * table; these numbers exist so the composer can be inspected without a Tauri host.
 */
const PREVIEW_PRICE = { input: 3, output: 15 }; // anthropic/claude-sonnet-4.5
const PREVIEW_CONTEXT_CHARS = 1_800; // system prompt + tool schemas, roughly

function previewQuote(prompt: string, spendMicros: number, capMicros: number | null): TurnQuote {
  const inputTokens = Math.ceil((PREVIEW_CONTEXT_CHARS + prompt.length) / 4);
  const outputTokens = Math.min(2_048, Math.max(256, Math.floor(inputTokens / 2)));
  const firstStep = Math.round(inputTokens * PREVIEW_PRICE.input + outputTokens * PREVIEW_PRICE.output);
  const remaining = capMicros === null ? null : capMicros - spendMicros;
  return {
    model: "anthropic/claude-sonnet-4.5",
    input_tokens: inputTokens,
    output_tokens: outputTokens,
    first_step_micros: firstStep,
    spend_micros: spendMicros,
    cap_micros: capMicros,
    remaining_micros: remaining,
    over_remaining: remaining !== null && firstStep > remaining,
    reserve_micros: 10_000,
  };
}

export interface KeyStatus {
  openrouter: boolean;
  omniroute: boolean;
  openai: boolean;
  anthropic: boolean;
  cursor: boolean;
}

/** One row of `adapters.json` (P4.1). */
export interface HarnessInfo {
  harness: "native" | "codex" | "claude" | "cursor" | "cline";
  installed: boolean;
  version: string | null;
  path: string | null;
  auth_mode: "api_key" | "subscription" | "none";
  capabilities: {
    acp: boolean;
    json_stream: boolean;
    programmatic_approval: boolean;
    worktree: boolean;
    nested_agents: boolean;
  };
  provider: string | null;
  install_hint: string | null;
}

/** Browser preview only: the shape of a real probe, with this project's recorded versions. */
const PREVIEW_HARNESSES: HarnessInfo[] = [
  {
    harness: "codex",
    installed: true,
    version: "0.162.1",
    path: "C:\\Program Files\\Codex\\codex.exe",
    auth_mode: "api_key",
    capabilities: { acp: false, json_stream: true, programmatic_approval: true, worktree: true, nested_agents: false },
    provider: "omniroute",
    install_hint: null,
  },
  {
    harness: "claude",
    installed: true,
    version: "2.1.251",
    path: "C:\\Users\\you\\.local\\bin\\claude.exe",
    auth_mode: "api_key",
    capabilities: { acp: false, json_stream: true, programmatic_approval: true, worktree: false, nested_agents: true },
    provider: "anthropic",
    install_hint: null,
  },
  {
    harness: "cursor",
    installed: false,
    version: null,
    path: null,
    auth_mode: "none",
    capabilities: { acp: true, json_stream: true, programmatic_approval: true, worktree: true, nested_agents: false },
    provider: "cursor",
    install_hint: "powershell -NoProfile -Command \"irm 'https://cursor.com/install?win32=true' | iex\"",
  },
  {
    harness: "cline",
    installed: false,
    version: null,
    path: null,
    auth_mode: "none",
    capabilities: { acp: true, json_stream: true, programmatic_approval: true, worktree: true, nested_agents: false },
    provider: null,
    install_hint: "npm install -g cline",
  },
];

/** The serialised `AgentEvent`; `type` is the discriminator. */
export type AgentEvent =
  | { type: "turn_started"; session_id: string; agent_id: string; turn_id: string; seq: number; prompt: string | null }
  | { type: "delta"; turn_id: string; kind: string; text: string }
  | { type: "item"; item: Item }
  | { type: "turn_finished"; turn_id: string; status: string; stop: string; usage: Usage; session_spend_micros: number; cap_micros: number | null }
  | { type: "budget_warning"; session_id: string; spend_micros: number; cap_micros: number; remaining_micros: number }
  | { type: "budget_exceeded"; session_id: string; spend_micros: number; cap_micros: number }
  | { type: "approval_requested"; approval_id: string; kind: string; detail: unknown }
  | { type: "approval_resolved"; approval_id: string; decision: string }
  | { type: "session_updated"; session: Session }
  | { type: "error"; turn_id: string | null; message: string };

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

  async startSession(project: string, title?: string, budgetCents?: number | null): Promise<Session> {
    if (!isTauri()) throw new Error("Start a session from the desktop app, not the browser preview.");
    return call<Session>("session_start", {
      project,
      title: title ?? null,
      budgetCents: budgetCents ?? null,
    });
  },

  async getSession(project: string, id: string): Promise<Session> {
    return call<Session>("session_get", { project, id });
  },

  async transcript(project: string, id: string): Promise<TurnTranscript[]> {
    return call<TurnTranscript[]>("session_transcript", { project, id });
  },

  async resumeSession(project: string, id: string, force: boolean): Promise<ResumeOutcome> {
    return call<ResumeOutcome>("session_resume", { project, id, force });
  },

  async listAgents(project: string, session: string): Promise<Agent[]> {
    return call<Agent[]>("agent_list", { project, session });
  },

  async spawnAgent(
    project: string,
    session: string,
    role?: string,
    model?: string,
  ): Promise<Agent> {
    return call<Agent>("agent_spawn", {
      project,
      session,
      harness: "native",
      role: role ?? null,
      model: model ?? null,
    });
  },

  async stopAgent(project: string, agent: string): Promise<void> {
    return call<void>("agent_stop", { project, agent });
  },

  async sendTurn(
    project: string,
    session: string,
    agent: string,
    prompt: string,
  ): Promise<TurnReply> {
    return call<TurnReply>("turn_send", { project, session, agent, prompt, model: null });
  },

  async cancelTurn(turn: string): Promise<void> {
    return call<void>("turn_cancel", { turn });
  },

  /**
   * Price the first step of a turn before it is sent. In the browser this is a local
   * heuristic over sample prices; in the app the quote comes from Rust, priced with the
   * same table that bills the step once it lands.
   */
  async quoteTurn(
    project: string,
    session: string,
    prompt: string,
    spendMicros: number,
    capMicros: number | null,
  ): Promise<TurnQuote> {
    if (!isTauri()) return previewQuote(prompt, spendMicros, capMicros);
    return call<TurnQuote>("turn_estimate", { project, session, prompt, model: null });
  },

  async getBudget(project: string, agent: string): Promise<BudgetSnapshot> {
    return call<BudgetSnapshot>("budget_get", { project, agent });
  },

  async setBudget(project: string, session: string, capCents: number | null): Promise<Session> {
    return call<Session>("budget_set", { project, session, capCents });
  },

  async raiseBudget(project: string, session: string, capCents: number): Promise<Session> {
    return call<Session>("budget_raise", { project, session, capCents });
  },

  async listPatches(project: string, session: string): Promise<Patch[]> {
    return call<Patch[]>("patch_list", { project, session });
  },

  async revertPatch(id: string): Promise<Patch> {
    return call<Patch>("patch_revert", { id });
  },

  /** Cached probe rows; `force` re-runs the binaries instead of trusting a fresh cache. */
  async harnessProbe(force = false): Promise<HarnessInfo[]> {
    if (!isTauri()) return PREVIEW_HARNESSES;
    return call<HarnessInfo[]>("harness_probe", { force });
  },

  async keyStatus(): Promise<KeyStatus> {
    if (!isTauri()) {
      return { openrouter: false, omniroute: false, openai: false, anthropic: false, cursor: false };
    }
    return call<KeyStatus>("key_status");
  },

  async setKey(provider: string, value: string): Promise<KeyStatus> {
    if (!isTauri()) {
      return { openrouter: true, omniroute: true, openai: true, anthropic: true, cursor: true };
    }
    return call<KeyStatus>("key_set", { provider, value });
  },

  async deleteKey(provider: string): Promise<KeyStatus> {
    if (!isTauri()) {
      return { openrouter: false, omniroute: false, openai: false, anthropic: false, cursor: false };
    }
    return call<KeyStatus>("key_delete", { provider });
  },

  async omnirouteStatus(): Promise<OmnirouteStatus> {
    if (!isTauri()) {
      return { running: true, origin: "http://127.0.0.1:20128", base_url: "(browser preview)" };
    }
    return call<OmnirouteStatus>("omniroute_status");
  },

  async omnirouteStart(): Promise<OmnirouteStatus> {
    if (!isTauri()) throw new Error("Start OmniRoute from the desktop app, not the browser preview.");
    return call<OmnirouteStatus>("omniroute_start");
  },

  async usage(project: string, session: string): Promise<Usage> {
    return call<Usage>("usage_summary", { project, session });
  },

  async getSettings(): Promise<Settings> {
    if (!isTauri()) {
      return {
        theme: "dark",
        default_budget_cents: 500,
        unattended_default: false,
        provider: "omniroute",
        model: "gpt-5.6-sol-medium",
      };
    }
    return call<Settings>("settings_get");
  },

  async saveSettings(settings: Settings): Promise<Settings> {
    if (!isTauri()) return settings;
    return call<Settings>("settings_set", { settings });
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

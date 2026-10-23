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

import { previewApprovals } from "./sessionMock";

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
  /** `omniroute` (local router), `openrouter`, or `gemini` (Google's API). */
  provider: string;
  model: string;
  /** Separate reasoning effort: low, medium or high. */
  effort: string;
  /** D90: per-event desktop notifications; no sound, and a watched turn stays silent. */
  notify_approval: boolean;
  notify_turn_done: boolean;
  notify_turn_failed: boolean;
}

/** D82: an opt-in, local-only diagnostics bundle. Nothing leaves the machine. */
export interface DiagnosticsBundle {
  path: string;
  body: string;
}

/** D89: a per-turn tree checkpoint. `turn_id` names the turn it followed. */
export interface Checkpoint {
  id: string;
  session_id: string;
  turn_id: string | null;
  tree_hash: string;
  created_at: number;
}

/** D95: one Markdown session bundle, with its JSON beside it. */
export interface SessionExport {
  markdown_path: string;
  json_path: string;
  markdown: string;
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

/// One file-scoped assignment on the session work board.
export interface TeamAgent {
  agent_key: string;
  role: string | null;
  kind: "native" | "room";
  files: string[];
  task: string;
  status: "assigned" | "working" | "done" | "failed" | string;
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

/** One `@@` block and the agent whose patch proposed it (P5.6). */
export interface AttributedHunk {
  patch_id: string;
  path: string;
  ordinal: number;
  agent_id: string;
  header: string;
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  created_at: number;
  status: string;
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
  gemini: boolean;
}

/** Health of the vendor chat server (agentchattr) the Chat tab fronts. */
export interface AgentChatStatus {
  running: boolean;
  /** True when Rust holds a session token for it — everything past the landing page needs one. */
  authed: boolean;
  origin: string;
  /** The checkout on this machine, when one was found. */
  dir: string | null;
  /** Install instructions, when no checkout exists. The core owns the copy. */
  setup: string | null;
}

/** The vendor store's message shape, narrowed to what the stream renders. */
export interface AgentChatMessage {
  id: number;
  sender: string;
  text: string;
  time: string;
  kind: string;
  channel: string;
}

/** One roster pill: agentchattr presence plus its own label/colour/role fields. */
export interface AgentChatAgent {
  available: boolean;
  busy: boolean;
  label: string;
  color: string;
  role: string | null;
}

/** One harness process Rigel started for the room, so the roster can offer a Stop. */
export interface AgentChatProcess {
  name: string;
  label: string;
  pid: number;
}

/** One agent a swarm can be sent to, as offered in the preflight. */
export interface SwarmTarget {
  name: string;
  label: string;
  role: string | null;
}

/** One harness the chat checkout configures; `ready` means its CLI/key is present. */
export interface AgentChatHarness {
  name: string;
  label: string;
  api: boolean;
  ready: boolean;
}

/** Browser preview only: the shape of a live room, from this project's recorded run. */
const PREVIEW_CHAT_MESSAGES: AgentChatMessage[] = [
  { id: 1, sender: "user", text: "@claude @codex Review crates/rigel-cli/src/phase3.rs — one improvement each, then debate.", time: "21:18:02", kind: "chat", channel: "general" },
  { id: 2, sender: "claude", text: "PatchCommand variants carry no help text while AgentCommand's do — add doc comments.", time: "21:18:41", kind: "chat", channel: "general" },
  { id: 3, sender: "codex", text: "--max-steps 0 is accepted, then the coordinator fails on the step bound at once — validate in Clap.", time: "21:19:03", kind: "chat", channel: "general" },
  { id: 4, sender: "claude", text: "@codex Agreed — your fix lands first; mine is docs-only follow-up. This enum has acp, not register.", time: "21:19:30", kind: "chat", channel: "general" },
];

/** One row of `adapters.json` (P4.1). */
export interface HarnessInfo {
  harness: "native" | "codex" | "claude" | "cursor" | "cline" | "gemini";
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
  {
    harness: "gemini",
    installed: false,
    version: null,
    path: null,
    auth_mode: "none",
    capabilities: { acp: true, json_stream: true, programmatic_approval: true, worktree: false, nested_agents: false },
    provider: "gemini",
    install_hint: "npm install -g @google/gemini-cli",
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
  /** One page of the current query. `offset` pages through the whole catalogue. */
  search(query: string, limit: number, offset?: number): Promise<SkillRow[]>;
  /** How many rows the query matches — the whole shelf when it is empty. */
  count(query: string): Promise<number>;
  load(id: string): Promise<SkillDetail>;
  status(): Promise<SourceStatus[]>;
  indexRepo?(repo: string): Promise<IngestReport>;
}


/** One question waiting for a human: a command, or a harness tool call (N2). */
export interface PendingApproval {
  id: string;
  /** The exact text a grant is keyed by, so a near-miss asks again. */
  key: string;
  kind: string;
  title: string;
  detail: Record<string, unknown> | null;
  requested_at: number;
}

export type ApprovalAnswer = "once" | "session" | "project" | "always" | "deny";


/** One claim in the ledger, as the Rust struct serialises. */
export interface Claim {
  id: string;
  path: string;
  agent_id: string;
  /** The agent's role, else its harness. */
  agent: string;
  harness: string;
  base_hash: string | null;
  acquired_at: number;
  heartbeat_at: number;
  released_at: number | null;
  release_reason: string | null;
  /** No heartbeat for 90 s: reported, never auto-released. */
  stale: boolean;
  held: boolean;
}

export interface ClaimConflict {
  id: string;
  path: string;
  claimant: string | null;
  writer: string | null;
  base_hash: string | null;
  observed_hash: string | null;
  detected_at: number;
  resolved_at: number | null;
  resolution: string | null;
}

export interface UnverifiedChange {
  path: string;
  agent: string | null;
  claimant: string | null;
  base_hash: string | null;
  observed_hash: string | null;
  ambiguous: boolean;
}

export interface CoordinationView {
  claims: Claim[];
  unverified_changes: UnverifiedChange[];
  conflicts: ClaimConflict[];
}

export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;


/** The browser preview's queue. Answering removes the row; nothing is sent anywhere. */
let previewQueue: PendingApproval[] = previewApprovals.map((pending) => ({ ...pending }));

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

export const api = {
  isTauri,

  /** The approval card: every open question, oldest first (P4.10e). */
  approvals: {
    async pending(): Promise<PendingApproval[]> {
      if (!isTauri()) return previewQueue;
      return call<PendingApproval[]>("approval_pending");
    },
    /** Answer one question. A refusal is `null` on the wire, never an unknown string. */
    async answer(id: string, scope: ApprovalAnswer): Promise<void> {
      if (!isTauri()) {
        previewQueue = previewQueue.filter((pending) => pending.id !== id);
        return;
      }
      return call<void>("approval_answer", { id, scope: scope === "deny" ? null : scope });
    },
  },

  async openProject(path: string): Promise<ProjectInfo> {
    if (!isTauri()) {
      return { root: path, name: path.split(/[\\/]/).pop() || path, db: "(browser preview)" };
    }
    return call<ProjectInfo>("project_open", { path });
  },

  /** The native folder picker. Returns null when the user cancels or in the browser preview. */
  async pickProjectFolder(): Promise<string | null> {
    if (!isTauri()) return null;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ directory: true, multiple: false, title: "Open a project folder" });
    return typeof picked === "string" ? picked : null;
  },

  /** Create a folder under the workspace root and open it. */
  async createProject(name: string): Promise<ProjectInfo> {
    if (!isTauri()) throw new Error("Create a workspace from the desktop app, not the browser preview.");
    return call<ProjectInfo>("project_create", { name });
  },

  /** Where "New workspace" puts folders: `%USERPROFILE%\\Rigel` on Windows. */
  async workspaceRoot(): Promise<string> {
    if (!isTauri()) return "~/Rigel";
    return call<string>("project_workspace_root");
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

  async team(project: string, session: string): Promise<TeamAgent[]> {
    return call<TeamAgent[]>("agent_team", { project, session });
  },

  async spawnAgent(
    project: string,
    session: string,
    harness: "native" | "claude" = "native",
    role?: string,
    model?: string,
  ): Promise<Agent> {
    return call<Agent>("agent_spawn", {
      project,
      session,
      harness,
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
    model?: string,
    effort?: string,
  ): Promise<TurnReply> {
    return call<TurnReply>("turn_send", {
      project,
      session,
      agent,
      prompt,
      model: model ?? null,
      effort: effort ?? null,
    });
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
    model?: string,
    effort?: string,
  ): Promise<TurnQuote> {
    if (!isTauri()) return previewQuote(prompt, spendMicros, capMicros);
    return call<TurnQuote>("turn_estimate", {
      project,
      session,
      prompt,
      model: model ?? null,
      effort: effort ?? null,
    });
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


  /** The merged diff: claims, unverified changes and open conflicts (P5.6). */
  async claimList(project: string): Promise<CoordinationView> {
    if (!isTauri()) {
      return { claims: [], unverified_changes: [], conflicts: [] };
    }
    return call<CoordinationView>("claim_list", { project });
  },

  async resolveConflict(project: string, id: string, resolution: string): Promise<void> {
    if (!isTauri()) return;
    return call<void>("conflict_resolve", { project, id, resolution });
  },

  /** Tell the desktop watcher which project to follow. */
  async watchProject(project: string): Promise<void> {
    if (!isTauri()) return;
    return call<void>("watch_project", { project });
  },

  async listPatches(project: string, session: string): Promise<Patch[]> {
    return call<Patch[]>("patch_list", { project, session });
  },

  async patchHunks(project: string, session: string): Promise<AttributedHunk[]> {
    return call<AttributedHunk[]>("patch_hunks", { project, session });
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
      return {
        openrouter: false,
        omniroute: false,
        openai: false,
        anthropic: false,
        cursor: false,
        gemini: false,
      };
    }
    return call<KeyStatus>("key_status");
  },

  async setKey(provider: string, value: string): Promise<KeyStatus> {
    if (!isTauri()) {
      return {
        openrouter: true,
        omniroute: true,
        openai: true,
        anthropic: true,
        cursor: true,
        gemini: true,
      };
    }
    return call<KeyStatus>("key_set", { provider, value });
  },

  async deleteKey(provider: string): Promise<KeyStatus> {
    if (!isTauri()) {
      return {
        openrouter: false,
        omniroute: false,
        openai: false,
        anthropic: false,
        cursor: false,
        gemini: false,
      };
    }
    return call<KeyStatus>("key_delete", { provider });
  },

  async harnesses(): Promise<HarnessInfo[]> {
    // A browser preview has no Rust host, so there is nothing to probe.
    if (!isTauri()) return [];
    return call<HarnessInfo[]>("harnesses_probe");
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

  /**
   * Boot every local service a turn may need — the router, and the chat server once the
   * project's checkout is known. Fire-and-forget: each service reports itself in its tab.
   */
  async startServers(project: string | null): Promise<void> {
    if (!isTauri()) return;
    await call<void>("servers_start", { project });
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
        effort: "medium",
        notify_approval: true,
        notify_turn_done: true,
        notify_turn_failed: true,
      };
    }
    return call<Settings>("settings_get");
  },

  async saveSettings(settings: Settings): Promise<Settings> {
    if (!isTauri()) return settings;
    return call<Settings>("settings_set", { settings });
  },

  /** Per-turn checkpoints, newest last. */
  async listCheckpoints(project: string, session: string): Promise<Checkpoint[]> {
    if (!isTauri()) return [];
    return call<Checkpoint[]>("checkpoint_list", { project, session });
  },

  /** Restore the snapshot a turn recorded. Explicit; the journal is not rolled back. */
  async restoreCheckpoint(project: string, id: string): Promise<{ files_restored: number }> {
    return call<{ files_restored: number }>("checkpoint_restore", { project, id });
  },

  /** Write a session's Markdown and JSON under the app data folder. */
  async exportSession(project: string, id: string): Promise<SessionExport> {
    if (!isTauri()) throw new Error("Export a session from the desktop app, not the browser preview.");
    return call<SessionExport>("session_export", { project, id });
  },

  /** Assemble the diagnostics bundle on demand; it is written locally and returned for copying. */
  async exportDiagnostics(project: string, session: string | null): Promise<DiagnosticsBundle> {
    if (!isTauri()) {
      const body = "# Rigel diagnostics\n\nThe browser preview has no database or logs.";
      return { path: "(browser preview)", body };
    }
    return call<DiagnosticsBundle>("diagnostics_export", { project, session });
  },

  /** The provider's live model catalogue for the picker. */
  async listModels(): Promise<string[]> {
    if (!isTauri()) {
      return [
        "gpt-5.6-sol-low",
        "gpt-5.6-sol-medium",
        "gpt-5.6-sol-high",
        "openai/gpt-4.1-mini",
        "anthropic/claude-sonnet-4.5",
      ];
    }
    return call<string[]>("models_list");
  },

  /** The vendor chat server the Chat tab fronts. Same boundary as the router above. */
  agentChat: {
    async status(project: string | null): Promise<AgentChatStatus> {
      if (!isTauri()) {
        return { running: true, authed: true, origin: "(browser preview)", dir: null, setup: null };
      }
      return call<AgentChatStatus>("chat_status", { project });
    },

    async start(project: string | null): Promise<AgentChatStatus> {
      if (!isTauri()) throw new Error("Start the chat server from the desktop app, not the browser preview.");
      return call<AgentChatStatus>("chat_start", { project });
    },

    async channels(): Promise<string[]> {
      if (!isTauri()) return ["general", "review", "debate"];
      return call<string[]>("chat_channels");
    },

    async agents(): Promise<Record<string, AgentChatAgent>> {
      if (!isTauri()) {
        return {
          claude: { available: true, busy: false, label: "Claude", color: "#da7756", role: "reviewer" },
          codex: { available: true, busy: false, label: "Codex", color: "#10a37f", role: "reviewer" },
        };
      }
      return call<Record<string, AgentChatAgent>>("chat_agents");
    },

    async messages(channel: string, sinceId = 0): Promise<AgentChatMessage[]> {
      if (!isTauri()) return sinceId > 0 ? [] : PREVIEW_CHAT_MESSAGES;
      return call<AgentChatMessage[]>("chat_messages", { channel, since_id: sinceId, limit: 100 });
    },

    async send(channel: string, text: string): Promise<void> {
      if (!isTauri()) {
        PREVIEW_CHAT_MESSAGES.push({
          id: Date.now(),
          sender: "user",
          text,
          time: new Date().toTimeString().slice(0, 8),
          kind: "chat",
          channel,
        });
        return;
      }
      await call<void>("chat_send", { channel, text });
    },

    /** Start the room's agents on a hidden console; returns what was launched. */
    async startAgents(project: string | null): Promise<string[]> {
      if (!isTauri()) throw new Error("Start agents from the desktop app, not the browser preview.");
      return call<string[]>("chat_start_agents", { project });
    },

    /** The agents Rigel owns. Anything else in the room was started by hand. */
    async launched(): Promise<AgentChatProcess[]> {
      if (!isTauri()) return [];
      return call<AgentChatProcess[]>("chat_launched");
    },

    async harnesses(project: string | null): Promise<AgentChatHarness[]> {
      if (!isTauri()) {
        return [
          { name: "claude", label: "Claude", api: false, ready: true },
          { name: "codex", label: "Codex", api: false, ready: true },
          { name: "phi4", label: "Phi4 (local)", api: true, ready: true },
        ];
      }
      return call<AgentChatHarness[]>("chat_harnesses", { project });
    },

    async startAgent(project: string | null, name: string): Promise<string> {
      if (!isTauri()) throw new Error("Start agents from the desktop app, not the browser preview.");
      return call<string>("chat_start_agent", { project, name });
    },

    async stopAgent(name: string): Promise<string> {
      if (!isTauri()) throw new Error("Stop agents from the desktop app, not the browser preview.");
      return call<string>("chat_stop_agent", { name });
    },
  },
};

/** The real shelf: SQLite FTS5 and the blob store, through the Rust core. */
export class TauriSkillSource implements SkillSource {
  readonly label = "index.db";

  async search(query: string, limit = 100, offset = 0): Promise<SkillRow[]> {
    return call<SkillRow[]>("skills_search", { query, limit, offset });
  }

  async count(query: string): Promise<number> {
    return call<number>("skills_count", { query });
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

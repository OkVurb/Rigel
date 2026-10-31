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
  /** D148: pinned sessions sort first in the Session pane's list. */
  pinned_at: number | null;
  /** D148: archived sessions are hidden by default, still searchable and still resumable. */
  archived_at: number | null;
}

export interface ProjectInfo {
  root: string;
  name: string;
  db: string;
}

/** D147: one palette row per section. The window never re-ranks these; the core already did. */
/** D150: a kept line. `target_id` is an item id or a patch hunk id. */
export interface Bookmark {
  id: string;
  session_id: string;
  kind: "item" | "hunk";
  target_id: string;
  label: string;
  created_at: number;
}

export interface PaletteSkill {
  id: string;
  name: string;
  description: string;
  grade: string | null;
  source: string;
}

export interface PaletteSession {
  id: string;
  title: string;
  status: string;
  created_at: number;
}

export interface PaletteTurn {
  session_id: string;
  turn_id: string;
  seq: number;
  role: string;
  harness: string;
  prompt: string;
  snippet: string;
  from_item: boolean;
  created_at: number;
}

export interface PalettePatch {
  id: string;
  session_id: string;
  path: string;
  status: string;
  created_at: number;
}

export interface PaletteResults {
  skills: PaletteSkill[];
  sessions: PaletteSession[];
  turns: PaletteTurn[];
  patches: PalettePatch[];
}

export interface Settings {
  theme: string;
  default_budget_cents: number;
  unattended_default: boolean;
  /** One row of the Rust-side adapter table: `omniroute`, `openrouter`, `gemini`, `vertex`, … */
  provider: string;
  model: string;
  /** Separate reasoning effort: low, medium or high. */
  effort: string;
  /** D90: per-event desktop notifications; no sound, and a watched turn stays silent. */
  notify_approval: boolean;
  notify_turn_done: boolean;
  notify_turn_failed: boolean;
  /** The Terminal surface: xterm's font size and scrollback, and the shell to open. */
  terminal_font_size: number;
  terminal_scrollback: number;
  /** Empty means PowerShell on Windows and `$SHELL` elsewhere. */
  terminal_shell: string;
  /** Re-sync a skill source whose data is older than the stale window, in the background. */
  skills_auto_update: boolean;
  /** D159: 90, 100, 110 or 125; applied to the shell, never to the terminal's cell size. */
  ui_scale: number;
  /** D159: tighter rows in the transcript, list and diff. */
  compact_density: boolean;
  /** How many model steps one native turn may take. */
  max_steps: number;
  /**
   * Whether the model picker hides rows that cannot answer a chat turn.
   * Saved, so the choice survives a restart; defaults to on.
   */
  chat_models_only: boolean;
}

/** The browser preview's stand-in for `settings.toml`; `null` until the first save. */
let previewSettings: Settings | null = null;

/**
 * The settings a window starts from before Rust answers — also the shape a first run writes.
 * One copy, so a new field is added here and in Rust's `Settings::default`, not in every form.
 */
export function defaultSettings(): Settings {
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
    terminal_font_size: 13,
    terminal_scrollback: 8_000,
    terminal_shell: "",
    skills_auto_update: true,
    max_steps: 10,
    chat_models_only: true,
    ui_scale: 100,
    compact_density: false,
  };
}

/** The update switch, this build's version and whether a publisher key is present. */
export interface UpdateStatus {
  current: string;
  enabled: boolean;
  auto_install: boolean;
  manifest_url: string;
  key_present: boolean;
}

/** A signed manifest that verified, whether or not it is newer than this build. */
export interface UpdateCheck {
  current: string;
  offered: string;
  available: boolean;
  verified: boolean;
  url: string;
  sha256: string;
  notes: string | null;
  msi_url: string | null;
  msi_sha256: string | null;
}

/** The Firebase project and account, as `rigel cloud status` reports them. No token is here. */
export interface CloudStatus {
  configured: boolean;
  project_id: string | null;
  signed_in: boolean;
  email: string | null;
  uid: string | null;
  expires_at: number | null;
  /**
   * Firebase's answer about the address. False when signed out, and for a session stored before
   * the check existed until `cloudCheckVerification` re-reads it.
   */
  email_verified: boolean;
  /** The App Check attestation endpoint, when this machine sends App Check tokens. */
  app_check: { attest_url: string } | null;
  /** The local manifest's clock: when this machine last pushed and pulled the shelf. */
  last_push_at: number | null;
  last_pull_at: number | null;
  last_sync_at: number | null;
}

/** The AgentMail inbox, as `rigel mail status` reports it. The key itself is never here. */
export interface MailStatus {
  configured: boolean;
  inbox: string | null;
  key_present: boolean;
}

/** One conversation in the inbox. Bodies are fetched only when it is opened. */
export interface MailThreadSummary {
  thread_id: string;
  subject: string;
  timestamp: string | null;
  message_count: number;
  unread: boolean;
}

/** One message inside an opened conversation. */
export interface MailThreadMessage {
  message_id: string;
  from: string;
  to: string[];
  subject: string;
  timestamp: string | null;
  text: string;
  extracted_text: string | null;
}

/** One conversation, oldest message first. */
export interface MailThread {
  thread_id: string;
  subject: string;
  messages: MailThreadMessage[];
}

/** What a send produced. */
export interface SentMail {
  message_id: string;
  thread_id: string | null;
}

/** What a push sent. Bodies never leave the machine. */
export interface CloudPushReport {
  project_id: string;
  uid: string;
  pushed: number;
  batches: number;
  /** Rows in the manifest that are gone locally; each became a tombstone. */
  deleted: number;
  /** The shelf exceeded the mirror cap, so deletions were not propagated. */
  truncated: boolean;
}

/** What a publish sent: the header and one document per turn. */
export interface CloudPublishReport {
  project_id: string;
  uid: string;
  session_id: string;
  title: string;
  turns: number;
  batches: number;
}

/** What a pull found. A pulled row never carries a body. */
export interface CloudPullReport {
  project_id: string;
  uid: string;
  received: number;
  written: number;
  skipped: number;
  pages: number;
  /** Tombstone documents seen. */
  tombstones: number;
  /** Local rows a tombstone actually removed. */
  deleted: number;
}

/** D82: an opt-in, local-only diagnostics bundle. Nothing leaves the machine. */
export interface DiagnosticsBundle {
  path: string;
  body: string;
}

/** D97: the exact title and body a pull request would use. */
export interface PullRequestPlan {
  title: string;
  body: string;
  branch: string;
  base: string;
}

/** D91: one project rule file the native preamble reads. */
export interface RuleSource {
  path: string;
  bytes: number;
  kind: "agents" | "claude" | "cursor" | string;
  truncated: boolean;
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

/** D98: a session's plan phase. Approval turns `mode` off and records `approved_at`. */
export interface PlanState {
  session_id: string;
  mode: boolean;
  approved_at: number | null;
  updated_at: number;
}

/** D94: a saved role prompt, model/effort pair and tool allowance. */
export interface RolePreset {
  id: string;
  title: string;
  brief: string;
  model: string;
  effort: string;
  tools: string[];
}

/** D92: one row of the cross-session spend report. `micros` is the canonical unit. */
export interface SpendRow {
  key: string;
  label: string;
  micros: number;
  turns: number;
}

/** D92: every recorded turn, grouped by session, day and agent. */
export interface SpendReport {
  total_micros: number;
  turns: number;
  by_session: SpendRow[];
  by_agent: SpendRow[];
  by_day: SpendRow[];
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

/** One request to the cheap model that tightens a draft before it is sent. */
export interface RefineRequest {
  draft: string;
  target: string;
  channel?: string | null;
  /** Recent room or turn messages, already flattened to `sender: text`. */
  recent: string[];
  /** The user's answers to the previous round's questions. */
  answers: string[];
  round: number;
}

/** Questions plus a proposed brief. Nothing here has been sent to an agent. */
export interface PromptRefinement {
  questions: string[];
  refined: string;
  reads: string[];
  read_notes: string[];
  notes: string[];
  usage: Usage;
  model: string;
}

export interface ModelOption {
  id: string;
  provider: "openrouter" | "omniroute" | "gemini" | string;
  name?: string | null;
  free: boolean;
  /** Whether this row can answer a chat turn; the picker hides the rest behind a toggle. */
  chat?: boolean;
}

export interface TerminalInfo {
  id: string;
  shell: string;
  cwd: string;
}

/** One chunk from the PTY, base64 so escape sequences and raw bytes survive the event. */
export interface TerminalOutput {
  id: string;
  data: string;
  exit: boolean;
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
  kind: "message" | "reasoning" | "command" | "file_change" | "mcp_call" | "web_search" | "plan" | "approval" | "usage" | "error" | "note" | string;
  status: string;
  body: Record<string, unknown>;
  created_at: number;
  raw?: unknown;
}

export interface TurnTranscript {
  turn: Turn;
  items: Item[];
}

/**
 * D113/D116: the safety knobs and today's ledger. The window reads this in Settings and
 * after a raise; the cap itself is enforced in the core, not here.
 */
export interface SafetyView {
  writable_agents: number;
  daily_cap_micros: number | null;
  day: string;
  today_micros: number;
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
/** D154: one unresolved row. `kind` names why it is here, never a guess at a cause. */
export interface AttentionItem {
  kind: string;
  /** Empty on an approval from a surface that recorded no session. */
  session_id: string;
  label: string;
  detail: string;
  at: number;
  turn_id: string | null;
  agent_id: string | null;
  patch_id: string | null;
}

export interface AttentionGroup {
  session_id: string;
  title: string;
  status: string;
  archived: boolean;
  items: AttentionItem[];
}

export interface Attention {
  groups: AttentionGroup[];
  total: number;
}

/** D152: one agent a project template spawns, with its own model (D135). */
export interface TemplateAgent {
  harness: string;
  role: string | null;
  model: string | null;
}

/** D152: the shape of a session before it starts, stored in `.rigel/templates.json`. */
export interface SessionTemplate {
  id: string;
  name: string;
  model: string | null;
  effort: string | null;
  max_steps: number | null;
  agents: TemplateAgent[];
  /** Left in the composer by a create; never sent. */
  prompt: string;
  created_at: number;
}

/** D156: the recorded chain behind a patch. Empty links are printed as "unknown". */
export interface Provenance {
  patch_id: string;
  path: string;
  status: string;
  created_at: number;
  applied_at: number | null;
  agent_id: string;
  agent_role: string | null;
  harness: string | null;
  model: string | null;
  turn_id: string | null;
  turn_seq: number | null;
  turn_status: string | null;
  item_id: string | null;
  item_kind: string | null;
  tool: string | null;
  complete: boolean;
}

export interface AttributedHunk {
  /** The `patch_hunks` row: what a comment attaches to. */
  id: string;
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

/** A comment on one hunk, addressed to the agent that wrote it (D88). */
export interface HunkComment {
  id: string;
  hunk_id: string;
  patch_id: string;
  path: string;
  header: string;
  /** The hunk's author: who the comment is for. */
  agent_id: string;
  /** `user`, or the id of the agent that wrote it. */
  author: string;
  body: string;
  created_at: number;
  delivered_turn_id: string | null;
  delivered_at: number | null;
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

/** Browser-preview attention: one session group plus a waiting approval. */
function previewAttention(): Attention {
  const now = Date.now();
  return {
    total: 3,
    groups: [
      {
        session_id: "",
        title: "Waiting on you",
        status: "active",
        archived: false,
        items: [
          {
            kind: "approval",
            session_id: "7c1e4a3d-0000-4000-8000-000000000000",
            label: "cargo test --workspace",
            detail: "command · waiting for an answer",
            at: now - 60_000,
            turn_id: "turn-preview-2",
            agent_id: "agent-preview-worker",
            patch_id: null,
          },
        ],
      },
      {
        session_id: "7c1e4a3d-0000-4000-8000-000000000000",
        title: "Fix the flaky resume test",
        status: "active",
        archived: false,
        items: [
          {
            kind: "blocked_merge",
            session_id: "7c1e4a3d-0000-4000-8000-000000000000",
            label: "Blocked merge from writer",
            detail: "blocked · 1 file · the caller asked for exactly this comment",
            at: now - 240_000,
            turn_id: null,
            agent_id: "c4d18e77-2b90-4a51-8f0e-6d3a9b5c7e21",
            patch_id: null,
          },
          {
            kind: "failed_turn",
            session_id: "7c1e4a3d-0000-4000-8000-000000000000",
            label: "Turn 2 failed",
            detail: "writer · no later turn on this agent completed",
            at: now - 300_000,
            turn_id: "turn-preview-2",
            agent_id: "agent-preview-worker",
            patch_id: null,
          },
        ],
      },
    ],
  };
}

/** Browser-preview templates: one sample, kept in memory for the session. */
let previewTemplates: SessionTemplate[] | null = null;

function previewTemplateList(): SessionTemplate[] {
  previewTemplates ??= [
    {
      id: "template-preview-1",
      name: "Fix the flaky test",
      model: "anthropic/claude-sonnet-4.5",
      effort: "medium",
      max_steps: 10,
      agents: [{ harness: "native", role: "writer", model: "anthropic/claude-sonnet-4.5" }],
      prompt: "The resume test fails on Windows. Find the cause before patching it.",
      created_at: Date.now() - 600_000,
    },
  ];
  return previewTemplates;
}

function previewTemplateSave(template: SessionTemplate): SessionTemplate {
  const list = previewTemplateList();
  const saved: SessionTemplate = {
    ...template,
    id: template.id || `template-preview-${Date.now()}`,
    name: template.name.split("\n")[0].trim().slice(0, 60) || "Session",
    prompt: template.prompt.trimEnd(),
    created_at: template.created_at || Date.now(),
  };
  const index = list.findIndex((row) => row.id === saved.id);
  previewTemplates = index >= 0 ? list.map((row) => (row.id === saved.id ? saved : row)) : [...list, saved];
  return saved;
}

/**
 * Browser-preview provenance only: the chain the sample diff and transcript share. The desktop
 * app reads the same shape from `provenance.rs`.
 */
function previewProvenance(patchId: string): Provenance {
  if (patchId !== "patch-preview-1") {
    throw new Error(`patch ${patchId} is not in the journal`);
  }
  return {
    patch_id: patchId,
    path: "ui/src/session.tsx",
    status: "applied",
    created_at: Date.now() - 120_000,
    applied_at: Date.now() - 120_000,
    agent_id: "agent-worker",
    agent_role: "worker",
    harness: "native",
    model: "anthropic/claude-sonnet-4.5",
    turn_id: "turn-preview-2",
    turn_seq: 2,
    turn_status: "completed",
    item_id: "item-preview-5",
    item_kind: "file_change",
    tool: "edit_file",
    complete: true,
  };
}

export interface KeyStatus {
  /** `true` per credential name: one entry per provider and one per harness key. */
  providers: Record<string, boolean>;
  /** Whether any chat provider has a credential — the answer the launch banner asks for. */
  native_ready: boolean;
}

/** One chat adapter, from the Rust-side table (`providers_list`). Never a key, only its absence. */
export interface ProviderRow {
  id: string;
  name: string;
  hint: string;
  /** The variable a headless run reads instead of the keychain. */
  env: string | null;
  /** A short-lived token rather than a key: the field's label and copy follow this. */
  access_token: boolean;
  default_model: string | null;
  /** Where a key is created, when the vendor publishes a page for it. */
  key_url: string | null;
  key_present: boolean;
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

/** The adapters a browser preview shows; the real list comes from the Rust-side table. */
const PREVIEW_PROVIDERS: ProviderRow[] = [
  {
    id: "omniroute",
    name: "OmniRoute",
    hint: "Local router · one key, any model",
    env: "OMNIROUTE_API_KEY",
    access_token: false,
    default_model: "gpt-5.6-sol-medium",
    key_url: null,
    key_present: false,
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    hint: "Hosted gateway · the widest roster",
    env: "OPENROUTER_API_KEY",
    access_token: false,
    default_model: "openai/gpt-4.1-mini",
    key_url: "https://openrouter.ai/keys",
    key_present: false,
  },
  {
    id: "vertex",
    name: "Google Vertex AI",
    hint: "Your GCP project · a short-lived access token",
    env: "VERTEX_ACCESS_TOKEN",
    access_token: true,
    default_model: "google/gemini-4-argon",
    key_url: "https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/start/openai",
    key_present: false,
  },
  {
    id: "anthropic",
    name: "Anthropic",
    hint: "Claude's own endpoint · the Messages API",
    env: "ANTHROPIC_API_KEY",
    access_token: false,
    default_model: "claude-opus-5-5",
    key_url: "https://platform.claude.com/settings/keys",
    key_present: false,
  },
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
  | {
      type: "turn_started";
      session_id: string;
      agent_id: string;
      turn_id: string;
      parent_turn_id: string | null;
      seq: number;
      prompt: string | null;
    }
  | { type: "delta"; turn_id: string; kind: string; text: string }
  | { type: "item"; item: Item }
  | { type: "turn_finished"; turn_id: string; status: string; stop: string; usage: Usage; session_spend_micros: number; cap_micros: number | null }
  | { type: "budget_warning"; session_id: string; spend_micros: number; cap_micros: number; remaining_micros: number }
  | { type: "budget_exceeded"; session_id: string; spend_micros: number; cap_micros: number }
  | { type: "task_budget_exceeded"; session_id: string; agent_id: string; spend_micros: number; cap_micros: number }
  | { type: "daily_budget_warning"; day: string; spend_micros: number; cap_micros: number }
  | { type: "daily_budget_exceeded"; day: string; spend_micros: number; cap_micros: number }
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
  /** Re-sync every source that is not fresh; what the auto-update switch runs. */
  refresh?(): Promise<RefreshReport>;
}

/** One source after an auto-update pass. */
export interface SourceRefresh {
  id: string;
  /** `ok` | `stale` | `broken` — the state the source is left in. */
  state: string;
  /** One line: what was fetched, or why the source was left alone. */
  detail: string;
}

/** What one auto-update pass did. */
export interface RefreshReport {
  checked: number;
  refreshed: number;
  sources: SourceRefresh[];
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
  /** D154: the project and origin the asking surface recorded; absent on older files. */
  project_root?: string;
  session_id?: string;
  agent_id?: string;
  turn_id?: string;
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

/** One stored verdict from the clean-context reviewer (D115). */
export interface MergeReviewView {
  id: string;
  session_id: string;
  agent_id: string;
  reviewer_model: string;
  verdict: "approved" | "blocked" | "unavailable";
  reason: string | null;
  /** The hash of the exact diff the verdict was about. */
  diff_sha: string;
  files: string[];
  /** The reason the user typed to merge over a block, if they did. */
  override_note: string | null;
  /** False means the reviewer answered but the merge did not happen (or has not yet). */
  merged: boolean;
  created_at: number;
}

/**
 * A writable subagent as the Diff view lists it (D80). The pending diff is not here: this
 * rides the two-second poll, and a worktree diff can be a whole file.
 */
export interface WritableAgentView {
  agent_id: string;
  role: string | null;
  model: string | null;
  status: string;
  worktree: string | null;
  plan: string | null;
  plan_note: string | null;
  review: MergeReviewView | null;
}

/** A worktree's pending changes as one unified diff, read on demand. */
export interface WritableDiffView {
  agent_id: string;
  files: string[];
  diff: string;
}

/** One file's outcome from a merge, as the CAS pipeline reported it. */
export interface MergeLineView {
  path: string;
  status: "applied" | "conflicted" | "deleted" | "unsupported" | "unchanged" | string;
  patch_id: string | null;
  detail: string | null;
}

export interface MergeReportView {
  agent_id: string;
  base: string;
  lines: MergeLineView[];
  applied: number;
  conflicted: number;
  skipped: number;
}

/** The result of a merge the user decided: the review it happened under, and what it did. */
export interface AppliedMerge {
  review: MergeReviewView;
  report: MergeReportView;
  worktree_removed: boolean;
}

/** One side of a conflict: its text and the hash those bytes must have (D79). */
export interface ConflictText {
  hash: string;
  text: string;
}

/** The three rendered versions. A side is null when Rigel could not rebuild it. */
export interface ConflictSides {
  id: string;
  path: string;
  base: ConflictText | null;
  ours: ConflictText | null;
  theirs: ConflictText | null;
  /** Why a side is missing, when one is. */
  note?: string;
  merged_patch_id?: string;
  merged_hash?: string;
}

/** The merge text, before anything is written. */
export interface MergedConflict {
  id: string;
  path: string;
  merged: string;
  conflicts: number;
  clean: boolean;
}

export interface MergeResolution {
  path: string;
  patch: Patch | null;
  unchanged: boolean;
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

  /** The approval card: every open question for this project, oldest first (P4.10e, D154). */
  approvals: {
    async pending(project: string): Promise<PendingApproval[]> {
      if (!isTauri()) return previewQueue;
      return call<PendingApproval[]>("approval_pending", { project });
    },
    /** Answer one question. A refusal is `null` on the wire, never an unknown string. */
    async answer(
      id: string,
      scope: ApprovalAnswer,
      options?: { skip?: boolean; note?: string },
    ): Promise<void> {
      if (!isTauri()) {
        previewQueue = previewQueue.filter((pending) => pending.id !== id);
        return;
      }
      return call<void>("approval_answer", {
        id,
        scope: scope === "deny" || options?.skip ? null : scope,
        skip: options?.skip ?? false,
        note: options?.note ?? null,
      });
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

  /** Copy files chosen in the native picker into `.rigel/attachments`, returning relative paths. */
  async importAttachments(project: string, paths: string[]): Promise<string[]> {
    if (!isTauri()) return [];
    return call<string[]>("attachment_import", { project, paths });
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

  /** D148: nil clears the name, so the automatic title is used again. */
  async renameSession(project: string, id: string, title: string | null): Promise<Session> {
    if (!isTauri()) throw new Error("Rename a session in the desktop app.");
    return call<Session>("session_rename", { project, id, title });
  },

  async pinSession(project: string, id: string, pinned: boolean): Promise<Session> {
    if (!isTauri()) throw new Error("Pin a session in the desktop app.");
    return call<Session>("session_pin", { project, id, pinned });
  },

  async archiveSession(project: string, id: string, archived: boolean): Promise<Session> {
    if (!isTauri()) throw new Error("Archive a session in the desktop app.");
    return call<Session>("session_archive", { project, id, archived });
  },

  async listBookmarks(project: string, session: string): Promise<Bookmark[]> {
    if (!isTauri()) return [];
    return call<Bookmark[]>("bookmark_list", { project, session });
  },

  /** D154: unresolved work, grouped by session, with open approvals leading the list. */
  async attention(project: string): Promise<Attention> {
    if (!isTauri()) return previewAttention();
    return call<Attention>("attention_list", { project });
  },

  /** D152: the project's own templates. Nothing here is committed to the repository. */
  templates: {
    async list(project: string): Promise<SessionTemplate[]> {
      if (!isTauri()) return previewTemplateList();
      return call<SessionTemplate[]>("template_list", { project });
    },

    async save(project: string, template: SessionTemplate): Promise<SessionTemplate> {
      if (!isTauri()) return previewTemplateSave(template);
      return call<SessionTemplate>("template_save", { project, template });
    },

    async delete(project: string, id: string): Promise<void> {
      if (!isTauri()) {
        previewTemplates = previewTemplateList().filter((row) => row.id !== id);
        return;
      }
      return call<void>("template_delete", { project, id });
    },
  },

  /** D156: read the chain for one patch. Nothing here is written or re-run. */
  async provenance(project: string, patchId: string): Promise<Provenance> {
    if (!isTauri()) return previewProvenance(patchId);
    return call<Provenance>("patch_provenance", { project, patchId });
  },

  /** `null` means the star was cleared. */
  async toggleBookmark(
    project: string,
    session: string,
    kind: "item" | "hunk",
    targetId: string,
    label: string,
  ): Promise<Bookmark | null> {
    if (!isTauri()) throw new Error("Bookmark a line in the desktop app.");
    return call<Bookmark | null>("bookmark_toggle", { project, session, kind, targetId, label });
  },

  /** D147: skills, sessions, turns and patches in one read. Capped per section in the core. */
  async searchAll(
    project: string | null,
    query: string,
    currentSession: string | null,
  ): Promise<PaletteResults> {
    if (!isTauri()) {
      return { skills: [], sessions: [], turns: [], patches: [] };
    }
    return call<PaletteResults>("search_all", { project, query, currentSession });
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

  /** D98: read the session's plan phase. */
  async planGet(project: string, session: string): Promise<PlanState> {
    return call<PlanState>("plan_get", { project, session });
  },

  /** D98: plan on; approving sets `enabled: false` and records the hand-off. */
  async planSet(project: string, session: string, enabled: boolean): Promise<PlanState> {
    return call<PlanState>("plan_set", { project, session, enabled });
  },

  /** D94: the four built-in role presets. */
  async rolePresets(): Promise<RolePreset[]> {
    if (!isTauri()) return [];
    return call<RolePreset[]>("role_presets");
  },

  /** D94: apply a preset from the roster; the next turn reads the new role. */
  async roleApply(
    project: string,
    session: string,
    agent: string,
    preset: string,
  ): Promise<Agent> {
    return call<Agent>("role_apply", { project, session, agent, preset });
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

  /** D92: spend across every session in the project. */
  /** The writable-agent cap and the day's spend. */
  async safetyGet(project: string): Promise<SafetyView> {
    return call<SafetyView>("safety_get", { project });
  },
  /**
   * Change one knob. `dailyCapCents: null` leaves it alone; pass `clearDailyCap` to turn it
   * off. A one-click raise is exactly this call with a larger number.
   */
  async safetySet(
    project: string,
    session: string,
    input: {
      writableAgents?: number;
      dailyCapCents?: number;
      clearDailyCap?: boolean;
    },
  ): Promise<SafetyView> {
    return call<SafetyView>("safety_set", {
      project,
      session,
      writableAgents: input.writableAgents ?? null,
      dailyCapCents: input.dailyCapCents ?? null,
      clearDailyCap: input.clearDailyCap ?? false,
    });
  },

  async spendReport(project: string): Promise<SpendReport> {
    return call<SpendReport>("spend_report", { project });
  },

  /** D92: cap one task (agent) below the session cap. `null` removes it. */
  async taskCapSet(
    project: string,
    session: string,
    agent: string,
    capCents: number | null,
  ): Promise<BudgetSnapshot> {
    return call<BudgetSnapshot>("task_cap_set", { project, session, agent, capCents });
  },


  /** The merged diff: claims, unverified changes and open conflicts (P5.6). */
  async claimList(project: string): Promise<CoordinationView> {
    if (!isTauri()) {
      return { claims: [], unverified_changes: [], conflicts: [] };
    }
    return call<CoordinationView>("claim_list", { project });
  },

  /** Writable subagents with their plans and latest review; polled with the claims. */
  async writableList(project: string, session: string): Promise<WritableAgentView[]> {
    if (!isTauri()) return [];
    return call<WritableAgentView[]>("writable_list", { project, session });
  },

  /** The worktree's pending changes as one diff, read once when a row is expanded. */
  async writableDiff(project: string, agent: string): Promise<WritableDiffView> {
    return call<WritableDiffView>("writable_diff", { project, agent });
  },

  /**
   * Merge a recorded review on the user's behalf. An approved review needs no reason; a
   * blocked or unavailable one needs the typed reason that will be stored beside it.
   */
  async writableMerge(
    project: string,
    session: string,
    agent: string,
    reason: string | null,
  ): Promise<AppliedMerge> {
    return call<AppliedMerge>("writable_merge", { project, session, agent, reason });
  },

  async resolveConflict(project: string, id: string, resolution: string): Promise<void> {
    if (!isTauri()) return;
    return call<void>("conflict_resolve", { project, id, resolution });
  },

  /** Read the base/ours/theirs text once, when a conflict row is expanded (D79). */
  async conflictSides(project: string, id: string): Promise<ConflictSides> {
    return call<ConflictSides>("conflict_sides", { project, id });
  },

  /** Compute the merge; nothing is written until `resolveMergedConflict`. */
  async mergeConflict(project: string, id: string): Promise<MergedConflict> {
    return call<MergedConflict>("conflict_merge", { project, id });
  },

  /** Write the merged text through the CAS patch pipeline and resolve the conflict (D79). */
  async resolveMergedConflict(
    project: string,
    session: string,
    id: string,
    merged: string,
  ): Promise<MergeResolution> {
    return call<MergeResolution>("conflict_resolve_merged", { project, session, id, merged });
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

  async hunkComments(project: string, session: string): Promise<HunkComment[]> {
    return call<HunkComment[]>("hunk_comments", { project, session });
  },

  /** Attach a comment to one hunk. Nothing is sent until `deliverHunkComment`. */
  async addHunkComment(project: string, hunk: string, body: string): Promise<HunkComment> {
    return call<HunkComment>("hunk_comment_add", { project, hunk, body });
  },

  /** Send the comment to the hunk's author as a turn. */
  async deliverHunkComment(
    project: string,
    session: string,
    id: string,
  ): Promise<TurnReply> {
    return call<TurnReply>("hunk_comment_deliver", { project, session, id });
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
    if (!isTauri()) return { providers: {}, native_ready: false };
    return call<KeyStatus>("key_status");
  },

  async setKey(provider: string, value: string): Promise<KeyStatus> {
    if (!isTauri()) return { providers: { [provider]: true }, native_ready: true };
    return call<KeyStatus>("key_set", { provider, value });
  },

  async deleteKey(provider: string): Promise<KeyStatus> {
    if (!isTauri()) return { providers: { [provider]: false }, native_ready: false };
    return call<KeyStatus>("key_delete", { provider });
  },

  /** The chat adapters, in the order the app lists them. A preview answers the same shape. */
  async providersList(): Promise<ProviderRow[]> {
    if (!isTauri()) return PREVIEW_PROVIDERS;
    return call<ProviderRow[]>("providers_list");
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
      return previewSettings ?? defaultSettings();
    }
    return call<Settings>("settings_get");
  },

  async saveSettings(settings: Settings): Promise<Settings> {
    if (!isTauri()) {
      // The browser preview has no settings.toml. Keep one in memory for the session so a saved
      // choice reads back the way it will in the window, instead of resetting on the next mount.
      previewSettings = settings;
      return settings;
    }
    return call<Settings>("settings_set", { settings });
  },

  async updateStatus(): Promise<UpdateStatus> {
    if (!isTauri()) {
      return {
        current: "0.1.1",
        enabled: false,
        auto_install: false,
        manifest_url: "https://github.com/OkVurb/Rigel/releases/latest/download/latest.json",
        key_present: false,
      };
    }
    return call<UpdateStatus>("update_status");
  },

  async updateSet(
    enabled: boolean,
    autoInstall: boolean,
    manifestUrl: string | null = null,
  ): Promise<UpdateStatus> {
    if (!isTauri()) {
      return {
        current: "0.1.1",
        enabled,
        auto_install: autoInstall,
        manifest_url:
          manifestUrl ?? "https://github.com/OkVurb/Rigel/releases/latest/download/latest.json",
        key_present: false,
      };
    }
    return call<UpdateStatus>("update_set", {
      enabled,
      autoInstall,
      manifestUrl,
    });
  },

  async updateCheck(): Promise<UpdateCheck> {
    if (!isTauri()) throw new Error("Update checks run in the desktop app.");
    return call<UpdateCheck>("update_check");
  },

  async updateInstall(): Promise<string> {
    if (!isTauri()) throw new Error("Updates install from the desktop app.");
    return call<string>("update_install");
  },

  /** The Firebase project and account. No network call; `cloud.json` and the keychain answer. */
  async cloudStatus(): Promise<CloudStatus> {
    if (!isTauri()) {
      return {
        configured: false,
        project_id: null,
        signed_in: false,
        email: null,
        uid: null,
        expires_at: null,
        email_verified: false,
        app_check: null,
        last_push_at: null,
        last_pull_at: null,
        last_sync_at: null,
      };
    }
    return call<CloudStatus>("cloud_status");
  },

  async cloudConfigure(project: string, apiKey: string): Promise<CloudStatus> {
    if (!isTauri()) throw new Error("Configure the Firebase project in the desktop app.");
    return call<CloudStatus>("cloud_configure", { project, apiKey });
  },

  /** One sign-in. The password is not stored; only the refresh token reaches the keychain. */
  async cloudLogin(email: string, password: string): Promise<CloudStatus> {
    if (!isTauri()) throw new Error("Rigel Cloud signs in from the desktop app.");
    return call<CloudStatus>("cloud_login", { email, password });
  },

  /** Create the account; Firebase returns the session, so the page is signed in afterwards. */
  async cloudSignup(email: string, password: string): Promise<CloudStatus> {
    if (!isTauri()) throw new Error("Rigel Cloud creates accounts from the desktop app.");
    return call<CloudStatus>("cloud_signup", { email, password });
  },

  /** Ask Firebase for a reset link. The answer is the address, not whether it has an account. */
  async cloudResetPassword(email: string): Promise<string> {
    if (!isTauri()) throw new Error("Rigel Cloud sends reset links from the desktop app.");
    return call<string>("cloud_reset_password", { email });
  },

  /**
   * Ask Firebase for a verification link to the signed-in account. Sending again is the resend
   * control; the answer is the address the link went to, never a token.
   */
  async cloudSendVerification(): Promise<string> {
    if (!isTauri()) throw new Error("Rigel Cloud sends verification links from the desktop app.");
    return call<string>("cloud_send_verification");
  },

  /** Re-read the verification state from Firebase; the stored session keeps the answer. */
  async cloudCheckVerification(): Promise<CloudStatus> {
    if (!isTauri()) throw new Error("Rigel Cloud checks verification from the desktop app.");
    return call<CloudStatus>("cloud_check_verification");
  },

  /**
   * Point App Check at an attestation endpoint, or turn it off with `null`. The endpoint answers
   * with short-lived tokens; none of them ever reaches this process.
   */
  async cloudAppCheck(url: string | null): Promise<CloudStatus> {
    if (!isTauri()) throw new Error("Rigel Cloud configures App Check in the desktop app.");
    return call<CloudStatus>("cloud_app_check", { url });
  },

  async cloudLogout(): Promise<CloudStatus> {
    if (!isTauri()) throw new Error("Rigel Cloud signs out from the desktop app.");
    return call<CloudStatus>("cloud_logout");
  },

  async cloudPush(): Promise<CloudPushReport> {
    if (!isTauri()) throw new Error("The shelf mirror runs in the desktop app.");
    return call<CloudPushReport>("cloud_push");
  },

  async cloudPull(): Promise<CloudPullReport> {
    if (!isTauri()) throw new Error("The shelf mirror runs in the desktop app.");
    return call<CloudPullReport>("cloud_pull");
  },

  /** The AgentMail inbox and whether a key is stored. No network call. */
  async mailStatus(): Promise<MailStatus> {
    if (!isTauri()) {
      return { configured: false, inbox: null, key_present: false };
    }
    return call<MailStatus>("mail_status");
  },

  async mailConfigure(inbox: string): Promise<MailStatus> {
    if (!isTauri()) throw new Error("The AgentMail inbox is configured in the desktop app.");
    return call<MailStatus>("mail_configure", { inbox });
  },

  /** The key goes to the OS keychain; this window never reads it back. */
  async mailSetKey(apiKey: string): Promise<MailStatus> {
    if (!isTauri()) throw new Error("The AgentMail API key is saved in the desktop app.");
    return call<MailStatus>("mail_set_key", { apiKey });
  },

  /** Conversations in the inbox, newest first. No body is fetched here. */
  async mailThreads(limit = 10): Promise<MailThreadSummary[]> {
    if (!isTauri()) return [];
    return call<MailThreadSummary[]>("mail_threads", { limit });
  },

  /** One conversation, oldest message first. */
  async mailThread(threadId: string): Promise<MailThread> {
    if (!isTauri()) throw new Error("Mail is read in the desktop app.");
    return call<MailThread>("mail_thread", { threadId });
  },

  /** Send from the page. An agent's send goes through the approval card instead. */
  async mailSend(to: string, subject: string, text: string): Promise<SentMail> {
    if (!isTauri()) throw new Error("Mail is sent from the desktop app.");
    return call<SentMail>("mail_send", { to, subject, text });
  },

  /** Publish one session to the phone: prompts and assistant text only. */
  async cloudPublishSession(project: string, session: string): Promise<CloudPublishReport> {
    if (!isTauri()) throw new Error("Sessions publish from the desktop app.");
    return call<CloudPublishReport>("cloud_publish_session", { project, session });
  },

  /** Compose the PR title/body locally, for a review before creating it. */
  async pullRequestPlan(project: string, session: string): Promise<PullRequestPlan> {
    return call<PullRequestPlan>("pull_request_plan", { project, session });
  },

  /** Run gh pr create. Only an explicit click calls this. */
  async createPullRequest(project: string, session: string): Promise<string> {
    return call<string>("pull_request_create", { project, session });
  },

  /** Project rule files a native turn would read (D91). */
  async rulesSources(project: string): Promise<RuleSource[]> {
    if (!isTauri()) return [];
    return call<RuleSource[]>("rules_sources", { project });
  },

  /** Committed memory as one copyable AGENTS.md block. */
  async exportMemory(project: string): Promise<string> {
    if (!isTauri()) return "";
    return call<string>("rules_export_memory", { project });
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

  /**
   * The merged model catalogue for the picker: every provider that holds a credential, the
   * current one first. Rows that cannot answer a chat turn come back marked, not removed — the
   * picker's toggle decides whether they are shown.
   */
  async listModels(): Promise<ModelOption[]> {
    if (!isTauri()) {
      return [
        { id: "gpt-5.6-sol-medium", provider: "omniroute", free: false, chat: true },
        { id: "openai/gpt-4.1-mini", provider: "openrouter", free: false, chat: true },
        { id: "baseten/mammoth-1:free", provider: "openrouter", free: true, chat: true },
        { id: "anthropic/claude-sonnet-4.5", provider: "openrouter", free: false, chat: true },
        { id: "aihorde/AbyssOrangeMix", provider: "omniroute", free: false, chat: false },
      ];
    }
    return call<ModelOption[]>("models_list");
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

  /** The integrated terminal: a real PTY owned by Rust, streamed to xterm.js. */
  terminal: {
    async open(
      project: string | null,
      cols: number,
      rows: number,
      /** The shell to open; empty or absent means the OS default. */
      shell?: string,
    ): Promise<TerminalInfo> {
      if (!isTauri()) throw new Error("The terminal runs in the desktop app, not the browser preview.");
      return call<TerminalInfo>("terminal_open", {
        project,
        cols,
        rows,
        shell: shell?.trim() ? shell.trim() : null,
      });
    },

    async write(id: string, data: string): Promise<void> {
      if (!isTauri()) return;
      return call<void>("terminal_write", { id, data });
    },

    async resize(id: string, cols: number, rows: number): Promise<void> {
      if (!isTauri()) return;
      return call<void>("terminal_resize", { id, cols, rows });
    },

    async close(id: string): Promise<void> {
      if (!isTauri()) return;
      return call<void>("terminal_close", { id });
    },
  },

  /**
   * A cheap model reads the draft and returns questions and a tighter brief. It never sends
   * anything: the card shows the result and the send button stays the user's.
   */
  refine: {
    async run(project: string | null, request: RefineRequest): Promise<PromptRefinement> {
      if (!isTauri()) return previewRefinement(request);
      return call<PromptRefinement>("prompt_refine", { project, request });
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

  async refresh(): Promise<RefreshReport> {
    return call<RefreshReport>("skills_refresh");
  }
}

/** Browser preview only: a canned refinement, labelled as such so no one mistakes it for a call. */
function previewRefinement(request: RefineRequest): PromptRefinement {
  const draft = request.draft.trim();
  const first = request.round === 0;
  return {
    questions: first
      ? ["Which file or crate should this change land in?", "What must stay unchanged?"]
      : [],
    refined: first
      ? draft
      : `${draft}\n\nName the files you change, keep the existing tests passing, and run the relevant test before reporting back.`,
    reads: [],
    read_notes: [],
    notes: ["Browser preview: no model was called and nothing was billed."],
    model: "preview",
    usage: {
      input_tokens: 0,
      cached_input_tokens: 0,
      output_tokens: 0,
      reasoning_tokens: 0,
      cost_micros: 0,
      provider_micros: null,
      source: "estimated",
    },
  };
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

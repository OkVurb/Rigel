import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
  ReactNode,
} from "react";

import {
  api,
  compact,
  defaultSettings,
  when,
  isTauri,
  type Agent,
  type AgentEvent,
  type Bookmark,
  type BudgetSnapshot,
  type Item,
  type ApprovalAnswer,
  type Checkpoint,
  type Patch,
  type PendingApproval,
  type Session,
  type SessionTemplate,
  type SkillPin,
  type SkillPinReview,
  type SkillRow,
  type SkillSource,
  type Settings,
  type TestRun,
  type Turn,
  type SwarmLead,
  type SwarmTarget,
  type TurnQuote,
  type TeamAgent,
  type RolePreset,
  type PlanState,
  type TurnTranscript,
  type RecoveryOffer,
  type SubagentGraph,
} from "./api";
import { StepCost } from "./inspector/step-cost";
import { Recovery } from "./recovery";
import { SubagentGraphView } from "./subagent-graph";
import { ApprovalCard } from "./session/approval-card";
import { DegradedBanner } from "./degraded-banner";
import { ModelControls } from "./modelpicker";
import BrowserPanel, { latestBrowserCall } from "./browser";
import RefineCard from "./refine";
import CompanionSponsor from "./companion";
import { notify } from "./notify";
import { useMenu } from "./menu";
import { bodyDiff } from "./skilldiff";
import {
  previewAgents,
  previewApprovals,
  previewArchivedSession,
  previewBudget,
  previewPatches,
  previewRecovery,
  previewSession,
  previewSubagentGraph,
  previewTranscript,
} from "./sessionMock";

interface Props {
  project: string;
  /** The same local index the Shelf reads; the pin menu searches it in place. */
  source: SkillSource;
  onSessions: (sessions: Session[]) => void;
  onActive: (session: Session | null) => void;
  onNotice: (notice: string) => void;
  onOpenSettings?: () => void;
  onBrowseSkills?: () => void;
  /** Open/Switch folder, New workspace and the workspace form, owned by App. */
  projectControls?: ReactNode;
  /** Swarm step one: who is online, or a refusal to show. Sends nothing. */
  onSwarmPrepare?: () => Promise<SwarmTarget[]>;
  /** Swarm step two: send one task to the chosen agents, under the chosen big boss. */
  onSwarm?: (task: string, names: string[], lead: SwarmLead) => Promise<void>;
  /** D147: open this session and land on a turn once its transcript is rendered. */
  focus?: { sessionId: string; turnId?: string } | null;
  onFocusHandled?: () => void;
  /** D150: a bookmarked hunk opens the Diff surface on that hunk. */
  onOpenHunk?: (hunkId: string) => void;
}

const money = (micros: number | null | undefined): string =>
  micros === null || micros === undefined ? "—" : `$${(micros / 1_000_000).toFixed(6)}`;

const cents = (value: number | null | undefined): string =>
  value === null || value === undefined ? "no cap" : `$${(value / 100).toFixed(2)}`;

/** D153: a range is read at a glance, so a fraction of a cent keeps its significant digits. */
const shortMoney = (micros: number): string =>
  `$${(micros / 1_000_000).toFixed(micros < 10_000 ? 6 : 4)}`;

/** A step can be running or interrupted, so only a finished turn has a duration. */
function stepDuration(startedAt: number, completedAt: number | null, status: string): string {
  if (!Number.isFinite(startedAt) || startedAt <= 0) return "—";
  if (completedAt === null) return status === "running" ? "running" : "—";
  const ms = Math.max(0, completedAt - startedAt);
  if (ms < 1_000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1_000).toFixed(1)} s`;
  const minutes = Math.floor(ms / 60_000);
  const seconds = Math.round((ms % 60_000) / 1_000);
  if (minutes < 60) return `${minutes}m ${seconds}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

/** Q246: the refusal rule, for a question whose file predates `expires_at`. */
const APPROVAL_WAIT_MS = 10 * 60_000;

/** The instant an unanswered question is refused, taken from the asker's own wait. */
function approvalDeadline(pending: PendingApproval): number {
  return pending.expires_at ?? pending.requested_at + APPROVAL_WAIT_MS;
}

/** Q246: `m:ss` left, never negative; zero is the refusal, not a countdown. */

/** Q246: what to ask the agent once its question went unanswered. A plan cannot be rebuilt
 * from its card, so only a command or a mail has a retry. */
function retryPrompt(pending: PendingApproval): string | null {
  if (pending.kind === "command") {
    const cwd = typeof pending.detail?.cwd === "string" ? ` in ${pending.detail.cwd}` : "";
    return `Retry the request that was refused after ten minutes without an answer: run \`${pending.title}\`${cwd}. Ask me again before it runs.`;
  }
  if (pending.kind === "mail") {
    return `Retry the request that was refused after ten minutes without an answer: ${pending.title}. Ask me again before it is sent.`;
  }
  return null;
}
function bodyText(item: Item, key: string): string {
  const value = item.body?.[key];
  return typeof value === "string" ? value : "";
}

function bodyRecord(item: Item, key: string): Record<string, unknown> | null {
  const value = item.body?.[key];
  return value && typeof value === "object" ? (value as Record<string, unknown>) : null;
}

function argsSummary(item: Item): string {
  const args = bodyRecord(item, "arguments");
  if (!args) return "";
  const path = typeof args.path === "string" ? args.path : null;
  if (path) return path;
  const query = typeof args.query === "string" ? args.query : null;
  if (query) return `“${query}”`;
  const id = typeof args.id === "string" ? args.id : null;
  if (id) return id;
  return "";
}

/** D150: the label the inspector draws, built from the line rather than a second query. */
function itemBookmarkLabel(item: Item, seq: number): string {
  const summary =
    item.kind === "message" || item.kind === "reasoning" || item.kind === "note"
      ? bodyText(item, "text")
      : item.kind === "error"
        ? bodyText(item, "message")
        : argsSummary(item);
  const tail = summary.replace(/\s+/g, " ").trim().slice(0, 100);
  return `turn ${seq} · ${item.kind}${tail ? ` · ${tail}` : ""}`;
}

/** Q266: the files a turn touched, taken from its own recorded patch rows. */
function handoffFiles(entry: TurnTranscript): string[] {
  const paths = entry.items
    .filter((item) => item.kind === "file_change")
    .map((item) => {
      const result = bodyRecord(item, "result");
      return result && typeof result.path === "string" ? result.path : bodyText(item, "path");
    })
    .filter((path) => path.length > 0);
  return [...new Set(paths)];
}

/** Q266: a prefilled handoff names the files and the last recorded outcome, nothing more. */
function handoffPrompt(entry: TurnTranscript, from: string): string {
  const summary = [...entry.items]
    .reverse()
    .find((item) => item.kind === "message" && bodyText(item, "text").trim());
  const text = summary ? bodyText(summary, "text").replace(/\s+/g, " ").trim() : "";
  const outcome = text
    ? text.length > 600
      ? `${text.slice(0, 597)}…`
      : text
    : `Turn ${entry.turn.seq} finished ${entry.turn.stop_reason ?? entry.turn.status} without a recorded summary.`;
  const files = handoffFiles(entry);
  return [
    `Handoff from turn ${entry.turn.seq}${from ? ` (${from})` : ""}.`,
    "",
    `Outcome: ${outcome}`,
    "",
    "Changed files:",
    ...(files.length > 0 ? files.map((path) => `- ${path}`) : ["- none recorded in this turn"]),
    "",
    "Take over from here: review the outcome and the changed files, then continue the work.",
  ].join("\n");
}

/* ------------------------- transcript filters (D136) ------------------------- */

/** The five views of one transcript. `yours` keeps the prompts and the notes you wrote. */
type TranscriptFilter = "all" | "tools" | "diffs" | "errors" | "yours";

const FILTERS: { id: TranscriptFilter; label: string }[] = [
  { id: "all", label: "all" },
  { id: "tools", label: "tools" },
  { id: "diffs", label: "diffs" },
  { id: "errors", label: "errors" },
  { id: "yours", label: "yours" },
];

const TOOL_KINDS = new Set(["command", "mcp_call", "web_search"]);
const BAD_STATES = new Set([
  "error",
  "failed",
  "cancelled",
  "denied",
  "timeout",
  "timed_out",
  "interrupted",
]);

function isErrorItem(item: Item): boolean {
  return item.kind === "error" || BAD_STATES.has(item.status);
}

function matchesFilter(item: Item, filter: TranscriptFilter): boolean {
  if (filter === "all") return true;
  if (filter === "tools") return TOOL_KINDS.has(item.kind);
  if (filter === "diffs") return item.kind === "file_change";
  if (filter === "errors") return isErrorItem(item);
  return item.kind === "note";
}

/** A prompt is yours, and it is the one line that is not an item. */
function filterShowsPrompt(filter: TranscriptFilter): boolean {
  return filter === "all" || filter === "yours";
}

const filterKey = (project: string, session: string) =>
  `rigel.transcriptFilter.${project}::${session}`;

function readFilter(project: string, session: string): TranscriptFilter {
  try {
    const saved = localStorage.getItem(filterKey(project, session));
    return FILTERS.some((row) => row.id === saved) ? (saved as TranscriptFilter) : "all";
  } catch {
    return "all";
  }
}

/* --------------------------- turn outline (D149) --------------------------- */

type OutlineKind = "prompt" | "diff" | "error" | "approval";

interface OutlineGroup {
  turnId: string;
  seq: number;
  marks: { key: string; kind: OutlineKind; title: string }[];
}

/* --------------------- composer history and snippets (D151) --------------------- */

interface PromptEntry {
  session: string;
  text: string;
  at: number;
}

interface Snippet {
  id: string;
  label: string;
  body: string;
}

const HISTORY_LIMIT = 100;

/* ----------------------- while you were away (D155) ----------------------- */

/** What changed between losing focus and getting it back, counted at focus time. */
interface AwaySummary {
  turns: number;
  patches: number;
  approvals: number;
  spend: number;
}

/* --------------------------- cost preflight (D153) --------------------------- */

/** The last few completed turns are the sample; the floor is when a range means anything. */
const PREFLIGHT_SAMPLE = 8;
const PREFLIGHT_FLOOR = 3;
/** Rough system prompt and tool schemas on every step; only used to scale by draft size. */
const CONTEXT_TOKENS = 400;
const SNIPPET_LIMIT = 20;
const historyKey = (project: string) => `rigel.prompts.${project}`;
const snippetsKey = (project: string) => `rigel.snippets.${project}`;

/** Three starter snippets, used only until the project has stored its own list. */
const DEFAULT_SNIPPETS: Snippet[] = [
  {
    id: "snippet-review",
    label: "Review the diff",
    body: "Review the current diff for {risk}. Read the diff before the description; name the one risk that matters.",
  },
  {
    id: "snippet-failure",
    label: "Explain a failure",
    body: "`{command}` failed. Find the cause; do not patch anything yet.",
  },
  {
    id: "snippet-test",
    label: "Add a test",
    body: "Add a test for {behavior}. Run it and report the result.",
  },
];

function readHistory(project: string): PromptEntry[] {
  try {
    const raw = localStorage.getItem(historyKey(project));
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (row): row is PromptEntry =>
        typeof row === "object" &&
        row !== null &&
        typeof (row as PromptEntry).session === "string" &&
        typeof (row as PromptEntry).text === "string",
    );
  } catch {
    return [];
  }
}

function readSnippets(project: string): Snippet[] {
  try {
    const raw = localStorage.getItem(snippetsKey(project));
    if (raw === null) return DEFAULT_SNIPPETS;
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return DEFAULT_SNIPPETS;
    return parsed.filter(
      (row): row is Snippet =>
        typeof row === "object" &&
        row !== null &&
        typeof (row as Snippet).id === "string" &&
        typeof (row as Snippet).label === "string" &&
        typeof (row as Snippet).body === "string",
    );
  } catch {
    return DEFAULT_SNIPPETS;
  }
}

function writeSnippets(project: string, rows: Snippet[]): void {
  try {
    localStorage.setItem(snippetsKey(project), JSON.stringify(rows));
  } catch {
    // A blocked store only forgets the list; the menu works for this run.
  }
}

/**
 * Which harness runs a turn. A native `<select>` looks like Windows, not Rigel, and its option
 * list cannot be styled at all; the shared menu already has the theme, checked state, focus
 * handling and viewport clamping this needs.
 */
function HarnessPicker({
  value,
  onChange,
  disabled,
}: {
  value: "native" | "claude";
  onChange: (next: "native" | "claude") => void;
  disabled?: boolean;
}) {
  const contextMenu = useMenu();
  const chosen = value === "claude" ? "Claude" : "Native";

  const open = (event: ReactMouseEvent<HTMLButtonElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    contextMenu.open(
      {
        clientX: rect.left,
        clientY: rect.top,
        preventDefault: () => event.preventDefault(),
        stopPropagation: () => event.stopPropagation(),
      },
      {
        eyebrow: "Agent",
        title: "Who runs the turn?",
        subtitle: value === "claude" ? "Claude Code harness" : "Rigel native loop",
        sections: [
          {
            label: "Harness",
            items: [
              {
                label: "Rigel native",
                detail: "Rigel's own loop, with the selected model and tools.",
                icon: "spark",
                checked: value === "native",
                onSelect: () => onChange("native"),
              },
              {
                label: "Claude Code",
                detail: "Hand the turn to the installed Claude harness.",
                icon: "terminal",
                checked: value === "claude",
                onSelect: () => onChange("claude"),
              },
            ],
          },
        ],
        footer: "This chooses who answers; the model picker still sets the model.",
      },
    );
  };

  return (
    <div className="harness-pick">
      <button
        type="button"
        className="harness-pick-button"
        title="Choose whether Rigel or Claude answers"
        aria-haspopup="menu"
        aria-label={`Agent harness: ${value}`}
        disabled={disabled}
        onClick={open}
      >
        <span className={`harness-dot ${value}`} aria-hidden />
        <span className="harness-pick-label">{chosen}</span>
        <svg
          className="harness-chevron"
          width="12"
          height="12"
          viewBox="0 0 20 20"
          fill="none"
          aria-hidden
        >
          <path
            d="m5 8 5 5 5-5"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </button>
    </div>
  );
}

export default function SessionView({
  project,
  source,
  onSessions,
  onActive,
  onNotice,
  onOpenSettings,
  onBrowseSkills,
  projectControls,
  onSwarmPrepare,
  onSwarm,
  focus,
  onFocusHandled,
  onOpenHunk,
}: Props) {
  const tauri = isTauri();
  const contextMenu = useMenu();
  const [session, setSession] = useState<Session | null>(() => (tauri ? null : previewSession));
  const [recovery, setRecovery] = useState<RecoveryOffer | null>(() =>
    !tauri && new URLSearchParams(window.location.search).get("recovery") === "1"
      ? previewRecovery
      : null,
  );
  const [readOnly, setReadOnly] = useState(false);
  const [agents, setAgents] = useState<Agent[]>(() => (tauri ? [] : previewAgents));
  const [team, setTeam] = useState<TeamAgent[]>([]);
  const [rolePresets, setRolePresets] = useState<RolePreset[]>([]);
  const [plan, setPlan] = useState<PlanState | null>(null);
  // Roadmap 42: one graph per parent turn, refreshed as child turns land.
  const [subagentGraphs, setSubagentGraphs] = useState<SubagentGraph[]>(() =>
    !tauri && new URLSearchParams(window.location.search).get("dag") === "1"
      ? previewSubagentGraph
      : [],
  );
  const [turns, setTurns] = useState<TurnTranscript[]>(() => (tauri ? [] : previewTranscript));
  const [budget, setBudget] = useState<BudgetSnapshot | null>(() => (tauri ? null : previewBudget));
  const [patches, setPatches] = useState<Patch[]>(() => (tauri ? [] : previewPatches));
  const [checkpoints, setCheckpoints] = useState<Checkpoint[]>([]);
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  // Q270: pins are per session. The menu searches the same index as Shelf and writes the
  // current body hash; a template copies the id + hash pair.
  const [pins, setPins] = useState<SkillPin[]>([]);
  const [pinOpen, setPinOpen] = useState(false);
  const [pinQuery, setPinQuery] = useState("");
  const [pinResults, setPinResults] = useState<SkillRow[]>([]);
  const [pinBusy, setPinBusy] = useState<string | null>(null);
  const [pinLabels, setPinLabels] = useState<Record<string, string>>({});
  // D141: a changed pin opens one card with both bodies and a recorded Keep or Update.
  const [pinReview, setPinReview] = useState<SkillPinReview | null>(null);
  const [pinReviewBusy, setPinReviewBusy] = useState<"keep" | "update" | null>(null);
  const [pinReason, setPinReason] = useState("");
  const [pinChangedOnly, setPinChangedOnly] = useState(false);
  const [pinBatchReason, setPinBatchReason] = useState("");
  const [pinBatchBusy, setPinBatchBusy] = useState<"keep" | "update" | null>(null);
  const pinRef = useRef<HTMLDivElement | null>(null);
  const [stream, setStream] = useState<Record<string, string>>({});
  const [liveTurn, setLiveTurn] = useState<Turn | null>(null);
  // D40: one transcript, or two side by side. Never more than two, and never a grid.
  const [shown, setShown] = useState<string[]>([]);
  // Manual spawn: one small form instead of a hidden command. The agent joins the roster
  // immediately and the composer switches to it so a first task can follow at once.
  const [spawnOpen, setSpawnOpen] = useState(false);
  const [spawnRole, setSpawnRole] = useState("");
  const [spawnHarness, setSpawnHarness] = useState<"native" | "claude">("native");
  const [spawning, setSpawning] = useState(false);
  const [raw, setRaw] = useState(false);
  const [text, setText] = useState("");
  // Files for the next turn. The native picker copies them into .rigel/attachments; the
  // composer appends their project-relative paths when the turn is delivered.
  const [attachments, setAttachments] = useState<string[]>([]);
  const [addOpen, setAddOpen] = useState(false);
  const addRef = useRef<HTMLDivElement | null>(null);
  // D151: snippets are project-local drafts; history is the prompts this project has sent.
  const [snippets, setSnippets] = useState<Snippet[]>(() => readSnippets(project));
  const [snipOpen, setSnipOpen] = useState(false);
  const snippetRef = useRef<HTMLDivElement | null>(null);
  // D152: project-local templates, offered by the "+ session" button. Never committed.
  const [templates, setTemplates] = useState<SessionTemplate[]>([]);
  const [templateOpen, setTemplateOpen] = useState(false);
  const [templateSaving, setTemplateSaving] = useState(false);
  const templateRef = useRef<HTMLDivElement | null>(null);
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const pendingSelection = useRef<[number, number] | null>(null);
  const [historyIndex, setHistoryIndex] = useState<number | null>(null);
  const [historyDraft, setHistoryDraft] = useState("");
  // The draft waiting in the review card. Non-null means Send paused before a turn.
  const [review, setReview] = useState<string | null>(null);
  const [quote, setQuote] = useState<TurnQuote | null>(null);
  const [capInput, setCapInput] = useState("");
  const [keyPresent, setKeyPresent] = useState<boolean | null>(tauri ? null : true);
  const [warning, setWarning] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [testRun, setTestRun] = useState<TestRun | null>(null);
  const [testRunning, setTestRunning] = useState(false);
  const testRunFor = useRef<string | null>(null);
  // Who the composer sends to. A Claude turn is run by the harness; everything else is the
  // native loop. The picker is the only place this is chosen, so the transcript header can say
  // which agent answered.
  const [target, setTarget] = useState<"native" | "claude">("native");
  const [busy, setBusy] = useState(false);
  // A refinement is in flight: Send is disabled so a draft cannot be sent twice.
  const [refining, setRefining] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  // D148: the left pane is either the project's sessions or this session's agents. The list
  // lives here rather than in App because every edit returns the row the core stored.
  const [pane, setPane] = useState<"sessions" | "agents">("agents");
  const [list, setList] = useState<Session[]>(() =>
    tauri ? [] : [previewSession, previewArchivedSession],
  );
  const [sessionQuery, setSessionQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<"all" | Session["status"]>("all");
  const [showArchived, setShowArchived] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameText, setRenameText] = useState("");
  const renamingRef = useRef<string | null>(null);
  const [listBusy, setListBusy] = useState<string | null>(null);
  // Q274: the row waiting on the typed confirmation, and what has been typed so far.
  const [deleting, setDeleting] = useState<Session | null>(null);
  const [deleteText, setDeleteText] = useState("");
  const copyText = useCallback(
    async (text: string, label: string) => {
      if (!text) {
        onNotice(`${label} is empty.`);
        return;
      }
      try {
        await navigator.clipboard.writeText(text);
        onNotice(`${label} copied.`);
      } catch {
        onNotice(`Could not copy ${label.toLowerCase()} from this window.`);
      }
    },
    [onNotice],
  );
  // D136/D149: the filter is remembered per session; the outline tracks where the reader is.
  const [filter, setFilter] = useState<TranscriptFilter>("all");
  const [activeTurnId, setActiveTurnId] = useState<string | null>(null);
  const transcriptRef = useRef<HTMLDivElement | null>(null);
  // D155: one card on focus, built from what changed while the window was in the background.
  const [away, setAway] = useState<AwaySummary | null>(null);
  const awayMark = useRef<{ at: number; spend: number; approvals: number } | null>(null);
  const turnsRef = useRef(turns);
  const patchesRef = useRef(patches);
  const [defaults, setDefaults] = useState<Settings | null>(null);
  const [swarming, setSwarming] = useState(false);
  const [preparing, setPreparing] = useState(false);
  // The preflight is a list, not a question: the task text is frozen when it opens so the
  // selection cannot drift from what will actually be sent.
  const [swarmTask, setSwarmTask] = useState("");
  const [swarmTargets, setSwarmTargets] = useState<SwarmTarget[] | null>(null);
  const [swarmPicked, setSwarmPicked] = useState<string[]>([]);
  const [swarmBoss, setSwarmBoss] = useState<"rigel" | "model" | "broadcast">("rigel");
  // The boss model is local to this preflight. It starts on the composer's model but the picker
  // reads the full chat roster, so choosing a different door never silently changes the chat
  // default the composer will use after the Swarm is sent.
  const [swarmBossModel, setSwarmBossModel] = useState<{
    provider: string;
    model: string;
  } | null>(null);
  // The commands waiting on this window (P4.10e). Empty is the normal state.
  const [approvals, setApprovals] = useState<PendingApproval[]>(() =>
    tauri ? [] : previewApprovals.map((pending) => ({ ...pending })),
  );
  const approvalsSeen = useRef(0);
  // Q246: a question the channel refused while this window was open. It stays on the card —
  // with Retry — instead of vanishing, because the refusal is the moment the user can act.
  const [refusedApprovals, setRefusedApprovals] = useState<PendingApproval[]>([]);
  // An id this window answered or dismissed: its disappearance is not a refusal.
  const answeredApprovals = useRef<Set<string>>(new Set());
  const approvalsRef = useRef(approvals);
  const defaultsRef = useRef<Settings | null>(defaults);
  useEffect(() => {
    defaultsRef.current = defaults;
  }, [defaults]);

  // Q268: a result belongs to the session that asked for it; switching sessions clears it, and
  // a late answer cannot paint over the new one.
  useEffect(() => {
    testRunFor.current = session?.id ?? null;
    setTestRun(null);
  }, [project, session?.id]);

  // The focus listener runs on session boundaries only, so it reads the latest rows through
  // refs rather than restarting — and resetting its baseline — on every streamed item.
  useEffect(() => {
    turnsRef.current = turns;
  }, [turns]);
  useEffect(() => {
    patchesRef.current = patches;
  }, [patches]);
  useEffect(() => {
    approvalsRef.current = approvals;
  }, [approvals]);

  useEffect(() => {
    let live = true;
    api
      .rolePresets()
      .then((next) => live && setRolePresets(next))
      .catch(() => live && setRolePresets([]));
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    let live = true;
    if (!tauri || !project || !session) {
      setPlan(null);
      return;
    }
    api
      .planGet(project, session.id)
      .then((next) => live && setPlan(next))
      .catch(() => live && setPlan(null));
    return () => {
      live = false;
    };
  }, [tauri, project, session?.id]);

  useEffect(() => {
    let live = true;
    // The browser preview ships its own sample graph; only the desktop reads the store.
    if (!tauri) return;
    if (!project || !session) {
      setSubagentGraphs([]);
      return;
    }
    api
      .subagentGraph(project, session.id)
      .then((next) => live && setSubagentGraphs(next))
      .catch(() => live && setSubagentGraphs([]));
    return () => {
      live = false;
    };
  }, [tauri, project, session?.id, turns.length]);

  const spendMicros = budget?.session_micros ?? (session ? session.spend_cents * 10_000 : 0);
  const capMicros = budget?.cap_micros ?? (session?.budget_cents ? session.budget_cents * 10_000 : null);
  const model = defaults?.model ?? "gpt-5.6-sol-medium";
  const effort = defaults?.effort ?? "medium";
  // Sent with the model so the turn cannot land on the previous door while a save is in flight.
  const provider = defaults?.provider ?? "";

  const sessionRef = useRef<Session | null>(session);
  // A palette jump must not race the mount effect that opens the newest session. The ref keeps
  // the latest request visible to that effect without making it a dependency.
  const focusRef = useRef(focus);
  useEffect(() => {
    focusRef.current = focus;
  }, [focus]);
  const projectRef = useRef(project);
  const bottomRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    sessionRef.current = session;
    onActive(session);
  }, [session, onActive]);
  useEffect(() => {
    projectRef.current = project;
  }, [project]);

  // Q270: switching sessions switches their pins. A read failure shows no pins rather than a
  // stale list from the session that was open before.
  useEffect(() => {
    setPinReview(null);
    setPinReason("");
    setPinChangedOnly(false);
    setPinBatchReason("");
    if (!session) {
      setPins([]);
      return;
    }
    let live = true;
    api.pins
      .list(project, session.id)
      .then((next) => live && setPins(next))
      .catch(() => live && setPins([]));
    return () => {
      live = false;
    };
  }, [project, session?.id]);

  useEffect(() => {
    let live = true;
    const ids = pins.map((pin) => pin.skill_id);
    if (ids.length === 0) {
      setPinLabels({});
      return;
    }
    void Promise.all(
      ids.map(async (id) => {
        try {
          return [id, (await source.load(id)).skill.name] as const;
        } catch {
          return [id, id.split(/[:/]/).filter(Boolean).pop() ?? id] as const;
        }
      }),
    ).then((rows) => live && setPinLabels(Object.fromEntries(rows)));
    return () => {
      live = false;
    };
  }, [pins, source]);

  useEffect(() => {
    if (!pinOpen) return;
    const query = pinQuery.trim();
    if (!query) {
      setPinResults([]);
      return;
    }
    let live = true;
    const timer = window.setTimeout(() => {
      source
        .search(query, 6)
        .then((rows) => live && setPinResults(rows))
        .catch(() => live && setPinResults([]));
    }, 160);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [pinOpen, pinQuery, source]);

  // A command can be asked for at any moment while a turn runs, so the card polls. The read is
  // scoped to the open project, so another checkout's question is not this card's to answer.
  // In the browser the same call returns the sample queue, which is how this state is
  // inspectable.
  useEffect(() => {
    let alive = true;
    let release: (() => void) | undefined;
    const tick = async () => {
      try {
        const open = await api.approvals.pending(project);
        if (alive) {
          // Q246: a question that vanished at its deadline was refused, not answered. Keep it
          // in front of the user with Retry instead of letting the card disappear.
          const ids = new Set(open.map((row) => row.id));
          const gone = approvalsRef.current.filter(
            (row) =>
              !ids.has(row.id) &&
              !answeredApprovals.current.has(row.id) &&
              Date.now() >= approvalDeadline(row),
          );
          if (gone.length > 0) {
            setRefusedApprovals((rows) => [
              ...rows.filter((row) => !gone.some((old) => old.id === row.id)),
              ...gone,
            ]);
          }
          if (open.length > approvalsSeen.current) {
            notify(
              "approval",
              "Rigel · approval waiting",
              open[open.length - 1]?.title ?? "A command is waiting for your answer.",
              defaultsRef.current,
              document.visibilityState === "visible" && document.hasFocus(),
            );
          }
          approvalsSeen.current = open.length;
          setApprovals(open);
        }
      } catch {
        // A window that cannot read the channel shows no card; the command stays refused.
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), 1500);
    // D154: the app's watcher pushes a change as soon as the shared channel moves, so the card
    // does not wait for the poll. The poll stays as the fallback for a missed event.
    if (tauri) {
      void (async () => {
        const { listen } = await import("@tauri-apps/api/event");
        const unlisten = await listen("rigel://approval", () => void tick());
        if (alive) release = unlisten;
        else unlisten();
      })();
    }
    return () => {
      alive = false;
      window.clearInterval(timer);
      release?.();
    };
  }, [project, session?.id, tauri]);

  // The window's watcher follows the project the session belongs to, so a write by another
  // agent becomes a conflict row while the turn is still running.
  useEffect(() => {
    if (tauri && session) void api.watchProject(project);
  }, [tauri, project, session?.id]);

  const answer = async (
    pending: PendingApproval,
    scope: ApprovalAnswer,
    options?: { skip?: boolean; note?: string },
  ) => {
    // The row disappears as soon as it is answered, whatever the answer: a question that has
    // been decided must never look open.
    answeredApprovals.current.add(pending.id);
    setApprovals((open) => open.filter((row) => row.id !== pending.id));
    setRefusedApprovals((rows) => rows.filter((row) => row.id !== pending.id));
    try {
      await api.approvals.answer(pending.id, scope, options);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  };

  /* ------------------------------ load state ------------------------------ */

  const refresh = useCallback(async () => {
    const current = sessionRef.current;
    if (!tauri || !projectRef.current || !current) return;
    try {
      const [nextTurns, nextAgents, nextPatches, nextSession, nextTeam, nextCheckpoints, nextBookmarks] =
        await Promise.all([
          api.transcript(projectRef.current, current.id),
          api.listAgents(projectRef.current, current.id),
          api.listPatches(projectRef.current, current.id),
          api.getSession(projectRef.current, current.id),
          api.team(projectRef.current, current.id),
          api.listCheckpoints(projectRef.current, current.id),
          api.listBookmarks(projectRef.current, current.id),
        ]);
      setTurns(nextTurns);
      setAgents(nextAgents);
      setPatches(nextPatches);
      setCheckpoints(nextCheckpoints);
      setTeam(nextTeam);
      setBookmarks(nextBookmarks);
      setSession(nextSession);
      if (nextAgents[0]) {
        setBudget(await api.getBudget(projectRef.current, nextAgents[0].id));
      }
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  }, [tauri]);

  const selectSession = useCallback(
    async (next: Session) => {
      setSession(next);
      setTurns([]);
      setAgents([]);
      setPatches([]);
      setCheckpoints([]);
      setBookmarks([]);
      setTeam([]);
      setBudget(null);
      setRecovery(null);
      setReadOnly(false);
      setError(null);
      if (!tauri || !project) return;
      try {
        const [
          loadedTurns,
          loadedAgents,
          loadedPatches,
          fresh,
          loadedTeam,
          loadedCheckpoints,
          loadedBookmarks,
        ] = await Promise.all([
          api.transcript(project, next.id),
          api.listAgents(project, next.id),
          api.listPatches(project, next.id),
          api.getSession(project, next.id),
          api.team(project, next.id),
          api.listCheckpoints(project, next.id),
          api.listBookmarks(project, next.id),
        ]);
        setTurns(loadedTurns);
        setAgents(loadedAgents);
        setPatches(loadedPatches);
        setCheckpoints(loadedCheckpoints);
        setBookmarks(loadedBookmarks);
        setTeam(loadedTeam);
        setSession(fresh);
        // The row in the list is the same record as the open session; a resume can change it.
        setList((rows) => rows.map((row) => (row.id === fresh.id ? fresh : row)));
        if (loadedAgents[0]) {
          setBudget(await api.getBudget(project, loadedAgents[0].id));
        }
        const keys = await api.keyStatus();
        setKeyPresent(keys.native_ready);
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    },
    [tauri, project],
  );

  // Roadmap 37: ask the database, not a transient process flag, what a dead process left open.
  useEffect(() => {
    if (!tauri || !project || !session?.id) return;
    let live = true;
    void api
      .recoveryOffer(project, session.id)
      .then((offer) => {
        if (live) setRecovery(offer);
      })
      .catch(() => {
        if (live) setRecovery(null);
      });
    return () => {
      live = false;
    };
  }, [tauri, project, session?.id]);

  useEffect(() => {
    if (!tauri) return;
    if (!project) {
      setSession(null);
      return;
    }
    let live = true;
    (async () => {
      try {
        const [listed, keys] = await Promise.all([api.listSessions(project), api.keyStatus()]);
        if (!live) return;
        setKeyPresent(keys.native_ready);
        setList(listed);
        if (focusRef.current) {
          // A palette jump names its session; opening the newest first would race it and could
          // leave the wrong transcript on screen.
          onSessions(listed);
          return;
        }
        const pick =
          listed.find((candidate) => candidate.status !== "ended") ?? listed[0] ?? null;
        if (pick) {
          await selectSession(pick);
        } else {
          setSession(null);
          setTurns([]);
          setAgents([]);
          setTeam([]);
        }
        onSessions(listed);
      } catch (problem) {
        if (live) setError(problem instanceof Error ? problem.message : String(problem));
      }
    })();
    return () => {
      live = false;
    };
    // `selectSession` is stable for a given project; re-running on every session object would
    // refetch the transcript on each save.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tauri, project]);

  // D147: the palette can name a session that is not the newest one. Open it, then scroll to
  // the requested turn after the transcript has rendered; clear the request either way so it
  // cannot fire again on the next save.
  useEffect(() => {
    if (!focus || !tauri || !project) return;
    let live = true;
    void (async () => {
      if (sessionRef.current?.id !== focus.sessionId) {
        try {
          const target = await api.getSession(project, focus.sessionId);
          if (!live) return;
          await selectSession(target);
        } catch (problem) {
          if (!live) return;
          setError(problem instanceof Error ? problem.message : String(problem));
          onFocusHandled?.();
          return;
        }
      }
      if (!live) return;
      if (!focus.turnId) {
        onFocusHandled?.();
        return;
      }
      // One paint for the transcript that selectSession just set.
      window.setTimeout(() => {
        if (!live) return;
        const row = document.getElementById(`turn-${focus.turnId}`);
        if (row) {
          row.scrollIntoView({ block: "center" });
          row.classList.add("jump");
          window.setTimeout(() => row.classList.remove("jump"), 1800);
        }
        onFocusHandled?.();
      }, 80);
    })();
    return () => {
      live = false;
    };
  }, [focus, project, tauri, selectSession, onFocusHandled]);

  // The saved default (D28). The picker writes through the same settings command as Settings.
  useEffect(() => {
    let live = true;
    api
      .getSettings()
      .then((next) => live && setDefaults(next))
      .catch(() => live && setDefaults(null));
    return () => {
      live = false;
    };
  }, []);

  const applyModel = useCallback(
    async (nextModel: string, nextEffort: string, nextProvider?: string) => {
      const base: Settings = defaults ?? defaultSettings();
      const next = {
        ...base,
        model: nextModel,
        effort: nextEffort,
        // A hand-picked model is not the free model Free mode proved, so the button's state
        // must not keep claiming it is.
        free_mode: false,
        ...(nextProvider ? { provider: nextProvider } : {}),
      };
      setDefaults(next);
      try {
        setDefaults(await api.saveSettings(next));
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    },
    [defaults],
  );

  // Free mode refreshes every configured provider, lets Antigravity rank the live zero-price
  // rows, and then proves the winner with a real request. The saved pair survives a restart.
  const [freePicking, setFreePicking] = useState(false);
  const pickFree = useCallback(async () => {
    setFreePicking(true);
    try {
      const choice = await api.freeMode();
      setDefaults(await api.getSettings());
      onNotice(choice.note);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setFreePicking(false);
    }
  }, [onNotice]);

  // The picker's chat filter is a saved preference, not popover state: the same command as the
  // model, so a restart reads back what this window last chose.
  const applyChatOnly = useCallback(
    async (chatModelsOnly: boolean) => {
      const base: Settings = defaults ?? defaultSettings();
      const next = { ...base, chat_models_only: chatModelsOnly };
      setDefaults(next);
      try {
        setDefaults(await api.saveSettings(next));
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    },
    [defaults],
  );

  /* ------------------------------- events -------------------------------- */

  useEffect(() => {
    if (!tauri) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;

    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const release = await listen<AgentEvent>("rigel://events", (event) => {
        const payload = event.payload;
        const current = sessionRef.current;
        if (!current) return;

        switch (payload.type) {
          case "turn_started": {
            setTurns((previous) => {
              if (previous.some((turn) => turn.turn.id === payload.turn_id)) return previous;
              const turn: Turn = {
                id: payload.turn_id,
                agent_id: payload.agent_id,
                parent_turn_id: payload.parent_turn_id ?? null,
                seq: payload.seq,
                status: "running",
                prompt: payload.prompt ?? null,
                started_at: Date.now(),
                completed_at: null,
                usage: null,
                stop_reason: null,
              };
              return [...previous, { turn, items: [] }];
            });
            setLiveTurn({
              id: payload.turn_id,
              agent_id: payload.agent_id,
              parent_turn_id: payload.parent_turn_id ?? null,
              seq: payload.seq,
              status: "running",
              prompt: payload.prompt ?? null,
              started_at: Date.now(),
              completed_at: null,
              usage: null,
              stop_reason: null,
            });
            setStream((previous) => ({ ...previous, [payload.turn_id]: "" }));
            // A subagent is registered and starts its turn in the same step. Refreshing here puts
            // its roster row up while it works, so it can be watched instead of only being
            // visible after the parent turn finishes.
            void api
              .listAgents(projectRef.current, current.id)
              .then(setAgents)
              .catch(() => {});
            break;
          }
          case "delta": {
            if (payload.kind !== "message") break;
            setStream((previous) => ({
              ...previous,
              [payload.turn_id]: (previous[payload.turn_id] ?? "") + payload.text,
            }));
            break;
          }
          case "item": {
            const item = payload.item;
            setTurns((previous) =>
              previous.map((turn) =>
                turn.turn.id === item.turn_id && !turn.items.some((row) => row.id === item.id)
                  ? { ...turn, items: [...turn.items, item] }
                  : turn,
              ),
            );
            if (item.kind === "file_change") {
              void api.listPatches(projectRef.current, current.id).then(setPatches).catch(() => {});
            }
            break;
          }
          case "budget_warning": {
            setWarning(true);
            if (sessionRef.current) {
              void api
                .getSession(projectRef.current, sessionRef.current.id)
                .then((fresh) => {
                  setSession(fresh);
                  setBudget((old) =>
                    old
                      ? { ...old, session_micros: payload.spend_micros, cap_micros: payload.cap_micros }
                      : old,
                  );
                })
                .catch(() => {});
            }
            break;
          }
          case "budget_exceeded": {
            setWarning(true);
            setError(
              `Budget exceeded: ${money(payload.spend_micros)} spent of ${money(payload.cap_micros)}. Every running agent was interrupted.`,
            );
            break;
          }
          case "task_budget_exceeded": {
            // D92: only the task's agent stopped; the session cap is untouched.
            setError(
              `Task cap reached for ${payload.agent_id}: ${money(payload.spend_micros)} spent of ${money(payload.cap_micros)}. Other agents continue.`,
            );
            break;
          }
          case "daily_budget_warning": {
            // D116: the day's 80% is a warning, not a stop. The raise lives in Settings.
            setError(
              `Daily spend is ${money(payload.spend_micros)} of ${money(payload.cap_micros)}. The cap applies across every session in this project; raise it in Settings → Safety.`,
            );
            break;
          }
          case "daily_budget_exceeded": {
            setWarning(true);
            setError(
              `Daily cap reached: ${money(payload.spend_micros)} spent of ${money(payload.cap_micros)} on ${payload.day}. Every running agent was interrupted; raise the cap in Settings → Safety.`,
            );
            break;
          }
          case "turn_finished": {
            if (payload.status === "failed") {
              notify(
                "failed",
                "Rigel · turn failed",
                "The turn stopped with an error. Open the transcript for the reason.",
                defaultsRef.current,
                document.visibilityState === "visible" && document.hasFocus(),
              );
            } else if (payload.status === "completed") {
              notify(
                "finished",
                "Rigel · turn finished",
                `Spent ${money(payload.usage.cost_micros)} this turn.`,
                defaultsRef.current,
                document.visibilityState === "visible" && document.hasFocus(),
              );
            }
            setStream((previous) => {
              const next = { ...previous };
              delete next[payload.turn_id];
              return next;
            });
            setLiveTurn(null);
            setWarning(false);
            void refresh();
            void api
              .getSession(projectRef.current, current.id)
              .then(setSession)
              .catch(() => {});
            break;
          }
          case "session_updated": {
            setSession(payload.session);
            break;
          }
          case "error": {
            setError(payload.message);
            break;
          }
          default:
            break;
        }
      });
      if (disposed) {
        release();
      } else {
        unlisten = release;
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [tauri, refresh]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" });
  }, [turns, stream]);

  // Debounced so typing does not hammer the database; a quote is a read, but a read per
  // keystroke is still waste. The hard cap does not depend on this succeeding.
  useEffect(() => {
    const prompt = text.trim();
    if (!prompt || !session) {
      setQuote(null);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      api
        .quoteTurn(
          projectRef.current,
          session.id,
          prompt,
          spendMicros,
          capMicros,
          model,
          provider,
          effort,
        )
        .then((next) => {
          if (!cancelled) setQuote(next);
        })
        .catch(() => {
          if (!cancelled) setQuote(null);
        });
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [session, text, spendMicros, capMicros, model, effort]);

  useEffect(() => {
    if (!addOpen && !snipOpen && !templateOpen && !pinOpen) return;
    const closeOnPointer = (event: PointerEvent) => {
      if (!addRef.current?.contains(event.target as Node)) setAddOpen(false);
      if (!snippetRef.current?.contains(event.target as Node)) setSnipOpen(false);
      if (!templateRef.current?.contains(event.target as Node)) setTemplateOpen(false);
      if (!pinRef.current?.contains(event.target as Node)) setPinOpen(false);
    };
    const closeOnKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setAddOpen(false);
        setSnipOpen(false);
        setTemplateOpen(false);
        setPinOpen(false);
      }
    };
    window.addEventListener("pointerdown", closeOnPointer);
    window.addEventListener("keydown", closeOnKey);
    return () => {
      window.removeEventListener("pointerdown", closeOnPointer);
      window.removeEventListener("keydown", closeOnKey);
    };
  }, [addOpen, snipOpen, templateOpen, pinOpen]);

  // Snippets are stored per project, so opening another project reads its own list.
  useEffect(() => {
    setSnippets(readSnippets(project));
    setSnipOpen(false);
  }, [project]);

  // D151: the placeholder is selected after the inserted text commits. A layout effect runs
  // before the next paint, so typing cannot land in the old caret position first.
  useLayoutEffect(() => {
    const pending = pendingSelection.current;
    if (!pending) return;
    pendingSelection.current = null;
    const area = composerRef.current;
    if (!area) return;
    area.focus();
    area.setSelectionRange(pending[0], pending[1]);
  }, [text]);

  const withAttachments = useCallback(
    (prompt: string) => {
      if (attachments.length === 0) return prompt;
      return `${prompt}\n\nAttached files (copied into the project; read them if the task needs them):\n${attachments
        .map((path) => `- ${path}`)
        .join("\n")}`;
    },
    [attachments],
  );

  /** D151: the prompt that actually ran, once, per project. A restart keeps the list. */
  const rememberPrompt = useCallback(
    (prompt: string) => {
      if (!project || !session) return;
      try {
        const entries = readHistory(project).filter(
          (row) => !(row.session === session.id && row.text === prompt),
        );
        entries.push({ session: session.id, text: prompt, at: Date.now() });
        localStorage.setItem(
          historyKey(project),
          JSON.stringify(entries.slice(-HISTORY_LIMIT)),
        );
      } catch {
        // History is a convenience; a blocked store must not block the turn.
      }
      setHistoryIndex(null);
    },
    [project, session],
  );

  // Q270: pinning resolves the current body hash in the core; the menu never invents a
  // version. A second pin of the same skill reports the version already kept.
  const pinSkill = useCallback(
    async (skillId: string) => {
      if (!project || !session) return;
      const label =
        pinLabels[skillId] ?? skillId.split(/[:/]/).filter(Boolean).pop() ?? skillId;
      const wasPinned = pins.some((pin) => pin.skill_id === skillId);
      setPinBusy(skillId);
      try {
        const pinned = await api.pins.pin(project, session.id, skillId);
        setPins((current) =>
          [...current.filter((pin) => pin.skill_id !== pinned.skill_id), pinned].sort(
            (a, b) => a.pinned_at - b.pinned_at,
          ),
        );
        setPinQuery("");
        setPinResults([]);
        onNotice(
          wasPinned
            ? `“${label}” is already pinned to this session at its recorded body.`
            : `“${label}” pinned at its current body; this session keeps that version.`,
        );
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setPinBusy(null);
      }
    },
    [project, session, pins, pinLabels, onNotice],
  );

  const unpinSkill = useCallback(
    async (pin: SkillPin) => {
      if (!project || !session) return;
      const label =
        pinLabels[pin.skill_id] ?? pin.skill_id.split(/[:/]/).filter(Boolean).pop() ?? pin.skill_id;
      setPinBusy(pin.skill_id);
      try {
        const removed = await api.pins.unpin(project, session.id, pin.skill_id);
        if (removed) {
          setPins((current) => current.filter((row) => row.skill_id !== pin.skill_id));
          if (pinReview?.pin.skill_id === pin.skill_id) setPinReview(null);
        }
        onNotice(`“${label}” ${removed ? "unpinned" : "was already unpinned"}.`);
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setPinBusy(null);
      }
    },
    [project, session, pinLabels, pinReview, onNotice],
  );

  const openPinReview = useCallback(
    async (pin: SkillPin) => {
      if (!project || !session) return;
      setPinOpen(false);
      setError(null);
      try {
        setPinReview(await api.pins.review(project, session.id, pin.skill_id));
        setPinReason("");
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    },
    [project, session],
  );

  const settlePin = useCallback(
    async (mode: "keep" | "update") => {
      if (!project || !session || !pinReview) return;
      const label =
        pinLabels[pinReview.pin.skill_id] ??
        pinReview.pin.skill_id.split(/[:/]/).filter(Boolean).pop() ??
        pinReview.pin.skill_id;
      const reason =
        pinReason.trim() ||
        (mode === "keep"
          ? "Kept the pinned body after reviewing the index change."
          : "Updated to the indexed body after reviewing the change.");
      setPinReviewBusy(mode);
      try {
        const next =
          mode === "keep"
            ? await api.pins.keep(project, session.id, pinReview.pin.skill_id, reason)
            : await api.pins.update(project, session.id, pinReview.pin.skill_id, reason);
        setPins((current) =>
          current.map((pin) => (pin.skill_id === next.skill_id ? next : pin)),
        );
        setPinReview(null);
        setPinReason("");
        onNotice(
          mode === "keep"
            ? `“${label}” keeps its pinned body; the reason is recorded on this session.`
            : `“${label}” now reads the indexed body; the update is recorded on this session.`,
        );
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setPinReviewBusy(null);
      }
    },
    [project, session, pinReview, pinLabels, pinReason, onNotice],
  );

  const changedPins = useMemo(() => pins.filter((pin) => pin.changed), [pins]);
  const visiblePins = useMemo(
    () => (pinChangedOnly ? changedPins : pins),
    [pinChangedOnly, changedPins, pins],
  );

  const updateAllChanged = useCallback(async () => {
    if (!project || !session || changedPins.length === 0) return;
    const reason =
      pinBatchReason.trim() ||
      "Updated every changed pinned skill from the Skills menu.";
    setPinBatchBusy("update");
    try {
      const next = await api.pins.updateAll(project, session.id, reason);
      const remaining = next.filter((pin) => pin.changed).length;
      setPins(next);
      setPinBatchReason("");
      setPinOpen(false);
      setPinReview(null);
      onNotice(
        remaining === 0
          ? `${changedPins.length} changed pinned skill${
              changedPins.length === 1 ? "" : "s"
            } updated at once; one reason is recorded on this session.`
          : `${changedPins.length - remaining} of ${changedPins.length} changed pins updated; ${remaining} still need attention.`,
      );
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setPinBatchBusy(null);
    }
  }, [project, session, changedPins, pinBatchReason, onNotice]);

  const keepAllChanged = useCallback(async () => {
    if (!project || !session || changedPins.length === 0) return;
    const reason =
      pinBatchReason.trim() ||
      "Kept every changed pinned body after reviewing the index change.";
    setPinBatchBusy("keep");
    try {
      const next = await api.pins.keepAll(project, session.id, reason);
      setPins(next);
      setPinBatchReason("");
      setPinOpen(false);
      setPinReview(null);
      onNotice(
        `${changedPins.length} changed pinned skill${
          changedPins.length === 1 ? "" : "s"
        } kept at the pinned version; one reason is recorded on this session.`,
      );
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setPinBatchBusy(null);
    }
  }, [project, session, changedPins, pinBatchReason, onNotice]);

  const pinDiff = useMemo(
    () =>
      pinReview
        ? bodyDiff(pinReview.pinned_body ?? "", pinReview.current_body ?? "")
        : [],
    [pinReview],
  );

  /* ------------------------------- actions ------------------------------- */

  const startSession = useCallback(
    async (template?: SessionTemplate) => {
      if (!tauri || !project) {
        onNotice("Open the desktop app to create a session.");
        return;
      }
      setStarting(true);
      setTemplateOpen(false);
      try {
        const created = await api.startSession(project, template?.name ?? "Session");
        const pinFailures: string[] = [];
        for (const skill of template?.skills ?? []) {
          try {
            await api.pins.pin(project, created.id, skill.skill_id, skill.body_sha);
          } catch {
            pinFailures.push(skill.skill_id);
          }
        }
        const listed = await api.listSessions(project);
        setList(listed);
        onSessions(listed);
        await selectSession(created);
        if (!template) {
          onNotice(
            created.budget_cents === null
              ? "Session started without a cap."
              : `Session started with a ${cents(created.budget_cents)} cap.`,
          );
          return;
        }
        // The template's model and effort are applied through the same path the picker uses,
        // because a turn reads one window-wide default (D130); the roster and the prompt are
        // the template's own.
        if (template.model) {
          await applyModel(template.model, template.effort ?? effort);
        }
        if (template.max_steps && defaults) {
          setDefaults(await api.saveSettings({ ...defaults, max_steps: template.max_steps }));
        }
        for (const entry of template.agents) {
          const spawned = await api.spawnAgent(
            project,
            created.id,
            entry.harness === "claude" ? "claude" : "native",
            entry.role ?? undefined,
            entry.model ?? undefined,
          );
          setAgents((current) => [...current, spawned]);
        }
        if (template.prompt) {
          setText(template.prompt);
          composerRef.current?.focus();
        }
        const restored = template.skills?.length ?? 0;
        onNotice(
          `Started from “${template.name}”.${
            restored
              ? ` ${restored} pinned skill${restored === 1 ? "" : "s"} restored.`
              : ""
          }${
            pinFailures.length
              ? ` ${pinFailures.length} pin${pinFailures.length === 1 ? "" : "s"} could not be restored.`
              : ""
          } The prompt is in the composer; nothing has been sent.`,
        );
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setStarting(false);
      }
    },
    [
      tauri,
      project,
      defaults,
      effort,
      applyModel,
      selectSession,
      onSessions,
      onNotice,
    ],
  );

  // D152: templates belong to the project, so opening another one shows its own list. A
  // missing or unreadable file is an empty list, never an error on the session surface.
  useEffect(() => {
    if (!project) {
      setTemplates([]);
      return;
    }
    let live = true;
    api.templates
      .list(project)
      .then((next) => live && setTemplates(next))
      .catch(() => live && setTemplates([]));
    return () => {
      live = false;
    };
  }, [project]);

  /** Save the current shape of the session: model, effort, steps, roster and the draft. */
  const saveTemplate = useCallback(async () => {
    if (!project || !session) return;
    setTemplateSaving(true);
    try {
      const name = (session.title || "Session").slice(0, 60);
      const existing = templates.find(
        (row) => row.name.toLowerCase() === name.toLowerCase(),
      );
      const saved = await api.templates.save(project, {
        id: existing?.id ?? "",
        name,
        model,
        effort,
        max_steps: defaults?.max_steps ?? null,
        agents: agents.map((agent) => ({
          harness: agent.harness,
          role: agent.role,
          model: agent.model,
        })),
        skills: pins.map((pin) => ({ skill_id: pin.skill_id, body_sha: pin.body_sha })),
        prompt: text,
        created_at: existing?.created_at ?? 0,
      });
      setTemplates((current) =>
        [...current.filter((row) => row.id !== saved.id), saved].sort((a, b) =>
          a.name.localeCompare(b.name),
        ),
      );
      onNotice(`“${saved.name}” saved in this project's templates. Model ids stay out of git.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setTemplateSaving(false);
    }
  }, [project, session, templates, model, effort, defaults, agents, pins, text, onNotice]);

  const removeTemplate = useCallback(
    async (template: SessionTemplate) => {
      if (!project) return;
      try {
        await api.templates.delete(project, template.id);
        setTemplates((current) => current.filter((row) => row.id !== template.id));
        onNotice(`“${template.name}” removed from this project.`);
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    },
    [project, onNotice],
  );

  /* ------------------------- the session list (D148) ------------------------- */

  /** Replace one row with the stored answer and keep App's count in step. */
  const applySession = useCallback(
    (next: Session) => {
      const updated = list.map((row) => (row.id === next.id ? next : row));
      setList(updated);
      onSessions(updated);
      setSession((current) => (current && current.id === next.id ? next : current));
    },
    [list, onSessions],
  );

  const openListItem = useCallback(
    async (row: Session) => {
      if (row.id === sessionRef.current?.id) return;
      await selectSession(row);
    },
    [selectSession],
  );

  const togglePin = useCallback(
    async (row: Session) => {
      if (listBusy) return;
      const pinned = row.pinned_at === null;
      setListBusy(row.id);
      try {
        const next = tauri
          ? await api.pinSession(project, row.id, pinned)
          : { ...row, pinned_at: pinned ? Date.now() : null };
        applySession(next);
        onNotice(pinned ? "Pinned to the top of the list." : "Unpinned.");
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setListBusy(null);
      }
    },
    [tauri, project, listBusy, applySession, onNotice],
  );

  const toggleArchive = useCallback(
    async (row: Session) => {
      if (listBusy) return;
      const archived = row.archived_at === null;
      setListBusy(row.id);
      try {
        const next = tauri
          ? await api.archiveSession(project, row.id, archived)
          : { ...row, archived_at: archived ? Date.now() : null };
        applySession(next);
        onNotice(
          archived
            ? "Archived. It stays searchable from the command palette and resumable from the list."
            : "Restored to the list.",
        );
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setListBusy(null);
      }
    },
    [tauri, project, listBusy, applySession, onNotice],
  );

  // Q274: permanent deletion is archive-first and needs the id typed again. A session that is
  // currently open is refused here as well as in the core, so the transcript cannot vanish from
  // under the pane the user is reading.
  const requestDelete = (row: Session) => {
    if (listBusy) return;
    if (row.archived_at === null) {
      onNotice("Archive the session before deleting it.");
      return;
    }
    if (session?.id === row.id) {
      onNotice("Open another session before deleting this one.");
      return;
    }
    setDeleteText("");
    setDeleting(row);
  };

  const confirmDelete = useCallback(async () => {
    if (!deleting || deleteText !== deleting.id || listBusy) return;
    const row = deleting;
    setListBusy(row.id);
    try {
      if (tauri) await api.deleteSession(project, row.id);
      const next = list.filter((candidate) => candidate.id !== row.id);
      setList(next);
      onSessions(next);
      setDeleting(null);
      setDeleteText("");
      onNotice(`Deleted session ${row.id.slice(0, 8)}.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setListBusy(null);
    }
  }, [deleting, deleteText, listBusy, list, tauri, project, onSessions, onNotice]);

  const beginRename = (row: Session) => {
    renamingRef.current = row.id;
    setRenaming(row.id);
    setRenameText(row.title ?? "");
  };

  const cancelRename = () => {
    renamingRef.current = null;
    setRenaming(null);
  };

  const commitRename = useCallback(
    async (row: Session, raw: string) => {
      // Enter blurs the input, so both paths can arrive; only the first one commits.
      if (renamingRef.current !== row.id) return;
      renamingRef.current = null;
      setRenaming(null);
      const title = raw.trim();
      if (title === (row.title ?? "")) return;
      try {
        const next = tauri
          ? await api.renameSession(project, row.id, title || null)
          : { ...row, title: title || null };
        applySession(next);
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    },
    [tauri, project, applySession],
  );

  const visibleSessions = useMemo(() => {
    const needle = sessionQuery.trim().toLowerCase();
    return list
      .filter((row) => showArchived || row.archived_at === null)
      .filter((row) => statusFilter === "all" || row.status === statusFilter)
      .filter(
        (row) =>
          !needle ||
          (row.title ?? "").toLowerCase().includes(needle) ||
          row.id.toLowerCase().startsWith(needle),
      )
      .sort(
        (a, b) =>
          (b.pinned_at ?? 0) - (a.pinned_at ?? 0) || b.created_at - a.created_at,
      );
  }, [list, sessionQuery, statusFilter, showArchived]);

  const resume = useCallback(
    async (force: boolean) => {
      if (!tauri || !project || !session) return;
      setBusy(true);
      try {
        const outcome = await api.resumeSession(project, session.id, force);
        setRecovery(null);
        setReadOnly(false);
        await selectSession(outcome.session);
        const pending = outcome.patches.filter((patch) => patch.state === "pending").length;
        onNotice(
          outcome.clean
            ? "Tree matches the checkpoint. The session can continue."
            : `Tree changed since the checkpoint. ${pending} patch journal${pending === 1 ? "" : "s"} pending. ${
                force ? "Continuing on the new tree." : "Showing the session as interrupted."
              }`,
        );
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setBusy(false);
      }
    },
    [tauri, project, session, selectSession, onNotice],
  );

  const discardRecovery = useCallback(async () => {
    if (!recovery) return;
    const turn = recovery.turns[0];
    setBusy(true);
    try {
      if (tauri && project) await api.discardTurn(project, turn.id);
      setRecovery(null);
      setReadOnly(true);
      onNotice(`Discarded turn ${turn.seq} and wrote its tombstone.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  }, [tauri, project, recovery, onNotice]);

  /** The confirmed text starts the turn; a failure leaves it in the composer. */
  const deliver = useCallback(
    async (
      prompt: string,
      options?: {
        agentId?: string;
        keepComposer?: boolean;
        /** A consent-gated fallback carries the chosen pair without waiting for a re-render. */
        model?: string;
        provider?: string;
      },
    ) => {
    if (readOnly) {
      setError("This transcript is read-only; turn sending is disabled.");
      return;
    }
    if (!prompt || !session || busy) return;
    const chosenModel = options?.model ?? model;
    const chosenProvider = options?.provider ?? provider;
    const turnPrompt = options?.keepComposer ? prompt : withAttachments(prompt);
    if (!options?.keepComposer) setText("");
    setError(null);

    if (!tauri) {
      // Preview: a canned answer, labelled in the transcript header as sample data.
      const id = `preview-${Date.now()}`;
      setTurns((previous) => [
        ...previous,
        {
          turn: {
            id,
            agent_id: options?.agentId ?? agents[0]?.id ?? "preview",
            parent_turn_id: null,
            seq: previous.length + 1,
            status: "completed",
            prompt: turnPrompt,
            started_at: Date.now(),
            completed_at: Date.now(),
            usage: { input_tokens: 120, cached_input_tokens: 0, output_tokens: 30, reasoning_tokens: 0, cost_micros: 140, provider_micros: 140, source: "provider" },
            stop_reason: "completed",
          },
          items: [
            {
              id: `${id}-m`,
              turn_id: id,
              seq: 1,
              kind: "message",
              status: "complete",
              body: {
                role: "assistant",
                text: "Browser preview: the sample answer. Open the desktop app to run a real turn against OpenRouter.",
              },
              created_at: Date.now(),
            },
          ],
        },
      ]);
      rememberPrompt(prompt);
      if (!options?.keepComposer) setAttachments([]);
      return;
    }

    setBusy(true);
    try {
      // Q249: a continued turn names the agent that stopped. Otherwise a watched agent is the
      // one the next turn is for; without a selection the harness picker keeps the old
      // behaviour: the first agent of that harness answers.
      const requested = options?.agentId
        ? agents.find((agent) => agent.id === options.agentId)
        : undefined;
      if (options?.agentId && !requested) {
        setError(
          "The agent that stopped is no longer in this session. Start a new turn from the composer.",
        );
        return;
      }
      const watched = shown.length === 1
        ? agents.find((agent) => agent.id === shown[0] && !observedOnly(agent))
        : undefined;
      let worker = requested ?? watched ?? agents.find((agent) => agent.harness === target);
      if (!worker) {
        const spawned = await api.spawnAgent(
          project,
          session.id,
          target,
          target === "claude" ? "claude" : "worker",
          chosenModel,
        );
        setAgents((previous) => [...previous, spawned]);
        worker = spawned;
      }
      const reply = await api.sendTurn(
        project,
        session.id,
        worker.id,
        turnPrompt,
        chosenModel,
        chosenProvider,
        effort,
      );
      rememberPrompt(prompt);
      if (!options?.keepComposer) setAttachments([]);
      // The window specs compare this with route_explain; a shipped window never sets it.
      if (document.documentElement.dataset.e2e === "1") {
        (window as unknown as { __rigelLastTurnRoute?: unknown }).__rigelLastTurnRoute =
          reply.route;
      }
      setLiveTurn(reply.turn);
      setTurns((previous) => [
        ...previous,
        { turn: reply.turn, items: [] },
      ]);
      setStream((previous) => ({ ...previous, [reply.turn.id]: "" }));
      setBudget(await api.getBudget(project, worker.id));
    } catch (problem) {
      const message = problem instanceof Error ? problem.message : String(problem);
      setError(message);
      if (!options?.keepComposer) setText(prompt);
    } finally {
      setBusy(false);
    }
  }, [
    session,
    busy,
    tauri,
    agents,
    shown,
    project,
    model,
    provider,
    effort,
    target,
    withAttachments,
    rememberPrompt,
    readOnly,
  ]);

  /** Q249: restart the stopped task with a fresh turn and a fresh step budget. */
  const continueTurn = useCallback(
    (entry: TurnTranscript) => {
      if (!entry.turn.prompt || busy || liveTurn) return;
      const stopped = agents.find((agent) => agent.id === entry.turn.agent_id);
      if (!stopped) {
        setError(
          "The agent that stopped is no longer in this session. Start a new turn from the composer.",
        );
        return;
      }
      void deliver(entry.turn.prompt, { agentId: stopped.id, keepComposer: true });
    },
    [agents, busy, deliver, liveTurn],
  );

  /** Q266: prepare a turn for another agent. The composer gets the draft; nothing is sent. */
  const handoffTurn = useCallback(
    (entry: TurnTranscript, agent: Agent) => {
      const source = agents.find((candidate) => candidate.id === entry.turn.agent_id);
      setText(handoffPrompt(entry, source ? source.role ?? source.harness : "another agent"));
      setShown([agent.id]);
      setError(null);
      composerRef.current?.focus();
      onNotice(`Handoff ready for ${agent.role ?? agent.harness}; nothing has been sent.`);
    },
    [agents, onNotice],
  );

  /** Q268: explicit, local, and outside the agent loop. Nothing is sent to a model. */
  const runTests = useCallback(async () => {
    if (!session || testRunning) return;
    const id = session.id;
    testRunFor.current = id;
    setTestRunning(true);
    setTestRun(null);
    setError(null);
    try {
      const report = await api.tests.run(project);
      if (testRunFor.current === id) setTestRun(report);
    } catch (problem) {
      if (testRunFor.current === id) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    } finally {
      setTestRunning(false);
    }
  }, [session, testRunning, project]);

  /** Q246: the asker stopped listening at the deadline, so Retry is a new turn that asks the
   * same agent to request the action again. The composer draft is left alone (Q249). */
  const retryApproval = (pending: PendingApproval) => {
    answeredApprovals.current.add(pending.id);
    setApprovals((rows) => rows.filter((row) => row.id !== pending.id));
    setRefusedApprovals((rows) => rows.filter((row) => row.id !== pending.id));
    const agent = pending.agent_id
      ? agents.find((candidate) => candidate.id === pending.agent_id)
      : undefined;
    const prompt = retryPrompt(pending);
    if (!agent || !prompt) {
      setError("The agent that asked is no longer in this session. Ask again from the composer.");
      return;
    }
    void deliver(prompt, { agentId: agent.id, keepComposer: true });
  };

  /** A refused question the user does not want to retry. The asker already stopped waiting. */
  const dismissApproval = (pending: PendingApproval) => {
    answeredApprovals.current.add(pending.id);
    setApprovals((rows) => rows.filter((row) => row.id !== pending.id));
    setRefusedApprovals((rows) => rows.filter((row) => row.id !== pending.id));
  };

  /** Copy files picked in the native dialog, then let the next turn name their paths. */
  const attachFiles = useCallback(async () => {
    setAddOpen(false);
    if (!tauri) return;
    try {
      setError(null);
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: true,
        title: "Attach files",
        defaultPath: project,
      });
      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      if (paths.length === 0) return;
      const imported = await api.importAttachments(project, paths);
      if (imported.length === 0) return;
      setAttachments((current) => [
        ...current,
        ...imported.filter((path) => !current.includes(path)),
      ]);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  }, [tauri, project]);

  const autoRefine = defaults?.auto_refine ?? false;

  /**
   * Send reviews first, unless auto-refine is on: then the cheap model tightens the brief and
   * the result goes straight out. A refiner failure is not a gate — the typed draft still sends.
   */
  const send = useCallback(async () => {
    if (readOnly) {
      setError("This transcript is read-only; turn sending is disabled.");
      return;
    }
    const prompt =
      text.trim() ||
      (attachments.length > 0 ? "Read the attached files and tell me what matters." : "");
    if (!prompt || !session || busy || review || refining) return;
    setAddOpen(false);
    // A scripted window has no provider to refine with, and refinement is a real model call.
    if (document.documentElement.dataset.e2e === "1") {
      await deliver(prompt);
      return;
    }
    if (!autoRefine) {
      setReview(prompt);
      return;
    }
    setRefining(true);
    setError(null);
    try {
      const result = await api.refine.run(project || null, {
        draft: prompt,
        target:
          shown.length === 1
            ? (agents.find((agent) => agent.id === shown[0])?.role ?? "watched agent")
            : target === "claude"
              ? "Claude Code"
              : "Rigel native",
        channel: null,
        recent: turns
          .slice(-4)
          .map((entry) => entry.turn.prompt)
          .filter((entry): entry is string => Boolean(entry))
          .map((entry) => `user: ${entry.slice(0, 400)}`),
        answers: [],
        round: 0,
      });
      await deliver(result.refined.trim() || prompt);
    } catch (problem) {
      // The refiner is an optimisation, not a gate: a failure still sends what was typed, and
      // says so. `deliver` clears the error line, so this notice is the durable record.
      onNotice(
        `Prompt refinement failed; sent the original draft. ${
          problem instanceof Error ? problem.message : String(problem)
        }`,
      );
      await deliver(prompt);
    } finally {
      setRefining(false);
    }
  }, [
    text,
    session,
    busy,
    review,
    refining,
    autoRefine,
    attachments,
    project,
    shown,
    agents,
    target,
    turns,
    deliver,
    onNotice,
  ]);

  /** Saved, not per-draft: a person who turns it on wants it on for the next prompt too. */
  const toggleAutoRefine = useCallback(async () => {
    if (!defaults) return;
    const base: Settings = defaults;
    const next = { ...base, auto_refine: !base.auto_refine };
    setDefaults(next);
    try {
      setDefaults(await api.saveSettings(next));
    } catch (problem) {
      setDefaults(base);
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  }, [defaults]);

  /** D151: newest first, scoped to this session unless a modifier asks for the project. */
  const promptHistory = useCallback(
    (all: boolean) => {
      if (!project || !session) return [];
      return readHistory(project)
        .filter((row) => all || row.session === session.id)
        .sort((a, b) => a.at - b.at)
        .slice(-HISTORY_LIMIT);
    },
    [project, session],
  );

  const cycleHistory = (direction: -1 | 1, all: boolean) => {
    const entries = promptHistory(all);
    if (entries.length === 0) return;
    if (direction === -1) {
      if (historyIndex === null) setHistoryDraft(text);
      const next = historyIndex === null ? entries.length - 1 : Math.max(0, historyIndex - 1);
      setHistoryIndex(next);
      setText(entries[next].text);
      return;
    }
    if (historyIndex === null) return;
    if (historyIndex >= entries.length - 1) {
      setHistoryIndex(null);
      setText(historyDraft);
    } else {
      setHistoryIndex(historyIndex + 1);
      setText(entries[historyIndex + 1].text);
    }
  };

  /** An inserted snippet is text, never a send: Enter still opens the review card. */
  const applySnippet = (snippet: Snippet) => {
    const field = composerRef.current;
    const focused = field !== null && document.activeElement === field;
    const start = focused ? (field.selectionStart ?? text.length) : text.length;
    const end = focused ? (field.selectionEnd ?? text.length) : text.length;
    const separator = !focused && text.length > 0 && !text.endsWith("\n") ? "\n\n" : "";
    const inserted = separator + snippet.body;
    setText(text.slice(0, start) + inserted + text.slice(end));
    setSnipOpen(false);
    setHistoryIndex(null);
    const offset = start + separator.length;
    const open = snippet.body.indexOf("{");
    if (open >= 0) {
      const close = snippet.body.indexOf("}", open);
      pendingSelection.current = [
        offset + open,
        offset + (close >= 0 ? close + 1 : open + 1),
      ];
    } else {
      const end = offset + snippet.body.length;
      pendingSelection.current = [end, end];
    }
  };

  const saveDraftSnippet = () => {
    const body = text.trim();
    if (!body) return;
    const label = (body.split("\n")[0] || "snippet").slice(0, 48);
    const next = [...snippets, { id: `snippet-${Date.now()}`, label, body }].slice(
      -SNIPPET_LIMIT,
    );
    setSnippets(next);
    writeSnippets(project, next);
    setSnipOpen(false);
    onNotice("Draft saved as a snippet. It inserts text; nothing sends.");
  };

  const removeSnippet = (id: string) => {
    const next = snippets.filter((row) => row.id !== id);
    setSnippets(next);
    writeSnippets(project, next);
  };

  /** Up/Down only at the boundary, so a caret inside a draft still moves like a caret. */
  const onComposerKeyDown = (event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void send();
      return;
    }
    if (event.shiftKey) return;
    const all = event.metaKey || event.ctrlKey;
    const field = event.currentTarget;
    if (
      event.key === "ArrowUp" &&
      field.selectionStart === 0 &&
      field.selectionEnd === 0
    ) {
      if (promptHistory(all).length === 0) return;
      event.preventDefault();
      cycleHistory(-1, all);
    } else if (
      event.key === "ArrowDown" &&
      field.selectionStart === text.length &&
      field.selectionEnd === text.length &&
      historyIndex !== null
    ) {
      event.preventDefault();
      cycleHistory(1, all);
    }
  };

  /** Swarm, first press: freeze the task and ask the room who can take it. */
  const prepareSwarm = useCallback(async () => {
    if (!onSwarmPrepare) return;
    const task = text.trim();
    if (!task) {
      onNotice("Type the swarm task in the composer first.");
      return;
    }
    setPreparing(true);
    setAddOpen(false);
    setSwarmBoss("rigel");
    setSwarmBossModel(provider && model ? { provider, model } : null);
    setError(null);
    try {
      const targets = await onSwarmPrepare();
      setSwarmTask(withAttachments(task));
      setSwarmTargets(targets);
      setSwarmPicked(targets.map((target) => target.name));
    } catch (problem) {
      // Refusals (no room, unauthed room, nobody online) read the same as any other error.
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setPreparing(false);
    }
  }, [onSwarmPrepare, text, onNotice, withAttachments, provider, model]);

  const toggleSwarmTarget = useCallback((name: string) => {
    setSwarmPicked((picked) =>
      picked.includes(name) ? picked.filter((known) => known !== name) : [...picked, name],
    );
  }, []);

  /** Swarm, second press: send to the agents that are still ticked, in roster order. */
  const launchSwarm = useCallback(async () => {
    if (!onSwarm || !swarmTargets) return;
    const chosen = swarmTargets
      .map((target) => target.name)
      .filter((name) => swarmPicked.includes(name));
    if (chosen.length === 0) return;
    const lead: SwarmLead =
      swarmBoss === "broadcast"
        ? { kind: "broadcast" }
        : swarmBoss === "model" && swarmBossModel
          ? { kind: "model", ...swarmBossModel }
          : { kind: "rigel" };
    if (swarmBoss === "model" && !swarmBossModel) {
      setError("Pick a chat model before using it as the big boss.");
      return;
    }
    setSwarming(true);
    setError(null);
    try {
      await onSwarm(swarmTask, chosen, lead);
      setSwarmTargets(null);
      setSwarmPicked([]);
      setSwarmBoss("rigel");
      setSwarmBossModel(null);
      setAttachments([]);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setSwarming(false);
    }
  }, [onSwarm, swarmTargets, swarmPicked, swarmTask, swarmBoss, swarmBossModel]);

  const cancel = useCallback(async () => {
    if (!liveTurn) return;
    try {
      await api.cancelTurn(liveTurn.id);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  }, [liveTurn]);

  const pause = useCallback(async () => {
    if (!liveTurn) return;
    try {
      await api.pauseTurn(liveTurn.id);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  }, [liveTurn]);

  const resumeTurn = useCallback(async () => {
    if (!liveTurn) return;
    try {
      await api.resumeTurn(liveTurn.id);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  }, [liveTurn]);

  const raise = useCallback(async () => {
    if (!tauri || !session) return;
    const dollars = Number(capInput || "0");
    if (!Number.isFinite(dollars) || dollars <= 0) return;
    try {
      const fresh = await api.raiseBudget(project, session.id, Math.round(dollars * 100));
      setSession(fresh);
      setWarning(false);
      setCapInput("");
      if (agents[0]) setBudget(await api.getBudget(project, agents[0].id));
      onNotice(`Cap raised to $${dollars.toFixed(2)}.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  }, [tauri, session, capInput, project, agents, onNotice]);

  const revert = useCallback(
    async (id: string) => {
      try {
        await api.revertPatch(id);
        if (session) setPatches(await api.listPatches(project, session.id));
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    },
    [project, session],
  );

  /* -------------------------------- render ------------------------------- */

  const toggleShown = useCallback((id: string) => {
    setShown((current) =>
      current.includes(id)
        ? current.filter((entry) => entry !== id)
        : current.length < 2
          ? [...current, id]
          : // A third selection replaces the older pane; the cap is what keeps two readable.
            [current[1], id],
    );
  }, []);

  /* ----------------------------- bookmarks (D150) ----------------------------- */

  const toggleItemBookmark = useCallback(
    async (item: Item, seq: number) => {
      if (!session) return;
      const label = itemBookmarkLabel(item, seq);
      const marked = bookmarks.some(
        (row) => row.kind === "item" && row.target_id === item.id,
      );
      try {
        const next = tauri
          ? await api.toggleBookmark(project, session.id, "item", item.id, label)
          : marked
            ? null
            : {
                id: `preview-${item.id}`,
                session_id: session.id,
                kind: "item" as const,
                target_id: item.id,
                label,
                created_at: Date.now(),
              };
        setBookmarks((rows) =>
          next
            ? [
                next,
                ...rows.filter(
                  (row) => !(row.kind === "item" && row.target_id === item.id),
                ),
              ]
            : rows.filter((row) => !(row.kind === "item" && row.target_id === item.id)),
        );
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    },
    [tauri, project, session, bookmarks],
  );

  const removeBookmark = useCallback(
    async (row: Bookmark) => {
      if (!session) return;
      try {
        if (tauri) {
          await api.toggleBookmark(project, session.id, row.kind, row.target_id, row.label);
        }
        setBookmarks((rows) => rows.filter((entry) => entry.id !== row.id));
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    },
    [tauri, project, session],
  );

  const jumpBookmark = (row: Bookmark) => {
    if (row.kind === "hunk") {
      onOpenHunk?.(row.target_id);
      return;
    }
    // The line may be filtered out of the transcript; a bookmark jump is still a jump.
    if (filter !== "all") chooseFilter("all");
    window.setTimeout(() => {
      const el = document.getElementById(`item-${row.target_id}`);
      if (!el) return;
      el.scrollIntoView({ block: "center" });
      el.classList.add("jump");
      window.setTimeout(() => el.classList.remove("jump"), 1800);
    }, 30);
  };

  // D40: a subagent is an agent of its own, spawned by a turn of another agent. The parent turn
  // names the row it hangs under, so the roster nests instead of listing children as peers.
  const roster = useMemo(() => {
    const parentOf = new Map<string, string>();
    for (const entry of turns) {
      const parentTurn = entry.turn.parent_turn_id;
      if (!parentTurn) continue;
      const parent = turns.find((candidate) => candidate.turn.id === parentTurn);
      if (parent && parent.turn.agent_id !== entry.turn.agent_id) {
        parentOf.set(entry.turn.agent_id, parent.turn.agent_id);
      }
    }
    const children = new Map<string, Agent[]>();
    const roots: Agent[] = [];
    for (const agent of agents) {
      const parent = parentOf.get(agent.id);
      if (parent && agents.some((candidate) => candidate.id === parent)) {
        children.set(parent, [...(children.get(parent) ?? []), agent]);
      } else {
        roots.push(agent);
      }
    }
    const rows: { agent: Agent; nested: boolean }[] = [];
    for (const root of roots) {
      rows.push({ agent: root, nested: false });
      for (const child of children.get(root.id) ?? []) {
        rows.push({ agent: child, nested: true });
      }
    }
    return rows;
  }, [agents, turns]);

  // Subagent turns are fetched for the roster and replay, but the conversation view shows the
  // top-level turns: children are summarised under the turn that spawned them. A pane for the
  // subagent itself is the exception — there, its own turns are exactly what it is for.
  const shownTurns = useMemo(
    () => turns.filter((entry) => entry.turn.parent_turn_id === null),
    [turns],
  );

  // D136/D149: counts and marks describe what is on screen, not the whole project. In the
  // all-agents view that is every top-level turn; in a watched pane it is that agent's turns.
  const paneEntries = useMemo(
    () =>
      shown.length === 0
        ? shownTurns
        : turns.filter((entry) => shown.includes(entry.turn.agent_id)),
    [shown, shownTurns, turns],
  );

  const filterCounts = useMemo(() => {
    const counts: Record<TranscriptFilter, number> = {
      all: 0,
      tools: 0,
      diffs: 0,
      errors: 0,
      yours: 0,
    };
    for (const entry of paneEntries) {
      if (entry.turn.prompt) {
        counts.all += 1;
        counts.yours += 1;
      }
      for (const item of entry.items) {
        counts.all += 1;
        if (matchesFilter(item, "tools")) counts.tools += 1;
        if (matchesFilter(item, "diffs")) counts.diffs += 1;
        if (matchesFilter(item, "errors")) counts.errors += 1;
        if (matchesFilter(item, "yours")) counts.yours += 1;
      }
    }
    return counts;
  }, [paneEntries]);

  const outline = useMemo<OutlineGroup[]>(() => {
    const groups: OutlineGroup[] = [];
    for (const entry of paneEntries) {
      const marks: OutlineGroup["marks"] = [];
      if (entry.turn.prompt) {
        marks.push({
          key: `prompt-${entry.turn.id}`,
          kind: "prompt",
          title: `turn ${entry.turn.seq} · your prompt: ${entry.turn.prompt.slice(0, 90)}`,
        });
      }
      for (const item of entry.items) {
        if (item.kind === "file_change") {
          const result = bodyRecord(item, "result");
          const path =
            result && typeof result.path === "string" ? result.path : bodyText(item, "path");
          marks.push({
            key: `diff-${item.id}`,
            kind: "diff",
            title: `turn ${entry.turn.seq} · patch ${path}`.trim(),
          });
        } else if (isErrorItem(item)) {
          const message =
            item.kind === "error"
              ? bodyText(item, "message")
              : `${item.kind} ${item.status}`;
          marks.push({
            key: `error-${item.id}`,
            kind: "error",
            title: `turn ${entry.turn.seq} · error: ${message.slice(0, 80)}`,
          });
        } else if (item.kind === "approval") {
          marks.push({
            key: `approval-${item.id}`,
            kind: "approval",
            title: `turn ${entry.turn.seq} · approval ${bodyText(item, "kind") || item.status}`,
          });
        }
      }
      if (marks.length > 0) {
        groups.push({ turnId: entry.turn.id, seq: entry.turn.seq, marks });
      }
    }
    return groups;
  }, [paneEntries]);

  // Q249: only the newest turn of an agent can be continued. An older stopped turn stays a
  // record, not a second button that would duplicate the task.
  const latestTurnByAgent = useMemo(() => {
    const latest = new Map<string, string>();
    for (const entry of turns) latest.set(entry.turn.agent_id, entry.turn.id);
    return latest;
  }, [turns]);

  // The filter is one state per project + session: opening another session shows its own view,
  // and a restart does not silently restore `all` over a deliberate choice.
  useEffect(() => {
    if (!project || !session) {
      setFilter("all");
      return;
    }
    setFilter(readFilter(project, session.id));
  }, [project, session?.id]);

  const chooseFilter = (next: TranscriptFilter) => {
    setFilter(next);
    if (!project || !session) return;
    try {
      localStorage.setItem(filterKey(project, session.id), next);
    } catch {
      // A blocked store forgets the choice; the transcript still filters for this run.
    }
  };

  const jumpToTurn = useCallback((turnId: string) => {
    const row = document.getElementById(`turn-${turnId}`);
    if (!row) return;
    row.scrollIntoView({ block: "center" });
    row.classList.add("jump");
    window.setTimeout(() => row.classList.remove("jump"), 1600);
  }, []);

  // D155: opening or switching a session is not time away; the first focus after it shows
  // nothing. The card reports ids and counts, never prompts, and the notification path (D139)
  // stays the place a click can open a turn.
  useEffect(() => {
    if (!session) return;
    const stamp = () => ({
      at: Date.now(),
      spend: sessionRef.current?.spend_micros ?? 0,
      approvals: approvalsRef.current.length,
    });
    awayMark.current = stamp();
    setAway(null);
    const onBlur = () => {
      awayMark.current = stamp();
    };
    const onFocus = () => {
      const mark = awayMark.current;
      if (!mark) return;
      const finished = turnsRef.current.filter(
        (entry) => entry.turn.completed_at !== null && entry.turn.completed_at > mark.at,
      ).length;
      const merged = patchesRef.current.filter(
        (patch) =>
          patch.status === "applied" && patch.applied_at !== null && patch.applied_at > mark.at,
      ).length;
      const waiting = approvalsRef.current.length;
      const spent = Math.max(0, (sessionRef.current?.spend_micros ?? mark.spend) - mark.spend);
      const changed = finished + merged + spent > 0 || waiting !== mark.approvals;
      awayMark.current = stamp();
      if (!changed) return;
      setAway({ turns: finished, patches: merged, approvals: waiting, spend: spent });
    };
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
    };
  }, [session?.id, project]);

  const trackOutline = useCallback(() => {
    const container = transcriptRef.current;
    if (!container) return;
    const rows = container.querySelectorAll<HTMLElement>("article.turn");
    if (rows.length === 0) return;
    const top = container.getBoundingClientRect().top + 32;
    let current = rows[0].id.replace(/^turn-/, "");
    for (const row of rows) {
      if (row.getBoundingClientRect().top <= top) current = row.id.replace(/^turn-/, "");
      else break;
    }
    setActiveTurnId(current);
  }, []);

  // Before the first scroll event, point the rail at the turn already in view (the newest one).
  useEffect(() => {
    if (activeTurnId === null && shownTurns.length > 0) {
      setActiveTurnId(shownTurns[shownTurns.length - 1].turn.id);
    }
  }, [activeTurnId, shownTurns]);

  const live = liveTurn !== null;
  const percent = capMicros ? Math.min(100, (spendMicros / capMicros) * 100) : 0;

  // D153: a range from what completed turns actually cost, scaled to this draft's size. Three
  // turns are the floor for saying anything at all; below that the line says so rather than
  // borrowing confidence from one bill. This is arithmetic on recorded bills, not a quote, and
  // it never disables Send.
  const costPreflight = useMemo(() => {
    const sample = turns
      .filter(
        (entry) =>
          entry.turn.parent_turn_id === null &&
          entry.turn.status === "completed" &&
          entry.turn.usage !== null &&
          entry.turn.usage.cost_micros > 0,
      )
      .slice(-PREFLIGHT_SAMPLE);
    const costs = sample.map((entry) => entry.turn.usage!.cost_micros).sort((a, b) => a - b);
    const prompts = sample
      .map((entry) => Math.ceil((entry.turn.prompt?.length ?? 0) / 4))
      .sort((a, b) => a - b);
    const draftTokens = Math.ceil((text.trim().length + attachments.join("").length) / 4);
    const typical = prompts.length > 0 ? prompts[Math.floor(prompts.length / 2)] : 0;
    const scale =
      typical > 0
        ? Math.min(3, Math.max(0.4, (draftTokens + CONTEXT_TOKENS) / (typical + CONTEXT_TOKENS)))
        : 1;
    const at = (fraction: number) =>
      Math.max(1, Math.round(costs[Math.round((costs.length - 1) * fraction)] * scale));
    return {
      ready: costs.length >= PREFLIGHT_FLOOR,
      count: costs.length,
      low: costs.length > 0 ? at(0.2) : 0,
      high: costs.length > 0 ? at(0.8) : 0,
      draftTokens,
    };
  }, [turns, text, attachments]);
  const quoteWarning = quote?.over_remaining
    ? quote.remaining_micros !== null && quote.remaining_micros < 0
      ? "the next turn is refused before it starts."
      : quote.remaining_micros !== null && quote.remaining_micros < quote.reserve_micros
        ? `too little for the ${money(quote.reserve_micros)} reserve; the turn will be refused before it starts.`
        : "more than the cap leaves; the cap stops the turn after the step that crosses."
    : null;
  const nearCap = capMicros !== null && capMicros > 0 && spendMicros >= capMicros * 0.8;
  // The newest browser call, whichever agent made it. `latestBrowserCall` reads the same item
  // bodies the transcript renders, so no extra session query is needed.
  const browserCall = useMemo(() => latestBrowserCall(turns), [turns]);
  const browserAgent = browserCall
    ? (agents.find((agent) => agent.id === browserCall.agentId) ?? null)
    : null;

  if (!project) {
    return (
      <>
        <aside className="list-pane">
          <div className="list-head">
            <div className="list-title">
              <h1>Session</h1>
              <span>P3</span>
            </div>
          </div>
          <p className="empty">Open a project first — the session log lives in its .rigel folder.</p>
        </aside>
        <main id="main" tabIndex={-1} className="detail session-main">
          {projectControls}
          <p className="empty">No project open.</p>
        </main>
        <aside className="inspector" />
      </>
    );
  }

  const togglePlan = async () => {
    if (!session) return;
    const enable = !(plan?.mode ?? false);
    try {
      const next = await api.planSet(project, session.id, enable);
      setPlan(next);
      onNotice(
        enable
          ? "Plan mode on: reads only, no edits or commands until you approve."
          : next.tools.length > 0
            ? `Plan approved; ${next.tools.length} tool${next.tools.length === 1 ? "" : "s"} may run without asking again.`
            : "Plan approved; the next turn may edit.",
      );
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };

  const applyRole = async (agent: Agent, preset: string) => {
    if (!session || !preset) return;
    try {
      const next = await api.roleApply(project, session.id, agent.id, preset);
      setAgents((rows) => rows.map((row) => (row.id === next.id ? next : row)));
      onNotice(`${next.role ?? "agent"} preset applied; the next turn reads it.`);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };

  const spawnAgent = async () => {
    if (!session || !tauri || spawning) return;
    setSpawning(true);
    setError(null);
    try {
      const role = spawnRole.trim() || `agent-${agents.length + 1}`;
      const spawned = await api.spawnAgent(project, session.id, spawnHarness, role, model);
      setAgents((rows) => [...rows, spawned]);
      setShown([spawned.id]);
      setSpawnOpen(false);
      setSpawnRole("");
      onNotice(
        `Spawned ${spawned.role ?? role}. The composer now sends to it; ask it for something small.`,
      );
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setSpawning(false);
    }
  };

  return (
    <>
      <aside className="list-pane roster" data-pane={pane}>
        <div className="list-head">
          <div className="segmented session-tabs" role="group" aria-label="Sessions or agents">
            <button
              type="button"
              className={pane === "sessions" ? "on" : ""}
              aria-pressed={pane === "sessions"}
              onClick={() => setPane("sessions")}
            >
              sessions
            </button>
            <button
              type="button"
              className={pane === "agents" ? "on" : ""}
              aria-pressed={pane === "agents"}
              onClick={() => setPane("agents")}
            >
              agents
            </button>
          </div>
          <div className="list-title">
            <h1>{pane === "sessions" ? "Sessions" : "Agents"}</h1>
            <span>
              {pane === "sessions"
                ? `${visibleSessions.length}/${list.length}`
                : session
                  ? session.status
                  : "no session"}
            </span>
          </div>
          {pane === "sessions" ? (
            <div className="session-filters">
              <label className="search">
                <span aria-hidden>⌕</span>
                <input
                  value={sessionQuery}
                  onChange={(event) => setSessionQuery(event.target.value)}
                  placeholder="Filter sessions"
                  aria-label="Filter sessions"
                />
              </label>
              <div className="session-filter-row">
                <select
                  className="chip"
                  aria-label="Filter sessions by status"
                  value={statusFilter}
                  onChange={(event) =>
                    setStatusFilter(event.target.value as "all" | Session["status"])
                  }
                >
                  <option value="all">any status</option>
                  <option value="active">active</option>
                  <option value="idle">idle</option>
                  <option value="interrupted">interrupted</option>
                  <option value="ended">ended</option>
                </select>
                <button
                  type="button"
                  className={showArchived ? "chip on" : "chip"}
                  aria-pressed={showArchived}
                  onClick={() => setShowArchived((shown) => !shown)}
                  title="Archived sessions are hidden by default and never deleted"
                >
                  archived
                </button>
                <div className="template-wrap" ref={templateRef}>
                  <button
                    type="button"
                    className="chip"
                    onClick={() =>
                      templates.length > 0 ? setTemplateOpen((open) => !open) : void startSession()
                    }
                    disabled={starting}
                    aria-haspopup={templates.length > 0 ? "menu" : undefined}
                    aria-expanded={templates.length > 0 ? templateOpen : undefined}
                    title={
                      templates.length > 0
                        ? "Start blank or from a project template"
                        : "Start a session with a $5.00 cap"
                    }
                  >
                    {starting ? "starting…" : "+ session"}
                  </button>
                  {templateOpen ? (
                    <div className="composer-menu template-menu" role="menu" aria-label="Session templates">
                      <p className="composer-menu-label">Start from</p>
                      <button
                        type="button"
                        role="menuitem"
                        className="composer-menu-item"
                        onClick={() => void startSession()}
                      >
                        <span>Blank session</span>
                        <small>$5.00 cap</small>
                      </button>
                      {templates.map((template) => (
                        <div className="template-row" key={template.id}>
                          <button
                            type="button"
                            role="menuitem"
                            className="composer-menu-item"
                            onClick={() => void startSession(template)}
                            title={
                              template.prompt
                                ? `Prompt: ${template.prompt.slice(0, 160)}`
                                : "No prompt skeleton"
                            }
                          >
                            <span>{template.name}</span>
                            <small>
                              {template.agents.length} agent
                              {template.agents.length === 1 ? "" : "s"}
                              {template.skills?.length
                                ? ` · ${template.skills.length} skill${template.skills.length === 1 ? "" : "s"}`
                                : ""}
                              {template.model ? ` · ${template.model.split("/").pop()}` : ""}
                            </small>
                          </button>
                          <button
                            type="button"
                            className="snippet-delete"
                            aria-label={`Delete template ${template.name}`}
                            title="Delete this template"
                            onClick={() => void removeTemplate(template)}
                          >
                            ×
                          </button>
                        </div>
                      ))}
                      <p className="composer-menu-hint">
                        Stored in .rigel/templates.json, never committed: a model id is
                        machine-specific. The prompt lands in the composer, unsent.
                      </p>
                    </div>
                  ) : null}
                </div>
              </div>
            </div>
          ) : !session ? (
            <button className="primary wide" onClick={() => void startSession()} disabled={starting}>
              {starting ? "Starting…" : "Start a session · $5.00 cap"}
            </button>
          ) : (
            <div className="roster-actions">
              <button className={shown.length === 0 ? "chip on" : "chip"} onClick={() => setShown([])}>
                all agents
              </button>
              <button
                className="chip"
                onClick={() => setSpawnOpen((open) => !open)}
                disabled={!tauri || !session}
                title="Start another native or Claude agent in this session"
              >
                + agent
              </button>
              <button
                className="chip"
                onClick={() => void saveTemplate()}
                disabled={!session || templateSaving}
                title="Save this session's model, effort, steps, roster, pinned skills and draft as a project template"
              >
                {templateSaving ? "saving…" : "save template"}
              </button>
              {shown.length === 2 ? <span className="fine">two transcripts side by side</span> : null}
              {shown.length === 1 && rolePresets.length ? (
                <select
                  className="chip"
                  aria-label="Apply a role preset to the selected agent"
                  value={
                    agents.find((agent) => agent.id === shown[0])?.role ?? ""
                  }
                  onChange={(event) => {
                    const agent = agents.find((row) => row.id === shown[0]);
                    if (agent) void applyRole(agent, event.target.value);
                  }}
                >
                  <option value="">role preset…</option>
                  {rolePresets.map((preset) => (
                    <option key={preset.id} value={preset.id}>
                      {preset.title} · {preset.effort}
                    </option>
                  ))}
                </select>
              ) : null}
              {session.status === "interrupted" ? (
                <>
                  <button className="chip warn" onClick={() => void resume(false)} disabled={busy}>
                    resume
                  </button>
                  <button className="chip danger" onClick={() => void resume(true)} disabled={busy}>
                    continue on changed tree
                  </button>
                </>
              ) : null}
            </div>
          )}
          {pane === "sessions" ? (
            <p className="fine roster-hint">
              Click a row to open it. Hover to rename, pin or archive; an archived session stays
              searchable from the command palette and resumable from this list.
            </p>
          ) : session ? (
            <p className="fine roster-hint">
              Click an agent to watch its transcript; two can sit side by side.
            </p>
          ) : null}
        </div>

        <div className="session-list-pane">
          <div className="rows">
            {visibleSessions.map((row) => (
              <div
                key={row.id}
                className={`session-row${session?.id === row.id ? " on" : ""}`}
                onContextMenu={(event) => {
                  if ((event.target as HTMLElement).closest("input, textarea")) return;
                  contextMenu.open(event, {
                    eyebrow: "Session",
                    title: row.title ?? "Untitled session",
                    subtitle: `${row.status} · ${money(row.spend_micros)} · ${when(row.created_at)}`,
                    sections: [
                      {
                        label: "Session",
                        items: [
                          {
                            label: session?.id === row.id ? "Current session" : "Open session",
                            detail: row.archived_at !== null ? "Archived, still resumable" : row.status,
                            icon: "open",
                            disabled: session?.id === row.id,
                            onSelect: () => void openListItem(row),
                          },
                          {
                            label: row.pinned_at !== null ? "Unpin from top" : "Pin to top",
                            icon: "pin",
                            checked: row.pinned_at !== null,
                            disabled: listBusy === row.id,
                            onSelect: () => void togglePin(row),
                          },
                          {
                            label: "Rename…",
                            detail: row.title ?? "Untitled session",
                            icon: "edit",
                            onSelect: () => beginRename(row),
                          },
                          {
                            label: row.archived_at !== null ? "Restore to list" : "Archive session",
                            detail:
                              row.archived_at !== null
                                ? "Searchable again"
                                : "Stays searchable and resumable",
                            icon: "archive",
                            disabled: listBusy === row.id,
                            onSelect: () => void toggleArchive(row),
                          },
                          {
                            label: "Delete permanently…",
                            detail:
                              row.archived_at === null
                                ? "Archive it first"
                                : "Typed confirmation; cannot be undone",
                            icon: "trash",
                            disabled:
                              row.archived_at === null ||
                              listBusy === row.id ||
                              session?.id === row.id,
                            onSelect: () => requestDelete(row),
                          },
                        ],
                      },
                      {
                        label: "Copy",
                        items: [
                          {
                            label: "Session id",
                            detail: row.id,
                            icon: "copy",
                            onSelect: () => void copyText(row.id, "Session id"),
                          },
                          {
                            label: "Session title",
                            icon: "quote",
                            onSelect: () =>
                              void copyText(row.title ?? "Untitled session", "Session title"),
                          },
                        ],
                      },
                    ],
                    footer: `${row.id.slice(0, 8)} · ${row.status}`,
                  });
                }}
              >
                {renaming === row.id ? (
                  <input
                    className="session-rename"
                    autoFocus
                    value={renameText}
                    onChange={(event) => setRenameText(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        event.currentTarget.blur();
                      } else if (event.key === "Escape") {
                        event.preventDefault();
                        cancelRename();
                      }
                    }}
                    onBlur={() => void commitRename(row, renameText)}
                    aria-label={`Rename ${row.title ?? "session"}`}
                    placeholder="Session name"
                  />
                ) : (
                  <button
                    type="button"
                    className="session-open"
                    onClick={() => void openListItem(row)}
                    title={`${row.title ?? "Untitled session"} · ${when(row.created_at)} · ${money(
                      row.spend_micros,
                    )}`}
                  >
                    <span className="row-text">
                      <span className="row-name">
                        {row.pinned_at !== null ? (
                          <span className="pin-flag" aria-hidden="true">
                            ★
                          </span>
                        ) : null}
                        <span className="session-name">{row.title ?? "Untitled session"}</span>
                        {row.archived_at !== null ? <span className="tag">archived</span> : null}
                      </span>
                      <span className="row-meta session-meta">
                        <span className={`session-state ${row.status}`}>{row.status}</span>
                        <span aria-hidden="true">·</span>
                        <span>{when(row.created_at).slice(0, 10)}</span>
                        <span aria-hidden="true">·</span>
                        <span>{money(row.spend_micros)}</span>
                      </span>
                    </span>
                  </button>
                )}
                {renaming === row.id ? null : (
                  <div className="session-row-actions">
                    <button
                      type="button"
                      className="session-act"
                      aria-pressed={row.pinned_at !== null}
                      disabled={listBusy === row.id}
                      title={row.pinned_at !== null ? "Unpin" : "Pin to the top"}
                      onClick={() => void togglePin(row)}
                    >
                      {row.pinned_at !== null ? "★" : "☆"}
                    </button>
                    <button
                      type="button"
                      className="session-act"
                      title="Rename"
                      aria-label={`Rename ${row.title ?? "session"}`}
                      onClick={() => beginRename(row)}
                    >
                      ✎
                    </button>
                    <button
                      type="button"
                      className="session-act"
                      disabled={listBusy === row.id}
                      title={
                        row.archived_at !== null
                          ? "Restore to the list"
                          : "Archive: hidden by default, still searchable and resumable"
                      }
                      aria-label={row.archived_at !== null ? "Restore session" : "Archive session"}
                      onClick={() => void toggleArchive(row)}
                    >
                      {row.archived_at !== null ? "restore" : "archive"}
                    </button>
                  </div>
                )}
              </div>
            ))}
            {visibleSessions.length === 0 ? (
              <p className="empty subtle">
                {list.length === 0
                  ? "No session in this project yet. Start one from the button above."
                  : "No session matches this filter."}
              </p>
            ) : null}
          </div>
          <div className="roster-foot">
            <span>
              {list.length} session{list.length === 1 ? "" : "s"}
            </span>
            <span className="grow" />
            <span className="mono">
              {list.filter((row) => row.archived_at !== null).length} archived
            </span>
          </div>
        </div>

        <div className="agent-list-pane">
        {spawnOpen ? (
          <form
            className="agent-spawn"
            aria-label="Spawn an agent"
            onSubmit={(event) => {
              event.preventDefault();
              void spawnAgent();
            }}
          >
            <input
              autoFocus
              value={spawnRole}
              onChange={(event) => setSpawnRole(event.target.value)}
              placeholder={`role, e.g. reviewer (agent-${agents.length + 1})`}
              aria-label="New agent role"
              disabled={spawning}
            />
            <div className="agent-spawn-actions">
              <HarnessPicker
                value={spawnHarness}
                onChange={setSpawnHarness}
                disabled={spawning}
              />
              <button className="primary" type="submit" disabled={spawning}>
                {spawning ? "Spawning…" : "Spawn"}
              </button>
              <button
                className="ghost-btn"
                type="button"
                onClick={() => setSpawnOpen(false)}
                disabled={spawning}
              >
                Cancel
              </button>
            </div>
          </form>
        ) : null}

        <div className="rows">
          {roster.map(({ agent, nested }) => {
            const assignment = team.find((row) => row.agent_key === agent.id);
            const working = assignment?.files.join(", ");
            return (
            <button
              key={agent.id}
              className={`row agent-row${nested ? " nested" : ""}${shown.includes(agent.id) ? " on" : ""}`}
              onClick={() => toggleShown(agent.id)}
              aria-pressed={shown.includes(agent.id)}
              title={
                nested
                  ? "A subagent: click to watch its transcript in the main pane; up to two can be open."
                  : "Click to watch this agent's transcript; up to two can be open."
              }
            >
              <span className={`status-dot ${agent.status}`} title={agent.status} />
              <span className="row-text">
                <span className="row-name">
                  {nested ? (
                    <span className="nest-mark" aria-hidden="true">
                      ↳
                    </span>
                  ) : null}
                  {agent.role ?? "agent"}
                  <span className="tag">{agent.harness}</span>
                  {observedOnly(agent) ? (
                    <span
                      className="tag observed"
                      title="Rigel shows this agent's turns but cannot send it new work yet."
                    >
                      observed only
                    </span>
                  ) : null}
                  {shown.includes(agent.id) ? <span className="tag watching">watching</span> : null}
                </span>
                <span className="row-desc" title={assignment?.task}>
                {working ? `working on ${working}` : agent.model ?? "no model"}
              </span>
              </span>
              <span className="row-meta">
                {agent.claims_held > 0 ? (
                  <span
                    className={`claim-count${agent.claims_stale > 0 ? " stale" : ""}`}
                    title={`${agent.claims_held} file claim(s) held${
                      agent.claims_stale > 0 ? `, ${agent.claims_stale} stale` : ""
                    } — see Diff`}
                  >
                    {agent.claims_held} claimed
                    {agent.claims_stale > 0 ? ` · ${agent.claims_stale} stale` : ""}
                  </span>
                ) : null}
                {money(agent.spend_micros)}
              </span>
            </button>
            );
          })}
          {agents.length === 0 && session ? (
            <p className="empty subtle">No agent yet. The first turn starts one.</p>
          ) : null}
        </div>

        <div className="roster-foot">
          <span className={`status-dot ${live ? "running" : "idle"}`} />
          <span>{live ? `turn ${liveTurn?.seq} running` : "idle"}</span>
          <span className="grow" />
          {/* D40 lists claims beside spend. The roster points at the Diff tab, where the ledger
              shows holder, staleness and conflicts together rather than a bare count here. */}
          <span
            className="claim-state"
            title="Advisory claims are in the ledger; the Diff tab shows who holds what, and who wrote a file they did not claim."
          >
            claims on the Diff tab
          </span>
          <span className="mono">{compact(agents.reduce((n, a) => n + a.tokens_in + a.tokens_out, 0))} tok</span>
        </div>
        </div>
      </aside>

      <main id="main" tabIndex={-1} className="detail session-main">
        {projectControls}
        <header className="detail-head">
          <div>
            <p className="eyebrow">
              {project.split(/[\\/]/).pop() || project}
              {!tauri ? " · sample transcript" : session ? ` · ${session.id.slice(0, 8)}` : ""}
            </p>
            <h2>
              {session?.title ?? "Session"}
              {readOnly ? <span className="pill read-only">read-only</span> : null}
            </h2>
            <p className="lede">
              {session
                ? `${session.interventions}/${session.tasks_done} interventions/tasks · ${money(
                    session.spend_micros,
                  )} spent · ${cents(session.budget_cents)} cap`
                : "No session in this project yet."}
            </p>
          </div>
          <div className="session-tools">
            <ModelControls
              model={model}
              provider={provider}
              effort={effort}
              chatOnly={defaults?.chat_models_only ?? true}
              onModel={(next, provider) => void applyModel(next, effort, provider)}
              onEffort={(next) => void applyModel(model, next)}
              onChatOnly={(next) => void applyChatOnly(next)}
            />
            <button
              type="button"
              className={`chip${defaults?.free_mode ? " on" : ""}`}
              onClick={() => void pickFree()}
              disabled={freePicking || live}
              title="Refresh every provider, let Antigravity rank the live free models, and probe the best candidates within five minutes"
            >
              {freePicking ? "scanning free models…" : "free mode"}
            </button>
            <HarnessPicker
              value={target}
              onChange={setTarget}
              disabled={live || review !== null}
            />
            {keyPresent === false && onOpenSettings ? (
              <button className="chip warn" onClick={onOpenSettings}>
                Add key
              </button>
            ) : null}
            {plan?.mode ? (
              <button className="chip on" onClick={() => void togglePlan()}>
                {plan.tools.length > 0 ? `approve plan (${plan.tools.length})` : "approve plan"}
              </button>
            ) : (
              <button className="chip" onClick={() => void togglePlan()} disabled={!session}>
                plan
              </button>
            )}
            <label className="toggle">
              <input type="checkbox" checked={raw} onChange={(event) => setRaw(event.target.checked)} />
              raw
            </label>
            {session ? (
              <button
                className="chip"
                onClick={() => void runTests()}
                disabled={testRunning}
                title="Run the project's detected test command; output stays local"
              >
                {testRunning ? "Running tests…" : "Run tests"}
              </button>
            ) : null}
            {session ? (
              <button
                className="chip"
                disabled={exporting}
                onClick={() => {
                  setExporting(true);
                  void api
                    .exportSession(project, session.id)
                    .then((bundle) => {
                      onNotice(`Session exported to ${bundle.markdown_path}`);
                      void navigator.clipboard?.writeText(bundle.markdown);
                    })
                    .catch((failure) =>
                      onNotice(failure instanceof Error ? failure.message : "Export failed."),
                    )
                    .finally(() => setExporting(false));
                }}
              >
                {exporting ? "Exporting…" : "Export session"}
              </button>
            ) : null}
            {session ? (
              <button
                className="chip"
                disabled={publishing}
                title="Send this session's prompts and replies to your Firebase project, where the phone can read them"
                onClick={() => {
                  setPublishing(true);
                  void api
                    .cloudPublishSession(project, session.id)
                    .then((report) =>
                      onNotice(
                        `Published ${report.turns} turn${report.turns === 1 ? "" : "s"} — the phone can read them when it signs in.`,
                      ),
                    )
                    .catch((failure) =>
                      onNotice(
                        failure instanceof Error ? failure.message : "Publish failed.",
                      ),
                    )
                    .finally(() => setPublishing(false));
                }}
              >
                {publishing ? "Publishing…" : "Publish to phone"}
              </button>
            ) : null}
            {liveTurn ? (
              <>
                <button className="chip" onClick={() => void pause()}>
                  Pause
                </button>
                <button className="chip" onClick={() => void resumeTurn()}>
                  Resume
                </button>
                <button className="danger-button" onClick={() => void cancel()}>
                  Stop turn
                </button>
              </>
            ) : null}
          </div>
        </header>

        {recovery ? (
          <Recovery
            offer={recovery}
            busy={busy}
            readOnly={readOnly}
            onResume={() => {
              if (tauri) void resume(false);
              else {
                setRecovery(null);
                setReadOnly(false);
              }
            }}
            onReadOnly={() => setReadOnly(true)}
            onDiscard={() => void discardRecovery()}
          />
        ) : null}

        {testRun ? (
          <section className="test-run" role="region" aria-label="Test results">
            <div className="test-run-head">
              <span className={`test-run-state ${testRun.ok ? "ok" : "bad"}`}>
                {testRun.timed_out ? "timed out" : testRun.ok ? "passed" : "failed"}
              </span>
              <span>
                <strong>{testRun.passed}</strong> passed
              </span>
              <span>
                <strong>{testRun.failed}</strong> failed
              </span>
              <span className="mono test-run-command">{testRun.command}</span>
              <span className="fine">
                exit {testRun.exit_code ?? "—"} ·{" "}
                {testRun.duration_ms < 1_000
                  ? `${testRun.duration_ms} ms`
                  : `${(testRun.duration_ms / 1_000).toFixed(1)} s`}
              </span>
              {testRun.truncated ? <span className="fine">output capped</span> : null}
              <span className="grow" />
              <button className="ghost-btn" onClick={() => setTestRun(null)}>
                Dismiss
              </button>
            </div>
            <details className="test-output">
              <summary>{testRun.truncated ? "output (capped)" : "full output"}</summary>
              <pre>{testRun.output || "No output."}</pre>
            </details>
          </section>
        ) : null}

        <DegradedBanner
          turns={turns}
          provider={provider}
          model={model}
          busy={busy || Boolean(liveTurn)}
          onRetry={(entry) => {
            if (!entry.turn.prompt) return;
            void deliver(entry.turn.prompt, {
              agentId: entry.turn.agent_id,
              keepComposer: true,
            });
          }}
          onFallback={(entry, offer) => {
            if (!entry.turn.prompt) return;
            void applyModel(offer.model, effort, offer.provider).then(() =>
              deliver(entry.turn.prompt!, {
                agentId: entry.turn.agent_id,
                keepComposer: true,
                model: offer.model,
                provider: offer.provider,
              }),
            );
          }}
        />

        {approvals.map((pending) => (
          <ApprovalCard
            key={pending.id}
            pending={pending}
            onAnswer={answer}
            onRetry={retryApproval}
            onDismiss={dismissApproval}
            retryDisabled={busy || Boolean(liveTurn)}
          />
        ))}
        {refusedApprovals.map((pending) => (
          <ApprovalCard
            key={pending.id}
            pending={pending}
            refused
            onAnswer={answer}
            onRetry={retryApproval}
            onDismiss={dismissApproval}
            retryDisabled={busy || Boolean(liveTurn)}
          />
        ))}

        {away ? (
          <div className="away-card" role="status" aria-live="polite">
            <div>
              <strong>While you were away</strong>
              <span>
                {[
                  away.turns ? `${away.turns} turn${away.turns === 1 ? "" : "s"} finished` : null,
                  away.patches
                    ? `${away.patches} patch${away.patches === 1 ? "" : "es"} merged`
                    : null,
                  away.approvals
                    ? `${away.approvals} approval${away.approvals === 1 ? "" : "s"} waiting`
                    : null,
                  away.spend ? `${money(away.spend)} spent` : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
            </div>
            <button className="ghost-btn" onClick={() => setAway(null)}>
              Dismiss
            </button>
          </div>
        ) : null}

        <p className="notice" role="status">
          {error
            ? error
            : !tauri
              ? "Browser preview: sample transcript. Sending adds a canned answer; no model is called."
              : keyPresent === false
                ? "No provider key in the OS keychain. The native agent cannot start a turn until one is stored."
                : warning
                  ? "Spend reached 80% of the cap. Raise it or stop; the next turn is refused once it is crossed."
                  : session?.status === "interrupted"
                    ? "This session was interrupted. Resume to reconcile the tree and the patch journal; nothing is auto-applied."
                    : "Every turn is billed to this session. The cap stops work within one turn."}
        </p>

        {!session ? (
          <section className="onboarding">
            <h3>No session yet</h3>
            <p>
              A session is the project-level thread. It stores turns, items and checkpoints
              locally; nothing is committed.
            </p>
          </section>
        ) : (
          <>
            <div className="transcript-bar" role="group" aria-label="Transcript filter">
              {FILTERS.map((row) => (
                <button
                  key={row.id}
                  type="button"
                  className={filter === row.id ? "chip on" : "chip"}
                  aria-pressed={filter === row.id}
                  onClick={() => chooseFilter(row.id)}
                  title={
                    row.id === "yours"
                      ? "Your prompts and notes, without the agent's tool traffic"
                      : `Show only ${row.label}`
                  }
                >
                  {row.label}
                  <span className="filter-count">{filterCounts[row.id]}</span>
                </button>
              ))}
            </div>
            <div className="transcript-shell">
            <div className="transcripts" ref={transcriptRef} onScroll={trackOutline}>
            <div className={shown.length === 2 ? "transcript-panes split" : "transcript-panes"}>
              {(shown.length === 0 ? [null] : shown).map((id) => {
                const agent =
                  id === null ? null : (agents.find((candidate) => candidate.id === id) ?? null);
                const entries =
                  id === null
                    ? shownTurns
                    : turns.filter((entry) => entry.turn.agent_id === id);
                return (
                  <section
                    key={id ?? "all"}
                    className="transcript"
                    aria-label={agent ? `${agent.role ?? agent.harness} transcript` : "Transcript"}
                  >
                    {agent ? (
                      <header className="pane-head">
                        <span className="row-name">
                          {agent.role ?? "agent"}
                          <span className="tag">{agent.harness}</span>
                        </span>
                        <span className="grow" />
                        <span className="mono">{money(agent.spend_micros)}</span>
                      </header>
                    ) : null}
                    {entries.length === 0 ? (
                      <p className="empty">No turns yet. Ask for something small first.</p>
                    ) : null}
                    {subagentGraphs
                      .filter((graph) =>
                        entries.some((entry) => entry.turn.id === graph.parent_turn_id),
                      )
                      .map((graph) => (
                        <SubagentGraphView key={graph.parent_turn_id} graph={graph} />
                      ))}
                    {entries.map((entry) => (
                      <TurnCard
                        key={entry.turn.id}
                        entry={entry}
                        turns={turns}
                        raw={raw}
                        filter={filter}
                        bookmarks={bookmarks}
                        onBookmark={toggleItemBookmark}
                        onCopy={copyText}
                        stream={stream}
                        checkpoint={checkpoints.find((row) => row.turn_id === entry.turn.id)}
                        canContinue={
                          latestTurnByAgent.get(entry.turn.agent_id) === entry.turn.id &&
                          session?.status !== "interrupted" &&
                          session?.status !== "ended"
                        }
                        continueDisabled={
                          busy || Boolean(liveTurn) || refining || Boolean(review)
                        }
                        onContinue={continueTurn}
                        handoffAgents={agents.filter((agent) => !observedOnly(agent))}
                        onHandoff={handoffTurn}
                        onRestore={async (checkpoint) => {
                          if (
                            !window.confirm(
                              "Restore the tree to this turn's checkpoint? Files changed since then will be replaced. The transcript and patch journal stay.",
                            )
                          ) {
                            return;
                          }
                          try {
                            const report = await api.restoreCheckpoint(project, checkpoint.id);
                            onNotice(`Restored ${report.files_restored} file(s) from turn checkpoint.`);
                          } catch (failure) {
                            onNotice(
                              failure instanceof Error ? failure.message : "Restore failed.",
                            );
                          }
                        }}
                      />
                    ))}
                  </section>
                );
              })}
            </div>
            <div ref={bottomRef} />
            </div>
            {outline.length > 0 && shown.length !== 2 ? (
              <nav className="turn-outline" aria-label="Turn outline">
                {outline.map((group) => (
                  <div className="outline-group" key={group.turnId}>
                    {group.marks.map((mark) => (
                      <button
                        key={mark.key}
                        type="button"
                        className={`outline-mark ${mark.kind}${
                          activeTurnId === group.turnId ? " on" : ""
                        }`}
                        title={mark.title}
                        aria-label={mark.title}
                        onClick={() => jumpToTurn(group.turnId)}
                      />
                    ))}
                  </div>
                ))}
              </nav>
            ) : null}
            </div>
          </>
        )}

        {session && nearCap ? (
          <div className="approval-inline" role="alert">
            <div>
              <strong>{percent >= 100 ? "Cap reached" : "80% of the cap"}</strong>
              <span>
                {money(spendMicros)} of {cents(session.budget_cents)}. Raise it, or stop and keep
                the transcript.
              </span>
            </div>
            <div className="cap-form">
              <input
                value={capInput}
                onChange={(event) => setCapInput(event.target.value)}
                placeholder="new cap, $"
                inputMode="decimal"
                aria-label="New cap in dollars"
              />
              <button className="chip warn" onClick={() => void raise()}>
                Raise cap
              </button>
              <button className="chip danger" onClick={() => void cancel()} disabled={!liveTurn}>
                Stop
              </button>
            </div>
          </div>
        ) : null}

        {session ? (
          <>
            {review ? (
              <RefineCard
                key={review}
                draft={review}
                target={
                  shown.length === 1
                    ? (agents.find((agent) => agent.id === shown[0])?.role ?? "watched agent")
                    : target === "claude"
                      ? "Claude Code"
                      : "Rigel native"
                }
                channel={null}
                recent={turns
                  .slice(-4)
                  .map((entry) => entry.turn.prompt)
                  .filter((prompt): prompt is string => Boolean(prompt))
                  .map((prompt) => `user: ${prompt.slice(0, 400)}`)}
                project={project}
                onSend={(refined) => {
                  setReview(null);
                  void deliver(refined);
                }}
                onDismiss={() => setReview(null)}
              />
            ) : null}
            {!readOnly ? (
              <CompanionSponsor project={project} sessionId={session.id} />
            ) : null}
            <div className="composer">
              {pinReview ? (
                <section className="pin-review" aria-label="Review changed skill pin">
                  <div className="pin-review-head">
                    <div>
                      <strong>
                        {pinLabels[pinReview.pin.skill_id] ??
                          pinReview.pin.skill_id.split(/[:/]/).filter(Boolean).pop() ??
                          pinReview.pin.skill_id}{" "}
                        moved
                      </strong>
                      <small>
                        pinned sha256:{pinReview.pin.body_sha.slice(0, 12)}
                        {pinReview.pin.current_body_sha
                          ? ` → current sha256:${pinReview.pin.current_body_sha.slice(0, 12)}`
                          : " · not on the shelf"}
                      </small>
                    </div>
                    <button
                      type="button"
                      className="snippet-delete"
                      aria-label="Close changed-pin review"
                      title="Close"
                      onClick={() => setPinReview(null)}
                    >
                      ×
                    </button>
                  </div>
                  {pinReview.pinned_body === null || pinReview.current_body === null ? (
                    <p className="pin-review-note">
                      {pinReview.pinned_body === null
                        ? "The pinned body is no longer on disk, so there is nothing to diff. Keep the pin, or update to what the index has now."
                        : "The current body is not on disk. Keep the pinned body; update is unavailable until the index has the body."}
                    </p>
                  ) : (
                    <pre className="pin-diff" aria-label="Skill body changes">
                      {pinDiff.map((line, index) => (
                        <span
                          className={`pin-diff-line ${line.kind}`}
                          key={`${line.oldLine}-${line.newLine}-${index}`}
                        >
                          <span className="pin-diff-no">{line.oldLine ?? ""}</span>
                          <span className="pin-diff-no">{line.newLine ?? ""}</span>
                          <span className="pin-diff-mark">
                            {line.kind === "add"
                              ? "+"
                              : line.kind === "del"
                                ? "-"
                                : line.kind === "gap"
                                  ? "…"
                                  : " "}
                          </span>
                          <span>{line.text || " "}</span>
                        </span>
                      ))}
                    </pre>
                  )}
                  <label className="pin-reason">
                    <span>Why?</span>
                    <input
                      value={pinReason}
                      onChange={(event) => setPinReason(event.target.value)}
                      placeholder="Optional note; a default is recorded"
                      aria-label="Why this pin decision"
                    />
                  </label>
                  <div className="pin-review-actions">
                    <button
                      type="button"
                      className="chip"
                      disabled={pinReviewBusy !== null}
                      onClick={() => void settlePin("keep")}
                    >
                      {pinReviewBusy === "keep" ? "Keeping…" : "Keep pin"}
                    </button>
                    <button
                      type="button"
                      className="primary"
                      disabled={pinReviewBusy !== null || pinReview.current_body === null}
                      onClick={() => void settlePin("update")}
                    >
                      {pinReviewBusy === "update" ? "Updating…" : "Update to current"}
                    </button>
                  </div>
                </section>
              ) : null}
              <div className="composer-field">
              {pins.length > 0 ? (
                <ul className="composer-pins" aria-label="Skills pinned to this session">
                  {pins.map((pin) => {
                    const name =
                      pinLabels[pin.skill_id] ??
                      pin.skill_id.split(/[:/]/).filter(Boolean).pop() ??
                      pin.skill_id;
                    return (
                      <li
                        className="composer-pin"
                        key={pin.skill_id}
                        title={`${pin.skill_id} · sha256:${pin.body_sha.slice(0, 12)}`}
                      >
                        <span>{name}</span>
                        {pin.changed ? (
                          <button
                            type="button"
                            className="pin-changed"
                            aria-label={`Review changed skill ${name}`}
                            title="The indexed body moved; review the diff"
                            onClick={() => void openPinReview(pin)}
                          >
                            changed
                          </button>
                        ) : null}
                        <button
                          type="button"
                          aria-label={`Unpin ${name}`}
                          title={`Unpin ${name}`}
                          onClick={() => void unpinSkill(pin)}
                          disabled={pinBusy === pin.skill_id}
                        >
                          ×
                        </button>
                      </li>
                    );
                  })}
                </ul>
              ) : null}
                <textarea
                  ref={composerRef}
                  value={text}
                  onChange={(event) => {
                    setText(event.target.value);
                    setHistoryIndex(null);
                  }}
                  onKeyDown={onComposerKeyDown}
                  placeholder={
                    readOnly
                      ? "Read-only transcript; sending is disabled"
                      : review
                        ? "Review the brief above before it sends…"
                        : refining
                        ? "Tightening the brief…"
                        : live
                          ? "A turn is running…"
                          : attachments.length > 0
                            ? "Ask about the attached files…"
                            : "Ask the agent; Enter sends, Shift+Enter adds a line"
                  }
                  rows={2}
                  disabled={readOnly || live || review !== null || refining}
                />
                <div className="composer-tray">
                  <div className="composer-add" ref={addRef}>
                    <button
                      type="button"
                      className="composer-plus"
                      aria-label="Add context"
                      aria-haspopup="menu"
                      aria-expanded={addOpen}
                      title="Add context"
                      onClick={() => setAddOpen((open) => !open)}
                      disabled={live || review !== null}
                    >
                      <svg width="14" height="14" viewBox="0 0 20 20" fill="none" aria-hidden>
                        <path
                          d="M10 4.5v11M4.5 10h11"
                          stroke="currentColor"
                          strokeWidth="1.5"
                          strokeLinecap="round"
                        />
                      </svg>
                    </button>
                    {addOpen ? (
                      <div className="composer-menu" role="menu" aria-label="Add context">
                        <p className="composer-menu-label">Add context</p>
                        <button
                          type="button"
                          role="menuitem"
                          className="composer-menu-item"
                          onClick={() => void attachFiles()}
                          disabled={!tauri}
                          title={
                            tauri
                              ? "Copy files into this project for the next turn"
                              : "Available in the desktop app"
                          }
                        >
                          <svg width="14" height="14" viewBox="0 0 20 20" fill="none" aria-hidden>
                            <path
                              d="M12.5 5.5 7 11a3 3 0 1 0 4.24 4.24l5.66-5.66a4 4 0 0 0-5.66-5.66L5.58 9.58a5 5 0 0 0 7.07 7.07l4.6-4.6"
                              stroke="currentColor"
                              strokeWidth="1.3"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            />
                          </svg>
                          <span>Attach files…</span>
                          {!tauri ? <small>desktop</small> : null}
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          className="composer-menu-item"
                          onClick={() => {
                            setAddOpen(false);
                            onBrowseSkills?.();
                          }}
                          disabled={!onBrowseSkills}
                        >
                          <svg width="14" height="14" viewBox="0 0 20 20" fill="none" aria-hidden>
                            <path
                              d="M10 2.5 11.3 7.2 16 8.5 11.3 9.8 10 14.5 8.7 9.8 4 8.5 8.7 7.2 10 2.5Z"
                              stroke="currentColor"
                              strokeWidth="1.2"
                              strokeLinejoin="round"
                            />
                            <path
                              d="M15.5 12.5 16 14.5 18 15 16 15.5 15.5 17.5 15 15.5 13 15 15 14.5 15.5 12.5Z"
                              stroke="currentColor"
                              strokeWidth="1.1"
                              strokeLinejoin="round"
                            />
                          </svg>
                          <span>Browse skills…</span>
                          <small>local index</small>
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          className="composer-menu-item"
                          onClick={() => {
                            setAddOpen(false);
                            onOpenSettings?.();
                          }}
                          disabled={!onOpenSettings}
                        >
                          <svg width="14" height="14" viewBox="0 0 20 20" fill="none" aria-hidden>
                            <circle cx="10" cy="10" r="2.3" stroke="currentColor" strokeWidth="1.3" />
                            <path
                              d="M10 3.2v1.6M10 15.2v1.6M3.2 10h1.6M15.2 10h1.6M5.2 5.2l1.1 1.1M13.7 13.7l1.1 1.1M14.8 5.2l-1.1 1.1M6.3 13.7l-1.1 1.1"
                              stroke="currentColor"
                              strokeWidth="1.2"
                              strokeLinecap="round"
                            />
                          </svg>
                          <span>Settings</span>
                        </button>
                        <p className="composer-menu-hint">
                          Files are copied into .rigel/attachments; the next turn gets their paths.
                        </p>
                      </div>
                    ) : null}
                  </div>
                  <div className="composer-snippets" ref={pinRef}>
                    <button
                      type="button"
                      className="composer-snippet-toggle"
                      aria-haspopup="menu"
                      aria-expanded={pinOpen}
                      aria-label={`Session skills: skills${pins.length > 0 ? ` ${pins.length}` : ""}${
                        changedPins.length > 0 ? ` ${changedPins.length} changed` : ""
                      }`}
                      title="Pin a skill to this session; the pin keeps the body version"
                      onClick={() => setPinOpen((open) => !open)}
                      disabled={!session}
                    >
                      skills{pins.length > 0 ? ` ${pins.length}` : ""}
                      {changedPins.length > 0 ? " " : null}
                      {changedPins.length > 0 ? (
                        <span
                          className="composer-snippet-changed"
                          title={`${changedPins.length} changed pinned skill${
                            changedPins.length === 1 ? "" : "s"
                          }`}
                        >
                          {changedPins.length} changed
                        </span>
                      ) : null}
                    </button>
                    {pinOpen ? (
                      <div className="composer-menu pin-menu" role="menu" aria-label="Session skills">
                        <div className="pin-menu-head">
                          <p className="composer-menu-label">Pinned skills</p>
                          <button
                            type="button"
                            className="pin-filter"
                            aria-pressed={pinChangedOnly}
                            title="Show only pins whose indexed body moved"
                            onClick={() => setPinChangedOnly((value) => !value)}
                          >
                            changed only{changedPins.length > 0 ? ` ${changedPins.length}` : ""}
                          </button>
                        </div>
                        {pins.length === 0 ? (
                          <p className="composer-menu-hint">
                            No skills pinned. Search below to pin one to this session.
                          </p>
                        ) : visiblePins.length === 0 ? (
                          <p className="composer-menu-hint">No pins match “changed only”.</p>
                        ) : (
                          <ul className="pin-rows">
                            {visiblePins.map((pin) => {
                              const name =
                                pinLabels[pin.skill_id] ??
                                pin.skill_id.split(/[:/]/).filter(Boolean).pop() ??
                                pin.skill_id;
                              return (
                                <li className="snippet-row" key={pin.skill_id}>
                                  <span className="pin-row-text" title={pin.skill_id}>
                                    <span>{name}</span>
                                    <small>sha256:{pin.body_sha.slice(0, 12)}</small>
                                  </span>
                                  {pin.changed ? (
                                    <button
                                      type="button"
                                      className="pin-changed"
                                      aria-label={`Review changed skill ${name}`}
                                      title="The indexed body moved; review the diff"
                                      onClick={() => void openPinReview(pin)}
                                    >
                                      changed
                                    </button>
                                  ) : null}
                                  <button
                                    type="button"
                                    className="snippet-delete"
                                    aria-label={`Unpin ${name}`}
                                    title={`Unpin ${name}`}
                                    onClick={() => void unpinSkill(pin)}
                                    disabled={pinBusy === pin.skill_id}
                                  >
                                    ×
                                  </button>
                                </li>
                              );
                            })}
                          </ul>
                        )}
                        {changedPins.length > 0 ? (
                          <div className="pin-batch">
                            <div className="pin-batch-head">
                              <span>
                                {changedPins.length} changed pin
                                {changedPins.length === 1 ? "" : "s"}
                              </span>
                              <small>one reason for all</small>
                            </div>
                            <div className="pin-batch-form">
                              <input
                                value={pinBatchReason}
                                onChange={(event) => setPinBatchReason(event.target.value)}
                                placeholder="reason for all (optional)"
                                aria-label="Reason for the batch pin decision"
                              />
                              <div className="pin-batch-actions">
                                <button
                                  type="button"
                                  className="chip"
                                  onClick={() => void keepAllChanged()}
                                  disabled={pinBatchBusy !== null}
                                >
                                  {pinBatchBusy === "keep" ? "Keeping…" : "Keep all changed"}
                                </button>
                                <button
                                  type="button"
                                  className="chip"
                                  onClick={() => void updateAllChanged()}
                                  disabled={pinBatchBusy !== null}
                                >
                                  {pinBatchBusy === "update"
                                    ? "Updating…"
                                    : "Update all changed"}
                                </button>
                              </div>
                            </div>
                          </div>
                        ) : null}
                        <label className="pin-search">
                          <span aria-hidden>⌕</span>
                          <input
                            value={pinQuery}
                            onChange={(event) => setPinQuery(event.target.value)}
                            placeholder="Search skills to pin…"
                            aria-label="Search skills to pin"
                          />
                        </label>
                        {pinQuery.trim()
                          ? pinResults
                              .filter((row) => !pins.some((pin) => pin.skill_id === row.id))
                              .map((row) => (
                                <button
                                  type="button"
                                  role="menuitem"
                                  className="composer-menu-item"
                                  key={row.id}
                                  onClick={() => void pinSkill(row.id)}
                                  disabled={!row.has_body || pinBusy !== null}
                                  title={
                                    row.has_body
                                      ? row.description
                                      : "This row has no indexed body to pin"
                                  }
                                >
                                  <span>{row.name}</span>
                                  <small>
                                    {pinBusy === row.id
                                      ? "pinning…"
                                      : row.has_body
                                        ? "pin"
                                        : "no body"}
                                  </small>
                                </button>
                              ))
                          : null}
                        {pinQuery.trim() && pinResults.length === 0 ? (
                          <p className="composer-menu-hint">No skill in the local index matches.</p>
                        ) : null}
                        {onBrowseSkills ? (
                          <button
                            type="button"
                            role="menuitem"
                            className="composer-menu-item"
                            onClick={() => {
                              setPinOpen(false);
                              onBrowseSkills();
                            }}
                          >
                            <span>Browse the shelf…</span>
                            <small>local index</small>
                          </button>
                        ) : null}
                        <p className="composer-menu-hint">
                          A pin records the current body hash. A changed shelf body never changes
                          what this session loads; open the changed badge to diff it and choose
                          Keep or Update.
                        </p>
                      </div>
                    ) : null}
                  </div>
                  <div className="composer-snippets" ref={snippetRef}>
                    <button
                      type="button"
                      className="composer-snippet-toggle"
                      aria-haspopup="menu"
                      aria-expanded={snipOpen}
                      title="Insert a saved prompt; placeholders are selected for you"
                      onClick={() => setSnipOpen((open) => !open)}
                      disabled={live || review !== null}
                    >
                      snippets
                    </button>
                    {snipOpen ? (
                      <div className="composer-menu" role="menu" aria-label="Snippets">
                        <p className="composer-menu-label">Snippets</p>
                        {snippets.length === 0 ? (
                          <p className="composer-menu-hint">
                            None saved. Write a draft, then save it here.
                          </p>
                        ) : null}
                        {snippets.map((snippet) => (
                          <div className="snippet-row" key={snippet.id}>
                            <button
                              type="button"
                              role="menuitem"
                              className="composer-menu-item"
                              onClick={() => applySnippet(snippet)}
                              title={snippet.body}
                            >
                              <span>{snippet.label}</span>
                              <small>{/\{[a-z]+\}/.test(snippet.body) ? "placeholder" : "insert"}</small>
                            </button>
                            <button
                              type="button"
                              className="snippet-delete"
                              aria-label={`Delete ${snippet.label}`}
                              title={`Delete ${snippet.label}`}
                              onClick={() => removeSnippet(snippet.id)}
                            >
                              ×
                            </button>
                          </div>
                        ))}
                        <button
                          type="button"
                          role="menuitem"
                          className="composer-menu-item"
                          onClick={saveDraftSnippet}
                          disabled={!text.trim()}
                        >
                          <span>Save this draft as a snippet</span>
                          <small>local</small>
                        </button>
                        <p className="composer-menu-hint">
                          Up/Down recalls sent prompts; Ctrl+Up widens to the whole project.
                          Inserted text stays a draft until you confirm it.
                        </p>
                      </div>
                    ) : null}
                  </div>
                  <button
                    type="button"
                    className="composer-snippet-toggle"
                    aria-pressed={autoRefine}
                    title={
                      autoRefine
                        ? "Auto-refine is on: every prompt is tightened by the cheap model, then sent without the review card"
                        : "Auto-refine is off: Send opens the review card. Turn this on to refine and send every prompt"
                    }
                    onClick={() => void toggleAutoRefine()}
                    disabled={!defaults || live || review !== null || refining}
                  >
                    auto-refine {autoRefine ? "on" : "off"}
                  </button>
                  {attachments.length > 0 ? (
                    <ul className="composer-attachments" aria-label="Attached files">
                      {attachments.map((path) => {
                        const name = path.split("/").pop() || path;
                        return (
                          <li key={path} className="composer-attachment">
                            <span title={path}>{name}</span>
                            <button
                              type="button"
                              aria-label={`Remove ${name}`}
                              title={`Remove ${name}`}
                              onClick={() =>
                                setAttachments((current) =>
                                  current.filter((item) => item !== path),
                                )
                              }
                            >
                              ×
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  ) : null}
                </div>
              </div>
              <div className="composer-actions">
                {onSwarmPrepare ? (
                  <button
                    className="chip swarm"
                    title={
                      !text.trim()
                        ? "Type the task first; Swarm sends it to the room"
                        : live
                          ? "Wait for the running turn to finish"
                          : review !== null
                            ? "Confirm or cancel the review card first"
                            : "Choose the big boss, then send to the agents you keep ticked"
                    }
                    onClick={() => void prepareSwarm()}
                    disabled={
                      readOnly ||
                      live ||
                      review !== null ||
                      preparing ||
                      !text.trim() ||
                      swarmTargets !== null
                    }
                  >
                    {preparing ? "Waking…" : "Swarm"}
                  </button>
                ) : null}
                <button
                  className="primary"
                  onClick={() => void send()}
                  disabled={
                    readOnly ||
                    refining ||
                    live ||
                    review !== null ||
                    (!text.trim() && attachments.length === 0)
                  }
                >
                  {refining ? "Refining…" : live ? "Running…" : "Send"}
                </button>
              </div>
            </div>
            {swarmTargets ? (
              <div className="swarm-preflight" role="dialog" aria-label="Swarm preflight">
                <div className="swarm-head">
                  <strong>Send this task to</strong>
                  <span className="fine">
                    {swarmTargets.length} online · {swarmPicked.length} selected
                  </span>
                </div>
                <div className="swarm-boss" role="radiogroup" aria-label="Big boss model">
                  <div className="swarm-boss-head">
                    <strong>Big boss model</strong>
                    <span className="fine">Rigel is the Muse coordinator</span>
                  </div>
                  <label className={`swarm-boss-row${swarmBoss === "rigel" ? " on" : ""}`}>
                    <input
                      type="radio"
                      name="swarm-boss"
                      checked={swarmBoss === "rigel"}
                      onChange={() => setSwarmBoss("rigel")}
                    />
                    <span className="swarm-boss-copy">
                      <span className="row-name">
                        Rigel <span className="tag">Muse</span>
                      </span>
                      <span className="fine">
                        Splits the task, assigns the ticked agents and keeps the plan.
                      </span>
                    </span>
                  </label>
                  <div className="swarm-boss-option">
                    <label className={`swarm-boss-row${swarmBoss === "model" ? " on" : ""}`}>
                      <input
                        type="radio"
                        name="swarm-boss"
                        checked={swarmBoss === "model"}
                        onChange={() => {
                          setSwarmBoss("model");
                          if (!swarmBossModel && provider && model) {
                            setSwarmBossModel({ provider, model });
                          }
                        }}
                      />
                      <span className="swarm-boss-copy">
                        <span className="row-name">
                          Rigel +{" "}
                          <span className="mono">
                            {swarmBossModel?.model || "choose a model"}
                          </span>
                        </span>
                        <span className="fine">
                          The same Muse coordinator, thinking with{" "}
                          {swarmBossModel
                            ? `${swarmBossModel.provider} / ${swarmBossModel.model}`
                            : "the chat model you choose"}
                          .
                        </span>
                      </span>
                    </label>
                    {swarmBoss === "model" ? (
                      <div className="swarm-boss-picker">
                        <ModelControls
                          model={swarmBossModel?.model ?? ""}
                          provider={swarmBossModel?.provider}
                          effort={defaults?.effort ?? "medium"}
                          chatOnly
                          label="choose model"
                          heading="Big boss model"
                          title="Choose any chat model for the Swarm big boss"
                          showEffort={false}
                          forceChatOnly
                          onModel={(next, nextProvider) =>
                            setSwarmBossModel({ provider: nextProvider, model: next })
                          }
                          onEffort={() => {}}
                          onChatOnly={() => {}}
                        />
                      </div>
                    ) : null}
                  </div>
                  <label className={`swarm-boss-row${swarmBoss === "broadcast" ? " on" : ""}`}>
                    <input
                      type="radio"
                      name="swarm-boss"
                      checked={swarmBoss === "broadcast"}
                      onChange={() => setSwarmBoss("broadcast")}
                    />
                    <span className="swarm-boss-copy">
                      <span className="row-name">No single boss</span>
                      <span className="fine">
                        Broadcast the same task to every ticked agent. Nobody assigns or reviews.
                      </span>
                    </span>
                  </label>
                </div>
                <ul>
                  {swarmTargets.map((target) => (
                    <li key={target.name}>
                      <label>
                        <input
                          type="checkbox"
                          checked={swarmPicked.includes(target.name)}
                          onChange={() => toggleSwarmTarget(target.name)}
                        />
                        <span className="row-name">@{target.name}</span>
                        {target.label !== target.name ? <span className="tag">{target.label}</span> : null}
                        {target.role ? <span className="tag">{target.role}</span> : null}
                      </label>
                    </li>
                  ))}
                </ul>
                <p className="fine">
                  {swarmBoss === "broadcast"
                    ? "One @mention broadcast on #general. Every ticked agent gets the same task text."
                    : "Rigel splits this task and assigns it to the ticked agents on #general."}
                </p>
                <div className="swarm-actions">
                  <button
                    className="chip"
                    onClick={() => {
                      setSwarmTargets(null);
                      setSwarmPicked([]);
                      setSwarmBoss("rigel");
                      setSwarmBossModel(null);
                    }}
                    disabled={swarming}
                  >
                    Cancel
                  </button>
                  <button
                    className="primary"
                    onClick={() => void launchSwarm()}
                    disabled={swarming || swarmPicked.length === 0}
                  >
                    {swarming
                      ? "Sending…"
                      : swarmBoss === "broadcast"
                        ? `Broadcast to ${swarmPicked.length}`
                        : `Send to ${swarmPicked.length}`}
                  </button>
                </div>
              </div>
            ) : null}
            {review === null && text.trim() ? (
              <p
                className="composer-preflight"
                role="status"
                title={
                  costPreflight.ready
                    ? `The 20th to 80th percentile of the last ${costPreflight.count} completed turns, scaled to this draft's ~${costPreflight.draftTokens} tokens. Arithmetic from recorded bills, not a quote; Send stays enabled.`
                    : `A range needs ${PREFLIGHT_FLOOR} completed turns in this session; the first-step quote above is the only estimate until then.`
                }
              >
                {costPreflight.ready ? (
                  <>
                    <span className="mono">
                      ≈ {shortMoney(costPreflight.low)}–{shortMoney(costPreflight.high)}
                    </span>{" "}
                    a turn · from {costPreflight.count} turns
                    {quote &&
                    quote.remaining_micros !== null &&
                    costPreflight.high > quote.remaining_micros ? (
                      <>{" · "}more than the remaining cap; the turn stops after the step that crosses it</>
                    ) : null}
                  </>
                ) : (
                  <>
                    cost history: no history yet — {costPreflight.count} of {PREFLIGHT_FLOOR}{" "}
                    completed turns recorded
                  </>
                )}
              </p>
            ) : null}
            {review === null && text.trim() && quote ? (
              <p
                className={`composer-quote${quoteWarning ? " over" : ""}${quote.price_stale ? " stale" : ""}`}
                role="status"
                title={`${quote.model} · ~${quote.input_tokens} in · ~${quote.output_tokens} out · first step only${quote.price_stale ? ` · price table fetched ${quote.price_fetched_on}` : ""}`}
              >
                <span className="mono">≈ {money(quote.first_step_micros)}</span> first step
                {quote.remaining_micros !== null ? (
                  <>
                    {" · "}
                    <span className="mono">{money(Math.abs(quote.remaining_micros))}</span>{" "}
                    {quote.remaining_micros >= 0
                      ? `left of ${cents(session.budget_cents)}`
                      : "over the cap"}
                  </>
                ) : null}
                {quoteWarning ? ` — ${quoteWarning}` : null}
                {quote.price_stale ? (
                  <span className="price-stale">
                    {" · "}stale prices: {quote.price_age_days} days old · {quote.price_source}
                  </span>
                ) : null}
              </p>
            ) : null}
          </>
        ) : null}
      </main>

      <aside className="inspector">
        <BrowserPanel
          call={browserCall}
          agent={browserAgent}
          sessionId={session?.id ?? null}
        />

        <section>
          <h3>Budget</h3>
          <div className="budget-line">
            <span className="mono">{money(spendMicros)}</span>
            <span className="faint">of {cents(session?.budget_cents ?? null)}</span>
          </div>
          <div className="budget-bar" title={capMicros ? `${percent.toFixed(0)}% of the cap` : "no cap"}>
            <span style={{ width: `${percent}%` }} className={percent >= 80 ? "hot" : ""} />
            <i style={{ left: "80%" }} />
          </div>
          <p className="fine">
            Each step is billed as it lands. The step that crosses the cap ends the turn —
            no further step runs — and the next turn is refused before it starts.
          </p>
          <div className="cap-form">
            <input
              value={capInput}
              onChange={(event) => setCapInput(event.target.value)}
              placeholder="new cap, $"
              inputMode="decimal"
              aria-label="New cap in dollars"
            />
            <button className="chip" onClick={() => void raise()} disabled={!session}>
              Raise
            </button>
          </div>
        </section>

        <section>
          <h3>Patches</h3>
          {patches.length === 0 ? (
            <p className="fine">No patch yet. Every write goes through the CAS pipeline.</p>
          ) : (
            <ul className="patch-list">
              {patches.map((patch) => (
                <li key={patch.id}>
                  <span className={`pill ${patch.status}`}>{patch.status}</span>
                  <span className="patch-path" title={patch.diff}>
                    {patch.path}
                  </span>
                  {patch.status === "applied" ? (
                    <button className="link" onClick={() => void revert(patch.id)}>
                      revert
                    </button>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </section>

        <section>
          <h3>Bookmarks</h3>
          {bookmarks.length === 0 ? (
            <p className="fine">
              Star a line in the transcript, or a hunk in Diff. A jump opens the session or the
              patch it lives in.
            </p>
          ) : (
            <ul className="bookmark-list">
              {bookmarks.map((row) => (
                <li key={row.id}>
                  <button
                    type="button"
                    className="bookmark-jump"
                    title={row.label}
                    onClick={() => jumpBookmark(row)}
                  >
                    <span className="tag">{row.kind}</span>
                    <span className="bookmark-label">{row.label}</span>
                  </button>
                  <button
                    type="button"
                    className="snippet-delete"
                    aria-label={`Remove bookmark ${row.label}`}
                    title="Remove bookmark"
                    onClick={() => void removeBookmark(row)}
                  >
                    ×
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section>
          <h3>Provider</h3>
          <p className="fine">
            {keyPresent === null
              ? "checking the OS keychain…"
              : keyPresent
                ? "Provider key in the OS keychain. The window never sees it."
                : "No provider key. The native agent cannot start a turn."}
          </p>
        </section>

        <section>
          <h3>Tool policy</h3>
          <p className="fine">
            read_file → edit_file is the only write path; edits are compare-and-swap. Skills
            load pinned to this session. Memory is validated before it can be committed.
          </p>
        </section>
      </aside>

      {/* Q274: the confirmation is part of the shell, not a native prompt, so it keeps the
          palette and cannot be dismissed by a webview that does not implement window.prompt. */}
      {deleting ? (
        <div
          className="delete-backdrop"
          role="presentation"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget && listBusy !== deleting.id) {
              setDeleting(null);
              setDeleteText("");
            }
          }}
        >
          <form
            className="delete-card"
            role="dialog"
            aria-modal="true"
            aria-labelledby="delete-title"
            onSubmit={(event) => {
              event.preventDefault();
              void confirmDelete();
            }}
          >
            <h2 id="delete-title">Delete this session permanently?</h2>
            <p className="fine">
              {deleting.title ?? "Untitled session"} is archived. Deleting it removes its turns,
              checkpoints and stored spend from this project; it cannot be undone.
            </p>
            <label className="fine" htmlFor="delete-confirm">
              Type the session id to confirm:
            </label>
            <code className="delete-id">{deleting.id}</code>
            <input
              id="delete-confirm"
              className="delete-input"
              autoFocus
              autoComplete="off"
              spellCheck={false}
              value={deleteText}
              onChange={(event) => setDeleteText(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape" && listBusy !== deleting.id) {
                  event.preventDefault();
                  setDeleting(null);
                  setDeleteText("");
                }
              }}
            />
            <div className="delete-actions">
              <button
                type="button"
                className="chip"
                disabled={listBusy === deleting.id}
                onClick={() => {
                  setDeleting(null);
                  setDeleteText("");
                }}
              >
                Cancel
              </button>
              <button
                type="submit"
                className="chip danger"
                disabled={deleteText !== deleting.id || listBusy === deleting.id}
              >
                {listBusy === deleting.id ? "Deleting…" : "Delete permanently"}
              </button>
            </div>
          </form>
        </div>
      ) : null}
    </>
  );
}

/**
 * Only the native agent accepts a turn; harness turns reach the transcript through `rigel codex`
 * and `rigel agent acp`. D40 wants that difference visible, so a harness row says so rather than
 * looking sendable.
 */
/** A harness Rigel can hand a turn to from this window. Codex, Cursor and Cline are read-only. */
const SENDS = new Set(["native", "claude"]);

function observedOnly(agent: Agent): boolean {
  return !SENDS.has(agent.harness);
}


/** One turn as it appears in a transcript pane. Both panes of a comparison use it. */
function TurnCard({
  entry,
  turns,
  raw,
  stream,
  checkpoint,
  onRestore,
  filter,
  bookmarks,
  onBookmark,
  onCopy,
  onContinue,
  canContinue,
  continueDisabled,
  handoffAgents,
  onHandoff,
}: {
  entry: TurnTranscript;
  turns: TurnTranscript[];
  raw: boolean;
  stream: Record<string, string>;
  checkpoint?: Checkpoint;
  onRestore?: (checkpoint: Checkpoint) => void;
  filter: TranscriptFilter;
  bookmarks: Bookmark[];
  onBookmark: (item: Item, seq: number) => void;
  onCopy: (text: string, label: string) => void;
  /** Q249: restart the stopped task with a fresh turn and a fresh step budget. */
  onContinue?: (entry: TurnTranscript) => void;
  canContinue?: boolean;
  continueDisabled?: boolean;
  /** Q266: sendable agents; the source agent is removed by the card. */
  handoffAgents?: Agent[];
  onHandoff?: (entry: TurnTranscript, agent: Agent) => void;
}) {
  const contextMenu = useMenu();
  const children = turns.filter((candidate) => candidate.turn.parent_turn_id === entry.turn.id);
  const visibleItems = entry.items.filter((item) => matchesFilter(item, filter));
  const hidden =
    entry.items.length -
    visibleItems.length +
    (entry.turn.prompt && !filterShowsPrompt(filter) ? 1 : 0);
  const childSpend = children.reduce(
    (sum, child) => sum + (child.turn.usage?.cost_micros ?? 0),
    0,
  );
  const usage = entry.turn.usage;
  const duration = stepDuration(entry.turn.started_at, entry.turn.completed_at, entry.turn.status);
  const handoffTargets = (handoffAgents ?? []).filter(
    (agent) => agent.id !== entry.turn.agent_id,
  );
  return (
    <article
      className="turn"
      id={`turn-${entry.turn.id}`}
      aria-label={`Turn ${entry.turn.seq}, ${entry.turn.status}`}
      onContextMenu={(event) =>
        contextMenu.open(event, {
          eyebrow: "Transcript turn",
          title: `Turn ${entry.turn.seq} · ${entry.turn.status}`,
          subtitle: entry.turn.prompt?.slice(0, 90) ?? "No prompt recorded",
          sections: [
            {
              label: "Turn",
              items: [
                {
                  label: "Copy prompt",
                  detail: entry.turn.prompt ?? "No prompt recorded",
                  icon: "copy",
                  disabled: !entry.turn.prompt,
                  onSelect: () => onCopy(entry.turn.prompt ?? "", "Turn prompt"),
                },
                {
                  label: "Copy turn id",
                  detail: entry.turn.id,
                  icon: "copy",
                  onSelect: () => onCopy(entry.turn.id, "Turn id"),
                },
                {
                  label: "Copy raw JSON",
                  detail: "The turn record, not the rendered rows",
                  icon: "quote",
                  onSelect: () => onCopy(JSON.stringify(entry.turn, null, 2), "Turn JSON"),
                },
                {
                  label: "Copy transcript rows",
                  detail: `${entry.items.length} row${entry.items.length === 1 ? "" : "s"}`,
                  icon: "diff",
                  disabled: entry.items.length === 0,
                  onSelect: () => onCopy(JSON.stringify(entry.items, null, 2), "Transcript rows"),
                },
              ],
            },
            {
              label: "Checkpoint",
              items: [
                {
                  label: "Restore this checkpoint",
                  detail: checkpoint ? when(checkpoint.created_at) : "No checkpoint for this turn",
                  icon: "restart",
                  disabled: !checkpoint || entry.turn.status !== "completed" || !onRestore,
                  onSelect: () => {
                    if (checkpoint) onRestore?.(checkpoint);
                  },
                },
              ],
            },
          ],
          footer: `${entry.turn.id.slice(0, 8)} · ${entry.turn.usage ? money(entry.turn.usage.cost_micros) : "no cost yet"}`,
        })
      }
    >
      <header className="turn-head">
        <span className="turn-seq">turn {entry.turn.seq}</span>
        <span className={`pill ${entry.turn.status}`}>{entry.turn.status}</span>
        {entry.turn.stop_reason && entry.turn.stop_reason !== entry.turn.status ? (
          <span className="turn-stop">{entry.turn.stop_reason}</span>
        ) : null}
        {entry.turn.stop_reason === "step_limit" &&
        canContinue &&
        entry.turn.prompt &&
        onContinue ? (
          <button
            type="button"
            className="chip warn turn-continue"
            disabled={continueDisabled}
            title="Start a new turn with this task and a fresh step budget"
            onClick={() => onContinue(entry)}
          >
            Continue
          </button>
        ) : null}
        {entry.turn.status === "completed" && onHandoff && handoffTargets.length > 0 ? (
          <select
            className="chip turn-handoff"
            aria-label={`Hand turn ${entry.turn.seq} to another agent`}
            value=""
            disabled={continueDisabled}
            title={
              continueDisabled
                ? "Finish the current step before handing off"
                : "Prefill the composer for another agent; nothing is sent"
            }
            onChange={(event) => {
              const target = handoffTargets.find((agent) => agent.id === event.target.value);
              if (target) onHandoff(entry, target);
            }}
          >
            <option value="">Hand to…</option>
            {handoffTargets.map((agent) => (
              <option key={agent.id} value={agent.id}>
                {agent.role ?? "agent"} · {agent.harness}
              </option>
            ))}
          </select>
        ) : null}
        <span className="grow" />
        {checkpoint && entry.turn.status === "completed" && onRestore ? (
          <button
            className="chip"
            title={`Snapshot taken after this turn · ${when(checkpoint.created_at)}`}
            onClick={() => onRestore(checkpoint)}
          >
            Restore checkpoint
          </button>
        ) : null}
        {children.length > 0 ? (
          <span className="turn-subagents" title="Child turns are billed to their own agents">
            {children.length} subagent{children.length === 1 ? "" : "s"} · {money(childSpend)}
          </span>
        ) : null}
        {entry.turn.usage ? (
          <span className="mono turn-cost">
            {money(entry.turn.usage.cost_micros)} ·{" "}
            {compact(entry.turn.usage.input_tokens + entry.turn.usage.output_tokens)} tok
          </span>
        ) : null}
      </header>

      <details className="step-details">
        <summary>step details</summary>
        {entry.items.find((item) => item.kind === "usage") ? (
          <StepCost body={entry.items.find((item) => item.kind === "usage")!.body} />
        ) : null}
        {usage ? (
          <dl>
            <div>
              <dt>input</dt>
              <dd>{compact(usage.input_tokens)} tok</dd>
            </div>
            <div>
              <dt>cached</dt>
              <dd>{compact(usage.cached_input_tokens)} tok</dd>
            </div>
            <div>
              <dt>output</dt>
              <dd>{compact(usage.output_tokens)} tok</dd>
            </div>
            <div>
              <dt>reasoning</dt>
              <dd>{compact(usage.reasoning_tokens)} tok</dd>
            </div>
            <div>
              <dt>total</dt>
              <dd>{compact(usage.input_tokens + usage.output_tokens)} tok</dd>
            </div>
            <div>
              <dt>cost</dt>
              <dd>
                {money(usage.cost_micros)}
                {usage.source === "estimated" ? " est." : ""}
              </dd>
            </div>
            {usage.served_model ? (
              <div>
                <dt>served</dt>
                <dd className="mono">{usage.served_model}</dd>
              </div>
            ) : null}
            <div>
              <dt>duration</dt>
              <dd>{duration}</dd>
            </div>
          </dl>
        ) : (
          <p className="step-details-empty">
            {entry.turn.status === "running"
              ? "This step is still running."
              : "No usage was recorded for this step."}{" "}
            Duration: {duration}.
          </p>
        )}
      </details>

      {entry.turn.prompt && filterShowsPrompt(filter) ? (
        <div className="msg user">
          <span className="gutter">you</span>
          <div className="msg-body">{entry.turn.prompt}</div>
        </div>
      ) : null}

      {visibleItems.map((item) => {
        const marked = bookmarks.some(
          (row) => row.kind === "item" && row.target_id === item.id,
        );
        return (
          <div className="item-shell" id={`item-${item.id}`} key={item.id}>
            <ItemRow item={item} raw={raw} />
            <button
              type="button"
              className={`bookmark-star${marked ? " on" : ""}`}
              aria-pressed={marked}
              aria-label={marked ? "Remove bookmark" : "Bookmark this line"}
              title={marked ? "Remove bookmark" : "Keep this line in the inspector"}
              onClick={() => onBookmark(item, entry.turn.seq)}
            >
              {marked ? "★" : "☆"}
            </button>
          </div>
        );
      })}

      {hidden > 0 ? (
        <p className="turn-hidden fine">
          {hidden} line{hidden === 1 ? "" : "s"} hidden by the {filter} filter
        </p>
      ) : null}

      {stream[entry.turn.id] ? (
        <div className="msg assistant">
          <span className="gutter">rigel</span>
          <div className="msg-body streaming">{stream[entry.turn.id]}</div>
        </div>
      ) : null}

      {raw ? (
        <details className="raw">
          <summary>raw turn json</summary>
          <pre>{JSON.stringify(entry.turn, null, 2)}</pre>
        </details>
      ) : null}
    </article>
  );
}

function ItemRow({ item, raw }: { item: Item; raw: boolean }) {
  const rawBlock = raw ? (
    <details className="raw">
      <summary>raw</summary>
      <pre>{JSON.stringify(item.raw ?? item.body, null, 2)}</pre>
    </details>
  ) : null;

  if (item.kind === "message") {
    return (
      <>
        <div className="msg assistant">
          <span className="gutter">rigel</span>
          <div className="msg-body">{bodyText(item, "text")}</div>
        </div>
        {rawBlock}
      </>
    );
  }

  if (item.kind === "reasoning") {
    return (
      <>
        <details className="reasoning">
          <summary>reasoning · {bodyText(item, "text").length} chars</summary>
          <pre>{bodyText(item, "text")}</pre>
        </details>
        {rawBlock}
      </>
    );
  }

  if (item.kind === "mcp_call") {
    const ok = item.status === "ok";
    return (
      <>
        <details className={`tool-line${ok ? "" : " bad"}`}>
          <summary>
            <span className="tool-name">{bodyText(item, "tool")}</span>
            <span className="tool-args">{argsSummary(item)}</span>
            <span className="tool-status">{ok ? "ok" : "error"}</span>
          </summary>
          <pre>{JSON.stringify(item.body, null, 2)}</pre>
        </details>
        {rawBlock}
      </>
    );
  }

  if (item.kind === "file_change") {
    const result = bodyRecord(item, "result");
    const diff = result && typeof result.diff === "string" ? result.diff : "";
    const path = result && typeof result.path === "string" ? result.path : bodyText(item, "path");
    return (
      <>
        <details className="file-change" open>
          <summary>
            <span className="tool-name">patch</span>
            <span className="tool-args">{path}</span>
          </summary>
          {diff ? <pre className="diff-block">{diff}</pre> : null}
        </details>
        {rawBlock}
      </>
    );
  }

  if (item.kind === "command") {
    const result = bodyRecord(item, "result");
    // A native command carries its text under `result`; a harness command is recorded from the
    // adapter's own vocabulary (P4.8), so the same line reads the same way in the window.
    const rawInput = bodyRecord(item, "raw_input");
    const command =
      result && typeof result.command === "string"
        ? result.command
        : bodyText(item, "command") ||
          (rawInput && typeof rawInput.command === "string" ? rawInput.command : "?");
    const stdout = result && typeof result.stdout === "string" ? result.stdout : "";
    const stderr = result && typeof result.stderr === "string" ? result.stderr : "";
    const harnessCode =
      typeof item.body?.exit_code === "number" ? String(item.body.exit_code) : null;
    const ok = item.status === "ok" || item.status === "complete" || item.status === "completed";
    const code =
      result && typeof result.exit_code === "number"
        ? String(result.exit_code)
        : harnessCode
          ? harnessCode
          : result && result.approved === false
            ? "denied"
            : result && result.timed_out === true
              ? "timed out"
              : item.status === "running" || item.status === "pending"
                ? "running"
                : "no exit code";
    const destructive = result !== null && Array.isArray(result.destructive) && result.destructive.length > 0;
    const round = result !== null && typeof result.deliberation === "object" && result.deliberation !== null
      ? (result.deliberation as { positions?: { stance?: string; text?: string }[] })
      : null;
    const advice = Array.isArray(round?.positions)
      ? round.positions.map((position) => `[${position.stance ?? "advisor"}] ${position.text ?? ""}`).join("\n\n")
      : "";
    return (
      <>
        <details className={`tool-line${ok ? "" : " bad"}`}>
          <summary>
            <span className="tool-name">$</span>
            <span className="tool-args">{command}</span>
            <span className="tool-status">
              {destructive ? `destructive · ${code}` : code}
            </span>
          </summary>
          <pre>
            {[advice, `${stdout}${stderr}`.trim()].filter(Boolean).join("\n\n") ||
              JSON.stringify(item.body, null, 2)}
          </pre>
        </details>
        {rawBlock}
      </>
    );
  }

  if (item.kind === "web_search") {
    const result = bodyRecord(item, "result");
    const url = result && typeof result.url === "string" ? result.url : bodyText(item, "url");
    const origin = result && typeof result.origin === "string" ? result.origin : "";
    const status =
      result && typeof result.status === "number"
        ? String(result.status)
        : result && result.approved === false
          ? "denied"
          : "error";
    const text = result && typeof result.text === "string" ? result.text : "";
    return (
      <>
        <details className={`tool-line${item.status === "ok" ? "" : " bad"}`}>
          <summary>
            <span className="tool-name">fetch</span>
            <span className="tool-args">{origin || url}</span>
            <span className="tool-status">
              {status} · untrusted
            </span>
          </summary>
          {text ? <pre>{text}</pre> : <pre>{JSON.stringify(item.body, null, 2)}</pre>}
        </details>
        {rawBlock}
      </>
    );
  }

  if (item.kind === "usage") {
    return (
      <>
        <StepCost body={item.body} />
        {rawBlock}
      </>
    );
  }

  if (item.kind === "error") {
    return (
      <>
        <p className="error-line">error · {bodyText(item, "message")}</p>
        {rawBlock}
      </>
    );
  }

  if (item.kind === "note") {
    return (
      <>
        <p className="note-line">you · {bodyText(item, "text")}</p>
        {rawBlock}
      </>
    );
  }

  if (item.kind === "approval") {
    const decision = bodyText(item, "decision");
    const title = bodyText(item, "kind") || bodyText(item, "summary") || "approval";
    return (
      <>
        <div className={`approval-item ${item.status}`}>
          <strong>{title}</strong>
          <span>{decision ? `decision: ${decision}` : "waiting for a decision"}</span>
        </div>
        {rawBlock}
      </>
    );
  }

  return (
    <>
      <details className="tool-line">
        <summary>{item.kind}</summary>
        <pre>{JSON.stringify(item.body, null, 2)}</pre>
      </details>
      {rawBlock}
    </>
  );
}

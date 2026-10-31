import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";

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
  type Settings,
  type Turn,
  type SwarmTarget,
  type TurnQuote,
  type TeamAgent,
  type RolePreset,
  type PlanState,
  type TurnTranscript,
} from "./api";
import { ModelControls } from "./modelpicker";
import RefineCard from "./refine";
import { notify } from "./notify";
import {
  previewAgents,
  previewApprovals,
  previewBudget,
  previewPatches,
  previewSession,
  previewTranscript,
} from "./sessionMock";

interface Props {
  project: string;
  onSessions: (sessions: Session[]) => void;
  onActive: (session: Session | null) => void;
  onNotice: (notice: string) => void;
  onOpenSettings?: () => void;
  onBrowseSkills?: () => void;
  /** Open/Switch folder, New workspace and the workspace form, owned by App. */
  projectControls?: ReactNode;
  /** Swarm step one: who is online, or a refusal to show. Sends nothing. */
  onSwarmPrepare?: () => Promise<SwarmTarget[]>;
  /** Swarm step two: broadcast one task to the chosen agent names. */
  onSwarm?: (task: string, names: string[]) => Promise<void>;
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

export default function SessionView({
  project,
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
  const [session, setSession] = useState<Session | null>(() => (tauri ? null : previewSession));
  const [agents, setAgents] = useState<Agent[]>(() => (tauri ? [] : previewAgents));
  const [team, setTeam] = useState<TeamAgent[]>([]);
  const [rolePresets, setRolePresets] = useState<RolePreset[]>([]);
  const [plan, setPlan] = useState<PlanState | null>(null);
  const [turns, setTurns] = useState<TurnTranscript[]>(() => (tauri ? [] : previewTranscript));
  const [budget, setBudget] = useState<BudgetSnapshot | null>(() => (tauri ? null : previewBudget));
  const [patches, setPatches] = useState<Patch[]>(() => (tauri ? [] : previewPatches));
  const [checkpoints, setCheckpoints] = useState<Checkpoint[]>([]);
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
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
  // Who the composer sends to. A Claude turn is run by the harness; everything else is the
  // native loop. The picker is the only place this is chosen, so the transcript header can say
  // which agent answered.
  const [target, setTarget] = useState<"native" | "claude">("native");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  // D148: the left pane is either the project's sessions or this session's agents. The list
  // lives here rather than in App because every edit returns the row the core stored.
  const [pane, setPane] = useState<"sessions" | "agents">("agents");
  const [list, setList] = useState<Session[]>(() => (tauri ? [] : [previewSession]));
  const [sessionQuery, setSessionQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<"all" | Session["status"]>("all");
  const [showArchived, setShowArchived] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameText, setRenameText] = useState("");
  const renamingRef = useRef<string | null>(null);
  const [listBusy, setListBusy] = useState<string | null>(null);
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
  // The commands waiting on this window (P4.10e). Empty is the normal state.
  const [approvals, setApprovals] = useState<PendingApproval[]>(() =>
    tauri ? [] : previewApprovals.map((pending) => ({ ...pending })),
  );
  const approvalsSeen = useRef(0);
  const approvalsRef = useRef(approvals);
  const defaultsRef = useRef<Settings | null>(defaults);
  useEffect(() => {
    defaultsRef.current = defaults;
  }, [defaults]);

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

  const spendMicros = budget?.session_micros ?? (session ? session.spend_cents * 10_000 : 0);
  const capMicros = budget?.cap_micros ?? (session?.budget_cents ? session.budget_cents * 10_000 : null);
  const model = defaults?.model ?? "gpt-5.6-sol-medium";
  const effort = defaults?.effort ?? "medium";

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

  // A command can be asked for at any moment while a turn runs, so the card polls. The read is
  // scoped to the open project, so another checkout's question is not this card's to answer.
  // In the browser the same call returns the sample queue, which is how this state is
  // inspectable.
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const open = await api.approvals.pending(project);
        if (alive) {
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
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [project, session?.id]);

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
    setApprovals((open) => open.filter((row) => row.id !== pending.id));
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
    if (!addOpen && !snipOpen && !templateOpen) return;
    const closeOnPointer = (event: PointerEvent) => {
      if (!addRef.current?.contains(event.target as Node)) setAddOpen(false);
      if (!snippetRef.current?.contains(event.target as Node)) setSnipOpen(false);
      if (!templateRef.current?.contains(event.target as Node)) setTemplateOpen(false);
    };
    const closeOnKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setAddOpen(false);
        setSnipOpen(false);
        setTemplateOpen(false);
      }
    };
    window.addEventListener("pointerdown", closeOnPointer);
    window.addEventListener("keydown", closeOnKey);
    return () => {
      window.removeEventListener("pointerdown", closeOnPointer);
      window.removeEventListener("keydown", closeOnKey);
    };
  }, [addOpen, snipOpen]);

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
        const created = await api.startSession(project, template?.name ?? "Session", 500);
        const listed = await api.listSessions(project);
        setList(listed);
        onSessions(listed);
        await selectSession(created);
        if (!template) {
          onNotice(`Session started with a $5.00 cap.`);
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
        onNotice(
          `Started from “${template.name}”. The prompt is in the composer; nothing has been sent.`,
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
  }, [project, session, templates, model, effort, defaults, agents, text, onNotice]);

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
            ? "Archived. It stays searchable with Ctrl+K and resumable from the list."
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

  /** The confirmed text starts the turn; a failure leaves it in the composer. */
  const deliver = useCallback(async (prompt: string) => {
    if (!prompt || !session || busy) return;
    const turnPrompt = withAttachments(prompt);
    setText("");
    setError(null);

    if (!tauri) {
      // Preview: a canned answer, labelled in the transcript header as sample data.
      const id = `preview-${Date.now()}`;
      setTurns((previous) => [
        ...previous,
        {
          turn: {
            id,
            agent_id: agents[0]?.id ?? "preview",
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
      setAttachments([]);
      return;
    }

    setBusy(true);
    try {
      // A watched agent is the one the next turn is for. Without a selection the harness picker
      // keeps the old behaviour: the first agent of that harness answers.
      const watched = shown.length === 1
        ? agents.find((agent) => agent.id === shown[0] && !observedOnly(agent))
        : undefined;
      let worker = watched ?? agents.find((agent) => agent.harness === target);
      if (!worker) {
        const spawned = await api.spawnAgent(
          project,
          session.id,
          target,
          target === "claude" ? "claude" : "worker",
          model,
        );
        setAgents((previous) => [...previous, spawned]);
        worker = spawned;
      }
      const reply = await api.sendTurn(project, session.id, worker.id, turnPrompt, model, effort);
      rememberPrompt(prompt);
      setAttachments([]);
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
      setText(prompt);
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
    effort,
    target,
    withAttachments,
    rememberPrompt,
  ]);

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

  /** Send now reviews first: the agent receives only what the user confirmed in the card. */
  const send = useCallback(() => {
    const prompt =
      text.trim() ||
      (attachments.length > 0 ? "Read the attached files and tell me what matters." : "");
    if (!prompt || !session || busy || review) return;
    setAddOpen(false);
    setReview(prompt);
  }, [text, session, busy, review, attachments]);

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
  }, [onSwarmPrepare, text, onNotice, withAttachments]);

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
    setSwarming(true);
    setError(null);
    try {
      await onSwarm(swarmTask, chosen);
      setSwarmTargets(null);
      setSwarmPicked([]);
      setAttachments([]);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setSwarming(false);
    }
  }, [onSwarm, swarmTargets, swarmPicked, swarmTask]);

  const cancel = useCallback(async () => {
    if (!liveTurn) return;
    try {
      await api.cancelTurn(liveTurn.id);
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
                disabled={!tauri || !session || templateSaving}
                title="Save this session's model, effort, steps, roster and draft as a project template"
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
              searchable with Ctrl+K and resumable from this list.
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
              <select
                value={spawnHarness}
                onChange={(event) => setSpawnHarness(event.target.value as "native" | "claude")}
                aria-label="Agent harness"
                disabled={spawning}
              >
                <option value="native">native</option>
                <option value="claude">claude</option>
              </select>
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
              <span className="row-meta">{money(agent.spend_micros)}</span>
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
            <h2>{session?.title ?? "Session"}</h2>
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
              effort={effort}
              chatOnly={defaults?.chat_models_only ?? true}
              onModel={(next, provider) => void applyModel(next, effort, provider)}
              onEffort={(next) => void applyModel(model, next)}
              onChatOnly={(next) => void applyChatOnly(next)}
            />
            {keyPresent === false && onOpenSettings ? (
              <button className="chip warn" onClick={onOpenSettings}>
                Add key
              </button>
            ) : null}
            {plan?.mode ? (
              <button className="chip on" onClick={() => void togglePlan()}>
                approve plan
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
              <button className="danger-button" onClick={() => void cancel()}>
                Stop turn
              </button>
            ) : null}
          </div>
        </header>

        {approvals.map((pending) => (
          <ApprovalCard key={pending.id} pending={pending} onAnswer={answer} />
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
                    {entries.map((entry) => (
                      <TurnCard
                        key={entry.turn.id}
                        entry={entry}
                        turns={turns}
                        raw={raw}
                        filter={filter}
                        bookmarks={bookmarks}
                        onBookmark={toggleItemBookmark}
                        stream={stream}
                        checkpoint={checkpoints.find((row) => row.turn_id === entry.turn.id)}
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
            <div className="composer">
              <div className="composer-field">
                <textarea
                  ref={composerRef}
                  value={text}
                  onChange={(event) => {
                    setText(event.target.value);
                    setHistoryIndex(null);
                  }}
                  onKeyDown={onComposerKeyDown}
                  placeholder={
                    review
                      ? "Review the brief above before it sends…"
                      : live
                        ? "A turn is running…"
                        : attachments.length > 0
                          ? "Ask about the attached files…"
                          : "Ask the agent; Enter sends, Shift+Enter adds a line"
                  }
                  rows={2}
                  disabled={live || review !== null}
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
                  <div className="composer-snippets" ref={snippetRef}>
                    <button
                      type="button"
                      className="composer-snippet-toggle"
                      aria-haspopup="menu"
                      aria-expanded={snipOpen}
                      aria-label="Snippets"
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
                <label className="harness-pick" title="Which agent answers the next turn">
                  <select
                    value={target}
                    onChange={(event) => setTarget(event.target.value as "native" | "claude")}
                    disabled={live || review !== null}
                    aria-label="Agent"
                  >
                    <option value="native">native</option>
                    <option value="claude">claude</option>
                  </select>
                </label>
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
                            : "Show which agents are online, then send to the ones you keep ticked"
                    }
                    onClick={() => void prepareSwarm()}
                    disabled={
                      live || review !== null || preparing || !text.trim() || swarmTargets !== null
                    }
                  >
                    {preparing ? "Waking…" : "Swarm"}
                  </button>
                ) : null}
                <button
                  className="primary"
                  onClick={() => void send()}
                  disabled={live || review !== null || (!text.trim() && attachments.length === 0)}
                >
                  {live ? "Running…" : "Send"}
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
                  One @mention broadcast on #general. Every ticked agent gets the same task text.
                </p>
                <div className="swarm-actions">
                  <button
                    className="chip"
                    onClick={() => {
                      setSwarmTargets(null);
                      setSwarmPicked([]);
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
                    {swarming ? "Sending…" : `Send to ${swarmPicked.length}`}
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
                className={`composer-quote${quoteWarning ? " over" : ""}`}
                role="status"
                title={`${quote.model} · ~${quote.input_tokens} in · ~${quote.output_tokens} out · first step only`}
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
              </p>
            ) : null}
          </>
        ) : null}
      </main>

      <aside className="inspector">
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


/**
 * One command waiting on this window.
 *
 * The exact text is the title, because that is what a grant is keyed by: the four scope buttons
 * are the only way to answer, and "reject" is the default in spirit even though it is styled as
 * one button among five. Destructive rows and the advisors' note are shown here, before consent.
 */
function ApprovalCard({
  pending,
  onAnswer,
}: {
  pending: PendingApproval;
  onAnswer: (
    pending: PendingApproval,
    scope: ApprovalAnswer,
    options?: { skip?: boolean; note?: string },
  ) => void;
}) {
  const detail = pending.detail ?? {};
  const [skippingPlan, setSkippingPlan] = useState(false);
  const [skipReason, setSkipReason] = useState("");
  const cwd = typeof detail.cwd === "string" ? detail.cwd : null;
  const timeout = typeof detail.timeout_secs === "number" ? detail.timeout_secs : null;
  const destructive = Array.isArray(detail.destructive)
    ? (detail.destructive as { id?: string; summary?: string }[])
    : [];
  const deliberation =
    detail.deliberation && typeof detail.deliberation === "object"
      ? (detail.deliberation as { note?: string })
      : null;
  // D114: a writable fan-out is its own question. It shows each child's plan before its
  // first edit, and the skip path asks for the reason the ledger will keep.
  const planned =
    pending.kind === "plan" && Array.isArray(detail.agents)
      ? (detail.agents as { role?: string; files?: string[]; task?: string; plan?: string }[])
      : [];
  if (pending.kind === "plan") {
    return (
      <section className="approval-card" aria-label="Writable agents awaiting approval">
        <header>
          <span className="pill running">plan</span>
          <span className="approval-title mono">{pending.title}</span>
          <span className="grow" />
          <span className="approval-wait">waiting for you</span>
        </header>
        <p className="approval-meta mono">
          each child works in its own worktree; its changes land only after a review
        </p>
        <ul className="approval-rows plan-rows">
          {planned.map((agent, index) => (
            <li key={`${agent.role ?? "agent"}-${index}`}>
              <span className="pill running">{agent.role ?? "agent"}</span>
              <span className="mono plan-files">{(agent.files ?? []).join(", ")}</span>
              {agent.task ? <p className="fine">{agent.task}</p> : null}
              <pre className="plan-body">{agent.plan ?? ""}</pre>
            </li>
          ))}
        </ul>
        {skippingPlan ? (
          <div className="plan-skip">
            <p className="fine">
              Skipping records a waiver instead of an approval. What is the reason?
            </p>
            <div className="cap-form">
              <input
                value={skipReason}
                onChange={(event) => setSkipReason(event.target.value)}
                placeholder="why this fan-out does not need a plan check"
                aria-label="Reason for skipping the plan check"
              />
              <button
                className="chip warn"
                onClick={() =>
                  onAnswer(pending, "deny", { skip: true, note: skipReason })
                }
              >
                Record skip
              </button>
            </div>
          </div>
        ) : null}
        <div className="approval-actions">
          <button className="chip" onClick={() => onAnswer(pending, "once")}>
            Approve plan
          </button>
          <button className="chip warn" onClick={() => setSkippingPlan(true)}>
            Skip check, with a reason
          </button>
          <button className="chip danger" onClick={() => onAnswer(pending, "deny")}>
            Reject
          </button>
        </div>
      </section>
    );
  }
  return (
    <section className="approval-card" aria-label="Approval required">
      <header>
        <span className={`pill ${destructive.length > 0 ? "failed" : "running"}`}>{pending.kind}</span>
        <span className="approval-title mono">{pending.title}</span>
        <span className="grow" />
        <span className="approval-wait">waiting for you</span>
      </header>
      <p className="approval-meta mono">
        {cwd ? `cwd ${cwd}` : null}
        {timeout ? ` · ${timeout}s` : null}
      </p>
      {destructive.length > 0 ? (
        <ul className="approval-rows">
          {destructive.map((row, index) => (
            <li key={`${row.id ?? "row"}-${index}`}>
              <span className="pill danger">{row.id ?? "policy"}</span> {row.summary ?? ""}
            </li>
          ))}
        </ul>
      ) : null}
      {deliberation?.note ? <p className="approval-note">{deliberation.note}</p> : null}
      <div className="approval-actions">
        <button className="chip" onClick={() => onAnswer(pending, "once")}>
          Allow once
        </button>
        <button className="chip" onClick={() => onAnswer(pending, "session")}>
          This session
        </button>
        <button className="chip" onClick={() => onAnswer(pending, "project")}>
          This project
        </button>
        <button className="chip" onClick={() => onAnswer(pending, "always")}>
          Always
        </button>
        <button className="chip danger" onClick={() => onAnswer(pending, "deny")}>
          Reject
        </button>
      </div>
    </section>
  );
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
}) {
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
  return (
    <article
      className="turn"
      id={`turn-${entry.turn.id}`}
      aria-label={`Turn ${entry.turn.seq}, ${entry.turn.status}`}
    >
      <header className="turn-head">
        <span className="turn-seq">turn {entry.turn.seq}</span>
        <span className={`pill ${entry.turn.status}`}>{entry.turn.status}</span>
        {entry.turn.stop_reason && entry.turn.stop_reason !== entry.turn.status ? (
          <span className="turn-stop">{entry.turn.stop_reason}</span>
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
    const usage = item.body as unknown as { cost_micros?: number; input_tokens?: number; output_tokens?: number };
    return (
      <>
        <p className="usage-line mono">
          {money(usage.cost_micros ?? 0)} · {compact((usage.input_tokens ?? 0) + (usage.output_tokens ?? 0))} tok
        </p>
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

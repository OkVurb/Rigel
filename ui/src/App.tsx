import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent } from "react";

import {
  api,
  compact,
  isTauri,
  when,
  TauriSkillSource,
  type AgentChatAgent,
  type KeyStatus,
  type ProjectInfo,
  type ProviderRow,
  type Session,
  type SkillDetail,
  type SkillRow,
  type SkillSource,
  type SourceStatus,
  type SwarmTarget,
  type UpdateCheck,
  type UpdateStatus,
} from "./api";
import AttentionView, { type AttentionJump } from "./attention";
import ChatView from "./agentchat";
import { RigelMark } from "./mark";
import { PreviewSkillSource } from "./mock";
import DiffView from "./diff";
import Palette, { type PaletteAction } from "./palette";
import SessionView from "./session";
import SettingsView from "./settings";
import { applyDisplay } from "./display";
import { applyTheme } from "./theme";
import Titlebar from "./titlebar";

// xterm and its CSS are only needed once the terminal is opened; keep them out of the first paint.
const TerminalView = lazy(() => import("./terminal"));

/**
 * The status line's answer: which adapter holds the key a native turn would spend. The rows come
 * from the Rust-side table, so a new adapter is named here without this file knowing about it.
 */
function nativeKeyLabel(keys: KeyStatus, providers: ProviderRow[]): string {
  const held = providers.find((row) => keys.providers[row.id]);
  return held ? `${held.name} key` : "no provider key";
}

const ICONS = {
  attention: "M10 2.5a5 5 0 0 1 5 5c0 3 1 4.5 2 5.5H3c1-1 2-2.5 2-5.5a5 5 0 0 1 5-5ZM8 16h4M9 18h2",
  shelf: "M10 2.5 17 6.5v7l-7 4-7-4v-7l7-4ZM3 6.5l7 4 7-4M10 10.5v7",
  session: "M3 4.5h14M3 9.5h14M3 14.5h9",
  chat: "M6 3.5h8a2.5 2.5 0 0 1 2.5 2.5v5a2.5 2.5 0 0 1-2.5 2.5H9.5L6 16.5v-3H6a2.5 2.5 0 0 1-2.5-2.5V6A2.5 2.5 0 0 1 6 3.5ZM6.5 8.5h.01M10 8.5h.01M13.5 8.5h.01",
  diff: "M6 3v14M14 3v14M3 7h6M11 13h6",
  terminal: "M3 4.5h14v11H3ZM5.5 8l2.5 2-2.5 2M10.5 12.5h4",
  settings:
    "M10 12.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5ZM10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2",
} as const;

type View = "shelf" | "attention" | "session" | "chat" | "diff" | "terminal" | "settings";

/** A palette jump: which session to open, and the row to land on once it is there. */
type Focus = { sessionId: string; turnId?: string; patchId?: string; hunkId?: string } | null;

const VIEWS: { id: View; label: string; d: string; phase: string }[] = [
  { id: "shelf", label: "Shelf", d: ICONS.shelf, phase: "P2" },
  { id: "attention", label: "Attention", d: ICONS.attention, phase: "P7" },
  { id: "session", label: "Session", d: ICONS.session, phase: "P3" },
  { id: "chat", label: "Chat", d: ICONS.chat, phase: "P6" },
  { id: "diff", label: "Diff", d: ICONS.diff, phase: "P5" },
  { id: "terminal", label: "Terminal", d: ICONS.terminal, phase: "P7" },
  { id: "settings", label: "Settings", d: ICONS.settings, phase: "P3" },
];

/** Rows fetched per page. The index has ~10k skills; the list pages as it scrolls. */
const PAGE = 250;
const LAST_PROJECT = "rigel.lastProject";

/* ---------------------------- pane widths (D159) ---------------------------- */

const PANE_DEFAULTS = { list: 264, inspector: 304 };
const PANE_LIMITS: Record<keyof typeof PANE_DEFAULTS, [number, number]> = {
  list: [200, 420],
  inspector: [220, 460],
};
const paneKey = (project: string) => `rigel.panes.${project}`;

function clampPane(edge: keyof typeof PANE_DEFAULTS, value: number): number {
  const [low, high] = PANE_LIMITS[edge];
  return Number.isFinite(value) ? Math.min(high, Math.max(low, value)) : PANE_DEFAULTS[edge];
}

export default function App() {
  const source: SkillSource = useMemo(
    () => (isTauri() ? new TauriSkillSource() : new PreviewSkillSource()),
    [],
  );

  const [query, setQuery] = useState("");
  const [rows, setRows] = useState<SkillRow[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<SkillDetail | null>(null);
  const [statuses, setStatuses] = useState<SourceStatus[]>([]);
  const [reload, setReload] = useState(0);

  const [loading, setLoading] = useState(true);
  const [slow, setSlow] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string>(() =>
    isTauri() ? "Search the local index. Nothing leaves this machine." : "Browser preview — sample rows, no database.",
  );

  const [view, setView] = useState<View>("shelf");
  // D159: widths are per project, because a project's folder names and paths decide how much
  // room the list deserves. The handles are the only way to change them; there is no setting.
  const [panes, setPanes] = useState(PANE_DEFAULTS);
  const panesRef = useRef(panes);
  const dragPane = useRef<{
    edge: keyof typeof PANE_DEFAULTS;
    startX: number;
    start: number;
  } | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  // D154: how many unresolved rows the rail badge shows. The Attention view reads the same list.
  const [attention, setAttention] = useState(0);
  const [focus, setFocus] = useState<Focus>(null);
  // Once visited, the terminal stays mounted (hidden) so switching tabs keeps its shell and
  // scrollback. It is never started before the first visit.
  const [terminalMounted, setTerminalMounted] = useState(false);
  const [project, setProject] = useState<ProjectInfo | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [total, setTotal] = useState<number | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const loadingMoreRef = useRef(false);
  const [newWorkspace, setNewWorkspace] = useState(false);
  const [newName, setNewName] = useState("");
  const [newRoot, setNewRoot] = useState("");
  const [newBusy, setNewBusy] = useState(false);
  const [activeSession, setActiveSession] = useState<Session | null>(null);
  const [keys, setKeys] = useState<KeyStatus | null>(null);
  const [providers, setProviders] = useState<ProviderRow[]>([]);
  const [keyPromptDismissed, setKeyPromptDismissed] = useState(false);
  const [swarmNotice, setSwarmNotice] = useState<string | null>(null);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
  const [update, setUpdate] = useState<UpdateCheck | null>(null);
  const [updateDismissed, setUpdateDismissed] = useState(false);
  const [updateBusy, setUpdateBusy] = useState(false);

  const setActive = useCallback((next: Session | null) => setActiveSession(next), []);
  const noticeFromSession = useCallback((next: string) => setNotice(next), []);

  /* --------------------------- palette jumps --------------------------- */

  const clearFocus = useCallback(() => setFocus(null), []);

  const runPaletteAction = useCallback((action: PaletteAction) => {
    if (action === "terminal") setTerminalMounted(true);
    setView(action);
  }, []);

  // D154: the badge is the view's own count, polled so a question raised in the background is
  // visible without opening the tab. A read failure shows no badge rather than a stale one.
  useEffect(() => {
    // The browser preview has no project row, so it reads the sample list under its mock root.
    const root = project?.root ?? (isTauri() ? "" : "/preview/rigel");
    if (!root) {
      setAttention(0);
      return;
    }
    let live = true;
    const read = () =>
      api
        .attention(root)
        .then((next) => live && setAttention(next.total))
        .catch(() => live && setAttention(0));
    void read();
    const timer = window.setInterval(read, 10_000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [project?.root]);

  // A row from the Attention view opens the session that owns it, and lands on the turn or the
  // patch the row names. An approval row has no session and stays on the Session card.
  const openAttention = useCallback(
    async (target: AttentionJump) => {
      if (project && target.sessionId) {
        const known = sessions.find((item) => item.id === target.sessionId);
        try {
          setActive(known ?? (await api.getSession(project.root, target.sessionId)));
        } catch {
          // The view still opens; it reports its own failure.
        }
      }
      setFocus({ sessionId: target.sessionId, turnId: target.turnId, patchId: target.patchId });
      setView(target.surface);
    },
    [project, sessions, setActive],
  );

  const openSkillFromPalette = useCallback((id: string) => {
    setSelectedId(id);
    setView("shelf");
  }, []);

  const openSessionFromPalette = useCallback((sessionId: string) => {
    setFocus({ sessionId });
    setView("session");
  }, []);

  const openTurnFromPalette = useCallback((sessionId: string, turnId: string) => {
    setFocus({ sessionId, turnId });
    setView("session");
  }, []);

  const openPatchFromPalette = useCallback(
    async (sessionId: string, patchId: string) => {
      // DiffView reads the session it is given, so a patch from another session must first become
      // the active one. A failure still opens the view; the patch then simply is not highlighted.
      if (project) {
        try {
          setActive(await api.getSession(project.root, sessionId));
        } catch {
          // The current session's patches remain readable.
        }
      }
      setFocus({ sessionId, patchId });
      setView("diff");
    },
    [project, setActive],
  );

  const [repoInput, setRepoInput] = useState("");
  const [indexing, setIndexing] = useState(false);
  const searchRef = useRef<HTMLInputElement | null>(null);
  /** Which project root the local services were last booted for; `undefined` means never. */
  const bootedFor = useRef<string | null | undefined>(undefined);

  /* ----------------------------- data ----------------------------- */

  // The saved theme, scale and density are applied once at launch; Settings → Appearance
  // applies a new choice immediately. An unreadable settings file means the defaults, which
  // are dark, 100% and roomy.
  useEffect(() => {
    let live = true;
    api
      .getSettings()
      .then((next) => {
        if (!live) return;
        applyTheme(next.theme);
        applyDisplay(next.ui_scale, next.compact_density);
      })
      .catch(() => {
        applyTheme("dark");
        applyDisplay(100, false);
      });
    return () => {
      live = false;
    };
  }, []);

  // D159: switching project restores that project's widths; a project with none gets the
  // defaults rather than the last project's shape. A blocked store keeps this run's widths.
  useEffect(() => {
    let saved: Partial<typeof PANE_DEFAULTS> = {};
    if (project) {
      try {
        const raw = localStorage.getItem(paneKey(project.root));
        if (raw) saved = JSON.parse(raw) as Partial<typeof PANE_DEFAULTS>;
      } catch {
        saved = {};
      }
    }
    setPanes({
      list: clampPane("list", Number(saved.list ?? PANE_DEFAULTS.list)),
      inspector: clampPane("inspector", Number(saved.inspector ?? PANE_DEFAULTS.inspector)),
    });
  }, [project?.root]);

  useEffect(() => {
    panesRef.current = panes;
  }, [panes]);

  const persistPanes = useCallback(() => {
    if (!project) return;
    try {
      localStorage.setItem(paneKey(project.root), JSON.stringify(panesRef.current));
    } catch {
      // A blocked store forgets the widths; dragging still works for this run.
    }
  }, [project]);

  const startPane = (edge: keyof typeof PANE_DEFAULTS, event: ReactPointerEvent) => {
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    dragPane.current = { edge, startX: event.clientX, start: panesRef.current[edge] };
  };

  /** Client x is in visual CSS pixels; the zoom is applied to #root, so divide it back out. */
  const movePane = (edge: keyof typeof PANE_DEFAULTS, clientX: number) => {
    const drag = dragPane.current;
    if (!drag || drag.edge !== edge) return;
    const zoom = Number(document.documentElement.dataset.scale ?? 100) / 100 || 1;
    const delta = (clientX - drag.startX) / zoom;
    const next = clampPane(edge, drag.start + (edge === "list" ? delta : -delta));
    setPanes((current) => (current[edge] === next ? current : { ...current, [edge]: next }));
  };

  const finishPane = () => {
    if (!dragPane.current) return;
    dragPane.current = null;
    persistPanes();
  };

  /** Arrow keys nudge by 12px; a double-click puts the default back. */
  const nudgePane = (edge: keyof typeof PANE_DEFAULTS, delta: number) => {
    setPanes((current) => {
      const next = { ...current, [edge]: clampPane(edge, current[edge] + delta) };
      panesRef.current = next;
      if (project) {
        try {
          localStorage.setItem(paneKey(project.root), JSON.stringify(next));
        } catch {
          // See persistPanes.
        }
      }
      return next;
    });
  };

  // The startup check: does a key exist? It asks the keychain, not the network, and the
  // answer is a boolean — the window never receives the key itself.
  useEffect(() => {
    if (!isTauri()) {
      setKeys({ providers: {}, native_ready: false });
      return;
    }
    let live = true;
    api
      .keyStatus()
      .then((next) => live && setKeys(next))
      .catch(() => live && setKeys({ providers: {}, native_ready: false }));
    // The adapter table, so the banner and the status line name a provider instead of guessing
    // from a hard-coded list of three.
    api
      .providersList()
      .then((next) => live && setProviders(next))
      .catch(() => live && setProviders([]));
    return () => {
      live = false;
    };
  }, []);

  // One signed check at launch when the switch is on. A missing release, an offline machine
  // and a disabled switch are all silent: this must never delay or break the window.
  useEffect(() => {
    if (!isTauri()) return;
    let live = true;
    void (async () => {
      try {
        const status = await api.updateStatus();
        if (!live) return;
        setUpdateStatus(status);
        if (!status.enabled) return;
        setUpdate(await api.updateCheck());
      } catch {
        // No release published, offline, or checks off: leave the window alone.
      }
    })();
    return () => {
      live = false;
    };
  }, []);

  const installUpdate = useCallback(async () => {
    setUpdateBusy(true);
    try {
      setNotice(await api.updateInstall());
    } catch (failure) {
      setUpdateBusy(false);
      setNotice(failure instanceof Error ? failure.message : String(failure));
    }
  }, []);

  // Auto-install waits briefly so the window shows the version first. Dismissing the banner
  // cancels this timer; a dev build never replaces itself with a release download.
  useEffect(() => {
    if (
      !isTauri() ||
      import.meta.env.DEV ||
      !update?.available ||
      updateDismissed ||
      !updateStatus?.auto_install
    ) {
      return;
    }
    const timer = setTimeout(() => void installUpdate(), 1500);
    return () => clearTimeout(timer);
  }, [installUpdate, update, updateDismissed, updateStatus]);

  useEffect(() => {
    let live = true;
    setLoading(true);
    setSlow(false);
    loadingMoreRef.current = false;
    setLoadingMore(false);
    setRows([]);
    setTotal(null);
    const slowTimer = setTimeout(() => live && setSlow(true), 400);
    const timer = setTimeout(() => {
      Promise.all([source.count(query), source.search(query, PAGE, 0)])
        .then(([count, next]) => {
          if (!live) return;
          setTotal(count);
          setRows(next);
          setError(null);
          setSelectedId((current) =>
            current && next.some((row) => row.id === current) ? current : (next[0]?.id ?? null),
          );
        })
        .catch((problem: unknown) => {
          if (!live) return;
          setRows([]);
          setError(problem instanceof Error ? problem.message : String(problem));
        })
        .finally(() => {
          if (!live) return;
          setLoading(false);
          setSlow(false);
        });
    }, 120);

    return () => {
      live = false;
      clearTimeout(timer);
      clearTimeout(slowTimer);
    };
  }, [source, query, reload]);

  /** The next page, appended only if no newer query has replaced the list meanwhile. */
  const loadMore = useCallback(async () => {
    if (loadingMoreRef.current || loading || error || total === null || rows.length >= total) return;
    const offset = rows.length;
    loadingMoreRef.current = true;
    setLoadingMore(true);
    try {
      const next = await source.search(query, PAGE, offset);
      setRows((current) => (current.length === offset ? [...current, ...next] : current));
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      loadingMoreRef.current = false;
      setLoadingMore(false);
    }
  }, [loading, error, total, rows.length, source, query]);

  useEffect(() => {
    let live = true;
    source
      .status()
      .then((next) => live && setStatuses(next))
      .catch(() => live && setStatuses([]));
    return () => {
      live = false;
    };
  }, [source, reload]);

  useEffect(() => {
    let live = true;
    if (!selectedId) {
      setDetail(null);
      return;
    }
    source
      .load(selectedId)
      .then((next) => live && setDetail(next))
      .catch(() => live && setDetail(null));
    return () => {
      live = false;
    };
  }, [source, selectedId, reload]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      // D147: Ctrl+K is the palette everywhere except a terminal, where the shell owns the key.
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        if (target?.closest(".terminal-surface")) return;
        event.preventDefault();
        setPaletteOpen((open) => !open);
        return;
      }
      if (
        event.key === "/" &&
        target?.tagName !== "INPUT" &&
        target?.tagName !== "TEXTAREA"
      ) {
        event.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // A skill opened from the palette may not be in the current result page. Falling back to the
  // loaded detail keeps the pane usable without changing the shelf's query behind the user.
  const selected =
    rows.find((row) => row.id === selectedId) ??
    (detail && detail.skill.id === selectedId ? detail.skill : null);
  const shown = detail?.skill.id === selected?.id ? detail : null;

  /* ---------------------------- actions --------------------------- */

  /** Adopt an opened project: persist it, load its sessions, say where it lives. */
  const adoptProject = useCallback(async (info: ProjectInfo) => {
    setProject(info);
    try {
      window.localStorage.setItem(LAST_PROJECT, info.root);
    } catch {
      // A read-only profile should not stop the project from opening.
    }
    const listed = await api.listSessions(info.root);
    setSessions(listed);
    setNotice(
      isTauri()
        ? `${info.name} · ${listed.length} session${listed.length === 1 ? "" : "s"}`
        : `${info.name} · browser preview has no database`,
    );
  }, []);

  const openPath = useCallback(
    async (path: string) => {
      if (!path) return;
      try {
        await adoptProject(await api.openProject(path));
      } catch (problem) {
        setNotice(problem instanceof Error ? problem.message : String(problem));
      }
    },
    [adoptProject],
  );

  // D24: restore the last project on launch instead of asking again.
  useEffect(() => {
    if (!isTauri()) return;
    let remembered = "";
    try {
      remembered = window.localStorage.getItem(LAST_PROJECT) ?? "";
    } catch {
      remembered = "";
    }
    if (remembered) void openPath(remembered);
  }, [openPath]);

  // Start every local service the app fronts as it opens, then again once the project is
  // known: the chat checkout is found beside it. Nothing waits on this; each tab polls state.
  useEffect(() => {
    if (!isTauri()) return;
    const root = project?.root ?? null;
    if (bootedFor.current === root) return;
    bootedFor.current = root;
    void api.startServers(root).catch(() => {});
  }, [project?.root]);

  const chooseFolder = useCallback(async () => {
    try {
      const picked = await api.pickProjectFolder();
      if (picked) await openPath(picked);
    } catch (problem) {
      setNotice(problem instanceof Error ? problem.message : String(problem));
    }
  }, [openPath]);

  const beginWorkspace = useCallback(async () => {
    setNewWorkspace(true);
    setNewName("");
    try {
      setNewRoot(await api.workspaceRoot());
    } catch {
      setNewRoot("");
    }
  }, []);

  const createWorkspace = useCallback(async () => {
    const name = newName.trim();
    if (!name || newBusy) return;
    setNewBusy(true);
    try {
      const info = await api.createProject(name);
      setNewWorkspace(false);
      await adoptProject(info);
    } catch (problem) {
      setNotice(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setNewBusy(false);
    }
  }, [newName, newBusy, adoptProject]);

  /** Swarm step one: boot the room and list who is online. Sends nothing yet. */
  const prepareSwarm = useCallback(async (): Promise<SwarmTarget[]> => {
    // A refusal stays on the Session tab, in the composer's error line. Only a delivered swarm
    // moves the window to Chat.
    const refuse = (message: string) => {
      setSwarmNotice(message);
      return new Error(message);
    };
    const online = (roster: Record<string, AgentChatAgent>): SwarmTarget[] =>
      Object.entries(roster)
        .filter(([, agent]) => agent.available)
        .map(([name, agent]) => ({
          name,
          label: agent.label || name,
          role: agent.role ?? null,
        }));
    const target = project?.root ?? (isTauri() ? "" : "/preview/rigel");
    if (!target) throw refuse("Open a project before starting a swarm.");
    setSwarmNotice("Waking the room…");
    const status = await api.agentChat.start(target);
    if (!status.authed) {
      throw refuse(
        "Rigel could not take over the chat room. Stop the server on that port, start it from the Chat tab, then press Swarm again.",
      );
    }

    let targets = online(await api.agentChat.agents());
    if (targets.length === 0) {
      // Swarm is the fan-out: an empty room is a step to finish, not a reason to send the user
      // to another tab. Start the checkout's ready harnesses and give them a moment to join.
      setSwarmNotice("Starting the room's configured agents…");
      try {
        await api.agentChat.startAgents(target);
      } catch {
        // Missing CLIs and keys are reported by the roster below; some agents may still join.
      }
      for (let attempt = 0; attempt < 24 && targets.length === 0; attempt += 1) {
        await new Promise((resolve) => setTimeout(resolve, 500));
        targets = online(await api.agentChat.agents());
      }
    }
    if (targets.length === 0) {
      throw refuse(
        "No agent is online yet. Open Chat, start an agent there, then press Swarm again — or configure a harness in vendor/agentchattr/config.toml.",
      );
    }
    return targets;
  }, [project]);

  /** Swarm step two: broadcast the task to the agents the preflight left ticked. */
  const sendSwarm = useCallback(async (task: string, names: string[]) => {
    if (names.length === 0) return;
    const mentions = names.map((name) => `@${name}`).join(" ");
    await api.agentChat.send(
      "general",
      `${mentions} — swarm task. Work on this in parallel in the current project; coordinate here so you do not duplicate or overwrite each other.

${task}`,
    );
    setView("chat");
    setSwarmNotice(
      `Swarm sent to ${names.length} agent${names.length === 1 ? "" : "s"}: ${names.join(
        ", ",
      )}. Watch #general.`,
    );
  }, []);

  const indexRepo = useCallback(async () => {
    const repo = repoInput.trim();
    if (!repo || !source.indexRepo) return;
    setIndexing(true);
    setNotice(`Walking ${repo}…`);
    try {
      const report = await source.indexRepo(repo);
      setNotice(
        `${report.with_body} skills from ${report.repo ?? repo} · ${report.added} added · ${
          report.http_requests
        } requests`,
      );
      setRepoInput("");
      setReload((value) => value + 1);
    } catch (problem) {
      setNotice(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setIndexing(false);
    }
  }, [repoInput, source]);

  /* ---------------------------- banners --------------------------- */

  const offline = isTauri() && statuses.length > 0 && statuses.every((s) => s.state === "broken");
  const neverIndexed = isTauri() && statuses.length === 0 && rows.length === 0;
  const stale = statuses.filter((s) => s.state === "stale" && s.last_sync_at);

  const native = isTauri();
  const sessionProject = project?.root ?? (native ? "" : "/preview/rigel");

  const workspaceForm = newWorkspace ? (
    <form
      className="workspace-form"
      onSubmit={(event) => {
        event.preventDefault();
        void createWorkspace();
      }}
    >
      <label>
        <span>Workspace name</span>
        <input
          autoFocus
          value={newName}
          onChange={(event) => setNewName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") setNewWorkspace(false);
          }}
          placeholder="my-workspace"
          aria-label="Workspace name"
          spellCheck={false}
        />
      </label>
      <p className="fine">
        {newRoot
          ? `Creates ${newRoot}\\${newName.trim() || "<name>"} and opens it.`
          : "Creates a folder under your home directory and opens it."}
      </p>
      <div className="workspace-form-actions">
        <button className="primary" type="submit" disabled={newBusy || !newName.trim()}>
          {newBusy ? "Creating…" : "Create workspace"}
        </button>
        <button className="chip" type="button" onClick={() => setNewWorkspace(false)}>
          Cancel
        </button>
      </div>
    </form>
  ) : null;

  // Project-level actions live with the session thread, not the shelf.
  const projectControls = (
    <div className="session-project">
      <div className="open-project">
        {project ? (
          <button
            className="project-current"
            title={`${project.root} — click to switch folders`}
            onClick={() => void chooseFolder()}
          >
            <span>project</span>
            <strong>{project.name}</strong>
          </button>
        ) : null}
        <button
          className="primary"
          onClick={() => void chooseFolder()}
          disabled={!native}
          title={
            native
              ? "Choose a folder with the system picker"
              : "The desktop app has the folder picker"
          }
        >
          {project ? "Switch folder…" : "Open folder…"}
        </button>
        <button
          className="primary"
          onClick={() => void beginWorkspace()}
          disabled={!native}
          title={
            native
              ? "Create a workspace folder under your home directory"
              : "Available in the desktop app"
          }
        >
          New workspace
        </button>
      </div>
      {workspaceForm}
    </div>
  );

  // The inspector only exists on these surfaces; the diff is one page, and the shelf and the
  // terminal own their layout.
  const hasInspector = view === "session" || view === "settings";
  const handleTop = native ? 36 : 0;

  return (
    <div
      className={`shell${native ? " chrome" : ""}${view === "session" ? " session" : ""}${
        view === "settings" ? " settings" : ""
      }${view === "diff" ? " diff" : ""}`}
      style={
        {
          "--list-w": `${panes.list}px`,
          "--insp-w": `${panes.inspector}px`,
        } as CSSProperties
      }
    >
      {native ? <Titlebar project={project?.name ?? null} /> : null}

      {/* D158: the first Tab stop skips the rail and the list, whether or not the mouse moved. */}
      <a className="skip-link" href="#main">
        Skip to content
      </a>

      <nav className="rail" aria-label="Primary">
        <div className="rail-brand" title="Rigel">
          <RigelMark />
        </div>

        {VIEWS.map((item) => (
          <button
            key={item.id}
            className={`rail-btn${view === item.id ? " active" : ""}`}
            title={item.label}
            aria-label={
              item.id === "attention" && attention > 0
                ? `${item.label}, ${attention} waiting`
                : item.label
            }
            aria-pressed={view === item.id}
            onClick={() => {
              if (item.id !== "chat") setSwarmNotice(null);
              if (item.id === "terminal") setTerminalMounted(true);
              setView(item.id);
            }}
          >
            <svg
              width="18"
              height="18"
              viewBox="0 0 20 20"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d={item.d} />
            </svg>
            {item.id === "attention" && attention > 0 ? (
              <span className="rail-count" aria-hidden>
                {attention > 99 ? "99+" : attention}
              </span>
            ) : null}
          </button>
        ))}

        <div className="rail-spacer" />
        <div
          className={`rail-status ${statuses.some((s) => s.state === "broken") ? "bad" : ""}`}
          title={statuses.map((s) => `${s.id}: ${s.state}`).join(" · ") || "no sources yet"}
          aria-hidden
        >
          ●
        </div>
      </nav>

      {/* D159: two handles, drawn over the gutters rather than as grid children. */}
      <div
        className="pane-handle pane-handle-list"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize the list"
        tabIndex={0}
        style={{ left: `calc(48px + ${panes.list}px - 3px)`, top: handleTop, bottom: 28 }}
        onPointerDown={(event) => startPane("list", event)}
        onPointerMove={(event) => movePane("list", event.clientX)}
        onPointerUp={finishPane}
        onPointerCancel={finishPane}
        onDoubleClick={() => nudgePane("list", PANE_DEFAULTS.list - panes.list)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
          event.preventDefault();
          nudgePane("list", event.key === "ArrowRight" ? 12 : -12);
        }}
      />
      {hasInspector ? (
        <div
          className="pane-handle pane-handle-inspector"
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize the inspector"
          tabIndex={0}
          style={{ right: `calc(${panes.inspector}px - 3px)`, top: handleTop, bottom: 28 }}
          onPointerDown={(event) => startPane("inspector", event)}
          onPointerMove={(event) => movePane("inspector", event.clientX)}
          onPointerUp={finishPane}
          onPointerCancel={finishPane}
          onDoubleClick={() => nudgePane("inspector", PANE_DEFAULTS.inspector - panes.inspector)}
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
            event.preventDefault();
            // The inspector grows toward the transcript, so the arrow sense is inverted.
            nudgePane("inspector", event.key === "ArrowLeft" ? 12 : -12);
          }}
        />
      ) : null}

      {isTauri() && update?.available && !updateDismissed ? (
        <div className="key-banner update-banner" role="status">
          <span>
            Rigel {update.offered} is available
            {updateStatus?.auto_install && !import.meta.env.DEV
              ? " — installing automatically"
              : "."}
          </span>
          <button
            className="chip warn"
            onClick={() => void installUpdate()}
            disabled={updateBusy}
          >
            {updateBusy ? "Installing…" : "Install update"}
          </button>
          <button
            className="key-banner-close"
            aria-label="Dismiss"
            title="Dismiss"
            onClick={() => setUpdateDismissed(true)}
          >
            ×
          </button>
        </div>
      ) : null}

      {isTauri() && keys && !keys.native_ready && !keyPromptDismissed ? (
        <div className="key-banner" role="status">
          <span>
            No provider key yet. The agent can price a turn, but it cannot send one.
          </span>
          <button className="chip warn" onClick={() => setView("settings")}>
            Add key
          </button>
          <button
            className="key-banner-close"
            aria-label="Dismiss"
            title="Dismiss"
            onClick={() => setKeyPromptDismissed(true)}
          >
            ×
          </button>
        </div>
      ) : null}

      {view === "terminal" ? null : view === "session" ? (
        <SessionView
          project={sessionProject}
          onSessions={setSessions}
          onActive={setActive}
          onNotice={noticeFromSession}
          onOpenSettings={() => setView("settings")}
          onBrowseSkills={() => setView("shelf")}
          projectControls={projectControls}
          onSwarmPrepare={prepareSwarm}
          onSwarm={sendSwarm}
          focus={focus}
          onFocusHandled={clearFocus}
          onOpenHunk={(hunkId) => {
            if (!activeSession) return;
            setFocus({ sessionId: activeSession.id, hunkId });
            setView("diff");
          }}
        />
      ) : view === "attention" ? (
        <AttentionView
          project={sessionProject}
          onJump={(target) => void openAttention(target)}
          onOpenSession={() => setView("session")}
        />
      ) : view === "diff" ? (
        <DiffView
          project={sessionProject}
          session={activeSession?.id ?? null}
          focus={focus}
          onFocusHandled={clearFocus}
        />
      ) : view === "chat" ? (
        <ChatView
          project={sessionProject}
          notice={swarmNotice}
          onNotice={setNotice}
          onChooseProject={() => void chooseFolder()}
        />
      ) : view === "settings" ? (
        <SettingsView
          keys={keys}
          project={sessionProject}
          session={activeSession?.id ?? null}
          source={source}
          onKeys={setKeys}
          onNotice={setNotice}
        />
      ) : (
      <>
      <aside className="list-pane">
        <div className="list-head">
          <div className="list-title">
            <h1>{VIEWS.find((item) => item.id === view)?.label ?? "Rigel"}</h1>
            <span>
              {view === "shelf"
                ? total === null
                  ? "…"
                  : `${total.toLocaleString()} skills`
                : "Phase 2"}
            </span>
          </div>

          {view === "shelf" ? (
            <label className="search">
              <span aria-hidden>⌕</span>
              <input
                ref={searchRef}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="Search skills…"
                aria-label="Search skills"
              />
              <kbd>/</kbd>
            </label>
          ) : null}
        </div>

        {view === "shelf" && offline ? (
          <p className="banner muted" role="status">
            No source reachable. Showing the cached index.
          </p>
        ) : null}

        {view === "shelf" && !offline && stale.length > 0 ? (
          <p className="banner" role="status">
            {stale
              .map((s) => `${s.id} stale since ${when(s.last_sync_at)}`)
              .join(" · ")}
          </p>
        ) : null}

        {view === "shelf" ? (
          <div
            className="rows"
            role="listbox"
            aria-label="Skills"
            onScroll={(event) => {
              const pane = event.currentTarget;
              if (pane.scrollHeight - pane.scrollTop - pane.clientHeight < 400) {
                void loadMore();
              }
            }}
          >
            {rows.map((row) => (
              <button
                key={row.id}
                role="option"
                aria-selected={selected?.id === row.id}
                className={`row${selected?.id === row.id ? " on" : ""}`}
                onClick={() => setSelectedId(row.id)}
              >
                <span
                  className={`grade ${row.grade ? `grade-${row.grade.toLowerCase()}` : "grade-none"}`}
                  title={row.grade ? `Grade ${row.grade} — static review only` : "Not scanned"}
                >
                  {row.grade ?? "—"}
                </span>
                <span className="row-text">
                  <span className="row-name">
                    {row.name}
                    {row.unmeasured ? (
                      <span className="tag unmeasured" title="Advertises a number nobody measured">
                        unmeasured
                      </span>
                    ) : null}
                    {row.variants > 1 ? (
                      <span className="tag" title="Near-identical rows are shown, never hidden">
                        {row.variants} variants
                      </span>
                    ) : null}
                  </span>
                  <span className="row-desc">{row.description}</span>
                </span>
                <span className="row-meta">
                  {row.installs !== null ? compact(row.installs) : compact(row.stars)}
                </span>
              </button>
            ))}

            {loading && !slow ? <p className="empty subtle">Searching the index…</p> : null}
            {loading && slow ? (
              <p className="empty subtle">
                Searching the index… taking longer than usual
                {rows.length > 0 ? " (showing the last result)" : ""}.
              </p>
            ) : null}
            {!loading && rows.length === 0 ? (
              <p className="empty">
                {error
                  ? error
                  : query
                    ? `No skill matches “${query}”.`
                    : neverIndexed
                      ? "The index is empty. Walk a repository to fill it."
                      : "Nothing here yet."}
              </p>
            ) : null}

            {loadingMore ? <p className="empty subtle">Loading more…</p> : null}
            {!loading && !loadingMore && error === null && total !== null && rows.length < total ? (
              <button className="load-more" onClick={() => void loadMore()}>
                Load more · {rows.length.toLocaleString()} of {total.toLocaleString()}
              </button>
            ) : null}
            {!loading && total !== null && total > PAGE && rows.length >= total ? (
              <p className="empty subtle">All {total.toLocaleString()} skills shown.</p>
            ) : null}
          </div>
        ) : (
          <p className="empty">This surface arrives later in the plan.</p>
        )}
      </aside>

      <main id="main" tabIndex={-1} className="detail">
        <header className="detail-head">
          <div>
            <p className="eyebrow">
              {project
                ? project.name
                : selected?.repo
                  ? selected.repo
                  : selected
                    ? selected.source
                    : "Rigel"}
            </p>
            <h2>{selected?.name ?? "Shelf"}</h2>
            <p className="lede">
              {selected?.description ??
                "One place where several coding agents work on the same project."}
            </p>
          </div>

        </header>

        <p className="notice" role="status">
          {notice}
        </p>

        {!selected ? (
          <section className="onboarding">
            <h3>{isTauri() ? "The index is empty" : "Browser preview"}</h3>
            <p>
              {isTauri()
                ? "Rigel keeps one global index of skills it has walked. Point it at a repository once and it stays searchable offline."
                : "This build has no Tauri host, so the rows on the left are a fixed sample. Open the desktop app to search a real index."}
            </p>
            {source.indexRepo ? (
              <div className="index-form">
                <input
                  value={repoInput}
                  onChange={(event) => setRepoInput(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void indexRepo();
                  }}
                  placeholder="owner/repo or a GitHub URL"
                  aria-label="Repository to index"
                  spellCheck={false}
                  disabled={indexing}
                />
                <button className="primary" onClick={() => void indexRepo()} disabled={indexing}>
                  {indexing ? "Walking…" : "Index repository"}
                </button>
              </div>
            ) : null}
          </section>
        ) : null}

        {selected ? (
          <>
            <section className="facts">
              <div>
                <span className="label">Grade</span>
                <span className="value mono">
                  {selected.grade ?? "not scanned"}
                  {selected.grade_scope ? ` · ${selected.grade_scope}` : ""}
                </span>
              </div>
              <div>
                <span className="label">Source</span>
                <span className="value mono">{selected.source}</span>
              </div>
              <div>
                <span className="label">Stars</span>
                <span className="value mono">{compact(selected.stars)}</span>
              </div>
              <div>
                <span className="label">Tokens</span>
                <span className="value mono">~{compact(selected.tokens)}</span>
              </div>
              <div>
                <span className="label">Last seen</span>
                <span className="value mono">{when(selected.last_seen_at)}</span>
              </div>
              <div>
                <span className="label">Body</span>
                <span className="value mono">{selected.has_body ? "indexed" : "not fetched"}</span>
              </div>
            </section>

            {shown && shown.evidence.length > 0 ? (
              <section className="evidence">
                <h3>Evidence</h3>
                <table>
                  <thead>
                    <tr>
                      <th>Metric</th>
                      <th>Measured</th>
                      <th>Advertised</th>
                      <th>Source</th>
                    </tr>
                  </thead>
                  <tbody>
                    {shown.evidence.map((row) => (
                      <tr key={`${row.metric}-${row.source}`}>
                        <td>{row.metric}</td>
                        <td className={row.measured === null ? "missing" : "mono"}>
                          {row.measured === null ? "unmeasured" : `${row.measured}%`}
                        </td>
                        <td className="mono">
                          {row.advertised === null ? "—" : `${row.advertised}%`}
                        </td>
                        <td>
                          {row.source}
                          {row.source_url ? (
                            <>
                              {" · "}
                              <a href={row.source_url} target="_blank" rel="noreferrer">
                                link
                              </a>
                            </>
                          ) : null}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <p className="fine">
                  Advertised numbers come from the skill's own description. A row that says
                  unmeasured has no independent measurement in this index.
                </p>
              </section>
            ) : null}

            {shown && shown.capabilities.length > 0 ? (
              <section className="touches">
                <h3>Static review</h3>
                <ul>
                  {shown.capabilities.map((capability) => (
                    <li key={capability.label} className={`cap cap-${capability.severity}`}>
                      <span className="sev">{capability.severity}</span>
                      {capability.label}
                      {capability.evidence.map((line) => (
                        <code key={`${line.file}:${line.line}`}>
                          {line.file}:{line.line} {line.code}
                        </code>
                      ))}
                    </li>
                  ))}
                </ul>
                <p className="fine">
                  No dangerous capabilities detected in static review is not the same as safe.
                  Only the markdown above was read; scripts it tells an agent to run later are
                  outside this review.
                </p>
              </section>
            ) : null}

            {shown && shown.variants.length > 0 ? (
              <section className="variants">
                <h3>
                  {shown.variants.length + 1} near-identical variants · showing every one
                </h3>
                <ul>
                  {shown.variants.map((variant) => (
                    <li key={variant.id}>
                      <button className="link" onClick={() => setSelectedId(variant.id)}>
                        {variant.name}
                      </button>
                      <span className="variant-repo">{variant.repo ?? "unknown repo"}</span>
                      <span className="variant-delta">{variant.delta}</span>
                      <span className="variant-seen">{when(variant.last_seen_at)}</span>
                    </li>
                  ))}
                </ul>
              </section>
            ) : null}

            {selected.url ? (
              <p className="fine">
                <a href={selected.url} target="_blank" rel="noreferrer">
                  Open the source file
                </a>
                {selected.last_seen_at ? ` · last seen ${when(selected.last_seen_at)}` : ""}
              </p>
            ) : null}
          </>
        ) : null}
      </main>
      </>
      )}

      {terminalMounted ? (
        <div className="terminal-surface" hidden={view !== "terminal"}>
          <Suspense fallback={<p className="empty">Loading the terminal…</p>}>
            <TerminalView project={project?.root ?? null} />
          </Suspense>
        </div>
      ) : null}

      <Palette
        open={paletteOpen}
        project={project?.root ?? null}
        currentSession={activeSession?.id ?? null}
        onClose={() => setPaletteOpen(false)}
        onAction={runPaletteAction}
        onSkill={openSkillFromPalette}
        onSession={openSessionFromPalette}
        onTurn={openTurnFromPalette}
        onPatch={openPatchFromPalette}
      />

      <footer className="status" aria-label="Status">
        <span className="status-model">
          rigel {isTauri() ? "desktop" : "browser"}
          {view === "session" && activeSession ? " · native" : ""}
          {view === "chat" ? " · chat" : ""}
        </span>
        <span className="sep">·</span>
        <span className="mono">
          {view === "session" && activeSession
            ? `$${(activeSession.spend_micros / 1_000_000).toFixed(4)}`
            : "$0.00 · 0 tok"}
        </span>
        <span className="sep">·</span>
        <span>
          {view === "session" && activeSession
            ? `session ${activeSession.id.slice(0, 8)}`
            : `${(total ?? rows.length).toLocaleString()} skills`}
        </span>
        <span className="sep">·</span>
        <span>
          {sessions.length} session{sessions.length === 1 ? "" : "s"}
        </span>
        <span className="sep">·</span>
        <span className="mono" title="Provider key in the OS keychain">
          {isTauri()
            ? keys
              ? nativeKeyLabel(keys, providers)
              : "checking key"
            : "preview key"}
        </span>
        <span className="sep">·</span>
        <span className="mono" title={`source: ${source.label}`}>
          {statuses.length > 0
            ? statuses.map((s) => `${s.id} ${s.state}`).join(" · ")
            : isTauri()
              ? "no sources"
              : "sample"}
        </span>
        <span className="grow" />
        <button
          className="chip"
          onClick={() => setPaletteOpen(true)}
          title="Search actions, skills, sessions, turns and patches (Ctrl+K)"
        >
          Search · Ctrl K
        </button>
        <span className="metric" title="Interventions per completed task — the product metric (D17)">
          {activeSession
            ? `${activeSession.interventions}/${activeSession.tasks_done} interventions/tasks`
            : "0/0 interventions/tasks"}
        </span>
      </footer>
    </div>
  );
}

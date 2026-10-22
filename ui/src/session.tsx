import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  api,
  compact,
  isTauri,
  type Agent,
  type AgentEvent,
  type BudgetSnapshot,
  type Item,
  type ApprovalAnswer,
  type Patch,
  type PendingApproval,
  type Session,
  type Settings,
  type Turn,
  type SwarmTarget,
  type TurnQuote,
  type TurnTranscript,
} from "./api";
import { ModelControls } from "./modelpicker";
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
  onChooseProject?: () => void;
  /** Swarm step one: who is online, or a refusal to show. Sends nothing. */
  onSwarmPrepare?: () => Promise<SwarmTarget[]>;
  /** Swarm step two: broadcast one task to the chosen agent names. */
  onSwarm?: (task: string, names: string[]) => Promise<void>;
}

const money = (micros: number | null | undefined): string =>
  micros === null || micros === undefined ? "—" : `$${(micros / 1_000_000).toFixed(6)}`;

const cents = (value: number | null | undefined): string =>
  value === null || value === undefined ? "no cap" : `$${(value / 100).toFixed(2)}`;

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

export default function SessionView({
  project,
  onSessions,
  onActive,
  onNotice,
  onOpenSettings,
  onChooseProject,
  onSwarmPrepare,
  onSwarm,
}: Props) {
  const tauri = isTauri();
  const [session, setSession] = useState<Session | null>(() => (tauri ? null : previewSession));
  const [agents, setAgents] = useState<Agent[]>(() => (tauri ? [] : previewAgents));
  const [turns, setTurns] = useState<TurnTranscript[]>(() => (tauri ? [] : previewTranscript));
  const [budget, setBudget] = useState<BudgetSnapshot | null>(() => (tauri ? null : previewBudget));
  const [patches, setPatches] = useState<Patch[]>(() => (tauri ? [] : previewPatches));
  const [stream, setStream] = useState<Record<string, string>>({});
  const [liveTurn, setLiveTurn] = useState<Turn | null>(null);
  // D40: one transcript, or two side by side. Never more than two, and never a grid.
  const [shown, setShown] = useState<string[]>([]);
  const [raw, setRaw] = useState(false);
  const [text, setText] = useState("");
  const [quote, setQuote] = useState<TurnQuote | null>(null);
  const [capInput, setCapInput] = useState("");
  const [keyPresent, setKeyPresent] = useState<boolean | null>(tauri ? null : true);
  const [warning, setWarning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
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

  const spendMicros = budget?.session_micros ?? (session ? session.spend_cents * 10_000 : 0);
  const capMicros = budget?.cap_micros ?? (session?.budget_cents ? session.budget_cents * 10_000 : null);
  const model = defaults?.model ?? "gpt-5.6-sol-medium";
  const effort = defaults?.effort ?? "medium";

  const sessionRef = useRef<Session | null>(session);
  const projectRef = useRef(project);
  const bottomRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    sessionRef.current = session;
    onActive(session);
  }, [session, onActive]);
  useEffect(() => {
    projectRef.current = project;
  }, [project]);

  // A command can be asked for at any moment while a turn runs, so the card polls. In the
  // browser the same call returns the sample queue, which is how this state is inspectable.
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const open = await api.approvals.pending();
        if (alive) setApprovals(open);
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

  const answer = async (pending: PendingApproval, scope: ApprovalAnswer) => {
    // The row disappears as soon as it is answered, whatever the answer: a question that has
    // been decided must never look open.
    setApprovals((open) => open.filter((row) => row.id !== pending.id));
    try {
      await api.approvals.answer(pending.id, scope);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  };

  /* ------------------------------ load state ------------------------------ */

  const refresh = useCallback(async () => {
    const current = sessionRef.current;
    if (!tauri || !projectRef.current || !current) return;
    try {
      const [nextTurns, nextAgents, nextPatches, nextSession] = await Promise.all([
        api.transcript(projectRef.current, current.id),
        api.listAgents(projectRef.current, current.id),
        api.listPatches(projectRef.current, current.id),
        api.getSession(projectRef.current, current.id),
      ]);
      setTurns(nextTurns);
      setAgents(nextAgents);
      setPatches(nextPatches);
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
      setBudget(null);
      setError(null);
      if (!tauri || !project) return;
      try {
        const [loadedTurns, loadedAgents, loadedPatches, fresh] = await Promise.all([
          api.transcript(project, next.id),
          api.listAgents(project, next.id),
          api.listPatches(project, next.id),
          api.getSession(project, next.id),
        ]);
        setTurns(loadedTurns);
        setAgents(loadedAgents);
        setPatches(loadedPatches);
        setSession(fresh);
        if (loadedAgents[0]) {
          setBudget(await api.getBudget(project, loadedAgents[0].id));
        }
        const keys = await api.keyStatus();
        setKeyPresent(keys.omniroute || keys.openrouter || keys.gemini);
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
        setKeyPresent(keys.omniroute || keys.openrouter || keys.gemini);
        const pick =
          listed.find((candidate) => candidate.status !== "ended") ?? listed[0] ?? null;
        if (pick) {
          await selectSession(pick);
        } else {
          setSession(null);
          setTurns([]);
          setAgents([]);
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
    async (nextModel: string, nextEffort: string) => {
      const base: Settings = defaults ?? {
        theme: "dark",
        default_budget_cents: 500,
        unattended_default: false,
        provider: "omniroute",
        model: nextModel,
        effort: nextEffort,
      };
      const next = { ...base, model: nextModel, effort: nextEffort };
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
                parent_turn_id: null,
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
              parent_turn_id: null,
              seq: payload.seq,
              status: "running",
              prompt: payload.prompt ?? null,
              started_at: Date.now(),
              completed_at: null,
              usage: null,
              stop_reason: null,
            });
            setStream((previous) => ({ ...previous, [payload.turn_id]: "" }));
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
          case "turn_finished": {
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

  /* ------------------------------- actions ------------------------------- */

  const startSession = useCallback(async () => {
    if (!tauri || !project) {
      onNotice("Open the desktop app to create a session.");
      return;
    }
    setStarting(true);
    try {
      const created = await api.startSession(project, "Session", 500);
      const listed = await api.listSessions(project);
      onSessions(listed);
      await selectSession(created);
      onNotice(`Session started with a $5.00 cap.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setStarting(false);
    }
  }, [tauri, project, onSessions, onNotice, selectSession]);

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

  const send = useCallback(async () => {
    const prompt = text.trim();
    if (!prompt || !session || busy) return;
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
            prompt,
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
      return;
    }

    setBusy(true);
    try {
      let worker = agents[0];
      if (!worker) {
        worker = await api.spawnAgent(project, session.id, "worker", model);
        setAgents([worker]);
      }
      const reply = await api.sendTurn(project, session.id, worker.id, prompt, model, effort);
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
  }, [text, session, busy, tauri, agents, project, model, effort]);

  /** Swarm, first press: freeze the task and ask the room who can take it. */
  const prepareSwarm = useCallback(async () => {
    if (!onSwarmPrepare) return;
    const task = text.trim();
    if (!task) {
      onNotice("Type the swarm task in the composer first.");
      return;
    }
    setPreparing(true);
    setError(null);
    try {
      const targets = await onSwarmPrepare();
      setSwarmTask(task);
      setSwarmTargets(targets);
      setSwarmPicked(targets.map((target) => target.name));
    } catch (problem) {
      // Refusals (no room, unauthed room, nobody online) read the same as any other error.
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setPreparing(false);
    }
  }, [onSwarmPrepare, text, onNotice]);

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
  const live = liveTurn !== null;
  const percent = capMicros ? Math.min(100, (spendMicros / capMicros) * 100) : 0;
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
        <main className="detail session-main">
          <p className="empty">No project open.</p>
          {onChooseProject ? (
            <div className="gate-actions">
              <button className="primary" onClick={onChooseProject}>
                Open a folder…
              </button>
            </div>
          ) : null}
        </main>
        <aside className="inspector" />
      </>
    );
  }

  return (
    <>
      <aside className="list-pane roster">
        <div className="list-head">
          <div className="list-title">
            <h1>Agents</h1>
            <span>{session ? session.status : "no session"}</span>
          </div>
          {!session ? (
            <button className="primary wide" onClick={() => void startSession()} disabled={starting}>
              {starting ? "Starting…" : "Start a session · $5.00 cap"}
            </button>
          ) : (
            <div className="roster-actions">
              <button className={shown.length === 0 ? "chip on" : "chip"} onClick={() => setShown([])}>
                all agents
              </button>
              {shown.length === 2 ? <span className="fine">two transcripts side by side</span> : null}
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
        </div>

        <div className="rows">
          {roster.map(({ agent, nested }) => (
            <button
              key={agent.id}
              className={`row agent-row${nested ? " nested" : ""}${shown.includes(agent.id) ? " on" : ""}`}
              onClick={() => toggleShown(agent.id)}
              title={
                nested
                  ? "A subagent: spawned by the turn above it and billed to this row"
                  : undefined
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
                      title="Rigel shows this agent's turns but cannot send it new work yet; harness turns arrive with P4.10."
                    >
                      observed only
                    </span>
                  ) : null}
                </span>
                <span className="row-desc">{agent.model ?? "no model"}</span>
              </span>
              <span className="row-meta">{money(agent.spend_micros)}</span>
            </button>
          ))}
          {agents.length === 0 && session ? (
            <p className="empty subtle">No agent yet. The first turn starts one.</p>
          ) : null}
        </div>

        <div className="roster-foot">
          <span className={`status-dot ${live ? "running" : "idle"}`} />
          <span>{live ? `turn ${liveTurn?.seq} running` : "idle"}</span>
          <span className="grow" />
          {/* D40 lists claims beside spend. The claim ledger is Phase 8, so the roster says so
              instead of rendering an empty column that would read as "nobody holds any files". */}
          <span
            className="claim-state"
            title="File claims arrive with the coordinator's claim ledger (Phase 8). Until then there is nothing to show, which is not the same as no claims."
          >
            claims not tracked yet
          </span>
          <span className="mono">{compact(agents.reduce((n, a) => n + a.tokens_in + a.tokens_out, 0))} tok</span>
        </div>
      </aside>

      <main className="detail session-main">
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
              onModel={(next) => void applyModel(next, effort)}
              onEffort={(next) => void applyModel(model, next)}
            />
            {keyPresent === false && onOpenSettings ? (
              <button className="chip warn" onClick={onOpenSettings}>
                Add key
              </button>
            ) : null}
            <label className="toggle">
              <input type="checkbox" checked={raw} onChange={(event) => setRaw(event.target.checked)} />
              raw
            </label>
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
          <div className="transcripts">
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
                        stream={stream}
                      />
                    ))}
                  </section>
                );
              })}
            </div>
            <div ref={bottomRef} />
          </div>
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
            <div className="composer">
              <textarea
                value={text}
                onChange={(event) => setText(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !event.shiftKey) {
                    event.preventDefault();
                    void send();
                  }
                }}
                placeholder={live ? "A turn is running…" : "Ask the agent; Enter sends, Shift+Enter adds a line"}
                rows={2}
                disabled={live}
              />
              <div className="composer-actions">
                {onSwarmPrepare ? (
                  <button
                    className="chip swarm"
                    title="Show which agents are online, then send to the ones you keep ticked"
                    onClick={() => void prepareSwarm()}
                    disabled={live || preparing || !text.trim() || swarmTargets !== null}
                  >
                    {preparing ? "Waking…" : "Swarm"}
                  </button>
                ) : null}
                <button className="primary" onClick={() => void send()} disabled={live || !text.trim()}>
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
            {text.trim() && quote ? (
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
 * Only the native agent accepts a turn today; the harnesses are watched until P4.10 wires their
 * turns. D40 wants that difference visible, so a harness row says so rather than looking sendable.
 */
function observedOnly(agent: Agent): boolean {
  return agent.harness !== "native";
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
  onAnswer: (pending: PendingApproval, scope: ApprovalAnswer) => void;
}) {
  const detail = pending.detail ?? {};
  const cwd = typeof detail.cwd === "string" ? detail.cwd : null;
  const timeout = typeof detail.timeout_secs === "number" ? detail.timeout_secs : null;
  const destructive = Array.isArray(detail.destructive)
    ? (detail.destructive as { id?: string; summary?: string }[])
    : [];
  const deliberation =
    detail.deliberation && typeof detail.deliberation === "object"
      ? (detail.deliberation as { note?: string })
      : null;
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
}: {
  entry: TurnTranscript;
  turns: TurnTranscript[];
  raw: boolean;
  stream: Record<string, string>;
}) {
  const children = turns.filter((candidate) => candidate.turn.parent_turn_id === entry.turn.id);
  const childSpend = children.reduce(
    (sum, child) => sum + (child.turn.usage?.cost_micros ?? 0),
    0,
  );
  return (
    <article className="turn">
      <header className="turn-head">
        <span className="turn-seq">turn {entry.turn.seq}</span>
        <span className={`pill ${entry.turn.status}`}>{entry.turn.status}</span>
        {entry.turn.stop_reason && entry.turn.stop_reason !== entry.turn.status ? (
          <span className="turn-stop">{entry.turn.stop_reason}</span>
        ) : null}
        <span className="grow" />
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

      {entry.turn.prompt ? (
        <div className="msg user">
          <span className="gutter">you</span>
          <div className="msg-body">{entry.turn.prompt}</div>
        </div>
      ) : null}

      {entry.items.map((item) => (
        <ItemRow key={item.id} item={item} raw={raw} />
      ))}

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
    const command = result && typeof result.command === "string" ? result.command : "?";
    const stdout = result && typeof result.stdout === "string" ? result.stdout : "";
    const stderr = result && typeof result.stderr === "string" ? result.stderr : "";
    const code =
      result && typeof result.exit_code === "number"
        ? String(result.exit_code)
        : result && result.approved === false
          ? "denied"
          : result && result.timed_out === true
            ? "timed out"
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
        <details className={`tool-line${item.status === "ok" ? "" : " bad"}`}>
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

/**
 * The Chat tab (P6): agentchattr, fronted by the core.
 *
 * The shape is a messaging app: bots down the left, one thread in the middle. Selecting a bot
 * narrows the stream to it — its own messages plus everything you wrote — and the composer
 * addresses it with `@name`, which is how the room routes a trigger. Selecting a channel puts
 * every bot back in view. Nothing here talks to the chat server: all state flows through
 * `api.agentChat` (Rust invoke), so the window never sees the session token.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { useMenu } from "./menu";
import RefineCard from "./refine";
import {
  api,
  type AgentChatAgent,
  type AgentChatAttachment,
  type AgentChatHarness,
  type AgentChatMessage,
  type AgentChatProcess,
} from "./api";

/** Mirrors `same_agent` in the core: the room may register `claude-2` for configured `claude`. */
function sameAgent(candidate: string, configured: string): boolean {
  return candidate === configured || candidate.startsWith(`${configured}-`);
}

type Status = Awaited<ReturnType<typeof api.agentChat.status>>;

const POLL_MS = 2_000;

/** The first glyph of a name, for the avatar tile. */
function initial(text: string): string {
  const found = text.trim().match(/[a-z0-9]/i);
  return (found ? found[0] : "?").toUpperCase();
}

/** White or near-black, whichever reads on an agent's configured colour. */
function readableOn(color: string | null | undefined): string {
  const value = (color ?? "").replace("#", "");
  if (value.length !== 6) return "var(--text)";
  const red = parseInt(value.slice(0, 2), 16);
  const green = parseInt(value.slice(2, 4), 16);
  const blue = parseInt(value.slice(4, 6), 16);
  if ([red, green, blue].some((part) => Number.isNaN(part))) return "var(--text)";
  return (0.299 * red + 0.587 * green + 0.114 * blue) / 255 > 0.62
    ? "var(--on-light)"
    : "var(--on-dark)";
}

/** `21:18:02` as a clock a person reads. */
function friendlyTime(time: string): string {
  const [hourText, minuteText] = time.split(":");
  const hour = Number(hourText);
  if (!Number.isFinite(hour)) return time;
  const suffix = hour >= 12 ? "PM" : "AM";
  const display = hour % 12 === 0 ? 12 : hour % 12;
  return `${display}:${minuteText ?? "00"} ${suffix}`;
}

/** Seconds into the day, so a gap across midnight stays a gap and not a negative number. */
function secondsOf(time: string): number {
  const [hour, minute, second] = time.split(":").map(Number);
  if (![hour, minute].every(Number.isFinite)) return 0;
  return hour * 3600 + minute * 60 + (Number.isFinite(second) ? second : 0);
}

function gapBefore(previous: AgentChatMessage, next: AgentChatMessage): number {
  const delta = secondsOf(next.time) - secondsOf(previous.time);
  return delta < 0 ? delta + 86_400 : delta;
}

/** Who the draft is addressed to, for the refiner's prompt. A bare mention is enough here. */
function mentionTarget(text: string): string {
  const found = text.match(/@([a-z0-9_-]+)/i);
  return found ? `@${found[1]}` : "the room";
}

function mentions(text: string, name: string): boolean {
  return new RegExp(`@${name}(?![a-z0-9_-])`, "i").test(text);
}

/** The one line under a bot's name: what it is, what it is doing, and who owns it. */
function statusOf(agent: AgentChatAgent, owned?: AgentChatProcess): string {
  const state = agent.available ? (agent.busy ? "working" : "online") : owned ? "starting" : "offline";
  return [agent.role, state, owned ? `pid ${owned.pid}` : null].filter(Boolean).join(" · ");
}

/** The last path segment, which is what a person calls the folder. */
function projectName(project: string): string {
  const trimmed = project.trim().replace(/[\\/]+$/, "");
  if (!trimmed) return "No project";
  const parts = trimmed.split(/[\\/]/);
  return parts[parts.length - 1] || trimmed;
}

/** Image attachments a bubble draws; anything else stays a named chip. */
function isImage(name: string): boolean {
  return /\.(png|jpe?g|gif|webp|avif|bmp)$/i.test(name);
}

/**
 * Room uploads drawn inside a bubble. The bytes come from the core as `data:` URLs, so the
 * renderer never holds the session token or the chat server's origin.
 */
function BubbleFiles({ files }: { files: AgentChatAttachment[] }) {
  const [sources, setSources] = useState<Record<string, string>>({});
  const requested = useRef(new Set<string>());
  const key = files.map((file) => file.url).join("|");

  useEffect(() => {
    let live = true;
    for (const file of files) {
      if (!isImage(file.name) || requested.current.has(file.url)) continue;
      requested.current.add(file.url);
      void api.agentChat
        .attachment(file.url)
        .then((source) => {
          if (live && source) setSources((current) => ({ ...current, [file.url]: source }));
        })
        .catch(() => undefined);
    }
    return () => {
      live = false;
    };
    // `key` names the file list; the effect must run once per distinct list, not per render.
  }, [key]);

  return (
    <div className="bots-files">
      {files.map((file) =>
        sources[file.url] ? (
          <img
            key={file.url}
            className="bots-file-image"
            src={sources[file.url]}
            alt={file.name}
            title={file.name}
            loading="lazy"
          />
        ) : (
          <span key={file.url} className="bots-file" title={file.name}>
            {file.name}
          </span>
        ),
      )}
    </div>
  );
}

export default function ChatView({
  project,
  notice,
  onNotice,
  onChooseProject,
}: {
  project: string;
  /** Cross-view feedback from the Session Swarm button, shown here after it navigates. */
  notice?: string | null;
  onNotice: (notice: string) => void;
  onChooseProject?: () => void;
}) {
  const contextMenu = useMenu();
  const [status, setStatus] = useState<Status | null>(null);
  const [booting, setBooting] = useState(false);
  const [rechecking, setRechecking] = useState(false);
  const [agents, setAgents] = useState<Record<string, AgentChatAgent>>({});
  const [processes, setProcesses] = useState<AgentChatProcess[]>([]);
  const [harnesses, setHarnesses] = useState<AgentChatHarness[]>([]);
  const [busyAgent, setBusyAgent] = useState<string | null>(null);
  const [channels, setChannels] = useState<string[]>(["general"]);
  const [channel, setChannel] = useState("general");
  const [messages, setMessages] = useState<AgentChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  // The draft waiting in the review card. Non-null means Send paused before the room.
  const [review, setReview] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  /** The bot whose thread is open; null is the room. */
  const [focus, setFocus] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  const copyText = useCallback(
    async (text: string, label: string) => {
      if (!text) return;
      try {
        await navigator.clipboard.writeText(text);
        onNotice(`${label} copied.`);
      } catch {
        onNotice(`Could not copy ${label.toLowerCase()} from this window.`);
      }
    },
    [onNotice],
  );
  const lastIdRef = useRef(0);
  const streamRef = useRef<HTMLDivElement | null>(null);

  /* ----------------------- the poll loop: reads only ----------------------- */

  useEffect(() => {
    // A channel switch is a reset: poll from zero, don't mix streams.
    lastIdRef.current = 0;
    setMessages([]);
    let live = true;
    let timer: ReturnType<typeof setTimeout> | null = null;

    const tick = async () => {
      try {
        const next = await api.agentChat.status(project || null);
        if (!live) return;
        setStatus(next);
        if (next.authed) {
          const [room, presence, owned, configured] = await Promise.all([
            api.agentChat.channels(),
            api.agentChat.agents(),
            api.agentChat.launched(),
            api.agentChat.harnesses(project || null),
          ]);
          if (!live) return;
          setChannels(room);
          setAgents(presence);
          setProcesses(owned);
          setHarnesses(configured);
          const fresh = await api.agentChat.messages(channel, lastIdRef.current);
          if (fresh.length > 0) {
            setMessages((current) => (lastIdRef.current === 0 ? fresh : [...current, ...fresh]));
            lastIdRef.current = fresh[fresh.length - 1].id;
          }
        }
      } catch (problem) {
        if (live) setStatus((current) => current ?? null);
        // Polling errors leave the last good frame up; the next tick retries.
        void problem;
      }
      if (live) timer = setTimeout(tick, POLL_MS);
    };

    void tick();
    return () => {
      live = false;
      if (timer) clearTimeout(timer);
    };
  }, [project, channel]);

  // New messages scroll the stream to the bottom.
  useEffect(() => {
    const stream = streamRef.current;
    if (!stream) return;
    stream.scrollTop = stream.scrollHeight;
  }, [messages.length, focus]);

  /* ------------------------------ actions ------------------------------ */

  const start = useCallback(async () => {
    if (!isStartable(project)) {
      onNotice("Open a project first — the chat checkout is found next to it (vendor/agentchattr).");
      return;
    }
    setBooting(true);
    onNotice("Booting the chat server…");
    try {
      const next = await api.agentChat.start(project || null);
      setStatus(next);
      onNotice(
        next.authed
          ? `chat up at ${next.origin}`
          : `Rigel could not take over the room at ${next.origin}: its landing page exposed no session token. Stop that server, then start the room here.`
      );
    } catch (problem) {
      onNotice(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBooting(false);
    }
  }, [project, onNotice]);

  /** Re-ask the core whether the checkout is there now (the poll also catches it). */
  const recheck = useCallback(async () => {
    setRechecking(true);
    try {
      setStatus(await api.agentChat.status(project || null));
    } catch (problem) {
      onNotice(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setRechecking(false);
    }
  }, [project, onNotice]);

  /** Launch every configured agent on a hidden console: they work, nothing opens on screen. */
  const startAgents = useCallback(async () => {
    setStarting(true);
    try {
      const launched = await api.agentChat.startAgents(project || null);
      onNotice(
        launched.length > 0
          ? `Started ${launched.join(", ")} in the background — no windows, no terminals.`
          : "Every configured agent is already in the room.",
      );
    } catch (problem) {
      onNotice(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setStarting(false);
    }
  }, [project, onNotice]);

  /** Start one configured harness; the roster refreshes on the next poll. */
  const startOne = useCallback(
    async (name: string) => {
      setBusyAgent(name);
      try {
        onNotice(await api.agentChat.startAgent(project || null, name));
      } catch (problem) {
        onNotice(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setBusyAgent(null);
      }
    },
    [project, onNotice],
  );

  /** Stop one agent Rigel owns, killing the wrapper and everything under it. */
  const stopOne = useCallback(
    async (name: string) => {
      setBusyAgent(name);
      try {
        onNotice(await api.agentChat.stopAgent(name));
      } catch (problem) {
        onNotice(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setBusyAgent(null);
      }
    },
    [onNotice],
  );

  /** The confirmed text goes to the room; a failure leaves it in the composer. */
  const deliver = useCallback(
    async (text: string) => {
      if (!text || sending) return;
      setSending(true);
      try {
        await api.agentChat.send(channel, text);
        setDraft("");
      } catch (problem) {
        setDraft(text);
        onNotice(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setSending(false);
      }
    },
    [sending, channel, onNotice],
  );

  /**
   * Send now reviews first: the room sees only what the user confirmed in the card. A thread
   * opened on one bot is addressed to it, which is exactly how the room routes a trigger.
   */
  const send = useCallback(() => {
    const text = draft.trim();
    if (!text || sending || review) return;
    setReview(focus && !mentions(text, focus) ? `@${focus} ${text}` : text);
  }, [draft, sending, review, focus]);

  /* ------------------------------- empty ------------------------------- */

  if (status && !status.running) {
    return (
      <>
        <aside className="list-pane">
          <div className="list-head">
            <div className="list-title">
              <h1>Chat</h1>
              <span>P6</span>
            </div>
          </div>
        </aside>
        <main id="main" tabIndex={-1} className="detail chat-main">
          {notice ? <p className="notice" role="status">{notice}</p> : null}
          <section className="chat-empty">
            <h3>{status.setup ? "agentchattr is not installed here" : "The room is not running"}</h3>
            {status.setup ? (
              <>
                <p>
                  One checkout, in this project; Rigel runs it with no console window and keeps
                  the session token in Rust, so this window never sees it.
                </p>
                <pre className="chat-setup">{status.setup}</pre>
                <div className="chat-setup-actions">
                  <button className="primary" onClick={() => void recheck()} disabled={rechecking}>
                    {rechecking ? "Checking…" : "Check again"}
                  </button>
                  {onChooseProject ? (
                    <button className="chip" onClick={onChooseProject}>
                      Open a folder…
                    </button>
                  ) : null}
                </div>
              </>
            ) : (
              <>
                <p>
                  Rigel starts agentchattr itself — the same way it boots OmniRoute — and holds
                  the session token so this window never sees it.
                </p>
                {isStartable(project) ? (
                  <button className="primary" onClick={() => void start()} disabled={booting}>
                    {booting ? "Booting…" : "Start the chat server"}
                  </button>
                ) : onChooseProject ? (
                  <button className="primary" onClick={onChooseProject}>
                    Open a folder…
                  </button>
                ) : null}
                {status.dir ? <p className="fine">{status.dir}</p> : null}
              </>
            )}
          </section>
        </main>
      </>
    );
  }

  if (status && status.running && !status.authed) {
    return (
      <>
        <aside className="list-pane">
          <div className="list-head">
            <div className="list-title">
              <h1>Chat</h1>
              <span>P6</span>
            </div>
          </div>
        </aside>
        <main id="main" tabIndex={-1} className="detail chat-main">
          {notice ? <p className="notice" role="status">{notice}</p> : null}
          <section className="chat-empty">
            <h3>This room is not Rigel's</h3>
            <p>
              Something is listening at {status.origin} ({status.dir ?? "checkout unknown"}
              ), but its landing page carried no session token Rigel could use. A room left by an
              earlier Rigel window is taken over automatically; one behind a proxy or another
              server needs to be closed, then started from here.
            </p>
            <button className="primary" onClick={() => void start()} disabled={booting}>
              {booting ? "Booting…" : "Try again"}
            </button>
          </section>
        </main>
      </>
    );
  }

  /* ------------------------------- live ------------------------------- */

  // `/api/status` also carries room-level keys (`paused`) with no presence fields; only real
  // agents get a row.
  const presence = Object.entries(agents)
    .filter(([, agent]) => typeof agent?.available === "boolean")
    .sort(([, left], [, right]) => Number(right.available) - Number(left.available));
  const online = presence.filter(([, agent]) => agent.available).length;
  const needle = query.trim().toLowerCase();
  const matches = (...fields: (string | null | undefined)[]): boolean =>
    needle.length === 0 || fields.some((field) => (field ?? "").toLowerCase().includes(needle));

  const bots = presence.filter(([name, agent]) =>
    matches(agent.label, name, agent.role, agent.busy ? "working" : "online"),
  );
  const startable = harnesses.filter(
    (harness) =>
      harness.ready &&
      !presence.some(([name]) => sameAgent(name, harness.name)) &&
      matches(harness.label, harness.name),
  );
  // Installed is not the same as usable. These rows make a login or missing key visible instead
  // of leaving a harness that cannot answer out of the roster entirely.
  const blocked = harnesses.filter(
    (harness) =>
      !harness.ready &&
      !presence.some(([name]) => sameAgent(name, harness.name)) &&
      matches(harness.label, harness.name, harness.status, harness.evidence),
  );
  const statusLabel = (harness: AgentChatHarness): string => {
    switch (harness.status) {
      case "missing":
        return "not installed";
      case "missing_key":
        return "key missing";
      case "unauthenticated":
        return "sign-in needed";
      case "provider_blocked":
        return "provider blocked";
      case "probe_timeout":
        return "still starting";
      case "probe_failed":
        return "probe failed";
      case "usable":
        return "usable";
      default:
        return "not checked";
    }
  };
  const explainHarness = (harness: AgentChatHarness): string =>
    [
      `${harness.label}: ${statusLabel(harness)}`,
      harness.evidence,
      harness.remediation ? `Fix: ${harness.remediation}` : null,
      harness.credential ? `Accepts: ${harness.credential}` : null,
    ]
      .filter(Boolean)
      .join("\n");

  const focused = focus ? agents[focus] ?? null : null;
  const focusedLabel =
    focused?.label || harnesses.find((harness) => harness.name === focus)?.label || focus || "";
  const focusedOwned = focus ? processes.find((process) => sameAgent(focus, process.name)) : undefined;
  const title = focus ? focusedLabel : `# ${channel}`;
  const subtitle = focus
    ? focused
      ? statusOf(focused, focusedOwned)
      : "not in the room"
    : `${online} online · ${presence.length} bot${presence.length === 1 ? "" : "s"} in the room`;

  // A bot's thread is what it said plus what was addressed to it; system lines (joins, leaves)
  // are context for both. A room-wide message stays in the room, where everyone saw it.
  const thread = focus
    ? messages.filter((message) =>
        message.kind !== "chat"
          ? true
          : message.sender === focus ||
            (message.sender === "user" && mentions(message.text, focus)),
      )
    : messages;

  const ownerLabel = (sender: string): string =>
    agents[sender]?.label || harnesses.find((harness) => harness.name === sender)?.label || sender;

  /** One menu for a bot, opened from its row's right-click or the thread header. */
  const openAgentMenu = (event: React.MouseEvent, name: string, agent: AgentChatAgent | null) => {
    const owned = processes.find((process) => sameAgent(name, process.name));
    contextMenu.open(event, {
      eyebrow: "Chat bot",
      title: agent?.label || name,
      subtitle: agent ? statusOf(agent, owned) : "configured, not running",
      sections: [
        {
          label: "Bot",
          items: [
            {
              label: "Copy mention",
              detail: `@${name}`,
              icon: "copy",
              onSelect: () => void copyText(`@${name}`, "Mention"),
            },
            {
              label: "Copy name",
              icon: "copy",
              onSelect: () => void copyText(name, "Bot name"),
            },
            {
              label: "Copy role",
              icon: "quote",
              disabled: !agent?.role,
              onSelect: () => void copyText(agent?.role ?? "", "Role"),
            },
          ],
        },
        {
          label: "Process",
          items: [
            owned
              ? {
                  label: "Stop this bot",
                  detail: `pid ${owned.pid}`,
                  icon: "stop",
                  danger: true,
                  disabled: busyAgent !== null,
                  onSelect: () => void stopOne(owned.name),
                }
              : {
                  label: agent ? "Not started by Rigel" : "Start this bot",
                  detail: agent ? "Start it from the roster or Settings" : "No windows, no terminals",
                  icon: "open",
                  disabled: agent !== null || busyAgent !== null,
                  onSelect: () => void startOne(name),
                },
          ],
        },
      ],
      footer: "The room sees every mention; a thread just narrows the view",
    });
  };

  return (
    <>
      <aside className="list-pane bots-pane" aria-label="Bots">
        <div className="bots-side-head">
          <div className="bots-project">
            <span className="bots-project-mark" aria-hidden>
              {initial(projectName(project))}
            </span>
            <div className="bots-project-text">
              <strong title={project}>{projectName(project)}</strong>
              <span>
                {online} online · {presence.length} bot{presence.length === 1 ? "" : "s"}
              </span>
            </div>
            <button
              className="bots-add"
              onClick={() => void startAgents()}
              disabled={starting}
              title="Start every configured bot on a hidden console"
              aria-label="Start every configured bot"
            >
              +
            </button>
          </div>

          <label className="search bots-search">
            <svg
              width="12"
              height="12"
              viewBox="0 0 20 20"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.6"
              aria-hidden
            >
              <circle cx="8.5" cy="8.5" r="5" />
              <path d="m12.2 12.2 4 4" strokeLinecap="round" />
            </svg>
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search"
              aria-label="Search bots"
              spellCheck={false}
            />
          </label>
        </div>

        <div className="bots-group">
          <span>Bots</span>
          <span>{bots.length}</span>
        </div>

        <div className="bots-list">
          {bots.map(([name, agent]) => {
            const owned = processes.find((process) => sameAgent(name, process.name));
            return (
              <button
                key={name}
                className={`bots-row${focus === name ? " on" : ""}`}
                onClick={() => setFocus(focus === name ? null : name)}
                onContextMenu={(event) => openAgentMenu(event, name, agent)}
                title={`${agent.label || name} · ${statusOf(agent, owned)}`}
              >
                <span
                  className="bots-avatar"
                  style={{ background: agent.color || undefined, color: readableOn(agent.color) }}
                  aria-hidden
                >
                  {initial(agent.label || name)}
                </span>
                <span className="bots-row-text">
                  <span className="bots-row-name">
                    {agent.label || name}
                    {agent.busy ? <span className="bots-typing" aria-label="working" /> : null}
                  </span>
                  <span className="bots-row-sub">{statusOf(agent, owned)}</span>
                </span>
              </button>
            );
          })}

          {startable.map((harness) => (
            <button
              key={harness.name}
              className="bots-row"
              onClick={() => void startOne(harness.name)}
              disabled={busyAgent !== null}
              title={`Start ${harness.label} on a hidden console, without starting the others.`}
            >
              <span className="bots-avatar ghost" aria-hidden>
                {initial(harness.label)}
              </span>
              <span className="bots-row-text">
                <span className="bots-row-name">{harness.label}</span>
                <span className="bots-row-sub">
                  {busyAgent === harness.name ? "starting…" : "not running · start"}
                </span>
              </span>
            </button>
          ))}

          {blocked.map((harness) => (
            <button
              key={harness.name}
              className="bots-row blocked"
              onClick={() => onNotice(explainHarness(harness))}
              title={explainHarness(harness)}
            >
              <span className="bots-avatar ghost" aria-hidden>
                {initial(harness.label)}
              </span>
              <span className="bots-row-text">
                <span className="bots-row-name">{harness.label}</span>
                <span className="bots-row-sub">
                  {statusLabel(harness)}
                  {harness.remediation ? ` · ${harness.remediation}` : ""}
                </span>
              </span>
            </button>
          ))}

          {bots.length === 0 && startable.length === 0 && blocked.length === 0 ? (
            <p className="bots-none">
              {needle ? "No bot matches that search." : "No bot is configured yet."}
            </p>
          ) : null}
        </div>

        <div className="bots-foot">
          <div className="bots-group foot">
            <span>Channels</span>
            <span>{channels.length}</span>
          </div>
          <div className="bots-channels">
            {channels.map((name) => (
              <button
                key={name}
                className={`bots-chip${!focus && channel === name ? " on" : ""}`}
                onClick={() => {
                  setFocus(null);
                  setChannel(name);
                }}
                onContextMenu={(event) =>
                  contextMenu.open(event, {
                    eyebrow: "Chat channel",
                    title: `# ${name}`,
                    subtitle: focus ? "open it to see every bot" : "the open channel",
                    sections: [
                      {
                        label: "Channel",
                        items: [
                          {
                            label: focus ? "Open the room" : "Current channel",
                            icon: "chat",
                            disabled: !focus && channel === name,
                            onSelect: () => {
                              setFocus(null);
                              setChannel(name);
                            },
                          },
                          {
                            label: "Copy channel name",
                            icon: "copy",
                            onSelect: () => void copyText(`#${name}`, "Channel name"),
                          },
                        ],
                      },
                    ],
                  })
                }
              >
                # {name}
              </button>
            ))}
          </div>
          <button
            className="bots-start"
            onClick={() => void startAgents()}
            disabled={starting}
            title="Starts every configured bot on a hidden console. They run unattended — no windows, no terminals."
          >
            <span aria-hidden>▶</span>
            {starting ? "Starting…" : "Start all bots"}
          </button>
        </div>
      </aside>

      <main id="main" tabIndex={-1} className="detail chat-main bots-main">
        <header className="bots-thread-head">
          <span
            className={`bots-avatar${focus ? "" : " ghost"}`}
            style={
              focused?.color
                ? { background: focused.color, color: readableOn(focused.color) }
                : undefined
            }
            aria-hidden
          >
            {focus ? initial(focusedLabel) : "#"}
          </span>
          <div className="bots-head-text">
            <h2>{title}</h2>
            <p>{subtitle}</p>
          </div>
          <div className="bots-head-actions">
            <button
              className="bots-pill"
              onClick={(event) =>
                contextMenu.open(event, {
                  eyebrow: "Conversation",
                  title: `# ${channel}`,
                  subtitle: focus ? `narrowed to ${focusedLabel}` : "every bot in the room",
                  sections: [
                    {
                      label: "Channels",
                      items: channels.map((name) => ({
                        label: `# ${name}`,
                        icon: "chat",
                        disabled: !focus && name === channel,
                        onSelect: () => {
                          setFocus(null);
                          setChannel(name);
                        },
                      })),
                    },
                  ],
                  footer: "Pick a bot on the left to talk to it alone",
                })
              }
              title="Switch channel — picking one widens the thread back to the whole room"
            >
              # {channel} <span aria-hidden>▾</span>
            </button>
            <button
              className="bots-more"
              aria-label="Conversation actions"
              title="Conversation actions"
              onClick={(event) =>
                focus
                  ? openAgentMenu(event, focus, focused)
                  : contextMenu.open(event, {
                      eyebrow: "The room",
                      title: `# ${channel}`,
                      subtitle: `${online} online · ${messages.length} message${messages.length === 1 ? "" : "s"}`,
                      sections: [
                        {
                          label: "Room",
                          items: [
                            {
                              label: "Copy channel name",
                              icon: "copy",
                              onSelect: () => void copyText(`#${channel}`, "Channel name"),
                            },
                            {
                              label: starting ? "Starting bots…" : "Start every configured bot",
                              icon: "open",
                              disabled: starting,
                              onSelect: () => void startAgents(),
                            },
                          ],
                        },
                      ],
                    })
              }
            >
              ⋯
            </button>
          </div>
        </header>

        {notice ? (
          <p className="notice bots-notice" role="status">
            {notice}
          </p>
        ) : null}

        <div className="bots-stream" ref={streamRef} aria-live="polite">
          {thread.length === 0 ? (
            <p className="bots-empty">
              {focus
                ? `Nothing between you and ${focusedLabel} yet. Say something — the room sees the mention.`
                : "Nothing here yet. Say something."}
            </p>
          ) : null}

          {thread.map((message, index) => {
            const previous = thread[index - 1];
            const mine = message.sender === "user";
            const system = message.kind !== "chat";
            const gap = previous ? gapBefore(previous, message) : Number.POSITIVE_INFINITY;
            const divider = gap > 30 * 60;
            const fresh = system || !previous || previous.sender !== message.sender || gap > 5 * 60;
            const sender = agents[message.sender];
            return (
              <div key={message.id} className="bots-turn">
                {divider ? (
                  <div className="bots-time">
                    <span>
                      {previous ? friendlyTime(message.time) : `Today ${friendlyTime(message.time)}`}
                    </span>
                  </div>
                ) : null}
                <article
                  className={`bots-msg ${system ? "sys" : mine ? "mine" : "theirs"}`}
                  title={`${message.sender} · ${message.time}`}
                  onContextMenu={(event) =>
                    contextMenu.open(event, {
                      eyebrow: "Chat message",
                      title: message.sender,
                      subtitle: `${message.time} · #${channel}`,
                      sections: [
                        {
                          label: "Message",
                          items: [
                            {
                              label: "Copy text",
                              icon: "copy",
                              onSelect: () => void copyText(message.text, "Message"),
                            },
                            {
                              label: "Copy as quote",
                              icon: "quote",
                              onSelect: () =>
                                void copyText(`> ${message.sender}: ${message.text}`, "Quote"),
                            },
                            {
                              label: "Reply in composer",
                              detail: `Mention @${message.sender}`,
                              icon: "chat",
                              disabled: system || mine,
                              onSelect: () => setDraft((current) => `@${message.sender} ${current}`),
                            },
                            {
                              label: "Open this bot's thread",
                              detail: system || mine ? undefined : "Narrow the room to it",
                              icon: "open",
                              disabled: system || mine,
                              onSelect: () => setFocus(message.sender),
                            },
                            {
                              label: "Copy message id",
                              detail: `#${message.id}`,
                              icon: "copy",
                              onSelect: () => void copyText(String(message.id), "Message id"),
                            },
                          ],
                        },
                      ],
                      footer: `#${channel} · ${thread.length} message${thread.length === 1 ? "" : "s"}`,
                    })
                  }
                >
                  {!system && !mine && fresh ? (
                    <span
                      className="bots-avatar sm"
                      style={
                        sender?.color
                          ? { background: sender.color, color: readableOn(sender.color) }
                          : undefined
                      }
                      aria-hidden
                    >
                      {initial(ownerLabel(message.sender))}
                    </span>
                  ) : null}
                  <div className="bots-bubble">
                    {!system && !mine && fresh && !focus ? (
                      <span className="bots-who">{ownerLabel(message.sender)}</span>
                    ) : null}
                    <p>{message.text}</p>
                    {message.attachments?.length ? (
                      <BubbleFiles files={message.attachments} />
                    ) : null}
                  </div>
                </article>
              </div>
            );
          })}
        </div>

        {review ? (
          <RefineCard
            key={`${channel}:${review}`}
            draft={review}
            target={mentionTarget(review)}
            channel={channel}
            recent={messages
              .filter((message) => message.kind === "chat")
              .slice(-6)
              .map((message) => `${message.sender}: ${message.text.slice(0, 400)}`)}
            project={project}
            onSend={(text) => {
              setReview(null);
              void deliver(text);
            }}
            onDismiss={() => setReview(null)}
          />
        ) : null}

        <div className="bots-composer">
          <button
            className="bots-insert"
            aria-label={focus ? `Mention ${focusedLabel}` : "Insert a mention"}
            title={focus ? `Mention ${focusedLabel}` : "Insert @ to address a bot"}
            onClick={() =>
              setDraft((current) =>
                focus
                  ? mentions(current, focus)
                    ? current
                    : `@${focus} ${current}`
                  : current.includes("@")
                    ? current
                    : `@${current}`,
              )
            }
          >
            @
          </button>
          <input
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                void send();
              }
            }}
            placeholder={
              review
                ? "Review the brief above before it sends…"
                : focus
                  ? `Message ${focusedLabel}`
                  : `Message #${channel} — @claude, @codex…`
            }
            aria-label={focus ? `Message ${focusedLabel}` : `Message ${channel}`}
            spellCheck={false}
            disabled={review !== null}
          />
          <button
            className="bots-send"
            onClick={() => void send()}
            disabled={sending || !draft.trim() || review !== null}
            aria-label="Send"
            title="Send — reviews first"
          >
            ↑
          </button>
        </div>
      </main>
    </>
  );
}

function isStartable(project: string): boolean {
  return project.trim().length > 0;
}

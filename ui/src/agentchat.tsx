/**
 * The Chat tab (P6): agentchattr, fronted by the core.
 *
 * Like every tab it owns its two grid panes — the roster sits in the list pane, the
 * stream and its composer in the detail pane. All state flows through `api.agentChat`
 * (Rust invoke), so the renderer never touches the chat server or its session token.
 * A room that is down is a normal state, not an error: boot is a button, exactly like
 * OmniRoute's autostart on the Session tab.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import RefineCard from "./refine";
import {
  api,
  type AgentChatAgent,
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

function senderClass(sender: string): string {
  if (sender === "user") return "me";
  if (sender === "system") return "sys";
  return "agent";
}

/** Who the draft is addressed to, for the refiner's prompt. A bare mention is enough here. */
function mentionTarget(text: string): string {
  const found = text.match(/@([a-z0-9_-]+)/i);
  return found ? `@${found[1]}` : "the room";
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
            setMessages((current) =>
              lastIdRef.current === 0 ? fresh : [...current, ...fresh],
            );
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
  }, [messages.length]);

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

  /** Send now reviews first: the room sees only what the user confirmed in the card. */
  const send = useCallback(() => {
    const text = draft.trim();
    if (!text || sending || review) return;
    setReview(text);
  }, [draft, sending, review]);

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
  // agents get a pill.
  const presence = Object.entries(agents).filter(
    ([, agent]) => typeof agent?.available === "boolean",
  );

  return (
    <>
      <aside className="list-pane roster">
        <div className="list-head">
          <div className="list-title">
            <h1>Chat</h1>
            <span>{presence.filter(([, agent]) => agent.available).length} online</span>
          </div>
        </div>

        <div className="chat-channels" role="tablist" aria-label="Channels">
          {channels.map((name) => (
            <button
              key={name}
              role="tab"
              aria-selected={channel === name}
              className={`row chat-channel${channel === name ? " on" : ""}`}
              onClick={() => setChannel(name)}
            >
              <span className="row-name"># {name}</span>
            </button>
          ))}
        </div>

        <section className="chat-roster" aria-label="Agents">
          {presence.map(([name, agent]) => {
            const owned = processes.find((process) => sameAgent(name, process.name));
            return (
              <div
                className="chat-agent"
                key={name}
                title={owned ? `${agent.role ?? name} · pid ${owned.pid}` : (agent.role ?? name)}
              >
                <span
                  className={`chat-dot ${agent.available ? (agent.busy ? "busy" : "up") : "down"}`}
                  aria-hidden
                >
                  ●
                </span>
                <span className="row-name">
                  {agent.label || name}
                  {agent.role ? <span className="tag">{agent.role}</span> : null}
                </span>
                {owned ? (
                  <button
                    className="chat-agent-control"
                    onClick={() => void stopOne(owned.name)}
                    disabled={busyAgent !== null}
                    title={`Rigel started this agent (pid ${owned.pid}); Stop kills the wrapper, its CLI and their children.`}
                  >
                    {busyAgent === owned.name ? "Stopping…" : "Stop"}
                  </button>
                ) : null}
              </div>
            );
          })}

          {/* Configured but not in the room: start one without starting them all. */}
          {harnesses
            .filter(
              (harness) =>
                harness.ready && !presence.some(([name]) => sameAgent(name, harness.name)),
            )
            .map((harness) => (
              <button
                key={harness.name}
                className="chat-agent-control start"
                onClick={() => void startOne(harness.name)}
                disabled={busyAgent !== null}
                title={`Start ${harness.label} on a hidden console, without starting the others.`}
              >
                {busyAgent === harness.name ? "Starting…" : `Start ${harness.label}`}
              </button>
            ))}

          <button
            className="chat-start-agents"
            onClick={() => void startAgents()}
            disabled={starting}
            title="Starts every configured agent on a hidden console. They run unattended in the background — no windows, no terminals."
          >
            {starting ? "Starting…" : "Start agents"}
          </button>
        </section>
      </aside>

      <main id="main" tabIndex={-1} className="detail chat-main">
        <header className="detail-head">
          <div>
            <p className="eyebrow">{channel === "general" ? "the room" : channel}</p>
            <h2># {channel}</h2>
            <p className="lede">
              @mention an agent to hand it work; it wakes through its trigger queue and replies
              here as a full turn of its own.
            </p>
          </div>
        </header>

        {notice ? <p className="notice" role="status">{notice}</p> : null}

        <div className="chat-stream" ref={streamRef} aria-live="polite">
          {messages.length === 0 ? <p className="empty">Nothing here yet. Say something.</p> : null}
          {messages.map((message) => (
            <article key={message.id} className={`chat-msg ${senderClass(message.sender)}`}>
              <header>
                <span className="who">{message.sender}</span>
                <span className="at">{message.time}</span>
              </header>
              <p>{message.text}</p>
            </article>
          ))}
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

        <div className="chat-composer">
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
              review ? "Review the brief above before it sends…" : `Message #${channel} — @claude, @codex…`
            }
            aria-label={`Message ${channel}`}
            spellCheck={false}
            disabled={review !== null}
          />
          <button
            className="primary"
            onClick={() => void send()}
            disabled={sending || !draft.trim() || review !== null}
          >
            Send
          </button>
        </div>
      </main>
    </>
  );
}

function isStartable(project: string): boolean {
  return project.trim().length > 0;
}

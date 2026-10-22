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

import { api, type AgentChatAgent, type AgentChatMessage } from "./api";

type Status = Awaited<ReturnType<typeof api.agentChat.status>>;

const POLL_MS = 2_000;

function senderClass(sender: string): string {
  if (sender === "user") return "me";
  if (sender === "system") return "sys";
  return "agent";
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
  const [agents, setAgents] = useState<Record<string, AgentChatAgent>>({});
  const [channels, setChannels] = useState<string[]>(["general"]);
  const [channel, setChannel] = useState("general");
  const [messages, setMessages] = useState<AgentChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
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
          const [room, presence] = await Promise.all([
            api.agentChat.channels(),
            api.agentChat.agents(),
          ]);
          if (!live) return;
          setChannels(room);
          setAgents(presence);
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
          : "chat is running, but it was started outside Rigel — restart it from here to get a session",
      );
    } catch (problem) {
      onNotice(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBooting(false);
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

  const send = useCallback(async () => {
    const text = draft.trim();
    if (!text || sending) return;
    setSending(true);
    try {
      await api.agentChat.send(channel, text);
      setDraft("");
    } catch (problem) {
      onNotice(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setSending(false);
    }
  }, [draft, sending, channel, onNotice]);

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
        <main className="detail chat-main">
          {notice ? <p className="notice" role="status">{notice}</p> : null}
          <section className="chat-empty">
            <h3>The room is not running</h3>
            <p>
              Rigel starts agentchattr itself — the same way it boots OmniRoute — and holds the
              session token so this window never sees it. The checkout is found next to the open
              project (vendor/agentchattr) or via RIGEL_AGENTCHAT_DIR.
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
        <main className="detail chat-main">
          {notice ? <p className="notice" role="status">{notice}</p> : null}
          <section className="chat-empty">
            <h3>Another window owns this room</h3>
            <p>
              Something is listening at {status.origin} ({status.dir ?? "checkout unknown"}
              ), but it was started outside Rigel, so Rigel holds no session token for it.
              Close it and restart from here to take control.
            </p>
            <button className="primary" onClick={() => void start()} disabled={booting}>
              {booting ? "Booting…" : "Restart from Rigel"}
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
          {presence.map(([name, agent]) => (
            <div className="chat-agent" key={name} title={agent.role ?? name}>
              <span
                className={`chat-dot ${agent.available ? (agent.busy ? "busy" : "up") : "down"}`}
                style={{ color: agent.color }}
                aria-hidden
              >
                ●
              </span>
              <span className="row-name">
                {agent.label || name}
                {agent.role ? <span className="tag">{agent.role}</span> : null}
              </span>
            </div>
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

      <main className="detail chat-main">
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
            placeholder={`Message #${channel} — @claude, @codex…`}
            aria-label={`Message ${channel}`}
            spellCheck={false}
          />
          <button className="primary" onClick={() => void send()} disabled={sending || !draft.trim()}>
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

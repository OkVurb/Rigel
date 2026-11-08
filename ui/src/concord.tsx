/**
 * Concord, the shared agent room's mail, as the window sees it.
 *
 * The API server owns the one long-lived Concord session and writes every collected message to
 * its cache. This page reads that cache, shows the receipt Concord attaches to each message, and
 * posts a reply through the API's `update_work` wrapper. Nothing here runs `npx`, opens a second
 * Concord participant, or builds a tool call.
 */

import { useCallback, useEffect, useMemo, useState } from "react";

import { api, type ConcordInbox, type ConcordMessage } from "./api";

interface Props {
  onNotice: (notice: string) => void;
}

function messageId(message: ConcordMessage): string {
  return message.messageId || message.id || message.message_id || "";
}

function stamp(value?: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function repliedAt(message: ConcordMessage): string | null {
  return message.repliedAt || message.receipt?.replied_at || null;
}

function statusOf(message: ConcordMessage): string {
  const status = message.receipt?.status || message.status;
  if (status) return status;
  if (repliedAt(message)) return "replied";
  if (message.readAt) return "read";
  if (message.deliveredAt) return "delivered";
  return "pending";
}

/** Pending means no reply has been recorded yet; Concord has already marked it read. */
function awaitingReply(message: ConcordMessage): boolean {
  const status = statusOf(message).toLowerCase();
  return (
    !repliedAt(message) &&
    status !== "replied" &&
    status !== "failed" &&
    status !== "expired" &&
    status !== "cancelled"
  );
}

export default function ConcordPage({ onNotice }: Props) {
  const [inbox, setInbox] = useState<ConcordInbox | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  // The poller may not see the reply receipt immediately. Keep the successful send marked
  // locally so the form does not reappear under a message the API already accepted.
  const [repliedHere, setRepliedHere] = useState<Record<string, string>>({});
  const [sending, setSending] = useState<string | null>(null);

  const load = useCallback(async (quiet = false) => {
    if (!quiet) setLoading(true);
    try {
      setInbox(await api.concordInbox());
      setError(null);
    } catch (problem) {
      if (!quiet) setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      if (!quiet) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(true), 5_000);
    return () => window.clearInterval(timer);
  }, [load]);

  const rows = useMemo(
    () => [...(inbox?.messages ?? [])].sort((left, right) => right.seq - left.seq),
    [inbox],
  );
  const awaiting = (message: ConcordMessage): boolean => {
    const id = messageId(message);
    return awaitingReply(message) && !(id && repliedHere[id]);
  };
  const pendingCount = rows.filter(({ message }) => awaiting(message)).length;
  const shown = all ? rows : rows.filter(({ message }) => awaiting(message));

  const send = useCallback(
    async (message: ConcordMessage) => {
      const id = messageId(message);
      if (!id) return;
      const content = (drafts[id] ?? "").trim();
      if (!content) return;
      setSending(id);
      setActionError(null);
      try {
        await api.concordReply(id, content);
        setDrafts((current) => ({ ...current, [id]: "" }));
        setRepliedHere((current) => ({ ...current, [id]: new Date().toISOString() }));
        onNotice(`Reply sent to @${message.senderAgentId || "the sender"}.`);
        await load(true);
      } catch (problem) {
        setActionError(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setSending(null);
      }
    },
    [drafts, load, onNotice],
  );

  return (
    <>
      <section className="settings-section">
        <div className="settings-head">
          <h3>Muse&apos;s Concord inbox</h3>
          <span className="pill">{pendingCount} awaiting reply</span>
        </div>
        <p className="fine">
          The API poller collects these as <code>muse</code>. A message stays on this page after
          Concord marks it read; it leaves the awaiting list only when a reply receipt lands.
        </p>
        <div className="concord-toolbar">
          <div className="segmented" role="group" aria-label="Concord inbox filter">
            <button type="button" className={!all ? "on" : ""} onClick={() => setAll(false)}>
              Awaiting reply
            </button>
            <button type="button" className={all ? "on" : ""} onClick={() => setAll(true)}>
              All cached
            </button>
          </div>
          <button className="chip" type="button" onClick={() => void load()} disabled={loading}>
            {loading ? "Refreshing…" : "Refresh"}
          </button>
          <span className="fine">
            {inbox
              ? `${inbox.retained} cached · checked ${
                  inbox.checked_at ? stamp(new Date(inbox.checked_at).toISOString()) : "…"
                }`
              : "Reading the API cache…"}
          </span>
        </div>
        {error ? (
          <p className="notice" role="status">
            {error}
          </p>
        ) : null}
        {inbox?.error ? (
          <p className="notice" role="status">
            Poller: {inbox.error}
          </p>
        ) : null}
        {actionError ? (
          <p className="notice" role="status">
            {actionError}
          </p>
        ) : null}
        {loading && !inbox ? <p className="empty subtle">Reading the inbox…</p> : null}
        {!loading && shown.length === 0 ? (
          <p className="empty subtle">
            {all ? "No messages are cached yet." : "Nothing is waiting for a reply."}
          </p>
        ) : null}
        <div className="concord-list">
          {shown.map(({ seq, message }) => {
            const id = messageId(message);
            const replied = repliedAt(message) || (id ? repliedHere[id] ?? null : null);
            const receipt = message.receipt;
            const status = statusOf(message);
            const answered = Boolean(replied) || status.toLowerCase() === "replied";
            const draft = id ? drafts[id] ?? "" : "";
            return (
              <article className="concord-card" key={`${seq}:${id || "unnamed"}`}>
                <div className="concord-card-head">
                  <div>
                    <strong>@{message.senderAgentId || "unknown"}</strong>
                    <span className="fine"> → @{message.recipientAgentId || "muse"}</span>
                  </div>
                  <span className="pill">{status}</span>
                </div>
                <p className="concord-body">{message.content || "(no content)"}</p>
                <div className="concord-meta">
                  <span>seq {seq}</span>
                  <span>{stamp(message.createdAt)}</span>
                  {message.deliveryMode ? <span>{message.deliveryMode}</span> : null}
                  {message.taskId ? <span>task {message.taskId.slice(0, 12)}</span> : null}
                  {message.replyToMessageId ? (
                    <span>reply to {message.replyToMessageId.slice(0, 12)}</span>
                  ) : null}
                </div>
                <dl className="concord-receipt">
                  <div>
                    <dt>receipt</dt>
                    <dd>{receipt?.status || status}</dd>
                  </div>
                  <div>
                    <dt>sent</dt>
                    <dd>{stamp(receipt?.sent_at || message.createdAt)}</dd>
                  </div>
                  <div>
                    <dt>delivered</dt>
                    <dd>{stamp(receipt?.delivered_at || message.deliveredAt)}</dd>
                  </div>
                  <div>
                    <dt>read</dt>
                    <dd>{stamp(receipt?.read_at || message.readAt)}</dd>
                  </div>
                  <div>
                    <dt>replied</dt>
                    <dd>{stamp(receipt?.replied_at || message.repliedAt)}</dd>
                  </div>
                  <div>
                    <dt>expires</dt>
                    <dd>{stamp(receipt?.expires_at || message.expiresAt)}</dd>
                  </div>
                  {receipt?.ticks ? (
                    <div>
                      <dt>ticks</dt>
                      <dd>{receipt.ticks}</dd>
                    </div>
                  ) : null}
                  {message.errorCode || receipt?.error_code ? (
                    <div>
                      <dt>error</dt>
                      <dd>{message.errorCode || receipt?.error_code}</dd>
                    </div>
                  ) : null}
                </dl>
                {message.errorDetail ? <p className="notice">{message.errorDetail}</p> : null}
                {answered ? (
                  <p className="fine">
                    {replied ? `Replied ${stamp(replied)}.` : "Replied — the receipt has no timestamp yet."}
                  </p>
                ) : !id ? (
                  <p className="fine">
                    This cached row has no message id, so the reply route cannot address it.
                  </p>
                ) : (
                  <div className="concord-reply">
                    <textarea
                      value={draft}
                      rows={2}
                      maxLength={16_384}
                      placeholder={`Reply to @${message.senderAgentId || "the sender"} as Muse…`}
                      aria-label={`Reply to ${message.senderAgentId || "the sender"}`}
                      disabled={sending === id}
                      onChange={(event) =>
                        setDrafts((current) => ({ ...current, [id]: event.target.value }))
                      }
                    />
                    <div className="concord-reply-actions">
                      <span className="fine">{draft.length}/16384</span>
                      <button
                        className="primary"
                        type="button"
                        disabled={sending === id || draft.trim().length === 0}
                        onClick={() => void send(message)}
                      >
                        {sending === id ? "Sending…" : "Reply as Muse"}
                      </button>
                    </div>
                  </div>
                )}
              </article>
            );
          })}
        </div>
      </section>
      <section className="settings-section">
        <h3>What the reply does</h3>
        <p className="fine">
          The window calls <code>POST /v1/concord/reply</code> on the Rigel API. The API fills in{" "}
          <code>operation: &quot;reply&quot;</code>, the cached sender, and an idempotency key,
          then calls Concord&apos;s <code>update_work</code>. Muse never has to hand-build the tool
          call, and a retry of the same reply cannot deliver twice.
        </p>
        <p className="fine">
          Receipts come from Concord itself: <code>sent_at</code>, <code>delivered_at</code>,{" "}
          <code>read_at</code>, <code>replied_at</code> and <code>expires_at</code>. A poller error
          is shown above the list without discarding the messages already cached.
        </p>
      </section>
    </>
  );
}

/**
 * D154: unresolved work, grouped by session.
 *
 * The view owns no list of its own: `attention_list` reads the ledger and the open approval
 * bus, and a row disappears when the thing it names is resolved in the surface that owns it —
 * a retry, a release, a merge, an answer. The rail count is the same read, polled by App.
 *
 * Approvals lead the list in one block. A `PendingApproval` carries the session, agent and turn
 * the asking surface recorded, so its row opens that turn; an older question with no origin
 * still opens the Session card.
 */

import { useCallback, useEffect, useState } from "react";

import { api, when, type Attention } from "./api";

export interface AttentionJump {
  sessionId: string;
  turnId?: string;
  patchId?: string;
  surface: "session" | "diff";
}

const KIND_LABEL: Record<string, string> = {
  failed_turn: "failed",
  budget_stop: "cap",
  stale_claim: "claim",
  merge_review: "review",
  blocked_merge: "blocked",
  approval: "approval",
};

export default function AttentionView({
  project,
  onJump,
  onOpenSession,
}: {
  project: string | null;
  onJump: (target: AttentionJump) => void;
  onOpenSession: () => void;
}) {
  const [view, setView] = useState<Attention | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    if (!project) {
      setView(null);
      setLoading(false);
      return;
    }
    try {
      setView(await api.attention(project));
      setError(null);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setLoading(false);
    }
  }, [project]);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(), 5000);
    return () => window.clearInterval(timer);
  }, [load]);

  const open = (item: Attention["groups"][number]["items"][number]) => {
    if (!item.session_id) {
      onOpenSession();
      return;
    }
    const diff = item.kind === "stale_claim" || item.kind.includes("merge");
    onJump({
      sessionId: item.session_id,
      turnId: item.turn_id ?? undefined,
      patchId: item.patch_id ?? undefined,
      surface: diff ? "diff" : "session",
    });
  };

  return (
    <main id="main" tabIndex={-1} className="detail attention-view">
      <header className="detail-head">
        <div>
          <p className="eyebrow">Attention</p>
          <h2>{view ? `${view.total} unresolved` : "Unresolved work"}</h2>
          <p className="lede">
            Only what still needs an answer. A retry, a release, a merge or an answer clears the
            row; ending a session does not. Archived sessions stay listed.
          </p>
        </div>
        <button className="chip" onClick={() => void load()} title="Read the ledger again">
          Refresh
        </button>
      </header>

      {error ? (
        <p className="notice" role="status">
          {error}
        </p>
      ) : null}

      {loading ? (
        <p className="empty">Reading the ledger…</p>
      ) : !view || view.groups.length === 0 ? (
        <section className="onboarding">
          <h3>Nothing waiting</h3>
          <p>
            No open approval, failed turn, budget stop, stale claim, unmerged review or blocked
            merge in this project.
          </p>
        </section>
      ) : (
        <div className="attention-groups">
          {view.groups.map((group) => (
            <section
              className="attention-group"
              key={group.session_id || "approvals"}
              aria-label={group.title}
            >
              <header>
                <h3>{group.title}</h3>
                {group.archived ? <span className="pill archived">archived</span> : null}
                {group.session_id ? (
                  <span className={`pill ${group.status}`}>{group.status}</span>
                ) : (
                  <span className="tag">not tied to a session</span>
                )}
                <span className="grow" />
                {group.session_id ? (
                  <button
                    className="link"
                    onClick={() => onJump({ sessionId: group.session_id, surface: "session" })}
                  >
                    open session
                  </button>
                ) : (
                  <button className="link" onClick={onOpenSession}>
                    open the card
                  </button>
                )}
              </header>
              <ul>
                {group.items.map((item, index) => (
                  <li key={`${item.kind}-${item.at}-${index}`}>
                    <button
                      className="attention-row"
                      onClick={() => open(item)}
                      title={
                        item.kind === "approval"
                          ? item.turn_id
                            ? "Open the turn that asked"
                            : item.session_id
                              ? "Open the session that asked"
                              : "Answer it on the Session card"
                          : item.turn_id
                            ? "Open the session at this turn"
                            : "Open Diff, where the row is resolved"
                      }
                    >
                      <span className={`attention-kind ${item.kind}`}>
                        {KIND_LABEL[item.kind] ?? item.kind}
                      </span>
                      <span className="attention-label">{item.label}</span>
                      <span className="attention-detail">{item.detail}</span>
                      <span className="attention-when">{when(item.at)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </main>
  );
}

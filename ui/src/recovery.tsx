/**
 * Crash recovery (roadmap 37).
 *
 * The offer names the exact turns a dead process left running. Resume closes them through the
 * normal resume path; read-only keeps the transcript visible while disabling the composer; and
 * discard writes a durable tombstone before the turn is closed.
 */
import type { RecoveryOffer } from "./api";

export function Recovery({
  offer,
  busy,
  readOnly,
  onResume,
  onReadOnly,
  onDiscard,
}: {
  offer: RecoveryOffer;
  busy: boolean;
  readOnly: boolean;
  onResume: () => void;
  onReadOnly: () => void;
  onDiscard: () => void;
}) {
  const turns = offer.turns;
  return (
    <section className="recovery-offer" role="region" aria-label="Crash recovery">
      <div className="recovery-head">
        <span className="recovery-kicker">recovered session</span>
        <strong>
          {turns.length} turn{turns.length === 1 ? "" : "s"} left running
        </strong>
      </div>
      <p>
        The process ended while {turns.length === 1 ? "this turn was" : "these turns were"} still
        open. The transcript and bill are intact.
      </p>
      <ul className="recovery-turns">
        {turns.map((turn) => (
          <li key={turn.id}>
            <span className="mono">turn {turn.seq}</span>
            <span className="mono recovery-id">{turn.id}</span>
            {turn.prompt ? <span className="recovery-prompt">{turn.prompt}</span> : null}
          </li>
        ))}
      </ul>
      {readOnly ? (
        <p className="recovery-readonly" role="status">
          Read-only view. Sending is disabled; the transcript remains available.
        </p>
      ) : (
        <div className="recovery-actions">
          <button className="primary" onClick={onResume} disabled={busy}>
            Resume the turn
          </button>
          <button className="chip" onClick={onReadOnly} disabled={busy}>
            Read-only view
          </button>
          <button className="chip danger" onClick={onDiscard} disabled={busy}>
            Discard and tombstone
          </button>
        </div>
      )}
    </section>
  );
}

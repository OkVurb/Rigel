/**
 * The review card between a draft and the agent.
 *
 * Opening it runs one cheap refinement automatically: questions the user can answer, and a
 * proposed brief they can edit. Nothing is sent from here — `onSend` is called only by the
 * user's Send buttons, and `onSend(draft)` remains the escape hatch. The card shows what the
 * refinement itself cost; the point is fewer wrong turns, not a free rewrite.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { api, compact, type PromptRefinement } from "./api";

const money = (micros: number): string => `$${(micros / 1_000_000).toFixed(6)}`;
/** A rough size, for comparing drafts; the provider's count is the one that bills. */
const estimate = (text: string): number => Math.ceil(text.trim().length / 4);

export default function RefineCard({
  draft,
  target,
  channel,
  recent,
  project,
  onSend,
  onDismiss,
}: {
  draft: string;
  target: string;
  channel?: string | null;
  recent: string[];
  project: string;
  onSend: (text: string) => void;
  onDismiss: () => void;
}) {
  const [result, setResult] = useState<PromptRefinement | null>(null);
  const [edited, setEdited] = useState(draft);
  const [answers, setAnswers] = useState<string[]>([]);
  const [round, setRound] = useState(0);
  const [working, setWorking] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // StrictMode mounts effects twice in development; the first request must not be doubled.
  const started = useRef(false);

  const run = useCallback(
    async (nextRound: number, nextAnswers: string[]) => {
      setWorking(true);
      setError(null);
      try {
        const next = await api.refine.run(project || null, {
          draft,
          target,
          channel: channel ?? null,
          recent,
          answers: nextAnswers,
          round: nextRound,
        });
        setResult(next);
        setEdited(next.refined);
        setRound(nextRound);
        // The previous round's answers have been used; the new questions start blank.
        setAnswers([]);
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
      } finally {
        setWorking(false);
      }
    },
    [draft, target, channel, recent, project],
  );

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void run(0, []);
  }, [run]);

  const filled = answers.map((answer) => answer.trim()).filter(Boolean);
  const usage = result?.usage;
  const tokens = usage ? usage.input_tokens + usage.output_tokens : 0;

  const again = () => {
    if (filled.length === 0 || round >= 2) return;
    void run(round + 1, filled);
  };

  return (
    <section className="refine-card" aria-label="Prompt refinement" aria-busy={working}>
      <header className="refine-head">
        <div>
          <span className="eyebrow">Before it sends</span>
          <strong>{target || "the agent"}</strong>
        </div>
        {usage ? (
          <span className="fine mono">
            {result?.model} · {compact(tokens)} tok · {money(usage.cost_micros)}
            {usage.source === "estimated" ? " est." : ""}
          </span>
        ) : (
          <span className="fine">{working ? "refining…" : ""}</span>
        )}
      </header>

      {error ? (
        <div className="refine-error" role="alert">
          <p>{error}</p>
          <p className="fine">Nothing was sent. The draft is unchanged.</p>
          <div className="refine-actions">
            <button className="chip" onClick={() => void run(round, answers)} disabled={working}>
              Try again
            </button>
            <button className="primary" onClick={() => onSend(draft)}>
              Send as-is
            </button>
            <button className="ghost-btn" onClick={onDismiss}>
              Cancel
            </button>
          </div>
        </div>
      ) : !result ? (
        <p className="fine" role="status">
          Asking the cheap model what is missing from this brief…
        </p>
      ) : (
        <>
          {result.questions.length > 0 ? (
            <>
              <p className="fine">
                Answer what matters, then refine again. Empty answers are ignored.
              </p>
              <ol className="refine-questions">
                {result.questions.map((question, index) => (
                  <li key={`${index}-${question}`}>
                    <span className="mono">{index + 1}</span>
                    <label>
                      <span>{question}</span>
                      <input
                        type="text"
                        value={answers[index] ?? ""}
                        onChange={(event) => {
                          const value = event.target.value;
                          setAnswers((current) => {
                            const next = [...current];
                            next[index] = value;
                            return next;
                          });
                        }}
                        placeholder="Your answer (optional)"
                        aria-label={`Answer ${index + 1}: ${question}`}
                        disabled={working}
                      />
                    </label>
                  </li>
                ))}
              </ol>
            </>
          ) : (
            <p className="fine">No blocking questions. Edit the brief or send it.</p>
          )}

          <textarea
            value={edited}
            onChange={(event) => setEdited(event.target.value)}
            aria-label="Refined prompt"
            rows={Math.min(8, Math.max(3, edited.split("\n").length + 1))}
            spellCheck={false}
            disabled={working}
          />

          <div className="refine-foot">
            <span className="fine mono" title="Rough size; the provider's token count is the bill">
              ≈ {compact(estimate(draft))} → {compact(estimate(edited))} tok · refinement billed
              separately
            </span>
            <div className="refine-actions">
              {result.questions.length > 0 && round < 2 ? (
                <button
                  className="chip"
                  onClick={again}
                  disabled={working || filled.length === 0}
                  title="Ask again with your answers; one more round is allowed"
                >
                  Refine again
                </button>
              ) : null}
              <button className="chip" onClick={() => onSend(draft)} disabled={working}>
                Send original
              </button>
              <button
                className="primary"
                onClick={() => onSend(edited.trim())}
                disabled={working || !edited.trim()}
              >
                {working ? "Refining…" : "Send refined"}
              </button>
              <button className="ghost-btn" onClick={onDismiss} disabled={working}>
                Cancel
              </button>
            </div>
          </div>

          {result.reads.length > 0 || result.read_notes.length > 0 || result.notes.length > 0 ? (
            <details className="refine-details">
              <summary className="fine">
                What it read and noted
                {result.reads.length > 0 ? ` (${result.reads.join(", ")})` : ""}
              </summary>
              <ul>
                {result.read_notes.map((note) => (
                  <li key={note}>{note}</li>
                ))}
                {result.notes.map((note) => (
                  <li key={note}>{note}</li>
                ))}
              </ul>
            </details>
          ) : null}
        </>
      )}
    </section>
  );
}

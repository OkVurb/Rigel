import { useEffect, useMemo, useState } from "react";

import { api, type ModelOption, type TurnTranscript } from "./api";

const DOOR_FAILURE =
  /(provider returned|http \d{3}|unreachable|timed out|timeout|connection|network|no api key|no key|401|403|408|429|5\d\d)/i;

interface DoorFailure {
  entry: TurnTranscript;
  message: string;
}

interface Offer {
  provider: string;
  model: string;
  outputMicros: number | null;
  deltaMicros: number | null;
}

function failureOf(turns: TurnTranscript[]): DoorFailure | null {
  // Bind the banner to the newest top-level turn. An older failure must not stay on screen
  // after a successful retry, and a subagent's failure is not the selected door's state.
  const entry = [...turns]
    .reverse()
    .find((row) => row.turn.parent_turn_id === null);
  if (!entry || entry.turn.status !== "failed") return null;
  const error = entry.items.filter((row) => row.kind === "error").at(-1);
  const message = typeof error?.body.message === "string" ? error.body.message : "";
  return DOOR_FAILURE.test(message) ? { entry, message } : null;
}

function chooseOffer(
  rows: ModelOption[],
  provider: string,
  model: string,
): Offer | null {
  const current = rows.find((row) => row.id === model);
  const candidates = rows
    .filter((row) => row.chat !== false && row.provider !== provider && row.id !== model)
    .sort((one, other) => {
      const left = one.output_micros_per_mtok ?? Number.MAX_SAFE_INTEGER;
      const right = other.output_micros_per_mtok ?? Number.MAX_SAFE_INTEGER;
      return left - right || one.id.localeCompare(other.id);
    });
  const next = candidates[0];
  if (!next) return null;
  const currentPrice = current?.output_micros_per_mtok ?? null;
  const nextPrice = next.output_micros_per_mtok ?? null;
  return {
    provider: next.provider,
    model: next.id,
    outputMicros: nextPrice,
    deltaMicros:
      currentPrice === null || nextPrice === null ? null : nextPrice - currentPrice,
  };
}

function dollars(micros: number | null): string {
  if (micros === null) return "price unknown";
  return `$${(micros / 1_000_000).toFixed(2)}/Mtok output`;
}

function delta(micros: number | null): string {
  if (micros === null) return "delta unknown";
  if (micros === 0) return "same output price";
  const sign = micros > 0 ? "+" : "−";
  return `${sign}$${(Math.abs(micros) / 1_000_000).toFixed(2)}/Mtok output`;
}

export function DegradedBanner({
  turns,
  provider,
  model,
  busy,
  onRetry,
  onFallback,
}: {
  turns: TurnTranscript[];
  provider: string;
  model: string;
  busy: boolean;
  onRetry: (entry: TurnTranscript) => void;
  onFallback: (entry: TurnTranscript, offer: Offer) => void;
}) {
  const failure = useMemo(() => failureOf(turns), [turns]);
  const [models, setModels] = useState<ModelOption[]>([]);

  useEffect(() => {
    if (!failure) return;
    let live = true;
    api
      .listModels()
      .then((rows) => live && setModels(rows))
      .catch(() => live && setModels([]));
    return () => {
      live = false;
    };
  }, [failure?.entry.turn.id]);

  if (!failure) return null;
  const offer = chooseOffer(models, provider, model);
  const door = provider ? `${provider} / ${model}` : model || "the selected door";

  return (
    <section className="degraded-banner" role="alert" aria-label="Provider degraded">
      <div className="degraded-copy">
        <strong>{provider || "The selected provider"} is not answering.</strong>
        <p>
          {failure.message}. This turn failed. Rigel did not switch providers or models on its
          own.
        </p>
        <small>
          Selected door: <span className="mono">{door}</span>. Retrying the same turn uses that
          exact door.
        </small>
        {offer ? (
          <div className="degraded-offer">
            <span>
              Consent-only fallback: <span className="mono">{offer.model}</span> on{" "}
              {offer.provider} · {dollars(offer.outputMicros)} · {delta(offer.deltaMicros)}
            </span>
            <button
              type="button"
              className="chip"
              disabled={busy}
              onClick={() => onFallback(failure.entry, offer)}
            >
              Retry on {offer.model}
            </button>
          </div>
        ) : null}
      </div>
      <button
        type="button"
        className="chip warn"
        disabled={busy}
        onClick={() => onRetry(failure.entry)}
      >
        Retry this turn
      </button>
    </section>
  );
}

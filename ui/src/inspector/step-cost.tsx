/**
 * Per-step cost and timing (roadmap 34).
 *
 * The usage row is the source of truth: tokens, wall time, queue time and cost are read from
 * the same body the JSON export writes, so the inspector cannot drift from the transcript.
 */

const money = (micros: number | null | undefined): string =>
  micros === null || micros === undefined ? "—" : `$${(micros / 1_000_000).toFixed(6)}`;

const tokens = (value: number | null | undefined): string => {
  if (value === null || value === undefined) return "—";
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}m`;
  if (value >= 1_000) return `${Math.round(value / 1_000)}k`;
  return String(value);
};

const millis = (value: number | null | undefined): string => {
  if (value === null || value === undefined) return "—";
  if (value < 1_000) return `${value} ms`;
  return `${(value / 1_000).toFixed(2)} s`;
};

function number(body: Record<string, unknown>, key: string): number | null {
  const value = body[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function StepCost({ body }: { body: Record<string, unknown> }) {
  const input = number(body, "input_tokens") ?? 0;
  const output = number(body, "output_tokens") ?? 0;
  const step = number(body, "step");
  const duration = number(body, "duration_ms");
  const queue = number(body, "queue_ms");
  const cost = number(body, "cost_micros");
  return (
    <dl className="step-cost" aria-label={step === null ? "step cost" : `step ${step} cost`}>
      <div>
        <dt>step</dt>
        <dd>{step ?? "—"}</dd>
      </div>
      <div>
        <dt>tokens</dt>
        <dd>{tokens(input + output)}</dd>
      </div>
      <div>
        <dt>wall</dt>
        <dd>{millis(duration)}</dd>
      </div>
      <div>
        <dt>queue</dt>
        <dd>{millis(queue)}</dd>
      </div>
      <div>
        <dt>cost</dt>
        <dd>
          {money(cost)}
          {body.source === "estimated" ? " est." : ""}
        </dd>
      </div>
    </dl>
  );
}

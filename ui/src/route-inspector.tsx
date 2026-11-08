/**
 * Route inspector (roadmap 13).
 *
 * The core resolver owns one answer, and this reads it rather than recomputing anything: the
 * saved pair, what the caller asked for, what won, why, and the core's own mismatch sentence.
 * `api.routeExplain` is the same command the turn path uses, so a difference between this panel
 * and the door a request took would be a bug in one call site, not a second opinion.
 */
import { useEffect, useState } from "react";

import { api, type RouteExplanation, type RoutingPolicy } from "./api";

/** Roadmap 26: the same three fields the request carries, in one readable line. */
function policyLine(policy: RoutingPolicy | null): string {
  if (!policy) return "…";
  const parts: string[] = [];
  if (policy.order.length) parts.push(`order ${policy.order.join(" -> ")}`);
  parts.push(`fallbacks ${policy.allow_fallbacks ? "on" : "off"}`);
  if (policy.quantizations.length) {
    parts.push(`quantizations ${policy.quantizations.join(", ")}`);
  }
  return parts.join(" · ");
}

export function RouteInspector({
  project,
  provider,
  model,
}: {
  project: string;
  provider: string;
  model: string;
}) {
  const [requestedProvider, setRequestedProvider] = useState("");
  const [requestedModel, setRequestedModel] = useState("");
  const [explanation, setExplanation] = useState<RouteExplanation | null>(null);
  const [policy, setPolicy] = useState<RoutingPolicy | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .routingPolicy(project)
      .then((next) => {
        if (live) setPolicy(next);
      })
      .catch(() => {
        if (live) setPolicy(null);
      });
    return () => {
      live = false;
    };
  }, [project]);

  useEffect(() => {
    let live = true;
    api
      .routeExplain(requestedProvider.trim() || null, requestedModel.trim() || null)
      .then((next) => {
        if (!live) return;
        setExplanation(next);
        setError(null);
      })
      .catch((problem: unknown) => {
        if (!live) return;
        setError(problem instanceof Error ? problem.message : String(problem));
      });
    return () => {
      live = false;
    };
  }, [requestedProvider, requestedModel, provider, model]);

  const named = requestedProvider.trim() !== "" || requestedModel.trim() !== "";
  const pair = (route: { provider: string; model: string } | null | undefined) =>
    route ? `${route.provider} / ${route.model}` : "—";

  return (
    <section className="settings-section route-inspector" aria-label="Route inspector">
      <h3>Route</h3>
      <p className="fine">
        The door one turn will use. These are the resolver&apos;s own fields; nothing here is
        recalculated in the window.
      </p>
      <dl className="route-rows">
        <div>
          <dt>selected</dt>
          <dd className="mono" data-testid="route-selected">
            {explanation ? pair(explanation.saved) : "…"}
          </dd>
        </div>
        <div>
          <dt>requested</dt>
          <dd className="mono" data-testid="route-requested">
            {explanation ? pair(explanation.requested) : "…"}
          </dd>
        </div>
        <div>
          <dt>resolved</dt>
          <dd className="mono" data-testid="route-resolved">
            {explanation ? pair(explanation.resolved) : "…"}
          </dd>
        </div>
        <div>
          <dt>reason</dt>
          <dd className="mono" data-testid="route-reason">
            {explanation ? explanation.reason : "…"}
          </dd>
        </div>
      </dl>
      <p className="fine route-policy" data-testid="route-policy">
        OpenRouter policy: <span className="mono">{policyLine(policy)}</span>
        {provider === "openrouter" ? "" : " (applies to OpenRouter turns)"}
      </p>
      {error ? (
        <p className="notice" role="status" data-testid="route-error">
          {error}
        </p>
      ) : null}
      <p className="fine" data-testid="route-mismatch">
        {explanation
          ? (explanation.mismatch ?? "The request and Settings agree; nothing to explain.")
          : "Reading the resolver…"}
      </p>
      <p className="fine">
        Preview another pair: a provider with the model, or a bare model id that another door owns.
      </p>
      <div className="settings-field-grid">
        <label className="settings-field">
          <span>Requested provider</span>
          <input
            value={requestedProvider}
            onChange={(event) => setRequestedProvider(event.target.value)}
            placeholder={provider}
            aria-label="Requested provider"
          />
        </label>
        <label className="settings-field">
          <span>Requested model</span>
          <input
            value={requestedModel}
            onChange={(event) => setRequestedModel(event.target.value)}
            placeholder={model}
            aria-label="Requested model"
          />
        </label>
      </div>
      {named ? (
        <button
          className="chip"
          onClick={() => {
            setRequestedProvider("");
            setRequestedModel("");
          }}
        >
          Clear preview
        </button>
      ) : null}
    </section>
  );
}

/**
 * Model + reasoning effort (D28).
 *
 * The catalogue is the provider's own list, read through Rust so the key never enters the
 * window. Effort is a separate saved setting; the provider adapter sends it per request.
 * The same control appears in Session and Settings so there is one interaction to learn.
 */

import { useEffect, useRef, useState } from "react";

import { api } from "./api";

const EFFORTS = ["low", "medium", "high"] as const;

const FALLBACK_MODELS = [
  "gpt-5.6-sol-low",
  "gpt-5.6-sol-medium",
  "gpt-5.6-sol-high",
  "openai/gpt-4.1-mini",
  "anthropic/claude-sonnet-4.5",
];

function shownModel(model: string): string {
  return model.trim() || "no model";
}

export function ModelControls({
  model,
  effort,
  disabled,
  onModel,
  onEffort,
}: {
  model: string;
  effort: string;
  disabled?: boolean;
  onModel: (model: string) => void;
  onEffort: (effort: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [models, setModels] = useState<string[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const searchRef = useRef<HTMLInputElement | null>(null);

  // Fetch once, the first time the popover opens. Deliberately not guarded by `loading`:
  // setting state from an effect that depends on that state would cancel its own request
  // (cleanup flips the liveness flag) and leave the picker on "Reading…" forever.
  useEffect(() => {
    if (!open || models) return;
    setLoading(true);
    api
      .listModels()
      .then((next) => {
        setModels(next.length > 0 ? next : FALLBACK_MODELS);
        setError(null);
      })
      .catch((problem: unknown) => {
        setModels(FALLBACK_MODELS);
        setError(problem instanceof Error ? problem.message : String(problem));
      })
      .finally(() => setLoading(false));
  }, [open, models]);

  useEffect(() => {
    if (!open) return;
    searchRef.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  const needle = filter.trim().toLowerCase();
  const filtered = (models ?? []).filter((id) => !needle || id.toLowerCase().includes(needle));
  const visible = filtered.slice(0, 300);

  return (
    <div className="model-picker">
      <button
        className="model-button"
        type="button"
        aria-haspopup="dialog"
        aria-expanded={open}
        disabled={disabled}
        title="Model and reasoning effort — saved as the default"
        onClick={() => setOpen((value) => !value)}
      >
        <span className="model-button-label">model</span>
        <span className="model-button-value">{shownModel(model)}</span>
        <span className="effort-pill">{effort || "medium"}</span>
      </button>

      {open ? (
        <>
          <button
            className="model-scrim"
            type="button"
            aria-label="Close model picker"
            onClick={() => setOpen(false)}
          />
          <div className="model-popover" role="dialog" aria-label="Model and reasoning effort">
            <div className="model-effort">
              <span>Effort</span>
              <div className="segmented" role="group" aria-label="Reasoning effort">
                {EFFORTS.map((value) => (
                  <button
                    key={value}
                    type="button"
                    aria-pressed={effort === value}
                    className={effort === value ? "on" : ""}
                    onClick={() => onEffort(value)}
                  >
                    {value}
                  </button>
                ))}
              </div>
            </div>

            <label className="model-search">
              <span aria-hidden>⌕</span>
              <input
                ref={searchRef}
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
                placeholder={`Search ${models?.length ?? "…"} models…`}
                aria-label="Search models"
                spellCheck={false}
              />
            </label>

            <div className="model-list" role="listbox" aria-label="Models">
              {loading ? <p className="empty subtle">Reading the catalogue…</p> : null}
              {!loading && visible.length === 0 ? (
                <p className="empty subtle">No model matches “{filter}”.</p>
              ) : null}
              {visible.map((id) => (
                <button
                  key={id}
                  type="button"
                  role="option"
                  aria-selected={id === model}
                  className={`model-row${id === model ? " on" : ""}`}
                  onClick={() => {
                    onModel(id);
                    setOpen(false);
                  }}
                >
                  <span className="mono">{id}</span>
                </button>
              ))}
              {!loading && filtered.length > visible.length ? (
                <p className="empty subtle">
                  {filtered.length - visible.length} more — narrow the search.
                </p>
              ) : null}
            </div>

            <p className="fine">
              {error
                ? `${error} Showing a short fallback list.`
                : "Saved as the default for new turns and sessions. Effort is sent per request."}
            </p>
          </div>
        </>
      ) : null}
    </div>
  );
}

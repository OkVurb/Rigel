/**
 * Model + reasoning effort (D28).
 *
 * The catalogue is read through Rust so keys never enter the window. It merges the current
 * provider with OpenRouter's full roster, free models included; choosing a row carries its
 * provider with it, so there is no second switch to set. Effort and the chat filter are separate
 * saved settings, passed in by the caller that owns them. The same control appears in Session
 * and Settings so there is one interaction to learn.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { api, type ModelOption } from "./api";

const EFFORTS = ["low", "medium", "high"] as const;

const FALLBACK_MODELS: ModelOption[] = [
  { id: "gpt-5.6-sol-medium", provider: "omniroute", free: false, chat: true },
  { id: "openai/gpt-4.1-mini", provider: "openrouter", free: false, chat: true },
  { id: "baseten/mammoth-1:free", provider: "openrouter", free: true, chat: true },
  { id: "nvidia/nemotron-3.5-super-vl:free", provider: "openrouter", free: true, chat: true },
  { id: "anthropic/claude-sonnet-4.5", provider: "openrouter", free: false, chat: true },
];

function shownModel(model: string): string {
  return model.trim() || "no model";
}

export function ModelControls({
  model,
  effort,
  chatOnly,
  disabled,
  onModel,
  onEffort,
  onChatOnly,
}: {
  model: string;
  effort: string;
  /** The saved `chat_models_only` setting; the caller writes the new value through settings_set. */
  chatOnly: boolean;
  disabled?: boolean;
  onModel: (model: string, provider: string) => void;
  onEffort: (effort: string) => void;
  onChatOnly: (chatOnly: boolean) => void;
}) {
  const [open, setOpen] = useState(false);
  const [models, setModels] = useState<ModelOption[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  // The merged roster can run to four figures — every provider that holds a credential is asked —
  // so the provider chips are how a person reaches the useful end of it without typing. The
  // chat filter is the other half: a chat picker should not open on a Stable-Diffusion checkpoint.
  // Rows the core marked `chat: false` are hidden, never dropped, and the count says how many.
  // The choice itself is the saved `chat_models_only` setting, so it survives a restart.
  const [door, setDoor] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);

  // Closing returns focus to the button that opened the popover: Escape and the × should not
  // drop a keyboard user back at the top of the document.
  const close = useCallback(() => {
    setOpen(false);
    // After the commit: the focused search box is being removed, and unmounting it would
    // otherwise hand focus back to the document rather than to this button.
    requestAnimationFrame(() => triggerRef.current?.focus());
  }, []);

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
      if (event.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, close]);

  const needle = filter.trim().toLowerCase();
  const doors = Array.from(new Set((models ?? []).map((option) => option.provider))).sort();
  const counts = new Map<string, number>();
  for (const option of models ?? []) {
    counts.set(option.provider, (counts.get(option.provider) ?? 0) + 1);
  }
  const nonChat = (models ?? []).filter((option) => option.chat === false).length;
  const filtered = (models ?? []).filter(
    (option) =>
      (!chatOnly || option.chat !== false) &&
      (!door || option.provider === door) &&
      (!needle ||
        option.id.toLowerCase().includes(needle) ||
        option.name?.toLowerCase().includes(needle)),
  );
  const visible = filtered.slice(0, 300);

  return (
    <div className="model-picker">
      <button
        ref={triggerRef}
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
            onClick={close}
          />
          <div
            className="model-popover"
            role="dialog"
            aria-modal="true"
            aria-label="Model and reasoning effort"
          >
            <div className="model-popover-head">
              <span>Model</span>
              <button
                className="model-close"
                type="button"
                aria-label="Close model picker"
                title="Close"
                onClick={close}
              >
                ×
              </button>
            </div>

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

            {nonChat > 0 ? (
              <label
                className="toggle model-chat-only"
                title="Image, video, audio and retrieval models cannot answer a chat turn. Untick to show them anyway."
              >
                <input
                  type="checkbox"
                  checked={chatOnly}
                  onChange={(event) => onChatOnly(event.target.checked)}
                />
                chat models only
                <span className="fine">
                  {chatOnly ? `${nonChat} hidden` : `all ${models?.length ?? 0} shown`}
                </span>
              </label>
            ) : null}

            {doors.length > 1 ? (
              <div className="model-doors" role="group" aria-label="Providers in this list">
                <button
                  type="button"
                  aria-pressed={door === null}
                  className={door === null ? "on" : ""}
                  onClick={() => setDoor(null)}
                >
                  all · {models?.length ?? 0}
                </button>
                {doors.map((id) => (
                  <button
                    key={id}
                    type="button"
                    aria-pressed={door === id}
                    className={door === id ? "on" : ""}
                    onClick={() => setDoor(door === id ? null : id)}
                  >
                    {id} · {counts.get(id) ?? 0}
                  </button>
                ))}
              </div>
            ) : null}

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
              {visible.map((option) => (
                <button
                  key={`${option.provider}:${option.id}`}
                  type="button"
                  role="option"
                  aria-selected={option.id === model}
                  className={`model-row${option.id === model ? " on" : ""}`}
                  title={option.name ?? option.id}
                  onClick={() => {
                    onModel(option.id, option.provider);
                    close();
                  }}
                >
                  <span className="model-row-text">
                    <span className="mono">{option.id}</span>
                    {option.name ? <span className="model-name">{option.name}</span> : null}
                  </span>
                  <span className="model-tags">
                    {option.free ? <span className="tag free">free</span> : null}
                    <span className="tag">{option.provider}</span>
                  </span>
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
                : "Every provider with a key is listed — the chips filter by door. Picking a model also switches to the provider that serves it; effort is sent per request."}
            </p>
          </div>
        </>
      ) : null}
    </div>
  );
}

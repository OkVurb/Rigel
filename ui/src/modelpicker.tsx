/**
 * Model + reasoning effort (D28).
 *
 * The catalogue is read through Rust so keys never enter the window. It merges the current
 * provider with OpenRouter's full roster, free models included; choosing a row carries its
 * provider with it, so there is no second switch to set. Effort and the chat filter are separate
 * saved settings, passed in by the caller that owns them. The same control appears in Session
 * and Settings so there is one interaction to learn.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import {
  api,
  type DoorHealth,
  type ModelCapability,
  type ModelOption,
} from "./api";

const EFFORTS = ["low", "medium", "high"] as const;

const FALLBACK_MODELS: ModelOption[] = [
  { id: "gpt-5.6-sol-medium", provider: "omniroute", free: false, chat: true },
  { id: "openai/gpt-4.1-mini", provider: "openrouter", free: false, chat: true },
  { id: "baseten/mammoth-1:free", provider: "openrouter", free: true, chat: true },
  { id: "nvidia/nemotron-3.5-super-vl:free", provider: "openrouter", free: true, chat: true },
  { id: "anthropic/claude-sonnet-4.5", provider: "openrouter", free: false, chat: true },
];

const FAVOURITES_KEY = "rigel.models.favourites";
const RECENTS_KEY = "rigel.models.recents";
const MODEL_PAGE = 200;

function modelKey(option: ModelOption): string {
  return `${option.provider}:${option.id}`;
}

function savedList(key: string): string[] {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(key) ?? "[]");
    return Array.isArray(parsed)
      ? parsed.filter((value): value is string => typeof value === "string")
      : [];
  } catch {
    return [];
  }
}

function saveList(key: string, values: string[]): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(values));
  } catch {
    // Favourites and recents are conveniences; a hardened webview may refuse storage.
  }
}

function costBand(option: ModelOption): { label: string; detail: string } | null {
  if (option.free) return null;
  const micros = option.output_micros_per_mtok;
  if (micros === undefined || micros === null) return null;
  if (micros === 0) {
    return { label: "free", detail: "No completion charge is published for this model." };
  }
  const dollars = micros / 1_000_000;
  return {
    label: dollars <= 2 ? "$" : dollars <= 10 ? "$$" : "$$$",
    detail: `About $${dollars.toFixed(2)} per million completion tokens, from the provider's list price.`,
  };
}

function shownModel(model: string): string {
  return model.trim() || "no model";
}

/** Item 18: every fact is named, and a fact this build does not know says so. */
function capabilitySummary(caps: ModelCapability): string {
  const flag = (value: boolean | null, label: string) =>
    `${label} ${value === null ? "unknown" : value ? "yes" : "no"}`;
  const count = (value: number | null) =>
    value === null
      ? "unknown"
      : value >= 1_000
        ? `${Math.round(value / 1_000)}k`
        : String(value);
  return [
    flag(caps.tools, "tools"),
    flag(caps.vision, "vision"),
    `ctx ${count(caps.context_tokens)}`,
    `max ${count(caps.max_output_tokens)}`,
    flag(caps.streaming, "stream"),
    caps.output_micros_per_mtok === null
      ? "price unknown"
      : `$${(caps.output_micros_per_mtok / 1_000_000).toFixed(2)}/M out`,
  ].join(" · ");
}

export function ModelControls({
  model,
  provider,
  effort,
  chatOnly,
  disabled,
  label = "model",
  title = "Model and reasoning effort — saved as the default",
  heading = "Model",
  showEffort = true,
  forceChatOnly = false,
  onModel,
  onEffort,
  onChatOnly,
}: {
  model: string;
  /** The model's door, when the caller knows it; disambiguates duplicate ids across providers. */
  provider?: string;
  effort: string;
  /** The saved `chat_models_only` setting; the caller writes the new value through settings_set. */
  chatOnly: boolean;
  disabled?: boolean;
  /** Trigger text; the Session and Settings pickers keep the default `model`. */
  label?: string;
  title?: string;
  /** Panel heading; the Swarm preflight names the role it is choosing. */
  heading?: string;
  /** Hide the effort control when the choice is only a model, not a full chat turn. */
  showEffort?: boolean;
  /** Keep non-chat rows hidden and remove the toggle that would reveal them. */
  forceChatOnly?: boolean;
  onModel: (model: string, provider: string) => void;
  onEffort: (effort: string) => void;
  onChatOnly: (chatOnly: boolean) => void;
}) {
  const [open, setOpen] = useState(false);
  const [models, setModels] = useState<ModelOption[] | null>(null);
  const [health, setHealth] = useState<DoorHealth[] | null>(null);
  const [capabilities, setCapabilities] = useState<Map<string, ModelCapability> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [visibleLimit, setVisibleLimit] = useState(MODEL_PAGE);
  const [favourites, setFavourites] = useState<string[]>(() => savedList(FAVOURITES_KEY));
  const [recents, setRecents] = useState<string[]>(() => savedList(RECENTS_KEY));
  // The merged roster can run to four figures — every provider that holds a credential is asked —
  // so the provider chips are how a person reaches the useful end of it without typing. The
  // chat filter is the other half: a chat picker should not open on a Stable-Diffusion checkpoint.
  // Rows the core marked `chat: false` are hidden, never dropped, and the count says how many.
  // The choice itself is the saved `chat_models_only` setting, so it survives a restart.
  const [door, setDoor] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);

  // Closing returns focus to the button that opened the popover: Escape and the × should not
  // drop a keyboard user back at the top of the document.
  const close = useCallback(() => {
    setOpen(false);
    // After the commit: the focused search box is being removed, and unmounting it would
    // otherwise hand focus back to the document rather than to this button.
    requestAnimationFrame(() => triggerRef.current?.focus());
  }, []);

  // Read on every open. Rust keeps this cheap with its short cache, while Free mode clears that
  // cache after a scan so changed free/paid labels appear without restarting the window.
  useEffect(() => {
    if (!open) return;
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
  }, [open]);

  // Item 15: the launch pass is cached in Rust; opening the picker reads that answer. A failed
  // probe is absent from the list rather than an error that keeps the models from showing.
  useEffect(() => {
    if (!open) return;
    let live = true;
    api
      .providerHealth()
      .then((rows) => live && setHealth(rows))
      .catch(() => live && setHealth(null));
    return () => {
      live = false;
    };
  }, [open]);

  // Item 18: one lookup for the whole list the picker is holding, so unknown rows stay unknown
  // instead of being inferred from a name.
  useEffect(() => {
    if (!open || !models) return;
    let live = true;
    api
      .modelCapabilities(
        models.map((option) => ({
          provider: option.provider,
          model: option.id,
          modality: option.modality ?? null,
          output_micros_per_mtok: option.output_micros_per_mtok ?? null,
          chat: option.chat ?? true,
        })),
      )
      .then((rows) => {
        if (!live) return;
        setCapabilities(
          new Map(rows.map((row) => [`${row.provider}:${row.model}`, row])),
        );
      })
      .catch(() => live && setCapabilities(null));
    return () => {
      live = false;
    };
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

  // The popover lives inside the session/settings pane, whose overflow is clipped on purpose.
  // A portalled, fixed panel keeps it whole even when it is wider than its trigger or sits in a
  // scrolled page. Coordinates are divided by the window zoom for the same reason as the menu.tsx
  // portal: getBoundingClientRect is in visual pixels while #root is zoomed.
  useLayoutEffect(() => {
    if (!open) {
      setPosition(null);
      return;
    }

    const place = () => {
      const trigger = triggerRef.current;
      const panel = popoverRef.current;
      if (!trigger || !panel) return;
      const rawScale = Number(document.documentElement.dataset.scale ?? 100);
      const scale = Number.isFinite(rawScale) && rawScale > 0 ? rawScale / 100 : 1;
      const rect = trigger.getBoundingClientRect();
      const width = panel.offsetWidth;
      const height = panel.offsetHeight;
      const margin = 12;
      const gap = 6;
      const vw = window.innerWidth / scale;
      const vh = window.innerHeight / scale;
      const triggerRight = rect.right / scale;
      const triggerTop = rect.top / scale;
      const triggerBottom = rect.bottom / scale;
      const below = triggerBottom + gap;
      const above = triggerTop - height - gap;
      const spaceBelow = vh - below - margin;
      const spaceAbove = triggerTop - margin - gap;
      const top = spaceBelow >= height || spaceBelow >= spaceAbove ? below : above;
      const left = Math.min(
        Math.max(margin, triggerRight - width),
        Math.max(margin, vw - width - margin),
      );
      const next = {
        left,
        top: Math.min(Math.max(margin, top), Math.max(margin, vh - height - margin)),
      };
      setPosition((current) =>
        current?.left === next.left && current.top === next.top ? current : next,
      );
    };

    place();
    const panel = popoverRef.current;
    const observer =
      panel && typeof ResizeObserver !== "undefined" ? new ResizeObserver(place) : null;
    if (panel && observer) observer.observe(panel);
    window.addEventListener("resize", place);
    // Capture scroll for the settings page and any scrollable ancestor; the trigger moves with it.
    window.addEventListener("scroll", place, true);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open]);

  const needle = filter.trim().toLowerCase();
  const selectedTiered = model.trim().toLowerCase().endsWith("-tiered");
  const doors = Array.from(new Set((models ?? []).map((option) => option.provider))).sort();
  const counts = new Map<string, number>();
  for (const option of models ?? []) {
    counts.set(option.provider, (counts.get(option.provider) ?? 0) + 1);
  }
  const nonChat = (models ?? []).filter((option) => option.chat === false).length;
  // The Swarm boss is a chat turn by definition: hide non-chat rows and do not offer a toggle that
  // would let a checkpoint be queued as the coordinator.
  const effectiveChatOnly = forceChatOnly || chatOnly;
  const filtered = (models ?? []).filter(
    (option) =>
      (!effectiveChatOnly || option.chat !== false) &&
      (!door || option.provider === door) &&
      (!needle ||
        option.id.toLowerCase().includes(needle) ||
        option.name?.toLowerCase().includes(needle)),
  );
  const favouriteSet = new Set(favourites);
  const recentSet = new Set(recents);
  const rank = (option: ModelOption) =>
    favouriteSet.has(modelKey(option)) ? 0 : recentSet.has(modelKey(option)) ? 1 : 2;
  const visible = [...filtered]
    .sort((one, other) => rank(one) - rank(other))
    .slice(0, visibleLimit);

  // A new filter starts at one page. Scrolling the list grows this window until every matching
  // row is mounted; the full roster remains searchable from the first page.
  useEffect(() => setVisibleLimit(MODEL_PAGE), [filter, door, effectiveChatOnly]);

  const loadMoreModels = () => {
    if (visibleLimit < filtered.length) {
      setVisibleLimit((current) => Math.min(current + MODEL_PAGE, filtered.length));
    }
  };

  const toggleFavourite = (key: string) => {
    setFavourites((current) => {
      const next = current.includes(key)
        ? current.filter((value) => value !== key)
        : [...current, key];
      saveList(FAVOURITES_KEY, next);
      return next;
    });
  };

  const choose = (option: ModelOption) => {
    const key = modelKey(option);
    onModel(option.id, option.provider);
    setRecents((current) => {
      const next = [key, ...current.filter((value) => value !== key)].slice(0, 5);
      saveList(RECENTS_KEY, next);
      return next;
    });
    close();
  };

  const portalRoot = document.getElementById("root") ?? document.body;

  return (
    <div className="model-picker">
      <button
        ref={triggerRef}
        className="model-button"
        type="button"
        aria-haspopup="dialog"
        aria-expanded={open}
        disabled={disabled}
        title={title}
        onClick={() => setOpen((value) => !value)}
      >
        <span className="model-button-label">{label}</span>
        <span className="model-button-value">{shownModel(model)}</span>
        {showEffort ? <span className="effort-pill">{effort || "medium"}</span> : null}
      </button>

      {open
        ? createPortal(
            <>
              <button
                className="model-scrim"
                type="button"
                aria-label="Close model picker"
                onClick={close}
              />
              <div
                ref={popoverRef}
                className="model-popover"
                role="dialog"
                aria-modal="true"
                aria-label="Model and reasoning effort"
                style={
                  position
                    ? { left: position.left, top: position.top }
                    : { left: 0, top: 0, visibility: "hidden" }
                }
              >
            <div className="model-popover-head">
              <span>{heading}</span>
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

            {health && health.length > 0 ? (
              <div className="model-health" role="status" aria-label="Provider health">
                {health.map((door) => (
                  <span
                    key={door.provider}
                    className={`model-health-door ${door.state}`}
                    title={door.detail ?? `${door.name} answered the launch probe`}
                  >
                    {door.provider} ·{" "}
                    {door.state === "ready"
                      ? `ready · ${door.models ?? 0} models · ${door.latency_ms ?? 0} ms`
                      : door.state}
                  </span>
                ))}
              </div>
            ) : null}

            {showEffort ? (
              <>
                <div className="model-effort">
                  <span>{selectedTiered ? "Tier" : "Effort"}</span>
                  <div
                    className="segmented"
                    role="group"
                    aria-label={selectedTiered ? "Reasoning tier" : "Reasoning effort"}
                  >
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
                {selectedTiered ? (
                  <p className="fine model-tier-note">
                    This id names the family; the tier travels as{" "}
                    <code>reasoning_effort</code>, so the id stays unchanged.
                  </p>
                ) : null}
              </>
            ) : null}

            {nonChat > 0 && !forceChatOnly ? (
              <label
                className="toggle model-chat-only"
                title="Image, video, audio and retrieval models cannot answer a chat turn. Untick to show them anyway."
              >
                <input
                  type="checkbox"
                  checked={effectiveChatOnly}
                  onChange={(event) => onChatOnly(event.target.checked)}
                />
                chat models only
                <span className="fine">
                  {effectiveChatOnly ? `${nonChat} hidden` : `all ${models?.length ?? 0} shown`}
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

            <div
              className="model-list"
              role="listbox"
              aria-label="Models"
              onScroll={(event) => {
                const list = event.currentTarget;
                if (list.scrollHeight - list.scrollTop - list.clientHeight < 240) {
                  loadMoreModels();
                }
              }}
            >
              {loading ? <p className="empty subtle">Reading the catalogue…</p> : null}
              {!loading && visible.length === 0 ? (
                <p className="empty subtle">No model matches “{filter}”.</p>
              ) : null}
              {visible.map((option) => {
                const key = modelKey(option);
                const favourite = favouriteSet.has(key);
                const recent = !favourite && recentSet.has(key);
                const cost = costBand(option);
                const caps = capabilities?.get(`${option.provider}:${option.id}`) ?? null;
                const selected =
                  option.id === model && (!provider || option.provider === provider);
                return (
                  <div className="model-row-wrap" role="presentation" key={key}>
                    <button
                      type="button"
                      role="option"
                      aria-selected={selected}
                      className={`model-row${selected ? " on" : ""}`}
                      title={`${option.name ? `${option.name} · ` : ""}${option.provider} / ${option.id}`}
                      onClick={() => choose(option)}
                    >
                      <span className="model-row-text">
                        <span className="mono">{option.id}</span>
                        {option.name ? <span className="model-name">{option.name}</span> : null}
                        {caps ? (
                          <span
                            className="model-caps"
                            title={`${caps.source} · ${caps.as_of}`}
                          >
                            {capabilitySummary(caps)}
                          </span>
                        ) : null}
                      </span>
                      <span className="model-tags">
                        {option.free ? <span className="tag free">free</span> : null}
                        {option.id.toLowerCase().endsWith("-tiered") ? (
                          <span
                            className="tag tiered"
                            title="Tiered family: the effort control chooses its tier."
                          >
                            tiered
                          </span>
                        ) : null}
                        {!option.free && cost ? (
                          <span className="tag model-cost" title={cost.detail}>
                            {cost.label}
                          </span>
                        ) : null}
                        {!option.free && !cost ? (
                          <span
                            className="tag model-cost unknown"
                            title="No list price is published for this model; Rigel does not guess one."
                          >
                            unknown
                          </span>
                        ) : null}
                        {recent ? <span className="tag model-recent">recent</span> : null}
                        {option.local ? <span className="tag local">local</span> : null}
                        <span className="tag">{option.provider}</span>
                      </span>
                    </button>
                    <button
                      type="button"
                      className={`model-star${favourite ? " on" : ""}`}
                      aria-pressed={favourite}
                      aria-label={`${favourite ? "Remove" : "Add"} ${option.id} ${
                        favourite ? "from" : "to"
                      } favourites`}
                      title={favourite ? "Remove from favourites" : "Add to favourites"}
                      onClick={() => toggleFavourite(key)}
                    >
                      {favourite ? "★" : "☆"}
                    </button>
                  </div>
                );
              })}
              {!loading && filtered.length > visible.length ? (
                <button className="load-more" type="button" onClick={loadMoreModels}>
                  Load more · {visible.length.toLocaleString()} of {filtered.length.toLocaleString()}
                </button>
              ) : null}
              {!loading && filtered.length > MODEL_PAGE && visible.length >= filtered.length ? (
                <p className="empty subtle">All {filtered.length.toLocaleString()} models shown.</p>
              ) : null}
            </div>

            <p className="fine">
              {error
                ? `${error} Showing a short fallback list.`                  : "Every provider with a key is listed — the chips filter by door. Picking a model carries the provider that serves it; effort (or a tiered model's tier) is sent per request."}
            </p>
              </div>
            </>,
            portalRoot,
          )
        : null}
    </div>
  );
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { api, when, type PaletteResults } from "./api";

/**
 * D147: one search over actions, skills, sessions, turns and patches. The palette owns no data
 * of its own — the core's `search_all` returns the four sections, already capped and ordered,
 * and this file only filters the six actions and decides where a row goes.
 *
 * Keyboard is the primary path: ↑/↓ move, Enter opens, Escape closes. The rows are buttons so a
 * pointer works too, and the active row is scrolled into view rather than the list jumping.
 */
export type PaletteAction =
  | "shelf"
  | "attention"
  | "session"
  | "chat"
  | "diff"
  | "terminal"
  | "settings";

interface Action {
  id: PaletteAction;
  label: string;
  detail: string;
}

const ACTIONS: Action[] = [
  { id: "shelf", label: "Shelf", detail: "Search the local skill index" },
  { id: "attention", label: "Attention", detail: "What still needs an answer" },
  { id: "session", label: "Session", detail: "Transcript, agents and composer" },
  { id: "chat", label: "Chat", detail: "The agent room" },
  { id: "diff", label: "Diff", detail: "Claims, patches and merges" },
  { id: "terminal", label: "Terminal", detail: "A shell in the project" },
  { id: "settings", label: "Settings", detail: "Providers, safety, appearance" },
];

interface Props {
  open: boolean;
  project: string | null;
  currentSession: string | null;
  onClose: () => void;
  onAction: (action: PaletteAction) => void;
  onSkill: (id: string) => void;
  onSession: (id: string) => void;
  onTurn: (sessionId: string, turnId: string) => void;
  onPatch: (sessionId: string, patchId: string) => void;
}

type Entry =
  | { kind: "action"; key: string; title: string; subtitle: string; meta: string; action: Action }
  | {
      kind: "skill";
      key: string;
      title: string;
      subtitle: string;
      meta: string;
      id: string;
    }
  | {
      kind: "session";
      key: string;
      title: string;
      subtitle: string;
      meta: string;
      id: string;
    }
  | {
      kind: "turn";
      key: string;
      title: string;
      subtitle: string;
      meta: string;
      sessionId: string;
      turnId: string;
    }
  | {
      kind: "patch";
      key: string;
      title: string;
      subtitle: string;
      meta: string;
      sessionId: string;
      patchId: string;
    };

const EMPTY: PaletteResults = { skills: [], sessions: [], turns: [], patches: [] };

function matches(action: Action, query: string): boolean {
  const haystack = `${action.label} ${action.detail}`.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((token) => haystack.includes(token));
}

function shortId(id: string): string {
  return id.slice(0, 8);
}

export default function Palette({
  open,
  project,
  currentSession,
  onClose,
  onAction,
  onSkill,
  onSession,
  onTurn,
  onPatch,
}: Props) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<PaletteResults>(EMPTY);
  const [loading, setLoading] = useState(false);
  const [active, setActive] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const restoreFocus = useRef<HTMLElement | null>(null);

  // Opening resets the query; closing hands focus back to whatever had it. The palette is a
  // layer, not a route, so it must not leave the keyboard stranded in a hidden input.
  useEffect(() => {
    if (!open) return;
    restoreFocus.current = document.activeElement as HTMLElement | null;
    setQuery("");
    setResults(EMPTY);
    setActive(0);
    setError(null);
    const timer = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => {
      window.clearTimeout(timer);
      restoreFocus.current?.focus?.();
    };
  }, [open]);

  // One debounced read. A dead project database or a missing skill index leaves the other
  // sections usable: the command answers with empty groups rather than failing the palette.
  useEffect(() => {
    if (!open) return;
    let live = true;
    setLoading(true);
    // Results for the previous query must not be Enter-able under the new one.
    setResults(EMPTY);
    const timer = window.setTimeout(() => {
      api
        .searchAll(project, query, currentSession)
        .then((next) => {
          if (!live) return;
          setResults(next);
          setError(null);
          setActive(0);
        })
        .catch((failure) => {
          if (!live) return;
          setResults(EMPTY);
          setError(failure instanceof Error ? failure.message : String(failure));
        })
        .finally(() => live && setLoading(false));
    }, 140);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [open, project, query, currentSession]);

  const entries = useMemo<Entry[]>(() => {
    const rows: Entry[] = ACTIONS.filter((action) => matches(action, query)).map((action) => ({
      kind: "action",
      key: `action:${action.id}`,
      title: action.label,
      subtitle: action.detail,
      meta: "action",
      action,
    }));

    for (const skill of results.skills) {
      rows.push({
        kind: "skill",
        key: `skill:${skill.id}`,
        title: skill.name,
        subtitle: skill.description,
        meta: skill.grade ? `skill · ${skill.grade}` : "skill · unscanned",
        id: skill.id,
      });
    }
    for (const session of results.sessions) {
      rows.push({
        kind: "session",
        key: `session:${session.id}`,
        title: session.title || "Untitled session",
        subtitle: `${session.status} · ${when(session.created_at)}`,
        meta: `session · ${shortId(session.id)}`,
        id: session.id,
      });
    }
    for (const turn of results.turns) {
      rows.push({
        kind: "turn",
        key: `turn:${turn.turn_id}`,
        title: turn.snippet || turn.prompt || `Turn ${turn.seq}`,
        subtitle: turn.from_item
          ? `from a tool result · ${turn.role || turn.harness}`
          : `you asked · ${turn.role || turn.harness}`,
        meta: `turn ${turn.seq} · ${when(turn.created_at)}`,
        sessionId: turn.session_id,
        turnId: turn.turn_id,
      });
    }
    for (const patch of results.patches) {
      rows.push({
        kind: "patch",
        key: `patch:${patch.id}`,
        title: patch.path,
        subtitle: `${patch.status} · ${when(patch.created_at)}`,
        meta: "patch",
        sessionId: patch.session_id,
        patchId: patch.id,
      });
    }
    return rows;
  }, [query, results]);

  const run = useCallback(
    (entry: Entry | undefined) => {
      if (!entry) return;
      onClose();
      if (entry.kind === "action") onAction(entry.action.id);
      else if (entry.kind === "skill") onSkill(entry.id);
      else if (entry.kind === "session") onSession(entry.id);
      else if (entry.kind === "turn") onTurn(entry.sessionId, entry.turnId);
      else onPatch(entry.sessionId, entry.patchId);
    },
    [onAction, onClose, onPatch, onSession, onSkill, onTurn],
  );

  const move = useCallback(
    (delta: number) => {
      setActive((current) => {
        if (entries.length === 0) return 0;
        return (current + delta + entries.length) % entries.length;
      });
    },
    [entries.length],
  );

  // The active row follows the keyboard even when the list is longer than the panel.
  useEffect(() => {
    document.getElementById(`palette-row-${active}`)?.scrollIntoView({ block: "nearest" });
  }, [active, entries]);

  if (!open) return null;

  const sections: { label: string; from: number; to: number }[] = [];
  const starts: Record<Entry["kind"], number> = {
    action: -1,
    skill: -1,
    session: -1,
    turn: -1,
    patch: -1,
  };
  entries.forEach((entry, index) => {
    if (starts[entry.kind] === -1) starts[entry.kind] = index;
  });
  const labels: Record<Entry["kind"], string> = {
    action: "Actions",
    skill: "Skills",
    session: "Sessions",
    turn: "Turns",
    patch: "Patches",
  };
  (Object.keys(starts) as Entry["kind"][]).forEach((kind) => {
    const from = starts[kind];
    if (from === -1) return;
    let to = from;
    while (to + 1 < entries.length && entries[to + 1].kind === kind) to++;
    sections.push({ label: labels[kind], from, to });
  });

  return (
    <div
      className="palette-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label="Search actions, skills, sessions, turns and patches"
      >
        <div className="palette-input-row">
          <span aria-hidden className="palette-glyph">
            ⌕
          </span>
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                onClose();
              } else if (event.key === "ArrowDown") {
                event.preventDefault();
                move(1);
              } else if (event.key === "ArrowUp") {
                event.preventDefault();
                move(-1);
              } else if (event.key === "Enter") {
                event.preventDefault();
                run(entries[active]);
              }
            }}
            placeholder="Search actions, skills, sessions, turns, patches…"
            aria-label="Search Rigel"
            aria-controls="palette-list"
            aria-activedescendant={entries.length > 0 ? `palette-row-${active}` : undefined}
            autoComplete="off"
            spellCheck={false}
          />
          <kbd>esc</kbd>
        </div>

        <div className="palette-list" id="palette-list" role="listbox" aria-label="Results">
          {sections.map((section) => (
            <div key={section.label} className="palette-section-block">
              <p className="palette-section" role="presentation">
                {section.label}
              </p>
              {entries.slice(section.from, section.to + 1).map((entry, offset) => {
                const index = section.from + offset;
                return (
                  <button
                    key={entry.key}
                    id={`palette-row-${index}`}
                    type="button"
                    role="option"
                    aria-selected={index === active}
                    className={`palette-row${index === active ? " on" : ""}`}
                    onMouseEnter={() => setActive(index)}
                    onMouseDown={(event) => {
                      event.preventDefault();
                      run(entry);
                    }}
                  >
                    <span className="palette-row-text">
                      <span className="palette-row-title">{entry.title}</span>
                      {entry.subtitle ? (
                        <span className="palette-row-subtitle">{entry.subtitle}</span>
                      ) : null}
                    </span>
                    <span className="palette-row-meta mono">{entry.meta}</span>
                  </button>
                );
              })}
            </div>
          ))}

          {entries.length === 0 && !loading ? (
            <p className="palette-empty">
              {error
                ? error
                : query
                  ? `Nothing matches “${query}”.`
                  : "No sessions or patches in this project yet."}
            </p>
          ) : null}
          {loading ? <p className="palette-empty subtle">Searching…</p> : null}
        </div>

        <footer className="palette-foot">
          <span>↑↓ move</span>
          <span>↵ open</span>
          <span>esc close</span>
          <span className="grow" />
          <span>{project ? "this project + local skills" : "local skills"}</span>
        </footer>
      </div>
    </div>
  );
}

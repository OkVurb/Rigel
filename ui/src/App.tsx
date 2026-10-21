import { useEffect, useMemo, useState } from "react";

import { api, isTauri, type ProjectInfo, type Session } from "./api";
import { RigelMark } from "./mark";
import { MockSkillSource, type SkillRow } from "./mock";

const ICONS = {
  shelf: "M10 2.5 17 6.5v7l-7 4-7-4v-7l7-4ZM3 6.5l7 4 7-4M10 10.5v7",
  session: "M3 4.5h14M3 9.5h14M3 14.5h9",
  diff: "M6 3v14M14 3v14M3 7h6M11 13h6",
  settings:
    "M10 12.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5ZM10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2",
} as const;

type View = "shelf" | "session" | "diff" | "settings";

const VIEWS: { id: View; label: string; d: string; phase: string }[] = [
  { id: "shelf", label: "Shelf", d: ICONS.shelf, phase: "P1.7" },
  { id: "session", label: "Session", d: ICONS.session, phase: "P1.7 skeleton, P3 full" },
  { id: "diff", label: "Diff", d: ICONS.diff, phase: "P5.6" },
  { id: "settings", label: "Settings", d: ICONS.settings, phase: "P3" },
];

export default function App() {
  const source = useMemo(() => new MockSkillSource(), []);
  const [query, setQuery] = useState("");
  const [rows, setRows] = useState<SkillRow[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const [view, setView] = useState<View>("shelf");
  const [projectPath, setProjectPath] = useState("");
  const [project, setProject] = useState<ProjectInfo | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [notice, setNotice] = useState<string>(() =>
    isTauri() ? "Open a project to begin." : "Browser preview — open the desktop app for sessions.",
  );

  useEffect(() => {
    let live = true;
    source.search(query).then((next) => {
      if (live) setRows(next);
    });
    return () => {
      live = false;
    };
  }, [source, query]);

  const selected = rows.find((row) => row.id === selectedId) ?? rows[0] ?? null;

  async function openProject() {
    const path = projectPath.trim();
    if (!path) return;
    try {
      const info = await api.openProject(path);
      setProject(info);
      const listed = await api.listSessions(path);
      setSessions(listed);
      setNotice(
        isTauri()
          ? `${info.name} · ${listed.length} session${listed.length === 1 ? "" : "s"}`
          : `${info.name} · browser preview has no database`,
      );
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <div className="shell">
      <nav className="rail" aria-label="Primary">
        <div className="rail-brand" title="Rigel">
          <RigelMark />
        </div>

        {VIEWS.map((item) => (
          <button
            key={item.id}
            className={`rail-btn${view === item.id ? " active" : ""}`}
            title={`${item.label} — ${item.phase}`}
            aria-label={item.label}
            aria-pressed={view === item.id}
            onClick={() => setView(item.id)}
          >
            <svg
              width="18"
              height="18"
              viewBox="0 0 20 20"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d={item.d} />
            </svg>
          </button>
        ))}

        <div className="rail-spacer" />
        <div className="rail-status" title={notice} aria-hidden>
          ●
        </div>
      </nav>

      <aside className="list-pane">
        <div className="list-head">
          <div className="list-title">
            <h1>{view === "shelf" ? "Shelf" : view[0].toUpperCase() + view.slice(1)}</h1>
            <span>{view === "shelf" ? `${rows.length} shown` : "Phase 1"}</span>
          </div>

          {view === "shelf" ? (
            <label className="search">
              <span aria-hidden>⌕</span>
              <input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="Search skills…"
                aria-label="Search skills"
              />
              <kbd>/</kbd>
            </label>
          ) : null}
        </div>

        {view === "shelf" ? (
          <div className="rows" role="listbox" aria-label="Skills">
            {rows.map((row) => (
              <button
                key={row.id}
                role="option"
                aria-selected={selected?.id === row.id}
                className={`row${selected?.id === row.id ? " on" : ""}`}
                onClick={() => setSelectedId(row.id)}
              >
                <span className={`grade grade-${row.grade.toLowerCase()}`}>{row.grade}</span>
                <span className="row-text">
                  <span className="row-name">
                    {row.name}
                    {row.installed ? <span className="dot" title="On disk" /> : null}
                  </span>
                  <span className="row-desc">{row.description}</span>
                </span>
                <span className="row-meta">{compact(row.installs)}</span>
              </button>
            ))}
            {rows.length === 0 ? <p className="empty">No skill matches “{query}”.</p> : null}
          </div>
        ) : (
          <p className="empty">
            {view === "session"
              ? "Sessions live here from P3. Phase 1 wires the database behind it."
              : "This surface arrives later in the plan."}
          </p>
        )}
      </aside>

      <main className="detail">
        <header className="detail-head">
          <div>
            <p className="eyebrow">{project ? project.name : "No project open"}</p>
            <h2>{selected?.name ?? "Rigel"}</h2>
            <p className="lede">
              {selected?.description ??
                "One place where several coding agents work on the same project."}
            </p>
          </div>

          <div className="open-project">
            <input
              value={projectPath}
              onChange={(event) => setProjectPath(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") void openProject();
              }}
              placeholder="C:\\path\\to\\repo"
              aria-label="Project path"
              spellCheck={false}
            />
            <button className="primary" onClick={() => void openProject()}>
              Open
            </button>
          </div>
        </header>

        <p className="notice" role="status">
          {notice}
        </p>

        <section className="facts">
          <div>
            <span className="label">Database</span>
            <span className="value mono">{project?.db ?? "—"}</span>
          </div>
          <div>
            <span className="label">Sessions</span>
            <span className="value mono">{sessions.length}</span>
          </div>
          <div>
            <span className="label">Latest</span>
            <span className="value mono">
              {sessions[0] ? `${sessions[0].id.slice(0, 8)} · ${sessions[0].status}` : "—"}
            </span>
          </div>
          <div>
            <span className="label">Source</span>
            <span className="value mono">MockSkillSource (P2 replaces)</span>
          </div>
        </section>

        {selected ? (
          <section className="touches">
            <h3>What this touches</h3>
            <ul>
              <li>Reads: repository files, shell history</li>
              <li>Writes: none until approved</li>
              <li>Network: no</li>
              <li>Scripts bundled: none</li>
            </ul>
            <p className="fine">
              Static review cannot prove a skill is harmless. The first run executes in a sandbox
              with the network off.
            </p>
          </section>
        ) : null}
      </main>

      <footer className="status" aria-label="Status">
        <span className="status-model">rigel {isTauri() ? "desktop" : "browser"}</span>
        <span className="sep">·</span>
        <span className="mono">$0.00 · 0 tok</span>
        <span className="sep">·</span>
        <span>
          {rows.length} skills
        </span>
        <span className="sep">·</span>
        <span>
          {sessions.length} session{sessions.length === 1 ? "" : "s"}
        </span>
        <span className="grow" />
        <span className="metric" title="Interventions per completed task — the product metric (D17)">
          0/0 interventions/tasks
        </span>
      </footer>
    </div>
  );
}

function compact(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}m`;
  if (value >= 1_000) return `${Math.round(value / 1_000)}k`;
  return String(value);
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  api,
  compact,
  isTauri,
  when,
  TauriSkillSource,
  type ProjectInfo,
  type Session,
  type SkillDetail,
  type SkillRow,
  type SkillSource,
  type SourceStatus,
} from "./api";
import { RigelMark } from "./mark";
import { PreviewSkillSource } from "./mock";
import SessionView from "./session";

const ICONS = {
  shelf: "M10 2.5 17 6.5v7l-7 4-7-4v-7l7-4ZM3 6.5l7 4 7-4M10 10.5v7",
  session: "M3 4.5h14M3 9.5h14M3 14.5h9",
  diff: "M6 3v14M14 3v14M3 7h6M11 13h6",
  settings:
    "M10 12.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5ZM10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2",
} as const;

type View = "shelf" | "session" | "diff" | "settings";

const VIEWS: { id: View; label: string; d: string; phase: string }[] = [
  { id: "shelf", label: "Shelf", d: ICONS.shelf, phase: "P2" },
  { id: "session", label: "Session", d: ICONS.session, phase: "P3" },
  { id: "diff", label: "Diff", d: ICONS.diff, phase: "P5" },
  { id: "settings", label: "Settings", d: ICONS.settings, phase: "P3" },
];

const LIMIT = 100;

export default function App() {
  const source: SkillSource = useMemo(
    () => (isTauri() ? new TauriSkillSource() : new PreviewSkillSource()),
    [],
  );

  const [query, setQuery] = useState("");
  const [rows, setRows] = useState<SkillRow[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<SkillDetail | null>(null);
  const [statuses, setStatuses] = useState<SourceStatus[]>([]);
  const [reload, setReload] = useState(0);

  const [loading, setLoading] = useState(true);
  const [slow, setSlow] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string>(() =>
    isTauri() ? "Search the local index. Nothing leaves this machine." : "Browser preview — sample rows, no database.",
  );

  const [view, setView] = useState<View>("shelf");
  const [projectPath, setProjectPath] = useState("");
  const [project, setProject] = useState<ProjectInfo | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSession, setActiveSession] = useState<Session | null>(null);

  const setActive = useCallback((next: Session | null) => setActiveSession(next), []);
  const noticeFromSession = useCallback((next: string) => setNotice(next), []);

  const [repoInput, setRepoInput] = useState("");
  const [indexing, setIndexing] = useState(false);
  const searchRef = useRef<HTMLInputElement | null>(null);

  /* ----------------------------- data ----------------------------- */

  useEffect(() => {
    let live = true;
    setLoading(true);
    setSlow(false);
    const slowTimer = setTimeout(() => live && setSlow(true), 400);
    const timer = setTimeout(() => {
      source
        .search(query, LIMIT)
        .then((next) => {
          if (!live) return;
          setRows(next);
          setError(null);
          setSelectedId((current) =>
            current && next.some((row) => row.id === current) ? current : (next[0]?.id ?? null),
          );
        })
        .catch((problem: unknown) => {
          if (!live) return;
          setRows([]);
          setError(problem instanceof Error ? problem.message : String(problem));
        })
        .finally(() => {
          if (!live) return;
          setLoading(false);
          setSlow(false);
        });
    }, 120);

    return () => {
      live = false;
      clearTimeout(timer);
      clearTimeout(slowTimer);
    };
  }, [source, query, reload]);

  useEffect(() => {
    let live = true;
    source
      .status()
      .then((next) => live && setStatuses(next))
      .catch(() => live && setStatuses([]));
    return () => {
      live = false;
    };
  }, [source, reload]);

  useEffect(() => {
    let live = true;
    if (!selectedId) {
      setDetail(null);
      return;
    }
    source
      .load(selectedId)
      .then((next) => live && setDetail(next))
      .catch(() => live && setDetail(null));
    return () => {
      live = false;
    };
  }, [source, selectedId, reload]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "/" && document.activeElement?.tagName !== "INPUT") {
        event.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const selected = rows.find((row) => row.id === selectedId) ?? null;
  const shown = detail?.skill.id === selected?.id ? detail : null;

  /* ---------------------------- actions --------------------------- */

  const openProject = useCallback(async () => {
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
    } catch (problem) {
      setNotice(problem instanceof Error ? problem.message : String(problem));
    }
  }, [projectPath]);

  const indexRepo = useCallback(async () => {
    const repo = repoInput.trim();
    if (!repo || !source.indexRepo) return;
    setIndexing(true);
    setNotice(`Walking ${repo}…`);
    try {
      const report = await source.indexRepo(repo);
      setNotice(
        `${report.with_body} skills from ${report.repo ?? repo} · ${report.added} added · ${
          report.http_requests
        } requests`,
      );
      setRepoInput("");
      setReload((value) => value + 1);
    } catch (problem) {
      setNotice(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setIndexing(false);
    }
  }, [repoInput, source]);

  /* ---------------------------- banners --------------------------- */

  const offline = isTauri() && statuses.length > 0 && statuses.every((s) => s.state === "broken");
  const neverIndexed = isTauri() && statuses.length === 0 && rows.length === 0;
  const stale = statuses.filter((s) => s.state === "stale" && s.last_sync_at);

  const sessionProject = project?.root ?? (isTauri() ? "" : "/preview/rigel");

  return (
    <div className={`shell${view === "session" ? " session" : ""}`}>
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
        <div
          className={`rail-status ${statuses.some((s) => s.state === "broken") ? "bad" : ""}`}
          title={statuses.map((s) => `${s.id}: ${s.state}`).join(" · ") || "no sources yet"}
          aria-hidden
        >
          ●
        </div>
      </nav>

      {view === "session" ? (
        <SessionView
          project={sessionProject}
          onSessions={setSessions}
          onActive={setActive}
          onNotice={noticeFromSession}
        />
      ) : (
      <>
      <aside className="list-pane">
        <div className="list-head">
          <div className="list-title">
            <h1>{view === "shelf" ? "Shelf" : view[0].toUpperCase() + view.slice(1)}</h1>
            <span>{view === "shelf" ? `${rows.length} shown` : "Phase 2"}</span>
          </div>

          {view === "shelf" ? (
            <label className="search">
              <span aria-hidden>⌕</span>
              <input
                ref={searchRef}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="Search skills…"
                aria-label="Search skills"
              />
              <kbd>/</kbd>
            </label>
          ) : null}
        </div>

        {view === "shelf" && offline ? (
          <p className="banner muted" role="status">
            No source reachable. Showing the cached index.
          </p>
        ) : null}

        {view === "shelf" && !offline && stale.length > 0 ? (
          <p className="banner" role="status">
            {stale
              .map((s) => `${s.id} stale since ${when(s.last_sync_at)}`)
              .join(" · ")}
          </p>
        ) : null}

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
                <span
                  className={`grade ${row.grade ? `grade-${row.grade.toLowerCase()}` : "grade-none"}`}
                  title={row.grade ? `Grade ${row.grade} — static review only` : "Not scanned"}
                >
                  {row.grade ?? "—"}
                </span>
                <span className="row-text">
                  <span className="row-name">
                    {row.name}
                    {row.unmeasured ? (
                      <span className="tag unmeasured" title="Advertises a number nobody measured">
                        unmeasured
                      </span>
                    ) : null}
                    {row.variants > 1 ? (
                      <span className="tag" title="Near-identical rows are shown, never hidden">
                        {row.variants} variants
                      </span>
                    ) : null}
                  </span>
                  <span className="row-desc">{row.description}</span>
                </span>
                <span className="row-meta">
                  {row.installs !== null ? compact(row.installs) : compact(row.stars)}
                </span>
              </button>
            ))}

            {loading && !slow ? <p className="empty subtle">Searching the index…</p> : null}
            {loading && slow ? (
              <p className="empty subtle">
                Searching the index… taking longer than usual
                {rows.length > 0 ? " (showing the last result)" : ""}.
              </p>
            ) : null}
            {!loading && rows.length === 0 ? (
              <p className="empty">
                {error
                  ? error
                  : query
                    ? `No skill matches “${query}”.`
                    : neverIndexed
                      ? "The index is empty. Walk a repository to fill it."
                      : "Nothing here yet."}
              </p>
            ) : null}
          </div>
        ) : (
          <p className="empty">This surface arrives later in the plan.</p>
        )}
      </aside>

      <main className="detail">
        <header className="detail-head">
          <div>
            <p className="eyebrow">
              {project
                ? project.name
                : selected?.repo
                  ? selected.repo
                  : selected
                    ? selected.source
                    : "Rigel"}
            </p>
            <h2>{selected?.name ?? "Shelf"}</h2>
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
              placeholder="C:\path\to\repo"
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

        {!selected ? (
          <section className="onboarding">
            <h3>{isTauri() ? "The index is empty" : "Browser preview"}</h3>
            <p>
              {isTauri()
                ? "Rigel keeps one global index of skills it has walked. Point it at a repository once and it stays searchable offline."
                : "This build has no Tauri host, so the rows on the left are a fixed sample. Open the desktop app to search a real index."}
            </p>
            {source.indexRepo ? (
              <div className="index-form">
                <input
                  value={repoInput}
                  onChange={(event) => setRepoInput(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void indexRepo();
                  }}
                  placeholder="owner/repo or a GitHub URL"
                  aria-label="Repository to index"
                  spellCheck={false}
                  disabled={indexing}
                />
                <button className="primary" onClick={() => void indexRepo()} disabled={indexing}>
                  {indexing ? "Walking…" : "Index repository"}
                </button>
              </div>
            ) : null}
          </section>
        ) : null}

        {selected ? (
          <>
            <section className="facts">
              <div>
                <span className="label">Grade</span>
                <span className="value mono">
                  {selected.grade ?? "not scanned"}
                  {selected.grade_scope ? ` · ${selected.grade_scope}` : ""}
                </span>
              </div>
              <div>
                <span className="label">Source</span>
                <span className="value mono">{selected.source}</span>
              </div>
              <div>
                <span className="label">Stars</span>
                <span className="value mono">{compact(selected.stars)}</span>
              </div>
              <div>
                <span className="label">Tokens</span>
                <span className="value mono">~{compact(selected.tokens)}</span>
              </div>
              <div>
                <span className="label">Last seen</span>
                <span className="value mono">{when(selected.last_seen_at)}</span>
              </div>
              <div>
                <span className="label">Body</span>
                <span className="value mono">{selected.has_body ? "indexed" : "not fetched"}</span>
              </div>
            </section>

            {shown && shown.evidence.length > 0 ? (
              <section className="evidence">
                <h3>Evidence</h3>
                <table>
                  <thead>
                    <tr>
                      <th>Metric</th>
                      <th>Measured</th>
                      <th>Advertised</th>
                      <th>Source</th>
                    </tr>
                  </thead>
                  <tbody>
                    {shown.evidence.map((row) => (
                      <tr key={`${row.metric}-${row.source}`}>
                        <td>{row.metric}</td>
                        <td className={row.measured === null ? "missing" : "mono"}>
                          {row.measured === null ? "unmeasured" : `${row.measured}%`}
                        </td>
                        <td className="mono">
                          {row.advertised === null ? "—" : `${row.advertised}%`}
                        </td>
                        <td>
                          {row.source}
                          {row.source_url ? (
                            <>
                              {" · "}
                              <a href={row.source_url} target="_blank" rel="noreferrer">
                                link
                              </a>
                            </>
                          ) : null}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <p className="fine">
                  Advertised numbers come from the skill's own description. A row that says
                  unmeasured has no independent measurement in this index.
                </p>
              </section>
            ) : null}

            {shown && shown.capabilities.length > 0 ? (
              <section className="touches">
                <h3>Static review</h3>
                <ul>
                  {shown.capabilities.map((capability) => (
                    <li key={capability.label} className={`cap cap-${capability.severity}`}>
                      <span className="sev">{capability.severity}</span>
                      {capability.label}
                      {capability.evidence.map((line) => (
                        <code key={`${line.file}:${line.line}`}>
                          {line.file}:{line.line} {line.code}
                        </code>
                      ))}
                    </li>
                  ))}
                </ul>
                <p className="fine">
                  No dangerous capabilities detected in static review is not the same as safe.
                  Only the markdown above was read; scripts it tells an agent to run later are
                  outside this review.
                </p>
              </section>
            ) : null}

            {shown && shown.variants.length > 0 ? (
              <section className="variants">
                <h3>
                  {shown.variants.length + 1} near-identical variants · showing every one
                </h3>
                <ul>
                  {shown.variants.map((variant) => (
                    <li key={variant.id}>
                      <button className="link" onClick={() => setSelectedId(variant.id)}>
                        {variant.name}
                      </button>
                      <span className="variant-repo">{variant.repo ?? "unknown repo"}</span>
                      <span className="variant-delta">{variant.delta}</span>
                      <span className="variant-seen">{when(variant.last_seen_at)}</span>
                    </li>
                  ))}
                </ul>
              </section>
            ) : null}

            {selected.url ? (
              <p className="fine">
                <a href={selected.url} target="_blank" rel="noreferrer">
                  Open the source file
                </a>
                {selected.last_seen_at ? ` · last seen ${when(selected.last_seen_at)}` : ""}
              </p>
            ) : null}
          </>
        ) : null}
      </main>
      </>
      )}

      <footer className="status" aria-label="Status">
        <span className="status-model">
          rigel {isTauri() ? "desktop" : "browser"}
          {view === "session" && activeSession ? " · native" : ""}
        </span>
        <span className="sep">·</span>
        <span className="mono">
          {view === "session" && activeSession
            ? `$${(activeSession.spend_micros / 1_000_000).toFixed(4)}`
            : "$0.00 · 0 tok"}
        </span>
        <span className="sep">·</span>
        <span>
          {view === "session" && activeSession
            ? `session ${activeSession.id.slice(0, 8)}`
            : `${rows.length} skills`}
        </span>
        <span className="sep">·</span>
        <span>
          {sessions.length} session{sessions.length === 1 ? "" : "s"}
        </span>
        <span className="sep">·</span>
        <span className="mono" title={`source: ${source.label}`}>
          {statuses.length > 0
            ? statuses.map((s) => `${s.id} ${s.state}`).join(" · ")
            : isTauri()
              ? "no sources"
              : "sample"}
        </span>
        <span className="grow" />
        <span className="metric" title="Interventions per completed task — the product metric (D17)">
          {activeSession
            ? `${activeSession.interventions}/${activeSession.tasks_done} interventions/tasks`
            : "0/0 interventions/tasks"}
        </span>
      </footer>
    </div>
  );
}

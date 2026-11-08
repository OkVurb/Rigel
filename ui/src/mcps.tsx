import { useCallback, useEffect, useRef, useState } from "react";

import { api, isTauri, type McpServer } from "./api";

export type ShelfKind = "skills" | "mcps";

export function ShelfTabs({
  kind,
  onKind,
}: {
  kind: ShelfKind;
  onKind: (kind: ShelfKind) => void;
}) {
  return (
    <div className="shelf-tabs" role="tablist" aria-label="Shelf catalogue">
      <button
        type="button"
        role="tab"
        aria-selected={kind === "skills"}
        className={kind === "skills" ? "on" : ""}
        onClick={() => onKind("skills")}
      >
        Skills
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={kind === "mcps"}
        className={kind === "mcps" ? "on" : ""}
        onClick={() => onKind("mcps")}
      >
        MCPs
      </button>
    </div>
  );
}

const PAGE = 250;
const PREVIEW: McpServer[] = [
  {
    id: "preview:chrome-devtools",
    name: "Chrome DevTools",
    qualified: "ChromeDevTools/chrome-devtools-mcp",
    description: "Drive a real Chrome tab: inspect the DOM, read console errors and capture network calls.",
    repo: "ChromeDevTools/chrome-devtools-mcp",
    homepage: "https://github.com/ChromeDevTools/chrome-devtools-mcp",
    source: "registry",
    sources: ["registry"],
    transport: "package",
    install: "npm:chrome-devtools-mcp",
    version: "0.8.0",
    installable: true,
  },
  {
    id: "preview:context7",
    name: "Context7",
    qualified: "upstash/context7",
    description: "Pull current library documentation into the agent instead of relying on its training cutoff.",
    repo: "upstash/context7",
    homepage: "https://mcp.context7.com/mcp",
    source: "registry",
    sources: ["registry"],
    transport: "streamable-http",
    install: "",
    version: "1.0.0",
    installable: true,
  },
  {
    id: "preview:faf",
    name: ".FAF Context",
    qualified: "io.github.Wolfe-Jam/faf-mcp",
    description: "Persistent project context shared by Cursor, Windsurf, Cline and VS Code.",
    repo: "wolfe-jam/faf-mcp",
    homepage: "https://github.com/wolfe-jam/faf-mcp",
    source: "registry",
    sources: ["registry", "awesome"],
    transport: "package",
    install: "npm:faf-mcp",
    version: "2.1.3",
    installable: true,
  },
  {
    id: "preview:notion",
    name: "Notion",
    qualified: "makenotion/notion-mcp-server",
    description: "Read and update Notion pages. This entry needs an integration token before it can run.",
    repo: "makenotion/notion-mcp-server",
    homepage: "https://mcp.notion.com/mcp?token={integration_token}",
    source: "registry",
    sources: ["registry"],
    transport: "streamable-http",
    install: "",
    version: "1.0.0",
    installable: false,
  },
];

function previewSearch(query: string): McpServer[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return PREVIEW;
  return PREVIEW.filter((server) =>
    [server.name, server.qualified, server.description, server.repo].some((value) =>
      value.toLowerCase().includes(needle),
    ),
  );
}

interface Props {
  project: string | null;
  kind: ShelfKind;
  onKind: (kind: ShelfKind) => void;
}

export default function McpShelf({ project, kind, onKind }: Props) {
  const [query, setQuery] = useState("");
  const [rows, setRows] = useState<McpServer[]>([]);
  const [total, setTotal] = useState<number | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const loadingMoreRef = useRef(false);
  const queryVersionRef = useRef(0);
  const [error, setError] = useState<string | null>(null);
  const [installed, setInstalled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState(
    "Choose a server. Installing writes only to the open project.",
  );

  useEffect(() => {
    let live = true;
    queryVersionRef.current += 1;
    setLoading(true);
    setLoadingMore(false);
    loadingMoreRef.current = false;
    setRows([]);
    setTotal(null);
    setError(null);
    const timer = window.setTimeout(() => {
      if (!isTauri()) {
        const next = previewSearch(query);
        if (!live) return;
        setRows(next);
        setTotal(next.length);
        setSelectedId((current) =>
          current && next.some((server) => server.id === current) ? current : (next[0]?.id ?? null),
        );
        setLoading(false);
        return;
      }
      Promise.all([api.mcpCatalog.count(query), api.mcpCatalog.search(query, PAGE, 0)])
        .then(([count, next]) => {
          if (!live) return;
          setTotal(count);
          setRows(next);
          setSelectedId((current) =>
            current && next.some((server) => server.id === current)
              ? current
              : (next[0]?.id ?? null),
          );
        })
        .catch((problem: unknown) => {
          if (!live) return;
          setRows([]);
          setTotal(0);
          setError(problem instanceof Error ? problem.message : String(problem));
        })
        .finally(() => {
          if (live) setLoading(false);
        });
    }, 120);

    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [query]);

  const loadMore = useCallback(async () => {
    if (
      !isTauri() ||
      loadingMoreRef.current ||
      loading ||
      error ||
      total === null ||
      rows.length >= total
    ) {
      return;
    }
    const offset = rows.length;
    const version = queryVersionRef.current;
    loadingMoreRef.current = true;
    setLoadingMore(true);
    try {
      const next = await api.mcpCatalog.search(query, PAGE, offset);
      if (queryVersionRef.current === version) {
        setRows((current) => (current.length === offset ? [...current, ...next] : current));
      }
    } catch (problem) {
      if (queryVersionRef.current === version) {
        setError(problem instanceof Error ? problem.message : String(problem));
      }
    } finally {
      if (queryVersionRef.current === version) {
        loadingMoreRef.current = false;
        setLoadingMore(false);
      }
    }
  }, [error, loading, query, rows.length, total]);

  const selected = rows.find((server) => server.id === selectedId) ?? null;

  useEffect(() => {
    let live = true;
    setInstalled(false);
    setFeedback("Choose a server. Installing writes only to the open project.");
    if (!selectedId || !project || !isTauri()) {
      return () => {
        live = false;
      };
    }
    api.mcpCatalog
      .installed(project, selectedId)
      .then((next) => live && setInstalled(next))
      .catch(() => live && setInstalled(false));
    return () => {
      live = false;
    };
  }, [project, selectedId]);

  const blocked = !isTauri()
    ? "MCP installs run in the desktop app."
    : !project
      ? "Open a project before installing an MCP server."
      : !selected
        ? "Choose an MCP server."
        : !selected.installable
          ? "This entry has no launch command the app can install without guessing."
          : "";

  const install = useCallback(async () => {
    if (!selected || !project || blocked) return;
    setBusy(true);
    setFeedback("Installing…");
    try {
      const report = await api.mcpCatalog.install(project, selected.id);
      setInstalled(true);
      setFeedback(
        `${report.name} installed as ${report.entry} in ${report.path}${
          report.created ? "" : " (already present)"
        }.`,
      );
    } catch (problem) {
      setFeedback(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  }, [blocked, project, selected]);

  return (
    <>
      <aside className="list-pane">
        <div className="list-head">
          <div className="list-title">
            <h1>Shelf</h1>
            <span>{total === null ? "…" : `${total.toLocaleString()} MCPs`}</span>
          </div>

          <ShelfTabs kind={kind} onKind={onKind} />

          <label className="search">
            <span aria-hidden>⌕</span>
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search MCP servers…"
              aria-label="Search MCP servers"
              spellCheck={false}
            />
            <kbd>/</kbd>
          </label>
        </div>

        {error ? (
          <p className="banner muted" role="status">
            {error}
          </p>
        ) : null}

        <div
          className="rows"
          role="listbox"
          aria-label="MCP servers"
          onScroll={(event) => {
            const pane = event.currentTarget;
            if (pane.scrollHeight - pane.scrollTop - pane.clientHeight < 400) {
              void loadMore();
            }
          }}
        >
          {rows.map((server) => (
            <button
              key={server.id}
              role="option"
              aria-selected={selected?.id === server.id}
              className={`row${selected?.id === server.id ? " on" : ""}`}
              onClick={() => setSelectedId(server.id)}
            >
              <span className="mcp-mark" aria-hidden>
                MCP
              </span>
              <span className="row-text">
                <span className="row-name">
                  {server.name}
                  {!server.installable ? <span className="tag">manual setup</span> : null}
                </span>
                <span className="row-desc">{server.description || server.qualified}</span>
              </span>
              <span className="row-meta">{server.source || "catalog"}</span>
            </button>
          ))}

          {loading ? <p className="empty subtle">Searching the MCP catalogue…</p> : null}
          {!loading && rows.length === 0 ? (
            <p className="empty">
              {error
                ? error
                : query
                  ? `No MCP server matches “${query}”.`
                  : "The catalogue is empty."}
            </p>
          ) : null}
          {loadingMore ? <p className="empty subtle">Loading more…</p> : null}
          {!loading && !loadingMore && error === null && total !== null && rows.length < total ? (
            <button className="load-more" type="button" onClick={() => void loadMore()}>
              Load more · {rows.length.toLocaleString()} of {total.toLocaleString()}
            </button>
          ) : null}
          {!loading && total !== null && total > PAGE && rows.length >= total ? (
            <p className="empty subtle">All {total.toLocaleString()} MCPs shown.</p>
          ) : null}
        </div>
      </aside>

      <main id="main" tabIndex={-1} className="detail">
        <header className="detail-head">
          <div>
            <p className="eyebrow">
              MCP catalogue{selected?.source ? ` · ${selected.source}` : ""}
            </p>
            <h2>{selected?.name ?? "MCP servers"}</h2>
            <p className="lede">
              {selected?.description ??
                "Server entries for the agents Rigel launches. Search the embedded registry catalogue and install one into the open project."}
            </p>
          </div>

          <div className="detail-actions">
            <button
              className="primary"
              type="button"
              onClick={() => void install()}
              disabled={busy || installed || Boolean(blocked)}
              title={blocked || "Write this server into <project>/.mcp.json"}
            >
              {busy ? "Installing…" : installed ? "✓ Installed" : "Install MCP"}
            </button>
          </div>
        </header>

        <p className="notice" role="status">
          {feedback}
        </p>

        {selected ? (
          <>
            <section className="facts">
              <div>
                <span className="label">Source</span>
                <span className="value mono">{selected.source || "—"}</span>
              </div>
              <div>
                <span className="label">Transport</span>
                <span className="value mono">{selected.transport || "—"}</span>
              </div>
              <div>
                <span className="label">Version</span>
                <span className="value mono">{selected.version || "—"}</span>
              </div>
              <div>
                <span className="label">Repository</span>
                <span className="value mono">{selected.repo || "—"}</span>
              </div>
              <div>
                <span className="label">Catalogue id</span>
                <span className="value mono">{selected.qualified || selected.id}</span>
              </div>
              <div>
                <span className="label">Launch data</span>
                <span className="value mono">{selected.install || "remote URL"}</span>
              </div>
            </section>

            <section className="onboarding">
              <h3>{selected.installable ? "Install target" : "Manual setup needed"}</h3>
              {selected.installable ? (
                <p>
                  Writes one <code>mcpServers</code> entry to{" "}
                  <code>{project ? `${project}/.mcp.json` : "<project>/.mcp.json"}</code>.
                </p>
              ) : (
                <p>
                  This catalogue entry has no launch command the app can write without guessing.
                  Open its source page for the required arguments or credentials.
                </p>
              )}
              <p className="fine">
                Claude Code and Rigel&apos;s Claude agents read <code>.mcp.json</code>. Codex and
                Cursor keep their own MCP settings; this button does not edit them.
              </p>
            </section>

            {selected.homepage || selected.repo ? (
              <p className="fine">
                {selected.homepage ? (
                  <a href={selected.homepage} target="_blank" rel="noreferrer">
                    Open the server page
                  </a>
                ) : null}
                {selected.homepage && selected.repo ? " · " : null}
                {selected.repo ? <code>{selected.repo}</code> : null}
              </p>
            ) : null}
          </>
        ) : (
          <section className="onboarding">
            <h3>{loading ? "Reading the catalogue" : "No MCP server selected"}</h3>
            <p>
              {loading
                ? "The embedded registry snapshot is loading."
                : "Search the left pane, or clear the query to browse the catalogue."}
            </p>
          </section>
        )}
      </main>
    </>
  );
}

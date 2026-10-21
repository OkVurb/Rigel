import { useEffect, useMemo, useState } from "react";
import type { Grade, Package } from "../data";
import type { IndexEntry, RepoIndex, RepoMeta } from "../github";
import { fetchRepoIndex, loadSkill, toPackage } from "../github";
import type { HubEntry, HubLoaded } from "../hub";
import { browseSkillHub, loadHubSkill, searchSkillsMP } from "../hub";
import { AgentActions, Chip, GradeBadge, Kbd } from "./ui";
import { downloadText, entriesText, saveEntries } from "../db";
import { SourceTabs } from "./SourceTabs";
import { DepsPanel } from "./DepsPanel";
import { deps } from "../deps";
import type { SourceTab } from "../sources";
import installed from "../installed.json";

const SEEDS = [
  { input: "affaan-m/ECC", label: "ECC" },
  { input: "anthropics/skills", label: "anthropics" },
  { input: "github/awesome-copilot", label: "copilot" },
  { input: "davila7/claude-code-templates", label: "templates" },
];

const PER_REPO = 120;
const CACHE_TTL = 60 * 60 * 1000;
const onDisk = new Set<string>(installed.names as string[]);
const bytes = (n: number) => (n >= 1024 ? `${(n / 1024).toFixed(1)} KB` : `${n} B`);

type Loadable = { status: "idle" } | { status: "loading" } | { status: "ready"; entries: HubEntry[] } | { status: "error"; error: string };

function readCache(input: string): RepoIndex | null {
  try {
    const raw = sessionStorage.getItem(`ghidx:${input}`);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as { at: number; index: RepoIndex };
    return Date.now() - parsed.at < CACHE_TTL ? parsed.index : null;
  } catch {
    return null;
  }
}

function writeCache(input: string, index: RepoIndex) {
  try {
    sessionStorage.setItem(`ghidx:${input}`, JSON.stringify({ at: Date.now(), index }));
  } catch {
    /* quota */
  }
}

interface Row {
  key: string;
  name: string;
  sub: string;
  meta: string;
  installed: boolean;
  grade?: Grade;
  github?: IndexEntry;
  hub?: HubEntry;
  origin?: string;
}

interface Props {
  selectedKey: string | null;
  onSelect: (key: string) => void;
  onAdd: (pkg: Package) => void;
  catalogue: Package[];
  onOpenImport: () => void;
}

export function DiscoverView({ selectedKey, onSelect, onAdd, catalogue, onOpenImport }: Props) {
  const [source, setSource] = useState<SourceTab>("all");
  const [query, setQuery] = useState("");
  const [repoFilter, setRepoFilter] = useState("all");
  const [hideInstalled, setHideInstalled] = useState(true);

  const [repos, setRepos] = useState<{ input: string; label: string; status: "loading" | "ready" | "error"; index?: RepoIndex; error?: string }[]>(
    SEEDS.map((s) => ({ input: s.input, label: s.label, status: "loading" })),
  );
  const [mp, setMp] = useState<Loadable>({ status: "idle" });
  const [mpQuery, setMpQuery] = useState("agent");
  const [hub, setHub] = useState<Loadable>({ status: "idle" });

  const [loaded, setLoaded] = useState<Record<string, HubLoaded>>({});
  const [loadingKey, setLoadingKey] = useState<string | null>(null);
  const [detailError, setDetailError] = useState("");
  const [addedKeys, setAddedKeys] = useState<Set<string>>(new Set());
  const [dbCount, setDbCount] = useState(0);

  /* ---- github indexes ---- */
  useEffect(() => {
    let cancelled = false;
    SEEDS.forEach((seed) => {
      const cachedIndex = readCache(seed.input);
      if (cachedIndex) {
        setRepos((prev) => prev.map((r) => (r.input === seed.input ? { ...r, status: "ready", index: cachedIndex } : r)));
        return;
      }
      fetchRepoIndex(seed.input, PER_REPO)
        .then((index) => {
          if (cancelled) return;
          writeCache(seed.input, index);
          setRepos((prev) => prev.map((r) => (r.input === seed.input ? { ...r, status: "ready", index } : r)));
        })
        .catch((e: unknown) => {
          if (cancelled) return;
          setRepos((prev) =>
            prev.map((r) => (r.input === seed.input ? { ...r, status: "error", error: e instanceof Error ? e.message : "failed" } : r)),
          );
        });
    });
    return () => {
      cancelled = true;
    };
  }, []);

  /* ---- marketplaces, loaded on first visit ---- */
  const loadMp = (q: string) => {
    setMp({ status: "loading" });
    searchSkillsMP(q)
      .then((entries) => setMp({ status: "ready", entries }))
      .catch((e: unknown) => setMp({ status: "error", error: e instanceof Error ? e.message : "failed" }));
  };

  useEffect(() => {
    if ((source === "skillsmp" || source === "all") && mp.status === "idle") loadMp(mpQuery);
    if ((source === "skillhub" || source === "all") && hub.status === "idle") {
      setHub({ status: "loading" });
      browseSkillHub()
        .then((entries) => setHub({ status: "ready", entries }))
        .catch((e: unknown) => setHub({ status: "error", error: e instanceof Error ? e.message : "failed" }));
    }
  }, [source, mp.status, hub.status, mpQuery]);

  const catalogueNames = useMemo(() => new Set(catalogue.map((p) => p.name.toLowerCase())), [catalogue]);
  const isInstalled = (name: string) => onDisk.has(name.toLowerCase()) || catalogueNames.has(name.toLowerCase());

  const githubEntries = useMemo(
    () => repos.flatMap((r) => (r.status === "ready" && r.index ? r.index.entries : [])),
    [repos],
  );

  const repoFor = (entry: IndexEntry): RepoMeta | null => {
    const r = repos.find((x) => x.status === "ready" && x.index?.entries.some((e) => e.key === entry.key));
    return r?.index?.repo ?? null;
  };

  const rows: Row[] = useMemo(() => {
    const q = query.trim().toLowerCase();
    const match = (hay: string[]) => q.length === 0 || hay.some((h) => h.toLowerCase().includes(q));

    const out: Row[] = [];

    if (source === "github" || source === "all") {
      for (const e of githubEntries) {
        if (repoFilter !== "all" && e.owner !== repoFilter) continue;
        if (!match([e.name, e.repo, e.path])) continue;
        out.push({
          key: e.key,
          name: e.name,
          sub: `${e.repo} · ${e.path.replace(/\/SKILL\.md$/, "")}`,
          meta: bytes(e.size),
          installed: isInstalled(e.name),
          grade: loaded[e.key]?.skill.grade,
          github: e,
          origin: "github",
        });
      }
    }

    const hubList = [
      ...((source === "skillsmp" || source === "all") && mp.status === "ready" ? mp.entries : []),
      ...((source === "skillhub" || source === "all") && hub.status === "ready" ? hub.entries : []),
    ];

    for (const h of hubList) {
      if (!match([h.name, h.author, h.description])) continue;
      out.push({
        key: h.key,
        name: h.name,
        sub: `${h.author}${h.shield ? ` · ${h.shield} audits` : ""}${h.installs ? ` · ${h.installs} installs` : ""}`,
        meta: h.stars ? `${(h.stars / 1000).toFixed(0)}k★` : (h.installs ?? ""),
        installed: isInstalled(h.name),
        grade: loaded[h.key]?.skill.grade,
        hub: h,
        origin: h.source,
      });
    }

    return out;
  }, [source, githubEntries, mp, hub, query, repoFilter, catalogueNames, loaded]);

  const visible = rows.filter((r) => (hideInstalled ? !r.installed : true));
  // no auto-selection: a body fetch should only happen on an explicit click
  const selected = rows.find((r) => r.key === selectedKey) ?? null;

  /* ---- lazy detail ---- */
  useEffect(() => {
    if (!selected || loaded[selected.key] || loadingKey === selected.key) return;
    const key = selected.key;
    setLoadingKey(key);
    setDetailError("");

    const resolve = async (): Promise<HubLoaded | null> => {
      if (selected.github) {
        const repo = repoFor(selected.github);
        if (!repo) return null;
        return { skill: await loadSkill(selected.github), repo };
      }
      if (selected.hub) return loadHubSkill(selected.hub);
      return null;
    };

    resolve()
      .then((res) => {
        if (!res) return;
        setLoaded((prev) => ({ ...prev, [key]: res }));
      })
      .catch((e: unknown) => setDetailError(e instanceof Error ? e.message : "Could not load SKILL.md"))
      .finally(() => setLoadingKey(null));
  }, [selected, loaded, loadingKey]);

  /* metadata-only index: descriptions and provenance, never bodies */
  useEffect(() => {
    if (rows.length === 0) return;
    const saved = saveEntries(
      rows.map((r) => ({
        key: r.key,
        name: r.name,
        description: r.hub?.description ?? r.sub,
        source: r.github ? `github:${r.github.repo}` : (r.hub?.source ?? "unknown"),
        url: r.github?.url ?? r.hub?.url ?? "",
        grade: r.grade,
        stars: r.hub?.stars,
        installs: r.hub?.installs,
        path: r.github?.path,
        savedAt: Date.now(),
      })),
    );
    setDbCount(saved.total);
  }, [rows]);

  const detail = selected ? loaded[selected.key] : undefined;
  const already = selected ? addedKeys.has(selected.key) || isInstalled(selected.name) : false;

  const notOnDisk = rows.filter((r) => !r.installed).length;
  const sourceNote =
    source === "github"
      ? `${githubEntries.filter((e) => !isInstalled(e.name)).length} of ${githubEntries.length} not on disk · diffed against ${installed.count.toLocaleString()} local skills`
      : source === "skillsmp"
        ? `SkillsMP · ${mp.status === "ready" ? mp.entries.length : 0} results · 50 anonymous requests/day`
        : source === "skillhub"
          ? `SkillHub · ${hub.status === "ready" ? hub.entries.length : 0} skills · scraped through the dev proxy`
          : source === "all"
            ? `${rows.length} skills from ${3} sources · ${notOnDisk} not on disk`
            : `${deps.length} tools`;

  if (source === "deps") {
    return <DepsPanel source={source} onSource={setSource} selectedId={selectedKey} onSelect={onSelect} />;
  }

  return (
    <>
      <div className="list-pane">
        <div className="list-head">
          <div className="list-title">
            <h1>Discover</h1>
            <span>{visible.length} shown</span>
          </div>

          <SourceTabs source={source} onSource={setSource} />

          <div className="filters">
            <button className={`filter${hideInstalled ? " on" : ""}`} onClick={() => setHideInstalled((v) => !v)}>
              {hideInstalled ? "on disk hidden" : "on disk shown"}
            </button>
          </div>

          {source === "skillsmp" ? (
            <label className="search">
              <span aria-hidden style={{ color: "var(--text-faint)", fontSize: 12 }}>
                ⌕
              </span>
              <input
                value={mpQuery}
                placeholder="Search 3M+ SkillsMP skills…"
                onChange={(e) => setMpQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && mpQuery.trim()) loadMp(mpQuery.trim());
                }}
                aria-label="Search SkillsMP"
              />
              <Kbd>↵</Kbd>
            </label>
          ) : (
            <label className="search">
              <span aria-hidden style={{ color: "var(--text-faint)", fontSize: 12 }}>
                ⌕
              </span>
              <input
                value={query}
                placeholder={source === "all" ? "Search every source…" : source === "skillhub" ? "Search SkillHub…" : "Search GitHub skills…"}
                onChange={(e) => setQuery(e.target.value)}
                aria-label="Search skills"
              />
              <Kbd>/</Kbd>
            </label>
          )}

          {source === "github" && (
            <div className="filters">
              <button className={`filter${repoFilter === "all" ? " on" : ""}`} onClick={() => setRepoFilter("all")}>
                All repos
              </button>
              {SEEDS.map((s) => (
                <button
                  key={s.input}
                  className={`filter${repoFilter === s.input.split("/")[0] ? " on" : ""}`}
                  onClick={() => setRepoFilter(s.input.split("/")[0])}
                >
                  {s.label}
                </button>
              ))}
            </div>
          )}

          <div className="target-note">{sourceNote}</div>
          <div className="head-actions">
            <button className="import-open" onClick={onOpenImport}>
              ＋ Add by URL
            </button>
            <button
              className="import-open"
              title="Metadata-only index of everything discovered so far"
              onClick={() => downloadText("skill-index.txt", entriesText())}
            >
              ⤓ index{dbCount ? ` · ${dbCount}` : ""}
            </button>
          </div>
        </div>

        <div className="list" role="listbox" aria-label="Discovered skills">
          {source === "github" &&
            repos.map((r) =>
              r.status === "loading" ? (
                <div className="discover-status" key={r.input}>
                  <span className="spinner" /> reading {r.input}…
                </div>
              ) : r.status === "error" ? (
                <div className="discover-status error" key={r.input}>
                  {r.input}: {r.error}
                </div>
              ) : null,
            )}

          {source !== "github" && (source === "skillsmp" ? mp : hub).status === "loading" && (
            <div className="discover-status">
              <span className="spinner" /> contacting {source === "skillsmp" ? "SkillsMP" : "SkillHub"}…
            </div>
          )}
          {source !== "github" && (source === "skillsmp" ? mp : hub).status === "error" && (
            <div className="discover-status error">
              {(source === "skillsmp" ? mp : hub).status === "error"
                ? (source === "skillsmp" ? (mp as { error: string }).error : (hub as { error: string }).error)
                : null}
            </div>
          )}

          {visible.length === 0 && (
            <div className="empty" style={{ paddingTop: 32, height: "auto" }}>
              <strong>Nothing to show</strong>
              <span>{hideInstalled ? "Everything here is already on disk — switch to “all”." : "No matches for that search."}</span>
            </div>
          )}

          {visible.map((row) => (
            <button
              key={row.key}
              role="option"
              aria-selected={selected?.key === row.key}
              className={`row${selected?.key === row.key ? " active" : ""}`}
              onClick={() => onSelect(row.key)}
            >
              <span className="row-name">
                {row.grade ? <GradeBadge grade={row.grade} /> : <span className="grade grade-unknown">?</span>}
                <span className="name">{row.name}</span>
                {row.installed && <Chip tone="curated">on disk</Chip>}
                {source === "all" && row.origin && <Chip>{row.origin}</Chip>}
                {addedKeys.has(row.key) && <Chip tone="clone">added</Chip>}
              </span>
              <span style={{ fontSize: 10.5, color: "var(--text-faint)", fontFamily: "var(--font-mono)" }}>{row.meta}</span>
              <span className="row-desc">{row.sub}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="detail">
        {!selected && (
          <div className="empty">
            <strong>Pick a skill</strong>
            <span>Indexes load first; SKILL.md bodies load on selection.</span>
          </div>
        )}

        {selected && (
          <>
            <div className="detail-head">
              <div className="detail-top">
                <div style={{ minWidth: 0 }}>
                  <div className="detail-id">{selected.sub}</div>
                  <h2 className="detail-title">{detail?.skill.name ?? selected.name}</h2>
                  <div className="detail-desc" style={{ marginTop: 4 }}>
                    {detail?.skill.description ?? (detailError || "Fetching SKILL.md…")}
                  </div>
                </div>

                <div className="head-actions">
                  {detail && <AgentActions pkg={toPackage(detail.skill, detail.repo)} />}
                  <button
                    className={`install${already ? " installed" : ""}`}
                    disabled={already || !detail}
                    onClick={() => {
                      if (!detail || !selected) return;
                      onAdd(toPackage(detail.skill, detail.repo));
                      setAddedKeys((prev) => new Set(prev).add(selected.key));
                    }}
                  >
                    {already ? "✓ In catalogue" : "Add to catalogue"}
                  </button>
                </div>
              </div>

              <div className="facts">
                <span>
                  source <b>{selected.hub ? selected.hub.source : "github"}</b>
                </span>
                {detail && (
                  <>
                    <span>
                      repo <b>{detail.repo.fullName}</b>
                    </span>
                    <span>
                      checks <b>{detail.skill.capabilities.length}</b>
                    </span>
                    <span>
                      context <b>~{detail.skill.tokens.toLocaleString()} tok</b>
                    </span>
                  </>
                )}
                {selected.hub?.shield && (
                  <span>
                    audits <b>{selected.hub.shield}</b>
                  </span>
                )}
                {selected.hub?.installs && (
                  <span>
                    installs <b>{selected.hub.installs}</b>
                  </span>
                )}
                <a
                  href={selected.github?.url ?? selected.hub?.url}
                  target="_blank"
                  rel="noreferrer"
                  style={{ color: "var(--info)", textDecoration: "none" }}
                >
                  open source ↗
                </a>
              </div>
            </div>

            {!detail && (
              <div className="detail-body">
                <div className="doc">
                  <p style={{ color: detailError ? "var(--danger)" : "var(--text-faint)" }}>
                    {detailError || `Reading ${selected.github?.path ?? selected.hub?.name ?? "…"}…`}
                  </p>
                  {selected.hub && (
                    <p style={{ fontSize: 11.5 }}>
                      Marketplaces are indexed through the dev proxy; the SKILL.md itself is fetched and scanned
                      like any GitHub import.
                    </p>
                  )}
                </div>
                <aside className="side" />
              </div>
            )}

            {detail && (
              <div className="detail-body">
                <div className="doc">
                  <div className="frontmatter">
                    {"---\n"}
                    <div>
                      <span className="key">name</span>: {detail.skill.name}
                    </div>
                    <div>
                      <span className="key">description</span>: {detail.skill.description}
                    </div>
                    <div>
                      <span className="key">allowed-tools</span>: {detail.skill.allowedTools?.join(", ") ?? "not declared"}
                    </div>
                    {"---"}
                  </div>

                  {detail.skill.doc.map((block, i) => {
                    if ("h" in block) return <h2 key={i}>{block.h}</h2>;
                    if ("p" in block) return <p key={i}>{block.p}</p>;
                    return (
                      <ul key={i}>
                        {block.ul.map((li) => (
                          <li key={li}>{li}</li>
                        ))}
                      </ul>
                    );
                  })}
                </div>

                <aside className="side">
                  <section className="side-section">
                    <div className="side-label">
                      <span>What this touches</span>
                      <span>{detail.skill.capabilities.length} checks</span>
                    </div>

                    {detail.skill.capabilities.map((cap) => (
                      <div className="cap" key={cap.label}>
                        <div className="cap-head">
                          <span className={`cap-dot ${cap.severity}`} />
                          {cap.label}
                        </div>
                        {cap.evidence.map((ev) => (
                          <div className="cap-ev" key={`${ev.file}:${ev.line}`}>
                            <span className="loc">
                              {ev.file.split("/").pop()}:{ev.line}
                            </span>
                            <span className="code" title={ev.code}>
                              {ev.code}
                            </span>
                          </div>
                        ))}
                      </div>
                    ))}

                    <div className="evidence-note" style={{ border: "1px solid var(--hairline)", borderRadius: 6 }}>
                      One file reviewed. Anything this skill later tells the agent to run is outside the scan.
                    </div>
                  </section>

                  <section className="side-section">
                    <div className="side-label">
                      <span>Provenance</span>
                    </div>
                    <div className="stat-list">
                      <div>
                        <b>{detail.repo.fullName}</b>
                      </div>
                      <div>
                        <b>{detail.skill.path}</b>
                      </div>
                      <div>
                        <b>{Object.keys(loaded).length}</b> bodies loaded this session
                      </div>
                    </div>
                  </section>

                  <section className="side-section">
                    <div className="side-label">
                      <span>Contents</span>
                      <span>{detail.skill.siblings.length} entries</span>
                    </div>
                    <div className="tree">
                      {detail.skill.siblings.map((f) => (
                        <div key={f.path}>
                          <b>{f.path}</b> <span style={{ opacity: 0.6 }}>{bytes(f.size)}</span>
                        </div>
                      ))}
                    </div>
                  </section>
                </aside>
              </div>
            )}
          </>
        )}
      </div>
    </>
  );
}


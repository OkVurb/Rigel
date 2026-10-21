import type { Kind, Package } from "../data";
import { fmt } from "../data";
import { Chip, GradeBadge, Kbd } from "./ui";

const KINDS: { id: Kind | "all"; label: string }[] = [
  { id: "all", label: "All" },
  { id: "skill", label: "Skills" },
  { id: "bundle", label: "Bundles" },
  { id: "mcp", label: "MCP" },
];

interface Props {
  items: Package[];
  selectedId: string;
  query: string;
  kind: Kind | "all";
  onQuery: (q: string) => void;
  onKind: (k: Kind | "all") => void;
  onSelect: (id: string) => void;
  onArrow: (dir: 1 | -1) => void;
}

export function PackageList({ items, selectedId, query, kind, onQuery, onKind, onSelect, onArrow }: Props) {
  return (
    <div className="list-pane">
      <div className="list-head">
        <div className="list-title">
          <h1>Packages</h1>
          <span>{items.length} · 16,494 indexed</span>
        </div>

        <label className="search">
          <span aria-hidden style={{ color: "var(--text-faint)", fontSize: 12 }}>
            ⌕
          </span>
          <input
            value={query}
            placeholder="Search skills, bundles, MCP…"
            onChange={(e) => onQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                onArrow(1);
              }
              if (e.key === "ArrowUp") {
                e.preventDefault();
                onArrow(-1);
              }
            }}
            aria-label="Search packages"
          />
          <Kbd>/</Kbd>
        </label>

        <div className="filters" role="tablist" aria-label="Package type">
          {KINDS.map((k) => (
            <button
              key={k.id}
              role="tab"
              aria-selected={kind === k.id}
              className={`filter${kind === k.id ? " on" : ""}`}
              onClick={() => onKind(k.id)}
            >
              {k.label}
            </button>
          ))}
        </div>
      </div>

      <div className="list" role="listbox" aria-label="Packages">
        {items.length === 0 && (
          <div className="empty" style={{ paddingTop: 40 }}>
            <strong>No matches</strong>
            <span>Try a repo name, or paste a GitHub URL.</span>
          </div>
        )}

        {items.map((p) => (
          <button
            key={p.id}
            role="option"
            aria-selected={p.id === selectedId}
            className={`row${p.id === selectedId ? " active" : ""}`}
            onClick={() => onSelect(p.id)}
          >
            <span className="row-name">
              <GradeBadge grade={p.grade} />
              <span className="name">{p.name}</span>
              {p.curated && <Chip tone="curated">curated</Chip>}
              {p.variants && p.variants.length > 1 && (
                <Chip tone="clone" title="Near-identical variants found">
                  {p.variants.length} variants
                </Chip>
              )}
            </span>
            <span style={{ fontSize: 10.5, color: "var(--text-faint)", fontFamily: "var(--font-mono)" }}>
              {fmt(p.installs)}
            </span>
            <span className="row-desc">{p.description}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

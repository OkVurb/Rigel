import { useEffect, useRef, useState } from "react";
import type { Package } from "../data";
import { Kbd } from "./ui";

type Row =
  | { kind: "go"; id: "targets" | "discover"; label: string; hint: string }
  | { kind: "pkg"; id: string; label: string; hint: string };

interface Props {
  open: boolean;
  packages: Package[];
  onClose: () => void;
  onPick: (id: string) => void;
  onGoTargets: () => void;
  onGoDiscover: () => void;
}

export function CommandPalette({ open, packages, onClose, onPick, onGoTargets, onGoDiscover }: Props) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  if (!open) return null;

  const q = query.trim().toLowerCase();

  const goRows: Row[] = (
    [
      { id: "discover" as const, label: "Discover — GitHub skills not installed", hint: "⌘2" },
      { id: "targets" as const, label: "Targets — harness matrix", hint: "⌘3" },
    ] as const
  )
    .filter((r) => q.length === 0 || `${r.label} ${r.id}`.toLowerCase().includes(q))
    .map((r) => ({ kind: "go" as const, ...r }));

  const pkgRows: Row[] = packages
    .filter((p) => p.name.toLowerCase().includes(q) || p.id.toLowerCase().includes(q))
    .slice(0, 8)
    .map((p) => ({ kind: "pkg", id: p.id, label: `Install ${p.name}`, hint: p.grade }));

  const rows = [...goRows, ...pkgRows];

  const commit = (row: Row) => {
    if (row.kind === "go") {
      if (row.id === "discover") onGoDiscover();
      else onGoTargets();
    } else {
      onPick(row.id);
    }
    onClose();
  };

  return (
    <div className="palette-scrim" onMouseDown={onClose}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="Command palette">
        <input
          ref={inputRef}
          value={query}
          placeholder="Install a package, or jump to a view…"
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") onClose();
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((a) => Math.min(a + 1, rows.length - 1));
            }
            if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => Math.max(a - 1, 0));
            }
            if (e.key === "Enter" && rows[active]) commit(rows[active]);
          }}
        />

        <div className="palette-list">
          {rows.length === 0 && (
            <div style={{ padding: "8px 10px", fontSize: 12, color: "var(--text-faint)" }}>
              No package matches “{query}”.
            </div>
          )}
          {rows.map((r, i) => (
            <div key={`${r.kind}-${r.id}`}>
              {i === 0 && r.kind === "go" && <div className="palette-group">Go</div>}
              {r.kind === "pkg" && (i === 0 || rows[i - 1].kind === "go") && (
                <div className="palette-group">Packages</div>
              )}
              <button className={`palette-row${i === active ? " on" : ""}`} onMouseEnter={() => setActive(i)} onClick={() => commit(r)}>
                <span style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--text-faint)" }}>
                  {r.hint}
                </span>
                {r.label}
                <Kbd>↵</Kbd>
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

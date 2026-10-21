import { useEffect, useMemo, useState } from "react";
import type { Kind } from "./data";
import { packages } from "./data";
import { PackageList } from "./components/PackageList";
import { PackageDetail } from "./components/PackageDetail";
import type { InstallState } from "./components/PackageDetail";
import { CommandPalette } from "./components/CommandPalette";
import { StatusBar } from "./components/StatusBar";
import { TargetsView } from "./components/TargetsView";
import { DiscoverView } from "./components/DiscoverView";
import { ImportDialog } from "./components/ImportDialog";
import { targets } from "./targets";
import type { Package } from "./data";
import { loadCatalogue, saveCatalogue } from "./db";

type View = "packages" | "discover" | "targets";

const initialInstalled = new Set<string>(["affaan-m/ECC", "anthropics/frontend-design", "chrome-devtools/mcp"]);

const ICONS = {
  sessions: "M3 4.5h14M3 9.5h14M3 14.5h9",
  packages: "M10 2.5 17 6.5v7l-7 4-7-4v-7l7-4ZM3 6.5l7 4 7-4M10 10.5v7",
  terminal: "M4 5l4 5-4 5M10 15h6",
  settings: "M10 12.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5ZM10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2",
  targets: "M3 3h5v5H3zM12 3h5v5h-5zM3 12h5v5H3zM12 12h5v5h-5z",
  discover: "M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM3 10h14M10 3c2.4 2.6 2.4 11.4 0 14M10 3c-2.4 2.6-2.4 11.4 0 14",
};

function RailButton({
  d,
  label,
  active,
  onClick,
}: {
  d: string;
  label: string;
  active?: boolean;
  onClick?: () => void;
}) {
  return (
    <button className={`rail-btn${active ? " active" : ""}`} title={label} aria-label={label} onClick={onClick}>
      <svg width="18" height="18" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
        <path d={d} />
      </svg>
    </button>
  );
}

export default function App() {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<Kind | "all">("all");
  const [selectedId, setSelectedId] = useState(packages[1].id);
  const [installed, setInstalled] = useState<Set<string>>(initialInstalled);
  const [installing, setInstalling] = useState<string | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [view, setView] = useState<View>("packages");
  const [targetId, setTargetId] = useState(targets[0].id);
  const [discoverKey, setDiscoverKey] = useState<string | null>(null);
  const [imported, setImported] = useState<Package[]>(() => loadCatalogue());
  const [importOpen, setImportOpen] = useState(false);

  const catalogue = useMemo(() => [...imported, ...packages], [imported]);

  const addImported = (pkg: Package) => {
    setImported((prev) => {
      if (prev.some((p) => p.id === pkg.id)) return prev;
      const next = [pkg, ...prev];
      saveCatalogue(next);
      return next;
    });
    setInstalled((prev) => new Set(prev).add(pkg.id));
  };

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return catalogue.filter((p) => {
      const kindOk = kind === "all" || p.kind === kind;
      const qOk =
        q.length === 0 ||
        p.name.toLowerCase().includes(q) ||
        p.id.toLowerCase().includes(q) ||
        p.description.toLowerCase().includes(q);
      return kindOk && qOk;
    });
  }, [query, kind, catalogue]);

  const selected = filtered.find((p) => p.id === selectedId) ?? filtered[0] ?? catalogue[0];

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((v) => !v);
        return;
      }
      if (e.key === "Escape") setPaletteOpen(false);
      if ((e.ctrlKey || e.metaKey) && e.key === "1") {
        e.preventDefault();
        setView("packages");
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "2") {
        e.preventDefault();
        setView("discover");
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "3") {
        e.preventDefault();
        setView("targets");
      }
      if (e.key === "/" && document.activeElement?.tagName !== "INPUT") {
        e.preventDefault();
        const el = document.querySelector<HTMLInputElement>(".search input");
        el?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const move = (dir: 1 | -1) => {
    const i = filtered.findIndex((p) => p.id === selected.id);
    const next = filtered[Math.min(Math.max(i + dir, 0), filtered.length - 1)];
    if (next) setSelectedId(next.id);
  };

  const install = () => {
    if (!selected || installing === selected.id || installed.has(selected.id)) return;
    const id = selected.id;
    setInstalling(id);
    window.setTimeout(() => {
      setInstalled((prev) => new Set(prev).add(id));
      setInstalling(null);
    }, 900);
  };

  const state: InstallState = installed.has(selected.id)
    ? "installed"
    : installing === selected.id
      ? "installing"
      : "idle";

  return (
    <div className="shell">
      <nav className="rail" aria-label="Primary">
        <div className="rail-brand" title="Orion">
          O
        </div>
        <RailButton d={ICONS.sessions} label="Sessions" />
        <RailButton d={ICONS.packages} label="Packages" active={view === "packages"} onClick={() => setView("packages")} />
        <RailButton d={ICONS.discover} label="Discover" active={view === "discover"} onClick={() => setView("discover")} />
        <RailButton d={ICONS.targets} label="Targets" active={view === "targets"} onClick={() => setView("targets")} />
        <RailButton d={ICONS.terminal} label="Terminal" />
        <div className="rail-spacer" />
        <RailButton d={ICONS.settings} label="Settings" />
      </nav>

      {view === "packages" && (
        <>
          <PackageList
            items={filtered}
            selectedId={selected.id}
            query={query}
            kind={kind}
            onQuery={setQuery}
            onKind={setKind}
            onSelect={setSelectedId}
            onArrow={move}
          />

          <PackageDetail pkg={selected} state={state} onInstall={install} />
        </>
      )}

      {view === "discover" && (
        <DiscoverView
          selectedKey={discoverKey}
          onSelect={setDiscoverKey}
          onAdd={addImported}
          catalogue={catalogue}
          onOpenImport={() => setImportOpen(true)}
        />
      )}

      {view === "targets" && <TargetsView selectedId={targetId} onSelect={setTargetId} />}

      <StatusBar installed={installed.size} targets={targets.length} view={view} />

      <ImportDialog
        open={importOpen}
        existing={catalogue}
        onClose={() => setImportOpen(false)}
        onAdd={addImported}
      />

      <CommandPalette
        open={paletteOpen}
        packages={catalogue}
        onClose={() => setPaletteOpen(false)}
        onPick={(id) => {
          setSelectedId(id);
          setView("packages");
        }}
        onGoTargets={() => setView("targets")}
        onGoDiscover={() => setView("discover")}
      />
    </div>
  );
}

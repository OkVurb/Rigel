import { SOURCES } from "../sources";
import type { SourceTab } from "../sources";

export function SourceTabs({ source, onSource }: { source: SourceTab; onSource: (s: SourceTab) => void }) {
  return (
    <div className="filters" role="tablist" aria-label="Skill source">
      {SOURCES.map((s) => (
        <button
          key={s.id}
          role="tab"
          aria-selected={source === s.id}
          title={s.hint}
          className={`filter${source === s.id ? " on" : ""}`}
          onClick={() => onSource(s.id)}
        >
          {s.label}
        </button>
      ))}
    </div>
  );
}

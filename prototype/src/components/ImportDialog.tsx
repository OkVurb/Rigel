import { useEffect, useMemo, useRef, useState } from "react";
import type { Package } from "../data";
import { fmt } from "../data";
import type { ImportResult } from "../github";
import { fetchRepoSkills, toPackage } from "../github";
import { GradeBadge, Chip, Kbd } from "./ui";

interface Props {
  open: boolean;
  existing: Package[];
  onClose: () => void;
  onAdd: (pkg: Package) => void;
}

type Phase = "idle" | "loading" | "ready" | "error";

export function ImportDialog({ open, existing, onClose, onAdd }: Props) {
  const [input, setInput] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [result, setResult] = useState<ImportResult | null>(null);
  const [error, setError] = useState("");
  const [added, setAdded] = useState<Set<string>>(new Set());
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setPhase("idle");
      setResult(null);
      setError("");
      setAdded(new Set());
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const existingNames = useMemo(
    () => new Map(existing.map((p) => [p.name.toLowerCase(), p.name])),
    [existing],
  );

  if (!open) return null;

  const run = async () => {
    if (!input.trim() || phase === "loading") return;
    setPhase("loading");
    setError("");
    setResult(null);
    try {
      const res = await fetchRepoSkills(input);
      setResult(res);
      setPhase("ready");
    } catch (e) {
      setError(e instanceof Error ? e.message : "Import failed.");
      setPhase("error");
    }
  };

  const add = (skillKey: string) => {
    if (!result) return;
    const skill = result.skills.find((s) => s.key === skillKey);
    if (!skill || added.has(skillKey)) return;
    onAdd(toPackage(skill, result.repo));
    setAdded((prev) => new Set(prev).add(skillKey));
  };

  const addAll = () => {
    if (!result) return;
    for (const s of result.skills) add(s.key);
  };

  return (
    <div className="palette-scrim" onMouseDown={onClose}>
      <div className="import-sheet" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="Add skills from GitHub">
        <div className="import-head">
          <div>
            <div className="import-title">Add from GitHub</div>
            <div className="import-sub">
              Paste a repo, a folder, or a SKILL.md link · read in the browser, nothing uploaded
            </div>
          </div>
          <button className="import-close" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </div>

        <div className="import-bar">
          <input
            ref={inputRef}
            value={input}
            placeholder="github.com/anthropics/skills  or  owner/repo/skills/pdf"
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") run();
              if (e.key === "Escape") onClose();
            }}
            aria-label="GitHub repository or URL"
          />
          <button className="install" onClick={run} disabled={phase === "loading" || !input.trim()}>
            {phase === "loading" ? "Reading…" : "Fetch"}
          </button>
        </div>

        <div className="import-hint">
          <span>
            Unauthenticated GitHub allows 60 requests/hour · one tree call per repo, one raw fetch per
            SKILL.md
          </span>
          <Kbd>↵</Kbd>
        </div>

        <div className="import-body">
          {phase === "idle" && (
            <div className="import-empty">
              <strong>Pull a skill without cloning anything.</strong>
              <span>
                The tree is read once, then SKILL.md bodies come from raw.githubusercontent — so API quota
                is spent on structure, not content. Each import is scanned for network, spawn, secret and
                destructive capabilities before it lands in the list.
              </span>
            </div>
          )}

          {phase === "loading" && (
            <div className="import-empty">
              <strong>Reading {input.trim()}…</strong>
              <span>Resolving default branch, fetching the tree, then the SKILL.md bodies.</span>
            </div>
          )}

          {phase === "error" && (
            <div className="import-error">
              <strong>{error}</strong>
              <span>Nothing was added. Adjust the URL and try again.</span>
            </div>
          )}

          {phase === "ready" && result && (
            <>
              <div className="import-repo">
                <span className="repo-name">{result.repo.fullName}</span>
                <span className="repo-meta">
                  {fmt(result.repo.stars)} stars · {result.repo.branch} · {result.repo.license ?? "no license"}
                </span>
                <span className="repo-meta" style={{ marginLeft: "auto" }}>
                  {result.skills.length} of {result.totalFound} SKILL.md
                </span>
                <button className="import-add-all" onClick={addAll}>
                  Add all
                </button>
              </div>

              <div className="import-list">
                {result.skills.map((s) => {
                  const dupe = existingNames.get(s.name.toLowerCase());
                  const isAdded = added.has(s.key);
                  return (
                    <div className="import-row" key={s.key}>
                      <GradeBadge grade={s.grade} />
                      <div className="import-row-main">
                        <div className="import-row-name">
                          <span className="name">{s.name}</span>
                          {dupe && (
                            <Chip tone="clone" title={`Already in your catalogue as ${dupe}`}>
                              duplicate
                            </Chip>
                          )}
                          {s.allowedTools && (
                            <Chip title={s.allowedTools.join(", ")}>{s.allowedTools.length} tools</Chip>
                          )}
                        </div>
                        <div className="import-row-desc">{s.description}</div>
                        <div className="import-row-path">
                          {s.path} · {s.capabilities.length} checks
                        </div>
                      </div>
                      <button
                        className={`import-add${isAdded ? " added" : ""}`}
                        onClick={() => add(s.key)}
                        disabled={isAdded}
                      >
                        {isAdded ? "✓ Added" : "Add"}
                      </button>
                    </div>
                  );
                })}
              </div>

              {result.totalFound > result.skills.length && (
                <div className="import-hint" style={{ paddingTop: 8 }}>
                  Showing the first {result.skills.length}. Paste a subfolder URL to import a specific set.
                </div>
              )}
              {result.truncatedTree && (
                <div className="import-hint" style={{ paddingTop: 8, color: "var(--accent)" }}>
                  Tree was truncated by GitHub at 100k entries — some skills may be missing.
                </div>
              )}
            </>
          )}
        </div>

        <div className="import-foot">
          <span>
            {added.size > 0 ? `${added.size} added to the catalogue` : "Scanned, not executed"}
          </span>
          <button className="import-done" onClick={onClose}>
            {added.size > 0 ? "Done" : "Cancel"}
          </button>
        </div>
      </div>
    </div>
  );
}

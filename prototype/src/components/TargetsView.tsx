import { useState } from "react";
import type { CheckStatus, Target } from "../targets";
import { divergence, measuredAt, skippedTotal, targets } from "../targets";
import { fmt } from "../data";
import { Chip } from "./ui";

const GLYPH: Record<CheckStatus, string> = { pass: "✓", warn: "!", fail: "×" };

function copyCommandFor(t: Target) {
  return t.id === "cursor" ? "node scripts/fix-cursor-hooks.mjs" : "node scripts/verify-ecc.mjs";
}

function TargetsList({ selectedId, onSelect }: { selectedId: string; onSelect: (id: string) => void }) {
  return (
    <div className="list-pane">
      <div className="list-head">
        <div className="list-title">
          <h1>Targets</h1>
          <span>{targets.length} harnesses</span>
        </div>

        <div className="divergence-strip">
          <div>
            <b>{fmt(divergence.union)}</b>
            <span>distinct skills</span>
          </div>
          <div>
            <b>{divergence.sharedByAll}</b>
            <span>in all four</span>
          </div>
          <div>
            <b className="warn">{fmt(divergence.singleStore)}</b>
            <span>in only one</span>
          </div>
        </div>

        <div className="target-note">
          One canonical store, materialised into each harness's own layout.
        </div>
      </div>

      <div className="list" role="listbox" aria-label="Harness targets">
        {targets.map((t) => (
          <button
            key={t.id}
            role="option"
            aria-selected={t.id === selectedId}
            className={`row target${t.id === selectedId ? " active" : ""}`}
            onClick={() => onSelect(t.id)}
          >
            <span className="row-name">
              <span className={`t-dot ${t.status}`} />
              <span className="name">{t.name}</span>
              {t.canonical && <Chip tone="curated">canonical</Chip>}
              {t.skipped.length > 0 && (
                <Chip tone="clone" title="Modules the adapter skipped">
                  {t.skipped.length} skipped
                </Chip>
              )}
            </span>
            <span style={{ fontSize: 10.5, color: "var(--text-faint)", fontFamily: "var(--font-mono)" }}>
              {fmt(t.skills)}
            </span>
            <span className="row-desc">{t.skillsPath}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

function TargetDetail({ t }: { t: Target }) {
  const [copied, setCopied] = useState(false);
  const max = Math.max(...divergence.stores.map((s) => s.count));

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(copyCommandFor(t));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="detail">
      <div className="detail-head">
        <div className="detail-top">
          <div style={{ minWidth: 0 }}>
            <div className="detail-id">
              {t.root} · {t.adapter}
            </div>
            <h2 className="detail-title">{t.name}</h2>
            <div className="detail-desc" style={{ marginTop: 4 }}>
              {t.summary}
            </div>
          </div>

          <button className="install" onClick={copy} style={{ background: "transparent", color: "var(--text-muted)", border: "1px solid var(--hairline-strong)" }}>
            {copied ? "✓ Copied" : "Copy verify command"}
          </button>
        </div>

        <div className="facts">
          <span>
            skills <b>{fmt(t.skills)}</b>
          </span>
          <span>
            from ECC <b>{t.eccSkills === null ? "—" : t.eccSkills}</b>
          </span>
          <span>
            managed files <b>{t.eccFiles === null ? "—" : t.eccFiles}</b>
          </span>
          <span>
            agents <b>{t.agents ?? "—"}</b>
          </span>
          <span>
            hooks <b>{t.hookEvents ? `${t.hookEvents} events` : "not supported"}</b>
          </span>
          <span>
            measured <b>{measuredAt}</b>
          </span>
        </div>
      </div>

      <div className="detail-body">
        <div className="doc">
          {t.doc.map((p, i) => (
            <p key={i}>{p}</p>
          ))}

          <h2>Readiness</h2>
          <div className="checks">
            {t.checks.map((c) => (
              <div className={`check ${c.status}`} key={c.label}>
                <span className="check-glyph" aria-hidden>
                  {GLYPH[c.status]}
                </span>
                <span className="check-body">
                  <span className="check-label">{c.label}</span>
                  <span className="check-detail">{c.detail}</span>
                </span>
              </div>
            ))}
          </div>

          {t.skipped.length > 0 && (
            <>
              <h2>Skipped by this adapter</h2>
              <div className="module-chips">
                {t.skipped.map((m) => (
                  <Chip key={m} tone="warn">
                    {m}
                  </Chip>
                ))}
              </div>
              <p style={{ fontSize: 11.5, color: "var(--text-faint)" }}>
                The installer records these in <code>resolution.skippedModules</code> rather than failing the
                install, so a harness can look fully installed while carrying a fraction of ECC.
              </p>
            </>
          )}
        </div>

        <aside className="side">
          <section className="side-section">
            <div className="side-label">
              <span>Store size</span>
            </div>
            <div className="path-line">{t.skillsPath}</div>

            <div className="bars">
              {divergence.stores.map((s) => {
                const self = s.id === t.id || (t.id === "cline" && s.id === "claude");
                return (
                  <div className={`bar-row${self ? " on" : ""}`} key={s.id}>
                    <span className="bar-label">{s.label}</span>
                    <span className="bar-track">
                      <span className="bar-fill" style={{ width: `${Math.max((s.count / max) * 100, 2)}%` }} />
                    </span>
                    <span className="bar-count">{fmt(s.count)}</span>
                  </div>
                );
              })}
            </div>

            {t.id === "cline" && (
              <div className="evidence-note" style={{ border: "1px solid var(--hairline)", borderRadius: 6 }}>
                Highlighted bar is the store Cline inherits — it has no skills directory of its own.
              </div>
            )}
          </section>

          <section className="side-section">
            <div className="side-label">
              <span>Divergence</span>
              <span>4 stores</span>
            </div>

            <div className="stat-list">
              <div>
                <b>{fmt(divergence.union)}</b> distinct skills across every store
              </div>
              <div>
                <b>{divergence.sharedByAll}</b> reachable from all four
              </div>
              <div>
                <b className="warn">{fmt(divergence.singleStore)}</b> reachable from exactly one
              </div>
              <div>
                <b>{skippedTotal}</b> adapter-skipped modules in total
              </div>
            </div>

            <div className="evidence-note" style={{ border: "1px solid var(--hairline)", borderRadius: 6 }}>
              Only-in-Codex examples: {divergence.examples.join(", ")}
            </div>
          </section>

          <section className="side-section">
            <div className="side-label">
              <span>Materialisation</span>
            </div>
            <div className="tree">
              <div>
                <b>~/.agents/skills</b> <span style={{ opacity: 0.6 }}>canonical</span>
              </div>
              <div style={{ paddingLeft: 10 }}>→ {t.skillsPath}</div>
              <div style={{ paddingLeft: 10 }}>hooks: {t.hookEvents ? `${t.hookEvents} events` : "unsupported"}</div>
              <div style={{ paddingLeft: 10 }}>mcp: {t.hosts.mcp ? "merged" : "not wired"}</div>
            </div>
          </section>
        </aside>
      </div>
    </div>
  );
}

export function TargetsView({ selectedId, onSelect }: { selectedId: string; onSelect: (id: string) => void }) {
  const selected = targets.find((t) => t.id === selectedId) ?? targets[0];
  return (
    <>
      <TargetsList selectedId={selected.id} onSelect={onSelect} />
      <TargetDetail t={selected} />
    </>
  );
}

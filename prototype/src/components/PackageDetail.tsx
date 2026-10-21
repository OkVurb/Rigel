import { useState } from "react";
import type { Package } from "../data";
import { fmt } from "../data";
import { AgentActions, Chip, GradeBadge } from "./ui";
import { runsOn, targets } from "../targets";

type InstallState = "idle" | "installing" | "installed";

interface Props {
  pkg: Package;
  state: InstallState;
  onInstall: () => void;
}

function Frontmatter({ pkg }: { pkg: Package }) {
  const rows: [string, string][] = [
    ["name", pkg.name],
    ["description", pkg.description],
    ["license", pkg.license ?? "none"],
  ];
  return (
    <div className="frontmatter">
      {"---\n"}
      {rows.map(([k, v]) => (
        <div key={k}>
          <span className="key">{k}</span>: {v}
        </div>
      ))}
      {"---"}
    </div>
  );
}

export function PackageDetail({ pkg, state, onInstall }: Props) {
  const [variantsOpen, setVariantsOpen] = useState(true);
  const critical = pkg.capabilities.filter((c) => c.severity === "critical").length;

  return (
    <div className="detail">
      <div className="detail-head">
        <div className="detail-top">
          <div style={{ minWidth: 0 }}>
            <div className="detail-id">
              {pkg.id} · {pkg.kind.replace("_", " ")}
            </div>
            <h2 className="detail-title">{pkg.name}</h2>
            <div className="detail-desc" style={{ marginTop: 4 }}>
              {pkg.description}
            </div>
          </div>

          <div className="head-actions">
            <AgentActions pkg={pkg} />
            <button
              className={`install${state === "installing" ? " installing" : ""}${state === "installed" ? " installed" : ""}`}
              onClick={onInstall}
              disabled={state !== "idle"}
            >
              {state === "idle" && "Install"}
              {state === "installing" && "Installing…"}
              {state === "installed" && "✓ Installed"}
            </button>
          </div>
        </div>

        <div className="facts">
          <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
            safety <GradeBadge grade={pkg.grade} />
          </span>
          <span>
            installs <b>{fmt(pkg.installs)}</b>
          </span>
          <span>
            stars <b>{fmt(pkg.stars)}</b>
          </span>
          <span>
            license <b>{pkg.license ?? "none"}</b>
          </span>
          <span>
            updated <b>{pkg.lastCommit}</b>
          </span>
          <span>
            context <b>~{pkg.tokens.toLocaleString()} tok</b>
          </span>
          {pkg.curated && <Chip tone="curated">curated</Chip>}
          {critical > 0 && <Chip tone="warn">{critical} critical</Chip>}
        </div>
      </div>

      <div className="detail-body">
        <div className="doc">
          <Frontmatter pkg={pkg} />
          {pkg.doc.map((block, i) => {
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
          {/* ---- harness portability ---- */}
          <section className="side-section">
            <div className="side-label">
              <span>Runs on</span>
              <span>
                {targets.filter((t) => runsOn(pkg.kind, t) === "full").length}/{targets.length} full
              </span>
            </div>

            <div className="runs-on">
              {targets.map((t) => {
                const support = runsOn(pkg.kind, t);
                return (
                  <div className={`run ${support}`} key={t.id}>
                    <span className="run-glyph" aria-hidden>
                      {support === "full" ? "✓" : support === "partial" ? "~" : "×"}
                    </span>
                    <span className="run-name">{t.name}</span>
                    <span className="run-note">
                      {support === "full" ? "full" : support === "partial" ? "partial" : "none"}
                    </span>
                  </div>
                );
              })}
            </div>

            {pkg.kind === "mcp" && (
              <div className="evidence-note" style={{ border: "1px solid var(--hairline)", borderRadius: 6 }}>
                MCP needs a per-harness server entry. The shared <code>.agents</code> store carries skills only.
              </div>
            )}
          </section>

          {/* ---- what this touches ---- */}
          <section className="side-section">
            <div className="side-label">
              <span>What this touches</span>
              <span style={{ color: critical ? "var(--danger)" : "var(--text-faint)" }}>
                {pkg.capabilities.length} {pkg.capabilities.length === 1 ? "check" : "checks"}
              </span>
            </div>

            {pkg.capabilities.map((cap) => (
              <div className="cap" key={cap.label}>
                <div className="cap-head">
                  <span className={`cap-dot ${cap.severity}`} />
                  {cap.label}
                </div>
                {cap.evidence.map((ev) => (
                  <div className="cap-ev" key={`${ev.file}:${ev.line}`}>
                    <span className="loc">
                      {ev.file}:{ev.line}
                    </span>
                    <span className="code" title={ev.code}>
                      {ev.code}
                    </span>
                  </div>
                ))}
              </div>
            ))}

            <div className="evidence-note" style={{ border: "1px solid var(--hairline)", borderRadius: 6 }}>
              Static review cannot prove a skill is harmless. First run executes in a sandbox with
              network off.
            </div>
          </section>

          {/* ---- evidence ---- */}
          {pkg.evidence && (
            <section className="side-section">
              <div className="side-label">
                <span>Measured evidence</span>
                <span style={{ color: "var(--verified)" }}>3rd party</span>
              </div>

              <div className="evidence">
                <table>
                  <thead>
                    <tr>
                      <th>Metric</th>
                      <th>Measured</th>
                      <th>Advertised</th>
                    </tr>
                  </thead>
                  <tbody>
                    {pkg.evidence.map((e) => (
                      <tr key={e.metric}>
                        <td style={{ color: "var(--text-muted)" }}>{e.metric}</td>
                        <td className={`measured${e.verdict === "worse" ? " worse" : ""}`}>{e.measured}</td>
                        <td className="advertised">{e.advertised}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <div className="evidence-note">
                  <div style={{ color: "var(--text-muted)", marginBottom: 3 }}>
                    {pkg.evidenceSource} · {pkg.evidenceDate}
                  </div>
                  {pkg.evidenceNote}
                </div>
              </div>
            </section>
          )}

          {/* ---- clone cluster ---- */}
          {pkg.variants && (
            <section className="side-section">
              <div className="side-label">
                <span>Variants</span>
              </div>

              <div className="variants">
                <button
                  className="variants-head"
                  onClick={() => setVariantsOpen((v) => !v)}
                  aria-expanded={variantsOpen}
                >
                  <span className="left">
                    <span className="caret">{variantsOpen ? "▾" : "▸"}</span>
                    {pkg.variants.length} near-identical · showing the best maintained
                  </span>
                </button>

                {variantsOpen &&
                  pkg.variants.map((v) => (
                    <div className={`variant${v.canonical ? " canonical" : ""}`} key={v.name}>
                      <span className="vname">{v.name}</span>
                      <span className="vnote">{v.note}</span>
                    </div>
                  ))}
              </div>
            </section>
          )}

          {/* ---- files ---- */}
          <section className="side-section">
            <div className="side-label">
              <span>Contents</span>
              <span>{pkg.files.length} entries</span>
            </div>
            <div className="tree">
              {pkg.files.map((f) => (
                <div key={f.path}>
                  <b>{f.path}</b> <span style={{ opacity: 0.6 }}>{f.size}</span>
                </div>
              ))}
            </div>
          </section>
        </aside>
      </div>
    </div>
  );
}

export type { InstallState };

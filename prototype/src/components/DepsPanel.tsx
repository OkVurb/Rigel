import { CATEGORY_LABEL, deps, depsByCategory } from "../deps";
import type { SourceTab } from "../sources";
import { downloadText } from "../db";
import { CopyButton } from "./ui";
import { SourceTabs } from "./SourceTabs";

interface Props {
  source: SourceTab;
  onSource: (s: SourceTab) => void;
  selectedId: string | null;
  onSelect: (id: string) => void;
}

export function DepsPanel({ source, onSource, selectedId, onSelect }: Props) {
  const selected = deps.find((d) => d.id === selectedId) ?? null;

  return (
    <>
      <div className="list-pane">
        <div className="list-head">
          <div className="list-title">
            <h1>Dependencies</h1>
            <span>{deps.length} tools</span>
          </div>

          <SourceTabs source={source} onSource={onSource} />

          <div className="target-note">
            What an agent needs but a fresh machine may not have — including the shells other tooling assumes.
          </div>
        </div>

        <div className="list" role="listbox" aria-label="Dependencies">
          {depsByCategory().map(([cat, list]) => (
            <div key={cat}>
              <div className="dep-group">{CATEGORY_LABEL[cat]}</div>
              {list.map((d) => (
                <button
                  key={d.id}
                  role="option"
                  aria-selected={selected?.id === d.id}
                  className={`row${selected?.id === d.id ? " active" : ""}`}
                  onClick={() => onSelect(d.id)}
                >
                  <span className="row-name">
                    <span className="dep-mark">⌗</span>
                    <span className="name">{d.name}</span>
                  </span>
                  <span style={{ fontSize: 10.5, color: "var(--text-faint)", fontFamily: "var(--font-mono)" }}>{d.size}</span>
                  <span className="row-desc">{d.provides.join(" · ")}</span>
                </button>
              ))}
            </div>
          ))}
        </div>
      </div>

      <div className="detail">
        {!selected && (
          <div className="empty">
            <strong>Pick a dependency</strong>
            <span>Each entry carries the real install command and the upstream docs link.</span>
          </div>
        )}

        {selected && (
          <>
            <div className="detail-head">
              <div className="detail-top">
                <div style={{ minWidth: 0 }}>
                  <div className="detail-id">
                    {CATEGORY_LABEL[selected.category]} · {selected.size}
                  </div>
                  <h2 className="detail-title">{selected.name}</h2>
                  <div className="detail-desc" style={{ marginTop: 4 }}>
                    {selected.why}
                  </div>
                </div>

                <div className="head-actions">
                  <button
                    className="ghost-btn"
                    onClick={() =>
                      downloadText(
                        `install-${selected.id}.ps1`,
                        `# ${selected.name}\n${selected.commands.map((c) => `# ${c.label}\n${c.command}`).join("\n\n")}\n`,
                        "text/plain;charset=utf-8",
                      )
                    }
                  >
                    ⤓ install script
                  </button>
                  <a className="install" href={selected.docs} target="_blank" rel="noreferrer" style={{ textDecoration: "none" }}>
                    Docs ↗
                  </a>
                </div>
              </div>

              <div className="facts">
                <span>
                  provides <b>{selected.provides.join(", ")}</b>
                </span>
                <span>
                  license <b>upstream</b>
                </span>
                <span>
                  installed <b>not detected yet</b>
                </span>
              </div>
            </div>

            <div className="detail-body">
              <div className="doc">
                <h2>Install</h2>
                <div className="cmd-list">
                  {selected.commands.map((c) => (
                    <div className="cmd" key={c.label}>
                      <div className="cmd-label">
                        <span>{c.label}</span>
                        <CopyButton text={c.command} label="Copy" />
                      </div>
                      <code className="cmd-line">{c.command}</code>
                    </div>
                  ))}
                </div>

                <h2>Why an agent asks for it</h2>
                <p>{selected.agentUse}</p>

                <h2>Terminal access</h2>
                <p>
                  Installing a tool is half the problem; the agent also needs a shell that can reach it. Every
                  command above is written to be run non-interactively, so it works from a spawned process as
                  well as a terminal profile.
                </p>
                <ul>
                  <li>Windows: PowerShell 7 and Git Bash both resolve winget installs on PATH after a new shell.</li>
                  <li>macOS / Linux: brew puts binaries on PATH immediately; no shell restart needed.</li>
                  <li>WSL: tools installed inside Ubuntu are invisible to Windows shells — install per environment.</li>
                </ul>
              </div>

              <aside className="side">
                <section className="side-section">
                  <div className="side-label">
                    <span>Used by</span>
                  </div>
                  <div className="stat-list">
                    <div>
                      <b>{selected.provides.length}</b> binaries or packages
                    </div>
                    <div>
                      <b>{selected.commands.length}</b> install paths
                    </div>
                    <div>
                      <b>session</b> scoped
                    </div>
                  </div>
                </section>

                <section className="side-section">
                  <div className="side-label">
                    <span>Download</span>
                  </div>
                  <div className="stat-list">
                    <div>
                      <a href={selected.docs} target="_blank" rel="noreferrer" style={{ color: "var(--info)" }}>
                        {selected.docs.replace(/^https?:\/\//, "")} ↗
                      </a>
                    </div>
                    <div>install script above is generated locally</div>
                  </div>
                </section>

                <section className="side-section">
                  <div className="side-label">
                    <span>Related</span>
                  </div>
                  <div className="stat-list">
                    {deps
                      .filter((d) => d.category === selected.category && d.id !== selected.id)
                      .map((d) => (
                        <div key={d.id}>
                          <button className="link-btn" onClick={() => onSelect(d.id)}>
                            {d.name}
                          </button>
                        </div>
                      ))}
                  </div>
                </section>
              </aside>
            </div>
          </>
        )}
      </div>
    </>
  );
}

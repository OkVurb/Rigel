import { useEffect, useState } from "react";
import { api, type TraySnapshot } from "./api";
import { RigelMark } from "./mark";
import { applyTheme } from "./theme";

function money(micros: number): string {
  const dollars = micros / 1_000_000;
  return dollars >= 1 ? `$${dollars.toFixed(2)}` : `$${dollars.toFixed(4)}`;
}

function memory(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${Math.round(mb)} MB`;
}

function cpu(value: number): string {
  return `${value < 10 ? value.toFixed(1) : Math.round(value)}%`;
}

/** The panel drawn inside the frameless `tray` window, not a native OS menu. */
export default function TrayPanel() {
  const [snapshot, setSnapshot] = useState<TraySnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    void api
      .getSettings()
      .then((settings) => {
        if (live) applyTheme(settings.theme);
      })
      .catch(() => applyTheme("dark"));
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    let live = true;
    const load = async () => {
      try {
        const next = await api.tray.snapshot();
        if (live) {
          setSnapshot(next);
          setError(null);
        }
      } catch (problem) {
        if (live) setError(problem instanceof Error ? problem.message : String(problem));
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 2_000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void api.tray.hide();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const act = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  const running = snapshot?.agents_running ?? 0;
  const projectName = snapshot?.project_name ?? "No project open";
  const projectPath = snapshot?.project ?? "Open a folder in Rigel to start a session.";
  const shown = snapshot?.agents ?? [];
  const hidden = snapshot?.agents_hidden ?? 0;

  return (
    <main className="tray-panel" aria-label="Rigel tray">
      <header className="tray-head">
        <div className="tray-brand">
          <span className="tray-mark">
            <RigelMark size={18} />
          </span>
          <div>
            <strong>Rigel</strong>
            <span>{running > 0 ? `${running} running` : "all agents idle"}</span>
          </div>
        </div>
        <button
          className="tray-close"
          onClick={() => void api.tray.hide()}
          aria-label="Close tray"
          title="Close"
        >
          ×
        </button>
      </header>

      <section className="tray-project">
        <span className="tray-eyebrow">Project</span>
        <strong className="tray-project-name">{projectName}</strong>
        <span className="tray-project-path" title={projectPath}>
          {projectPath}
        </span>
      </section>

      <section className="tray-metrics" aria-label="Live project metrics">
        <div className="tray-metric">
          <span>Agents</span>
          <strong>{running}</strong>
          <small>{snapshot ? `${snapshot.sessions} sessions` : "reading…"}</small>
        </div>
        <div className="tray-metric">
          <span>CPU</span>
          <strong>{cpu(snapshot?.cpu_percent ?? 0)}</strong>
          <small title={snapshot?.power_note}>{snapshot?.power ?? "idle"} power load</small>
        </div>
        <div className="tray-metric">
          <span>Memory</span>
          <strong>{memory(snapshot?.memory_bytes ?? 0)}</strong>
          <small>Rigel + children</small>
        </div>
        <div className="tray-metric">
          <span>Spend</span>
          <strong>{money(snapshot?.spend_micros ?? 0)}</strong>
          <small>project total</small>
        </div>
      </section>

      <section className="tray-agents" aria-label="Running agents">
        <div className="tray-section-head">
          <span>Running agents</span>
          <span className="tray-live">
            <span className="tray-live-dot" />
            live
          </span>
        </div>

        {error ? <p className="tray-error">{error}</p> : null}

        {!snapshot && !error ? <p className="tray-empty">Reading agents and spend…</p> : null}

        {snapshot && shown.length === 0 ? (
          <p className="tray-empty">
            No agents running. Open Rigel to start a session, or leave one waiting here.
          </p>
        ) : null}

        {shown.map((agent) => (
          <div className="tray-agent" key={agent.id}>
            <span className="tray-dot" />
            <div className="tray-agent-text">
              <strong className="tray-agent-name" title={agent.name}>
                {agent.name}
              </strong>
              <span className="tray-agent-meta" title={agent.session}>
                {agent.harness}
                {agent.model ? ` · ${agent.model}` : ""}
                {agent.source === "chat" ? " · chat" : ""}
              </span>
            </div>
            <span className="tray-agent-cost">
              {agent.source === "chat" ? "chat" : money(agent.spend_micros)}
            </span>
          </div>
        ))}

        {hidden > 0 ? <p className="tray-more">+{hidden} more running</p> : null}
      </section>

      <footer className="tray-foot">
        <button
          className="primary tray-open"
          onClick={() => void act(api.tray.open)}
          disabled={busy}
        >
          Open Rigel
        </button>
        <button
          className="tray-quit"
          onClick={() => void act(api.tray.quit)}
          disabled={busy}
          title="Stop Rigel and every agent it launched"
        >
          Quit
        </button>
      </footer>
    </main>
  );
}

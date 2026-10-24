/**
 * The Terminal surface.
 *
 * xterm.js draws and captures keys; the shell itself is a PTY owned by Rust. Output arrives on
 * `rigel://terminal` as base64 chunks, input goes back through `terminal_write`, and the fit
 * addon's resize is forwarded to the PTY so full-screen tools lay out correctly. Switching
 * tabs does not kill the shell because App keeps this surface mounted after the first visit.
 */

import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import { api, isTauri, type TerminalOutput } from "./api";

const THEME = {
  background: "#0b0c0e",
  foreground: "#e6e8eb",
  cursor: "#e9a23b",
  cursorAccent: "#0b0c0e",
  selectionBackground: "rgba(233, 162, 59, 0.28)",
  black: "#121417",
  red: "#e5484d",
  green: "#4cc38a",
  yellow: "#e9a23b",
  blue: "#6aa9ff",
  magenta: "#c792ea",
  cyan: "#57c7d4",
  white: "#e6e8eb",
  brightBlack: "#6b727c",
  brightRed: "#ff6b6f",
  brightGreen: "#6fe0a8",
  brightYellow: "#ffc46b",
  brightBlue: "#8cbdff",
  brightMagenta: "#d9a9f5",
  brightCyan: "#7adce8",
  brightWhite: "#ffffff",
};

export default function TerminalView({ project }: { project: string | null }) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [generation, setGeneration] = useState(0);
  const [shell, setShell] = useState<string | null>(null);
  const [cwd, setCwd] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [live, setLive] = useState(false);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !isTauri()) return;

    setError(null);
    setLive(false);
    setShell(null);
    setCwd(null);

    const terminal = new Terminal({
      allowProposedApi: true,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: "bar",
      fontFamily: "JetBrains Mono, Cascadia Mono, Consolas, monospace",
      fontSize: 13,
      lineHeight: 1.2,
      scrollback: 8000,
      theme: THEME,
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);
    fit.fit();

    let disposed = false;
    let id: string | null = null;
    let release: (() => void) | undefined;
    const input = terminal.onData((data) => {
      if (id) void api.terminal.write(id, data).catch(() => {});
    });
    const resized = terminal.onResize(({ cols, rows }) => {
      if (id) void api.terminal.resize(id, cols, rows).catch(() => {});
    });

    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const unlisten = await listen<TerminalOutput>("rigel://terminal", (event) => {
        if (event.payload.id !== id) return;
        if (event.payload.exit) {
          setLive(false);
          terminal.write(
            "\r\n\x1b[38;5;245m[shell exited — press Restart to open another]\x1b[0m\r\n",
          );
          return;
        }
        const binary = window.atob(event.payload.data);
        const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
        terminal.write(bytes);
      });
      if (disposed) {
        unlisten();
        return;
      }
      release = unlisten;

      try {
        const info = await api.terminal.open(project, terminal.cols, terminal.rows);
        if (disposed) {
          void api.terminal.close(info.id);
          return;
        }
        id = info.id;
        setShell(info.shell);
        setCwd(info.cwd);
        setLive(true);
        terminal.focus();
      } catch (problem) {
        const message = problem instanceof Error ? problem.message : String(problem);
        setError(message);
        terminal.write(`\x1b[31m${message}\x1b[0m\r\n`);
      }
    })();

    const observer = new ResizeObserver(() => {
      if (!disposed) fit.fit();
    });
    observer.observe(host);
    const focus = () => terminal.focus();
    host.addEventListener("pointerdown", focus);

    return () => {
      disposed = true;
      observer.disconnect();
      host.removeEventListener("pointerdown", focus);
      input.dispose();
      resized.dispose();
      release?.();
      terminal.dispose();
      if (id) void api.terminal.close(id);
    };
  }, [project, generation]);

  if (!isTauri()) {
    return (
      <>
        <aside className="list-pane">
          <div className="list-head">
            <div className="list-title">
              <h1>Terminal</h1>
              <span>shell</span>
            </div>
          </div>
          <p className="empty">A real shell is available in the desktop window.</p>
        </aside>
        <main className="detail terminal-main">
          <p className="empty">The browser preview has no PTY.</p>
        </main>
      </>
    );
  }

  return (
    <>
      <aside className="list-pane terminal-pane">
        <div className="list-head">
          <div className="list-title">
            <h1>Terminal</h1>
            <span>{live ? "running" : "closed"}</span>
          </div>
          <div className="roster-actions">
            <button className="chip" onClick={() => setGeneration((value) => value + 1)}>
              Restart
            </button>
          </div>
        </div>
        <div className="terminal-meta">
          <p className="fine">
            {shell ? <span className="mono">{shell}</span> : "Starting a shell…"}
          </p>
          {cwd ? <p className="fine mono terminal-cwd">{cwd}</p> : null}
          {error ? <p className="terminal-error">{error}</p> : null}
        </div>
        <p className="fine terminal-hint">
          A real shell in the project root. Keys go straight to the PTY; the app does not parse
          or approve them, so run what you mean to run.
        </p>
      </aside>
      <main className="detail terminal-main">
        <div className="terminal-host" ref={hostRef} role="application" aria-label="Terminal" />
      </main>
    </>
  );
}

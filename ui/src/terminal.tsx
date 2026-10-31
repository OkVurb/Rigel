/**
 * The Terminal surface.
 *
 * xterm.js draws and captures keys; the shell itself is a PTY owned by Rust. Output arrives on
 * `rigel://terminal` as base64 chunks, input goes back through `terminal_write`, and the fit
 * addon's resize is forwarded to the PTY so full-screen tools lay out correctly. Switching
 * tabs does not kill the shell because App keeps this surface mounted after the first visit.
 *
 * Font size, scrollback and the shell itself are settings, so the surface looks and behaves the
 * way the rest of the window is configured. In the browser preview there is no PTY, so the same
 * terminal runs against a local echo — the surface can be seen (and its options checked) without
 * pretending a shell exists.
 */

import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import { api, isTauri, type Settings, type TerminalOutput } from "./api";
import { currentTheme, type Theme } from "./theme";

/**
 * Two complete ANSI palettes, one per theme (D126). xterm draws its own canvas, so it cannot
 * inherit the CSS tokens; the light palette is measured against white instead of flipped. Every
 * entry below is at least 4.5:1 on its background, including the dim slots a prompt uses for
 * metadata — otherwise `ls` colours would go soft the moment the window turned light.
 */
const TERMINAL_THEMES: Record<Theme, ITheme> = {
  dark: {
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
    brightBlack: "#7b838d",
    brightRed: "#ff6b6f",
    brightGreen: "#6fe0a8",
    brightYellow: "#ffc46b",
    brightBlue: "#8cbdff",
    brightMagenta: "#d9a9f5",
    brightCyan: "#7adce8",
    brightWhite: "#ffffff",
  },
  light: {
    background: "#ffffff",
    foreground: "#14161a",
    cursor: "#a36100",
    cursorAccent: "#ffffff",
    selectionBackground: "rgba(233, 162, 59, 0.35)",
    black: "#14161a",
    red: "#b42318",
    green: "#16794a",
    yellow: "#8a5a00",
    blue: "#1d5fbf",
    magenta: "#7a3ea8",
    cyan: "#0b6b78",
    white: "#565d67",
    brightBlack: "#616a74",
    brightRed: "#a01a12",
    brightGreen: "#0f6b3f",
    brightYellow: "#7a4f00",
    brightBlue: "#1150a8",
    brightMagenta: "#6b2f9c",
    brightCyan: "#085f6b",
    brightWhite: "#14161a",
  },
};

const FONT_STACK = "JetBrains Mono, Cascadia Mono, SFMono-Regular, Consolas, monospace";

/**
 * The one place xterm's options are decided, so the preview and the desktop window cannot drift
 * apart. `letterSpacing: 0` and an integral font size keep the cell grid exact — a fractional
 * advance is what makes a terminal look blurry and its last row half-drawn.
 */
function terminalOptions(settings: Settings | null, theme: Theme) {
  return {
    allowProposedApi: true,
    convertEol: false,
    cursorBlink: true,
    cursorStyle: "bar" as const,
    cursorInactiveStyle: "outline" as const,
    drawBoldTextInBrightColors: true,
    fontFamily: FONT_STACK,
    fontSize: settings?.terminal_font_size ?? 13,
    fontWeight: "400" as const,
    fontWeightBold: "600" as const,
    letterSpacing: 0,
    lineHeight: 1.2,
    macOptionIsMeta: true,
    rightClickSelectsWord: true,
    scrollback: settings?.terminal_scrollback ?? 8_000,
    scrollOnUserInput: true,
    smoothScrollDuration: 60,
    theme: TERMINAL_THEMES[theme],
  };
}

/** The prompt a preview session shows, so the surface is readable without a shell behind it. */
const PREVIEW_BANNER =
  "\x1b[90mBrowser preview — this terminal echoes locally. The desktop window opens a real\x1b[0m\r\n" +
  "\x1b[90mshell in the project root; try `ls`, then click the canvas and type.\x1b[0m\r\n\r\n";

/** The dim prompt line; ANSI 90 is bright black, so it follows whichever palette is active. */
const PROMPT = "\x1b[90m$\x1b[0m ";

export default function TerminalView({ project }: { project: string | null }) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const settingsRef = useRef<Settings | null>(null);
  const [generation, setGeneration] = useState(0);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [theme, setTheme] = useState<Theme>(() => currentTheme());
  const [shell, setShell] = useState<string | null>(null);
  const [cwd, setCwd] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [live, setLive] = useState(false);

  // Settings arrive after the first paint; the terminal is created with whatever is known then
  // and re-styled in place, so a slow settings read never restarts a running shell.
  useEffect(() => {
    let alive = true;
    api
      .getSettings()
      .then((next) => {
        if (alive) {
          settingsRef.current = next;
          setSettings(next);
        }
      })
      .catch(() => alive && setSettings(null));
    return () => {
      alive = false;
    };
  }, []);

  // Settings → Appearance writes `<html data-theme>`; watch it here rather than threading a prop,
  // because the settings page and the terminal are separate routes under the same document.
  useEffect(() => {
    const root = document.documentElement;
    setTheme(currentTheme());
    const observer = new MutationObserver(() => setTheme(currentTheme()));
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const terminal = termRef.current;
    if (!terminal || !settings) return;
    terminal.options.fontSize = settings.terminal_font_size;
    terminal.options.scrollback = settings.terminal_scrollback;
    fitRef.current?.fit();
  }, [settings]);

  // Recolour in place: the PTY and its scrollback survive the switch.
  useEffect(() => {
    if (termRef.current) termRef.current.options.theme = TERMINAL_THEMES[theme];
  }, [theme]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    setError(null);
    setLive(false);
    setShell(null);
    setCwd(null);

    const terminal = new Terminal(terminalOptions(settingsRef.current, currentTheme()));
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);
    fit.fit();
    termRef.current = terminal;
    fitRef.current = fit;

    let disposed = false;
    let id: string | null = null;
    let release: (() => void) | undefined;
    let line = "";
    const shellPath = settingsRef.current?.terminal_shell ?? "";

    const input = terminal.onData((data) => {
      if (id) {
        void api.terminal.write(id, data).catch(() => {});
        return;
      }
      if (isTauri()) return;
      // Preview only: a line editor good enough to prove the surface works without a PTY.
      if (data === "\r") {
        terminal.write("\r\n");
        if (line.trim() === "clear") terminal.clear();
        line = "";
        terminal.write(PROMPT);
        return;
      }
      if (data === "\u007f") {
        if (line.length > 0) {
          line = line.slice(0, -1);
          terminal.write("\b \b");
        }
        return;
      }
      if (data === "\u0003") {
        line = "";
        terminal.write(`^C\r\n${PROMPT}`);
        return;
      }
      if (!data.startsWith("\u001b")) {
        line += data;
        terminal.write(data);
      }
    });

    const resized = terminal.onResize(({ cols, rows }) => {
      if (id) void api.terminal.resize(id, cols, rows).catch(() => {});
    });

    if (!isTauri()) {
      setShell("preview shell (local echo)");
      setCwd(project ?? "no project open");
      setLive(true);
      terminal.write(PREVIEW_BANNER);
      terminal.write(PROMPT);
    }

    (async () => {
      if (!isTauri()) return;
      const { listen } = await import("@tauri-apps/api/event");
      const unlisten = await listen<TerminalOutput>("rigel://terminal", (event) => {
        if (event.payload.id !== id) return;
        if (event.payload.exit) {
          setLive(false);
          terminal.write("\r\n\x1b[90m[shell exited — press Restart to open another]\x1b[0m\r\n");
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
        const info = await api.terminal.open(project, terminal.cols, terminal.rows, shellPath);
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
      termRef.current = null;
      fitRef.current = null;
      if (id) void api.terminal.close(id);
    };
  }, [project, generation]);

  const stateClass = live ? "pill completed" : "pill cancelled";
  const hint = isTauri()
    ? "A real shell in the project root. Keys go straight to the PTY; the app does not parse or approve them, so run what you mean to run."
    : "The preview echoes locally. The desktop window opens a real PTY in the project root, with this same surface.";

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

        <div className="terminal-status">
          <div className="terminal-status-row">
            <span>shell</span>
            <code className="mono">{shell ?? "starting…"}</code>
          </div>
          <div className="terminal-status-row">
            <span>cwd</span>
            <code className="mono">{cwd ?? "—"}</code>
          </div>
          <div className="terminal-status-row">
            <span>state</span>
            <span className={stateClass}>{live ? "running" : "closed"}</span>
          </div>
          {settings ? (
            <div className="terminal-status-row">
              <span>font</span>
              <code className="mono">
                {settings.terminal_font_size}px ·{" "}
                {settings.terminal_scrollback.toLocaleString()} lines
              </code>
            </div>
          ) : null}
        </div>

        {error ? <p className="terminal-error">{error}</p> : null}

        <p className="fine terminal-hint">{hint}</p>
      </aside>

      <main id="main" tabIndex={-1} className="detail terminal-main">
        <div className="terminal-frame">
          <div className="terminal-host" ref={hostRef} role="application" aria-label="Terminal" />
        </div>
      </main>
    </>
  );
}

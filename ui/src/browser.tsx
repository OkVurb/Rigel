import { useEffect, useState } from "react";

import { api, type Agent, type Item, type TurnTranscript } from "./api";

/**
 * One browser call, whichever surface made it. Native `browser_open` carries the screenshot its
 * own Rigel b process just wrote; a harness MCP call has the exact frame its Rigel b server wrote
 * after the action. The old URL snapshot remains a fallback for harnesses without `--capture`.
 */
export interface BrowserCall {
  item: Item;
  agentId: string;
  tool: string;
  url: string | null;
  screenshot: string | null;
  native: boolean;
  error: string | null;
}

interface Capture {
  source: "native" | "harness" | "snapshot";
  tool: string;
  at: number;
}

const VISUAL_TOOLS = [
  "browser_navigate",
  "browser_run",
  "browser_click",
  "browser_fill",
  "browser_type",
  "browser_press_key",
  "browser_select_option",
  "browser_evaluate",
  "browser_wait_for",
  "browser_wait_for_function",
  "browser_back",
  "browser_forward",
  "browser_reload",
  "browser_wait_for_text",
  "browser_fill_form",
  "browser_scroll",
  "browser_tab_new",
  "browser_tab_switch",
  "browser_set_cookie",
  "browser_clear_cookies",
  "browser_set_storage_state",
];

const isVisualTool = (tool: string): boolean =>
  VISUAL_TOOLS.some((name) => tool === name || tool.endsWith(`__${name}`));

const record = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const text = (value: unknown): string | null =>
  typeof value === "string" && value.trim() ? value : null;

const httpUrl = (value: unknown): string | null => {
  const raw = text(value);
  if (!raw) return null;
  try {
    const parsed = new URL(raw);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? raw : null;
  } catch {
    return null;
  }
};

function asBrowserCall(item: Item, agentId: string): BrowserCall | null {
  const body = record(item.body) ?? {};
  const tool = text(body.tool) ?? "";
  const server = text(body.server) ?? "";
  const native = tool === "browser_open";
  const named =
    server.toLowerCase().includes("obscura") ||
    server.toLowerCase().includes("rigel") ||
    tool.toLowerCase().includes("obscura") ||
    tool.toLowerCase().includes("rigel") ||
    tool.toLowerCase().startsWith("browser_");
  if (!native && !(named && isVisualTool(tool))) return null;

  const result = record(body.result);
  const args = record(body.arguments) ?? record(body.input) ?? record(body.raw_input);
  const rawScreenshot = text(result?.screenshot);
  const screenshot =
    rawScreenshot && !rawScreenshot.startsWith("data:") ? rawScreenshot : null;
  const errorObject = record(result?.error) ?? record(body.error);
  const error =
    text(errorObject?.message) ??
    text(body.error) ??
    (item.status === "ok" || item.status === "complete" || item.status === "completed"
      ? null
      : text(result?.message) ?? text(body.message));

  return {
    item,
    agentId,
    tool: tool || "browser",
    url:
      httpUrl(args?.url) ??
      httpUrl(result?.url) ??
      httpUrl(body.url),
    screenshot,
    native,
    error,
  };
}

/** The newest browser call in the transcript, with the last URL seen for click/fill calls. */
export function latestBrowserCall(turns: TurnTranscript[]): BrowserCall | null {
  let latest: BrowserCall | null = null;
  let lastUrl: string | null = null;
  for (const entry of turns) {
    for (const item of entry.items) {
      const call = asBrowserCall(item, entry.turn.agent_id);
      if (!call) continue;
      if (call.url) lastUrl = call.url;
      latest = { ...call, url: call.url ?? lastUrl };
    }
  }
  return latest;
}

const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

/**
 * The Browser section of the session inspector. Both the native tool and the harness MCP server
 * now save the exact frame from their own Rigel b process; the URL renderer is only a fallback.
 */
export default function BrowserPanel({
  call,
  agent,
  sessionId,
}: {
  call: BrowserCall | null;
  agent: Agent | null;
  sessionId: string | null;
}) {
  const [image, setImage] = useState<string | null>(null);
  const [capture, setCapture] = useState<Capture | null>(null);
  const [phase, setPhase] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);

  const shot = call?.screenshot ?? null;
  const snapshotUrl = call && !call.native ? call.url : null;
  const canLoad = Boolean(shot || call?.url || (!call?.native && sessionId));

  useEffect(() => {
    let alive = true;
    if (!call) {
      setImage(null);
      setCapture(null);
      setPhase("idle");
      setError(null);
      return;
    }
    if (!shot && !sessionId) {
      setPhase("ready");
      setError(null);
      return;
    }

    setImage(null);
    setCapture(null);
    setPhase("loading");
    setError(null);
    void (async () => {
      try {
        let nextImage: string;
        let nextSource: Capture["source"];
        let nextTool = call.tool;
        let nextAt = Date.now();
        if (shot) {
          nextImage = await api.browserImage(shot);
          nextSource = "native";
        } else {
          let exact: Awaited<ReturnType<typeof api.browserHarnessCapture>> | null = null;
          for (let attempt = 0; attempt < 24; attempt += 1) {
            try {
              const candidate = await api.browserHarnessCapture(sessionId!, call.agentId);
              if (candidate.captured_at >= call.item.created_at) {
                exact = candidate;
                break;
              }
            } catch {
              // The action may still be finishing; the next poll can see the file.
            }
            await sleep(250);
          }
          if (exact) {
            nextImage = exact.image;
            nextSource = "harness";
            nextTool = exact.tool ?? call.tool;
            nextAt = exact.captured_at || Date.now();
          } else if (snapshotUrl) {
            const snapshot = await api.browserCapture(snapshotUrl);
            nextImage = snapshot.image;
            nextSource = "snapshot";
            nextAt = Date.now();
          } else {
            throw new Error("Rigel b has not written a capture for this call yet.");
          }
        }
        if (!alive) return;
        setImage(nextImage);
        setCapture({
          source: nextSource,
          tool: nextTool,
          at: nextAt,
        });
        setPhase("ready");
      } catch (problem) {
        if (!alive) return;
        setError(problem instanceof Error ? problem.message : String(problem));
        setPhase("error");
      }
    })();

    return () => {
      alive = false;
    };
  }, [call?.item.id, nonce, sessionId]);

  const agentLabel = agent?.role ?? agent?.harness ?? call?.agentId.slice(0, 8) ?? null;

  return (
    <section className="browser-section" aria-label="Agent browser">
      <div className="browser-head">
        <h3>Browser</h3>
        <span className={`browser-badge ${phase}`}>
          {phase === "loading" ? "capturing…" : call ? "Rigel b" : "idle"}
        </span>
      </div>

      {!call ? (
        <p className="fine">
          When an agent opens a page with <code>browser_open</code> or the Rigel b MCP, its
          capture appears here.
        </p>
      ) : (
        <>
          <div className="browser-call">
            <span className="browser-tool mono" title={call.tool}>
              {call.tool}
            </span>
            {agentLabel ? <span className="tag">{agentLabel}</span> : null}
          </div>
          <p className="fine browser-note">
            Runs in the background, in its own Space for MCP calls. It cannot move your
            cursor, send your keystrokes, or switch your tabs.
          </p>
          {call.url ? (
            <p className="browser-url" title={call.url}>
              {call.url}
            </p>
          ) : null}

          <div className="browser-frame">
            {image ? (
              <img src={image} alt={`Browser capture of ${call.url ?? "the agent's page"}`} />
            ) : (
              <p className="browser-placeholder">
                {phase === "loading"
                  ? "Waiting for Rigel b…"
                  : (error ?? call.error ?? "This call recorded no screenshot.")}
              </p>
            )}
          </div>

          {error ? <p className="browser-error">{error}</p> : null}

          <div className="browser-foot">
            <span className="fine">
              {capture
                ? `${capture.source === "native" ? "native screenshot" : capture.source === "harness" ? "exact harness capture" : "rendered snapshot"} · ${capture.tool} · ${new Date(capture.at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
                : call.native
                  ? "screenshot follows the native call"
                  : "capture follows the harness call"}
            </span>
            {canLoad ? (
              <button
                type="button"
                className="link"
                onClick={() => setNonce((value) => value + 1)}
                disabled={phase === "loading"}
              >
                refresh
              </button>
            ) : null}
          </div>

          {!call.native && image && capture?.source === "snapshot" ? (
            <p className="fine browser-note">
              This harness was not configured for exact capture. This is a fresh Rigel b render of
              the URL it visited, not its own tab.
            </p>
          ) : null}
          {!call.native && image && capture?.source === "harness" ? (
            <p className="fine browser-note">
              Captured directly from the harness's Rigel b tab after its last browser action. It
              updates after each call, not continuously.
            </p>
          ) : null}
        </>
      )}
    </section>
  );
}

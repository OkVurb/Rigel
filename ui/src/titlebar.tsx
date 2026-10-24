import { useEffect, useRef, useState } from "react";
import type { Window } from "@tauri-apps/api/window";

import { RigelMark } from "./mark";

/**
 * The window chrome, inside the webview.
 *
 * Tauri runs with `decorations: false`, so this bar owns the drag region and the three window
 * controls. Because it is ordinary DOM, the custom pointer can be drawn over it — including the
 * drag state — which a native title bar could never allow.
 */
export default function Titlebar({ project }: { project: string | null }) {
  const windowRef = useRef<Window | null>(null);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;

    void (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      if (!alive) return;
      const current = getCurrentWindow();
      windowRef.current = current;

      const sync = async () => {
        try {
          const next = await current.isMaximized();
          if (alive) setMaximized(next);
        } catch {
          // A window that cannot answer keeps the last known state.
        }
      };

      await sync();
      if (!alive) return;
      try {
        const off = await current.onResized(() => void sync());
        if (alive) unlisten = off;
        else off();
      } catch {
        // Resizing still works; only the icon would lag.
      }
    })();

    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  const minimize = () => {
    void windowRef.current?.minimize().catch(() => {});
  };

  const toggle = () => {
    void (async () => {
      const current = windowRef.current;
      if (!current) return;
      try {
        await current.toggleMaximize();
        setMaximized(await current.isMaximized());
      } catch {
        // The OS refused the change; leave the icon as it was.
      }
    })();
  };

  const close = () => {
    void windowRef.current?.close().catch(() => {});
  };

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="titlebar-brand" data-tauri-drag-region>
        <RigelMark size={16} />
        <span>Rigel</span>
      </div>
      <span className="titlebar-project" data-tauri-drag-region>
        {project ?? "No project"}
      </span>

      <div className="titlebar-controls">
        <button className="titlebar-btn" onClick={minimize} title="Minimize" aria-label="Minimize">
          <svg width="12" height="12" viewBox="0 0 20 20" aria-hidden>
            <path d="M4 10h12" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
          </svg>
        </button>
        <button
          className="titlebar-btn"
          onClick={toggle}
          title={maximized ? "Restore" : "Maximize"}
          aria-label={maximized ? "Restore" : "Maximize"}
        >
          {maximized ? (
            <svg width="12" height="12" viewBox="0 0 20 20" fill="none" aria-hidden>
              <rect x="4" y="6.5" width="9.5" height="9.5" rx="1" stroke="currentColor" />
              <path d="M6.5 6.5V4.5h9v9H13.5" stroke="currentColor" />
            </svg>
          ) : (
            <svg width="12" height="12" viewBox="0 0 20 20" fill="none" aria-hidden>
              <rect x="4.5" y="4.5" width="11" height="11" rx="1" stroke="currentColor" />
            </svg>
          )}
        </button>
        <button
          className="titlebar-btn close"
          onClick={close}
          title="Close"
          aria-label="Close"
        >
          <svg width="12" height="12" viewBox="0 0 20 20" aria-hidden>
            <path
              d="M5 5l10 10M15 5L5 15"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
            />
          </svg>
        </button>
      </div>
    </header>
  );
}

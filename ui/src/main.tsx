import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import { isTauri } from "./api";
import Cursor from "./cursor";
import { MenuProvider } from "./menu";
import TrayPanel from "./tray";
import "./shell.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root missing from index.html");

async function isTrayWindow(): Promise<boolean> {
  if (
    window.location.hash === "#tray" ||
    new URLSearchParams(window.location.search).get("tray") === "1"
  ) {
    return true;
  }
  if (!isTauri()) return false;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow().label === "tray";
}

void isTrayWindow().then((trayWindow) => {
  if (!root) return;
  if (trayWindow) {
    document.documentElement.dataset.surface = "tray";
    createRoot(root).render(
      <StrictMode>
        <TrayPanel />
      </StrictMode>,
    );
  } else {
    createRoot(root).render(
      <StrictMode>
        <MenuProvider>
          <Cursor />
          <App />
        </MenuProvider>
      </StrictMode>,
    );
  }
});

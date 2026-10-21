/**
 * The only place the renderer talks to Rust.
 *
 * In a browser (`npm run dev`) there is no Tauri host, so these fall back to empty data and
 * the shell says so instead of pretending. In the app every call goes through `invoke`, and
 * the Rust core remains the single owner of state (handoff §3.2, S2).
 */

export interface Session {
  id: string;
  project_root: string;
  title: string | null;
  status: "active" | "idle" | "interrupted" | "ended";
  budget_cents: number | null;
  spend_cents: number;
  interventions: number;
  tasks_done: number;
  created_at: number;
  ended_at: number | null;
}

export interface ProjectInfo {
  root: string;
  name: string;
  db: string;
}

export interface Settings {
  theme: string;
  default_budget_cents: number;
  unattended_default: boolean;
}

export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

export const api = {
  isTauri,

  async openProject(path: string): Promise<ProjectInfo> {
    if (!isTauri()) {
      return { root: path, name: path.split(/[\\/]/).pop() || path, db: "(browser preview)" };
    }
    return call<ProjectInfo>("project_open", { path });
  },

  async listSessions(project: string): Promise<Session[]> {
    if (!isTauri()) return [];
    return call<Session[]>("session_list", { project });
  },

  async startSession(project: string, title?: string): Promise<Session> {
    if (!isTauri()) throw new Error("Start a session from the desktop app, not the browser preview.");
    return call<Session>("session_start", { project, title: title ?? null, budgetCents: null });
  },

  async getSettings(): Promise<Settings> {
    if (!isTauri()) {
      return { theme: "dark", default_budget_cents: 500, unattended_default: false };
    }
    return call<Settings>("settings_get");
  },
};

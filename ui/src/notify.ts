/**
 * D90: desktop notifications for three events, with one rule that matters more than the copy —
 * a turn the user is already watching never raises a notification. The WebView's Notification API
 * is the transport (Tauri 2 exposes it on Windows); a window that has not been granted permission
 * simply stays quiet.
 */
export type NotifyKind = "approval" | "finished" | "failed";

export interface NotifySettings {
  notify_approval: boolean;
  notify_turn_done: boolean;
  notify_turn_failed: boolean;
}

/** Ask once, from a user gesture (a Settings click), never at launch. */
export async function ensurePermission(): Promise<boolean> {
  if (typeof Notification === "undefined") return false;
  if (Notification.permission === "granted") return true;
  if (Notification.permission === "denied") return false;
  return (await Notification.requestPermission()) === "granted";
}

/**
 * `status` is the turn's terminal state. Callers pass `watching` when the window is focused and
 * the transcript is on screen, so the event that the user can already see stays silent.
 */
export function notify(
  kind: NotifyKind,
  title: string,
  body: string,
  settings: NotifySettings | null,
  watching: boolean,
): void {
  if (!settings || watching) return;
  const enabled =
    kind === "approval"
      ? settings.notify_approval
      : kind === "failed"
        ? settings.notify_turn_failed
        : settings.notify_turn_done;
  if (!enabled) return;
  if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
  try {
    new Notification(title, { body, silent: true });
  } catch {
    // A platform without a notification service must not break a turn.
  }
}

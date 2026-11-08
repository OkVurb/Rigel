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
  /** Q283: a local-time window in which every desktop notification stays silent. */
  quiet_hours_start?: string;
  quiet_hours_end?: string;
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
function minutes(value: string | undefined): number | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec((value ?? "").trim());
  if (!match) return null;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (hour > 23 || minute > 59) return null;
  return hour * 60 + minute;
}

/**
 * Q283: `start` inclusive, `end` exclusive. A window that crosses midnight (`22:00`–`07:00`)
 * wraps; a start equal to the end is off rather than "silent all day".
 */
export function inQuietHours(now: Date, start?: string, end?: string): boolean {
  const from = minutes(start);
  const to = minutes(end);
  if (from === null || to === null || from === to) return false;
  const at = now.getHours() * 60 + now.getMinutes();
  return from < to ? at >= from && at < to : at >= from || at < to;
}

export function notify(
  kind: NotifyKind,
  title: string,
  body: string,
  settings: NotifySettings | null,
  watching: boolean,
): void {
  if (!settings || watching) return;
  if (inQuietHours(new Date(), settings.quiet_hours_start, settings.quiet_hours_end)) return;
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

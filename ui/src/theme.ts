/**
 * The window's theme (D126, Q277).
 *
 * The saved setting is `dark`, `light` or `system`; the resolved value is written to
 * `<html data-theme>` so the CSS carries both palettes and components do not need to know about
 * the setting. `system` follows the OS preference and keeps following it while the window is
 * open. Dark stays the default, because a window that changes under the user without being
 * asked is worse than one they can switch.
 */
export type Theme = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";

/** D158: an explicit high-contrast palette, independent of the OS preference. */
export function applyContrast(high: boolean): void {
  if (high) document.documentElement.dataset.contrast = "high";
  else delete document.documentElement.dataset.contrast;
}

export function isTheme(value: string | null | undefined): value is Theme {
  return value === "dark" || value === "light" || value === "system";
}

/** The OS preference, read at the moment it is needed. */
export function systemTheme(): ResolvedTheme {
  try {
    return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
  } catch {
    return "dark";
  }
}

/** `system` and an unrecognised hand-edited value both resolve to a palette. */
export function resolveTheme(theme: string | null | undefined): ResolvedTheme {
  if (theme === "system") return systemTheme();
  return theme === "light" ? "light" : "dark";
}

/** Apply the saved value; an unrecognised one (hand-edited settings) is dark. */
export function applyTheme(theme: string | null | undefined): ResolvedTheme {
  const next = resolveTheme(theme);
  document.documentElement.dataset.theme = next;
  document.documentElement.dataset.themePreference =
    theme === "system" ? "system" : theme === "light" ? "light" : "dark";
  document.documentElement.style.colorScheme = next;
  return next;
}

/** The saved choice, as opposed to the palette it resolved to. */
export function themePreference(): string {
  return document.documentElement.dataset.themePreference ?? "dark";
}

/** The theme currently on the document, for components that draw outside CSS (xterm). */
export function currentTheme(): ResolvedTheme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

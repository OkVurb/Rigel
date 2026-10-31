/**
 * The window's theme (D126).
 *
 * The saved setting is `dark` or `light`; the resolved value is written to
 * `<html data-theme>` so the CSS carries both palettes and components do not need to know about
 * the setting. The OS's colour-scheme preference is deliberately not followed: the brief makes
 * dark the default, and a window that changes under the user without being asked is worse than
 * one they can switch.
 */
export type Theme = "dark" | "light";

export function isTheme(value: string | null | undefined): value is Theme {
  return value === "dark" || value === "light";
}

/** Apply the saved value; an unrecognised one (hand-edited settings) is dark. */
export function applyTheme(theme: string | null | undefined): Theme {
  const next: Theme = isTheme(theme) ? theme : "dark";
  document.documentElement.dataset.theme = next;
  document.documentElement.style.colorScheme = next;
  return next;
}

/** The theme currently on the document, for components that draw outside CSS (xterm). */
export function currentTheme(): Theme {
  return isTheme(document.documentElement.dataset.theme)
    ? document.documentElement.dataset.theme
    : "dark";
}

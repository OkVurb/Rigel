/**
 * D159: window scale and row density.
 *
 * Both are written to `<html>` so the CSS owns the layout and components keep no copy. The scale
 * is a `zoom` on `#root`, with its box divided by the same factor so the shell still fills the
 * window; the terminal keeps its own font size, because a PTY cell is measured rather than
 * scaled.
 */
export const UI_SCALES = [90, 100, 110, 125] as const;
export type UiScale = (typeof UI_SCALES)[number];

export function isUiScale(value: number | null | undefined): value is UiScale {
  return UI_SCALES.some((scale) => scale === Number(value));
}

/** Apply the saved values; an unrecognised scale is 100 rather than a fraction of one. */
export function applyDisplay(
  scale: number | null | undefined,
  compact: boolean | null | undefined,
): void {
  const next = isUiScale(scale) ? scale : 100;
  document.documentElement.style.setProperty("--ui-scale", String(next / 100));
  document.documentElement.dataset.density = compact ? "compact" : "roomy";
  document.documentElement.dataset.scale = String(next);
}

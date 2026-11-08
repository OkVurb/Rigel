# Reduced motion and contrast (roadmap 129)

Measured 2026-11-06 in the Playwright Chromium build on Windows, with both OS preferences
emulated together:

```
cd ui
npx playwright test tests/a11y.spec.ts
```

| Signal | Measured |
|---|---|
| `prefers-reduced-motion: reduce` | matched |
| `prefers-contrast: more` | matched |
| `--faint` on `--bg` | `#aeb5be` on `#0b0c0e` |
| Contrast ratio | **9.46:1** |
| Longest transition/animation on `.focus-chip` | **0.000001 s** |

The ratio clears AAA for the metadata pair. The motion assertion is not a visual judgement: the
test reads the computed duration, so a later stylesheet that reintroduces a spatial animation
fails the spec instead of merely looking slower.

The token-only rule lives in `ui/src/shell.css`: `@media (prefers-contrast: more)` raises
`--muted`, `--faint`, `--hairline` and `--sep`, and `@media (prefers-reduced-motion: reduce)`
collapses transitions and animations. The dark and light palettes are separately measured in the
same spec.

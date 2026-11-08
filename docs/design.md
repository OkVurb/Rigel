# Design system lock

**Status:** locked 2026-11-06. The window uses one token sheet, `ui/src/tokens.css`; a raw
colour outside its documented exceptions fails `node scripts/check-tokens.mjs`. The surface
table below is checked by the same script: every cell is filled and every surface names a
rhythm that follows from its job.

The doctrine is [Hallmark v1.0.0](https://github.com/nutlope/hallmark) applied to a
developer tool: instrument dark, one amber accent, hairline depth, tabular numbers, and no
surface that borrows its rhythm from a marketing page.

## Colour

Colour is defined once in `ui/src/tokens.css`. Dark is the default; `data-theme="light"` and
`prefers-contrast: more` / `data-contrast="high"` override only text, hairlines and shadows.

| Token | Job | Dark | Light |
|---|---|---|---|
| `--bg` | window background | `#0b0c0e` | `#fafaf9` |
| `--surface` | first raised plane | `#121417` | `#ffffff` |
| `--raised` | card inside a plane | `#171a1f` | `#f2f2f0` |
| `--well` | recessed comparison side | `#0e1013` | `#f7f7f5` |
| `--text` | primary copy | `#e6e8eb` | `#14161a` |
| `--muted` | secondary copy | `#9ba1a9` | `#565d67` |
| `--faint` | 10–11px metadata | `#7b838d` | `#616a74` |
| `--accent` | the one accent; fills | `#e9a23b` | `#e9a23b` |
| `--accent-text` | accent-coloured labels | `#e9a23b` | `#9c5d00` |
| `--verified` | success | `#4cc38a` | `#16794a` |
| `--danger` | failure and destructive intent | `#e5484d` | `#b42318` |

Measured contrast: dark text **15.3:1**, dark faint **5.0:1**, light text **16.9:1**, light
faint **5.5:1**, and the high-contrast metadata pair **9.46:1** (`#aeb5be` on `#0b0c0e`).
The light-theme measurements are recorded beside the token block and in
`docs/qa/reduced-motion.md`.

**Documented colour exceptions:** `ui/src/terminal.tsx` holds the xterm ANSI palette, which
needs concrete hex values rather than CSS variables; `ui/src/api.ts` holds fixture provider
badge colours used by the offline harness. No other file in `ui/src` may introduce a colour
literal.

## Type

| Role | Size | Weight | Family |
|---|---|---|---|
| Window title | 12px | 600 | `--ui` |
| Turn body | 13px | 400 | `--ui` |
| Transcript code | 12px | 400 | `--mono` |
| Metadata | 10–11px | 500 | `--ui`, uppercase tracking 0.04em |
| Statistic | 22px | 600, tabular | `--mono` |
| Approval command | 13px | 500 | `--mono` |

Nothing renders below 11px except the metadata scale's 10px tracking label, which is measured
at 5.0:1 or better. Numbers are `font-variant-numeric: tabular-nums`.

## Spacing and depth

The scale is 2 / 4 / 6 / 8 / 12 / 16 / 24 / 32px. A card is separated by a hairline, not a
shadow; a popover is the only surface with `--shadow-popover`. Radii are 4px (control), 5px
(card) and 7px (window). The status line is 28px.

## Motion

One easing (`cubic-bezier(0.2, 0, 0, 1)`), 120ms for state changes and 180ms for a panel.
Under `prefers-reduced-motion: reduce` every spatial transition collapses to a crossfade of
at most 0.001s; the test records the measured value. No motion is decorative.

## Surface jobs

Each screen has one job, one macrostructure and one primary action. Different jobs get
different rhythms: the composer is transcript-led, the approval is a single document, the
cost surface is stat-led, and the picker is a searchable list count. Two surfaces with
different jobs do not share a section rhythm.

| Surface | Job | Macrostructure | Rhythm | Primary action |
|---|---|---|---|---|
| Composer / session | watch work and keep control | split workspace, transcript-led | stacked turn rows, composer pinned at the bottom | send |
| Approval card | decide safely | single document, command first | command → blast radius → scope → snapshot, one screen | approve once |
| Model picker | choose a door | searchable list with provider groups | search field, grouped rows, capability line | use model |
| Cost / usage | understand spend | stat-led with a ledger below | three stat cards, then a table | export |
| Onboarding | configure once | guided checklist of detected facts | one check per row, no fake progress | finish setup |
| Diff | judge a change | document with a provenance rail | file header, hunk rows, verdict rail | apply / revert |
| CMS admin | publish content | dashboard plus console navigation | section cards, then an editing console | edit FAQ |

The session and the approval deliberately do not share a rhythm: the session is a stream the
user watches, the approval is a page the user reads once. The cost dashboard is stat-led
because the job is comprehension, not action; the picker is a list because the job is
selection. Empty states use the same rhythm as their surface and offer one real action.

## Responsive contract

The minimum window is 960×600. The webview and the marketing site are verified at 320, 375,
414 and 768px: no horizontal scroll, and no clickable label wraps to two lines at the
minimum window width. Focus mode keeps its exit affordance visible at every size.

## Verification

- `node scripts/check-tokens.mjs` — raw-colour lint plus this table's completeness.
- `node scripts/check-claims.mjs` — every percentage in public or product copy has a
  recorded source or a labelled placeholder.
- `npm --prefix ui run build` — the token import compiles.
- `npx playwright test a11y states keyboard` — contrast, eight states and the keyboard path.

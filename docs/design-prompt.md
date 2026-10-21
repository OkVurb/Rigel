# Design prompt — the app's visual system

Paste the block below into a design or coding agent. It is written to be used *as-is*; everything after it explains the choices and what to adjust.

---

```text
You are designing the visual system for a Windows-first desktop AI coding app.
Stack: Tauri 2 + React + TypeScript. Dark-first, 1280x800 minimum, 1920x1080 target.

DESIGN THESIS
This is an instrument, not a chatbot. It should feel like a measuring device you trust:
quiet surfaces, dense information, exact numbers, no decoration that doesn't carry meaning.
The user opens it many times an hour. Every screen should answer "what is the agent doing,
what will it cost, what did it change" within one glance.

ANCHORS (steal the discipline, not the skin)
- Linear: information density and restraint; hairline borders instead of shadows.
- Zed: speed as an aesthetic; chrome that disappears.
- Raycast: one command palette that reaches everything.
- Warp: terminal output as structured blocks, not a wall of text.
Do NOT copy any of their layouts literally.

ANTI-ANCHORS (reject on sight)
- Purple/blue "AI" gradients, gradient meshes, glassmorphism, glow.
- Chat bubbles for agent output. Use full-width transcript blocks with a role gutter.
- Emoji as icons. Rounded 24px cards. Drop shadows above 8% opacity.
- Skeleton shimmer on anything that resolves under 400ms — show nothing, then show it.
- Stock "Copilot" three-column symmetry. Our layout is deliberately asymmetric.

COLOR — dark default, light required
  bg          #0B0C0E   app background
  surface     #121417   panes, cards
  raised      #171A1F   hover, selected, popovers
  hairline    rgba(255,255,255,0.07)   1px borders; depth comes from borders, never shadows
  text        #E6E8EB   primary
  text-muted  #9BA1A9   secondary
  text-faint  #6B727C   tertiary, timestamps, metadata
  accent      #E9A23B   amber — the only accent. Actions, focus, the active thing.
  verified    #4CC38A   "checks passed", installed
  danger      #E5484D   destructive, failed checks, critical capability flags
  info        #6E9BFF   links and neutral notices only
Amber is the identity: it reads as measurement, and it is not what any competitor uses.
Never introduce a second accent hue. Semantic colors are for state only, never decoration.
Light theme: invert surfaces to #FAFAF9 / #FFFFFF / #F2F2F0, keep the same amber, darken text
to #14161A — and verify contrast, don't just flip values.

TYPE
  UI:    Inter (fallback system-ui). Weights 400/500/600 only. Never 700+.
  Code:  JetBrains Mono — also used for every number in the UI (cost, tokens, timing).
  Scale: 28 (screen title) / 20 (section) / 15 (subhead) / 13 (body, default) / 12 (meta)
         / 11 (labels, uppercase, +0.06em tracking). Nothing below 11px, ever.
  Line height: 1.45 body, 1.3 headings, 1.6 code.
  Numbers are tabular (font-variant-numeric: tabular-nums) so meters don't jitter.

SPACE, RADIUS, DENSITY
  4px base. Steps: 4 / 8 / 12 / 16 / 24 / 32. Nothing else.
  Radius: 6px controls, 8px panes, 999px only for status pills.
  List rows: 28px tall, 13px text. Controls: 28px. This is a dense tool, not a landing page.
  Sidebar 320px (resizable 260–420). Icon rail 48px. Status bar 28px.

LAYOUT SHELL
  ┌────┬──────────────┬─────────────────────────────────────┐
  │ 48 │   context    │   content                           │
  │ px │   320px      │   (flex)                            │
  │rail│   list       │                                     │
  ├────┴──────────────┴─────────────────────────────────────┤
  │ status bar 28px: │ model · branch │ cost $0.42 · 18.2k tok │ ● 3 skills │
  └──────────────────────────────────────────────────────────┘
The status bar is always visible. Cost and token count are permanent residents, not a panel
you have to open. This is the product's promise made structural.

THREE SCREENS TO DESIGN

1. PACKAGE BROWSER (the wedge — spend most of the effort here)
   - Search field at top, autofocused, with a `/` badge. Results appear from cache instantly.
   - Result row: name (13px, 500), one-line description (12px muted), then a signal strip:
     safety grade badge (A–F), install count, a small "4 variants" chip when the cluster is
     bigger than one, and "Curated" for seed entries.
   - Detail view: rendered SKILL.md on the left (readable measure, max 72ch), and on the right
     a fixed "What this touches" panel — file tree, capability list (network / writes / spawn /
     secrets), and the exact evidence lines with file:line. Install button pinned in the header.
   - Duplicate disclosure: "4 near-identical variants · showing the best maintained" expandable
     to a diff-per-variant list. Never hide the duplicates.
   - Evidence block for token-saving skills: one table, MEASURED column first, ADVERTISED second,
     with the source and date. Unsubstantiated claims get a muted "unsubstantiated" tag.

2. SESSION (chat + diff + terminal)
   - Transcript is full-width blocks, role gutter on the left (2px amber bar = assistant).
   - Tool calls collapse to a single 28px line: icon · command · duration · exit code.
     Expandable. Failed checks are red on the line itself, not hidden in a panel.
   - Diff pane: side-by-side optional, unified default, per-hunk accept/reject on hover.
   - Approval prompt: inline in the transcript, never a modal. Shows the exact command plus
     scope buttons: Once · Session · Project · Always.
   - "Verified" badge appears only when the check command actually returned 0 — with the
     command and exit code visible on hover.

3. COMMAND PALETTE
   - Ctrl+K. 560px wide, top 15% of the viewport. Fuzzy results, grouped by verb
     (Install, Open, Run, Toggle). Show the keybinding per row on the right.
   - Empty state teaches: three example queries, no illustrations.

MOTION
  120ms ease-out for hover/selection, 160ms for pane transitions. No bounce, no spring,
  no parallax. Nothing animates on first paint. Respect prefers-reduced-motion by removing
  all non-essential transitions.

STATES — every one must be designed, none left to default
  empty (with a next action) · loading (>400ms only) · error (what failed + retry) ·
  installed (version + update-available dot) · installing (inline progress on the row) ·
  offline (cached results, muted banner) · permission-denied · unverified-source warning

ICONOGRAPHY
One family, 16px, 1.5px stroke, geometric. Never mix families. Never fill+stroke together.

COPY VOICE
Verb-first imperatives, sentence case, no exclamation marks, no "Oops", no emoji.
"Install skill" not "Let's get started!". Errors state the cause and the fix in one line.

ACCESSIBILITY — non-negotiable
  AA contrast on all text. 2px accent focus ring, never removed. Full keyboard path to every
  action. Visible focus order. 200% zoom without clipping. Screen-reader labels on every
  icon-only button.

DELIVERABLES
  1. tokens.json — color, type, space, radius, motion.
  2. Three screens above, at 1440x900, dark and light.
  3. Component set: row, badge, chip, meter, inline approval, diff hunk, evidence table.
  4. Keyboard map overlay.
  5. One paragraph justifying each decision that departs from this brief.

ACCEPTANCE CHECKS
  - No purple, no gradient, no glow, no shadow anywhere.
  - The cost meter is visible on all three screens.
  - 13px is the default body size; nothing renders below 11px.
  - Every list row is 28px and every number uses tabular figures.
  - Every interactive element has a visible focus state.
```

---

## Why the brief is shaped this way

**Amber, not blue.** Every competitor — Cursor, Claude, Codex, Copilot — sits in purple-to-blue. A single amber accent on near-black reads as instrumentation and makes the app recognisable in a screenshot, which matters for a product competing on being *trustworthy*.

**The status bar is a design decision, not layout.** The teardown found cost opacity to be the most emotional complaint across all five competitors. Putting cost and tokens in permanent chrome, in tabular figures, makes "we are accountable" a structural property rather than a settings page.

**"What this touches" gets equal weight to the description.** Skills execute scripts. The install decision is a security decision, and the panel is the visual expression of the only feature in the gap table that no competitor has.

**Measured-before-advertised in the evidence table is deliberate.** Column order is the argument. Putting the advertised number first means the user reads the marketing, then the correction.

**Density over whitespace.** 28px rows and 13px text will look "too tight" to a consumer-app designer and exactly right to someone who lives in an IDE. That is the correct side to err on, and it is cheaper to loosen later than to compress.

**Dropped from the brief on purpose:** an onboarding tour (the empty states should teach), a settings page design (nothing in it yet), and any illustration style (illustrations age badly and this product's aesthetic is atonal).

---

## Open design questions for you

1. **Light theme: ship it at all?** A coding tool that lives beside an editor is usually dark-only. Supporting light doubles the QA surface for a small gain — but some users will refuse to open a dark app.
2. **Transcript density.** Collapsed tool calls (one line each) is my recommendation; a "verbose mode" showing streaming output by default would match Warp but fights the instrument thesis.
3. **Where does the package browser live** — a real pane in the sidebar, or a full-screen overlay invoked by `Ctrl+Shift+P`? A pane makes it ambient and discoverable; an overlay keeps the session screen clean.

# Accessibility audit

**Audited 2026-11-06.** The session and approval flows are the two surfaces the release gate
names. This page records what is automated, what was measured, and the one exception that
needs a human.

## Automated gate

`ui/tests/axe.spec.ts` runs in the UI CI job (the job's `npm run test:e2e` executes the whole
Playwright directory). It uses `@axe-core/playwright` with WCAG 2.0/2.1/2.2 A and AA tags on:

| Flow | Result |
|---|---|
| Session (open, transcript, composer) | no axe violations |
| Approval (attention → command card) | no axe violations |
| Dense icon controls | every visible icon button is at least 20×20, and the pin controls are 26×26 |

Run: `npx playwright test axe` → **3 passed**. No rule is disabled or suppressed. The three
violations found during the audit were fixed in the product:

- the two pane separators now carry `aria-valuemin`, `aria-valuemax` and `aria-valuenow`;
- the changed-skill pill uses `--danger-text` for AA contrast at 9px;
- the turn outline, filter chips, focus chip, changed-skill pill and unpin control now meet
  the 24px target-size rule (26px for adjacent pin controls).

## Contrast

`ui/tests/a11y.spec.ts` measures both palettes live and asserts every text/faint pair is at
least 4.5:1. The high-contrast metadata pair was measured at **9.46:1** (`#aeb5be` on
`#0b0c0e`) and is recorded in [reduced-motion.md](reduced-motion.md). The light palette was
measured in the token block in `ui/src/tokens.css`: muted 6.65 / 5.93, faint 5.49 / 4.90,
accent text 5.28 / 4.71, verified 5.43 / 4.84, danger 6.57 / 5.87 on the worst pairing.

## Keyboard

`ui/tests/keyboard.spec.ts` completes open project → send → approve → diff with Tab and Enter
only, and fails when a stop shows no visible focus indication. `ui/tests/states.spec.ts`
enforces the eight-state table and the reachable focus/hover/disabled states.

## Target size

The rule is 24×24 CSS pixels for interactive controls, with 26×26 for adjacent pin controls
that share a row and have no spacing exception. Inline prose links are exempt. The outline
marks keep a 24px hit target while drawing a 4–6px bar inside it.

## Screen-reader pass

| Item | Owner | Date | Status |
|---|---|---|---|
| Manual NVDA/VoiceOver pass over session, approval and recovery | release-qa | 2026-11-06 | owned exception: not automatable on this machine |

The exception is dated and owned, so a new automated violation still fails CI; the manual
pass is a release-checklist item, not a silent gap. Accessible names, live regions and the
transcript's labelled articles are asserted by role queries in the Playwright suite.

## CI

A new violation fails the `ui` job through `npm run test:e2e`. The eight-state table, the
keyboard path, the contrast assertions and the axe flows run in the same job, so a control
that regresses has one place to look.

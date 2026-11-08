# Sprint 0.3.1 — proof and safety (Wave 1)

**Goal:** make the first ten Wave-1 commitments executable and verifiable. This sprint is the
proof layer under the rest of [roadmap-0.3.md](roadmap-0.3.md): a native window test, a migration
rehearsal, credential and secret guards, a file jail, shared rate limiting/cancellation, a route
inspector, an approval card, and the state/keyboard audits. It does not add a new product surface.

**Exit criteria (all must be true):**

- [x] Ten work packages below are merged with their named tests.
      Evidence: the 0.3.0 preview tree; `cargo test --workspace` passes 53 test binaries and the
      named per-package tests are linked from `docs/roadmap-0.3.md`.
- [x] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace`, `npm --prefix ui run build` and `node scripts/check-docs.mjs` pass.
      Evidence (2026-11-08): all five exit 0; the UI Playwright suite is 55/55.
- [ ] The native-window test runs on a Windows CI runner and fails when an IPC command is renamed.
- [x] The migration rehearsal upgrades every historical project/index schema prefix.
      Evidence: `cargo test -p rigel-core --test schema` -> 13 passed; `node scripts/rehearse-migrations.mjs` -> 17 prefix runs.
- [x] No credential or secret appears in a diagnostic, log or crash payload in the test corpus.
      Evidence: `cargo test -p rigel-core --lib redaction` -> 6 passed; the diagnostics corpus has no known-shaped secret.
- [x] A path outside the project cannot be read or written through `..`, a symlink, a junction,
      a UNC path or a case variant. Evidence: `cargo test -p rigel-core --test jail` -> 4 passed.
- [x] A 429 from one provider does not produce parallel retries from other agents, and a cancelled
      stream is billed at most once. Evidence: the limiter concurrency test and
      `cargo test -p rigel-core --test native a_cancelled_stream_is_billed_once_and_closes_as_cancelled`.
- [x] The approval card and every control it touches pass the eight-state and keyboard audits.
      Evidence: `docs/qa/eight-states.md`, `ui/tests/states.spec.ts`, `ui/tests/keyboard.spec.ts` and `e2e/specs/approval.spec.ts`.

**Not in this sprint:** roadmap items 3 (fuzz targets), 4 (crash-log upload) and 22–23 (fallback and
retry classes) are Wave 2; 60 (pre-write secret scan), 64 (batched approvals), 75 (keychain parity)
and 78 (updater E2E) are Wave 3. Both wave lists are fixed in [roadmap-0.3.md](roadmap-0.3.md) and
checked against this table by `scripts/check-roadmap.mjs`.

## Work packages

| WP | Roadmap items | Size | Depends on | Primary files |
|---|---|---|---|---|
| 1. Native window driver | 1 | L | CI Windows runner | `crates/rigel-app/`, `e2e/` |
| 2. Migration rehearsal | 2 | M | — | `crates/rigel-core/tests/schema.rs`, `migrate.rs` |
| 3. Supply-chain CI lane | 9 | M | — | `.github/workflows/ci.yml`, `deny.toml` |
| 4. Credential shape guards | 24 | M | — | `agent/providers.rs`, `agent/vertex.rs`, `credentials.rs` |
| 5. Secret redaction | 59 | M | — | `redaction.rs`, `diagnostics.rs`, `rigel-adapters` |
| 6. File-jail traversal tests | 71 | M | — | `agent/tools.rs`, `tests/tools.rs` |
| 7. Shared limiter + cancellation | 16, 17 | L | — | `agent/limiter.rs`, `coordinator.rs`, provider clients |
| 8. Route inspector and CLI | 13, 14 | M | routing module | `agent/routing.rs`, `rigel-app`, `rigel-cli`, `ui/` |
| 9. Approval card v2 | 135 | M | WP10 | `ui/src/session.tsx`, `agent/approval.rs` |
| 10. Eight-state and keyboard audits | 127, 128 | M | — | `ui/src/shell.css`, `ui/tests/` |

## 1. Native window driver

**Why:** every other guarantee is asserted below the window; this is the only test that proves the
window itself can start, open a project, send a turn, answer an approval and show the diff.

**Approach:** WebdriverIO over `tauri-driver` for the native shell, because Playwright does not
speak the WebDriver protocol that Tauri exposes. Keep a Playwright suite for the webview DOM where
it can attach; the native smoke is the gate.

**Tasks:**

1. Add `e2e/` with `wdio.conf.ts`, a `package.json` script and a fixture project under
   `e2e/fixtures/project/`.
2. Build the debug app (`cargo tauri build --debug`) and start `tauri-driver` with the matching
   `msedgedriver` on Windows.
3. The spec: launch, open the fixture, select the scripted model, send "list the files", approve a
   read-only tool if asked, assert the turn row and the transcript text.
4. A second spec renames an IPC command in a test build and asserts the driver fails with a clear
   message (the "fails when broken" proof).
5. Cache the Rust and WebDriver downloads in CI; cap the job at 15 minutes.

**Done when:** `npm run test:window` passes locally and on the Windows CI runner, and the
deliberately renamed IPC command fails the suite.

**Risk:** Tauri's WebDriver support on Windows needs a matching Edge driver. Pin the driver version
to the runner's Edge build and fail with the versions in the error.

## 2. Migration rehearsal

**Why:** a schema change that only works on an empty database is the most expensive release bug.

**Approach:** deterministic prefix rehearsal, no binary fixture to rot. For every project and index
migration prefix, build a database at that prefix, insert representative rows, apply the rest, and
assert the rows and schema.

**Tasks:**

1. Add `every_project_prefix_upgrades` and `every_index_prefix_upgrades` to
   `crates/rigel-core/tests/schema.rs`, using `apply` with the prefix slice.
2. Seed one row per table that has a foreign-key-safe insert path; keep the seed helper next to the
   test.
3. Assert the final `schema_migrations` list, the preserved row ids, and a reopen with no work.
4. Add `scripts/rehearse-migrations.mjs` as the release wrapper: run the tests, copy a scratch DB,
   reopen it with the current binary, print the versions.

**Done when:** the tests pass for every prefix and the release checklist can cite one command.

**Risk:** a historical migration may not run alone because it references a later table. If so, seed
at the nearest prefix the migration supports and record the exception in the test.

## 3. Supply-chain CI lane

**Why:** a dependency advisory or a committed key should fail a PR, not a release.

**Tasks:**

1. Add a `supply-chain` job to `.github/workflows/ci.yml`: `cargo audit`, `cargo deny check`,
   `gitleaks detect --no-git --source .`, and `node scripts/make-sbom.mjs`.
2. Add `deny.toml` with the workspace licence set, advisory denial and duplicate-crate warnings
   that are already accepted in `docs/release-checklist.md`.
3. Pin every action by full commit SHA; keep the Dependabot/Renovate path documented.
4. Add a tiny gitleaks fixture in a test branch to prove a fake key fails the job; do not commit the
   fixture to `main`.
5. Upload the SBOM as a build artifact on every run.

**Done when:** the job is required on `main`, a local run is clean, and the fixture proves failure.

**Risk:** unmaintained transitive crates can make `cargo deny` noisy. Allowlist only with a dated
reason in `deny.toml`, matching the existing audit record.

## 4. Credential shape guards

**Why:** a wrong-shaped credential produces a provider 401 that looks like a routing bug; the Vertex
guard already proved the remedy is better than the error.

**Tasks:**

1. Add a `CredentialShape` check next to the provider table: documented prefixes only
   (`sk-ant-`, `sk-or-`, `gsk_`, `xai-`, `AIza`, `ya29.`, JWT, service-account JSON), with an
   "unknown shape" state that is allowed rather than guessed.
2. Call it from `providers::credential` before the request, preserving the provider's own error for
   a genuinely rejected key.
3. Keep the Vertex-specific messages (`AIza…`, `{…`, `eyJ…`) and make them the template for the
   others.
4. Table-driven tests: one accepted prefix, one wrong prefix and one unknown per provider.

**Done when:** every documented provider has a wrong-shape test and the message names the remedy
and the provider.

**Risk:** a vendor changes its prefix. Unknown shapes stay allowed, so a new prefix is not a false
rejection; only a documented-but-wrong shape is refused.

## 5. Secret redaction

**Why:** output, logs, diagnostics and crash payloads are copied into issues; a key must not leave
the machine through them.

**Tasks:**

1. Extend `crates/rigel-core/src/redaction.rs` with PEM blocks, AWS `AKIA…`, GitHub `ghp_…`,
   Slack `xoxb-…`, Stripe `sk_live_…`, Google `AIza…`, JWTs and the provider keys already known.
2. Make `diagnostics.rs` and the adapter redactors call the same core function; delete duplicate
   pattern lists.
3. Redact before truncation and before JSON serialisation, and keep the existing idempotence test.
4. Add a corpus test with a real-shaped fake for each pattern and a benign lookalike that must
   survive (`sk-etch`, a short `AIza` prose word).

**Done when:** the corpus passes, idempotence holds, and a crash/diagnostic fixture contains no
known-shaped secret.

**Risk:** over-redaction hides debugging context. Keep the replacement labelled (`[redacted:key]`)
and record the pattern name, never the value.

## 6. File-jail traversal tests

**Why:** the file tools claim a project jail; Windows junctions and case-insensitive paths are the
places that claim usually breaks.

**Tasks:**

1. Add fixtures in `crates/rigel-core/tests/tools.rs`: `..` traversal, an absolute outside path, a
   symlink (skip if the OS refuses to create it), a directory junction on Windows, a UNC path and
   a case-variant of the project root.
2. Assert read, write, edit and list all refuse, and that the error names the resolved path and the
   root without leaking the outside file's contents.
3. Add one allowed case with a symlink that stays inside the root, so the test is not "refuse all
   links".
4. Run the Windows-only cases in CI and record which cases are skipped on other hosts.

**Done when:** every escape shape is refused and the inside-link control passes.

**Risk:** creating a symlink needs a privilege on Windows; use a junction for the directory case and
mark the file-symlink case skipped with the exact reason.

## 7. Shared limiter and cancellation accounting

**Why:** several agents on one provider must not multiply a 429, and a cancelled stream must not be
billed twice.

**Tasks:**

1. Add `crates/rigel-core/src/agent/limiter.rs`: one token bucket per provider, shared through the
   process, honouring `Retry-After`, with an injectable clock for tests.
2. Route the existing OpenRouter and Anthropic retry loops through it; keep the provider-specific
   status handling.
3. Add a test with three concurrent callers and one token: exactly one sends, the others wait and
   then succeed, and the order is recorded.
4. In `coordinator.rs`, assert a cancelled stream records its partial usage once and the turn ends
   with `cancelled`, not `failed`.
5. Expose the wait in the transcript as a `rate_limit` row, never as a silent delay.

**Done when:** the concurrency test is deterministic and the cancellation test bills one partial
usage.

**Risk:** global state leaks between tests. Inject the limiter into the client rather than using a
`static` in tests; the production wiring can own a process-wide instance.

## 8. Route inspector and CLI

**Why:** the 0.2.0 routing repair is only trustworthy if a user can see which door a turn will use
before sending it.

**Tasks:**

1. Add `RouteExplanation` to `agent/routing.rs`: selected pair, requested pair, resolved pair,
   reason (`picker`, `preset`, `saved`, `bare-id-repair`, `refused`) and the mismatch sentence.
2. Add the `route_explain` Tauri command and a small inspector in the composer/model picker; it
   reads the same resolver, never a second copy.
3. Add `rigel route explain --provider … --model … --json` with the same fields.
4. Tests: a picker pair beats a stale save; a bare id repairs only through its owner; an unknown
   door is refused; the CLI JSON matches the core struct.

**Done when:** the inspector and CLI agree with `turn_send` on the same inputs.

**Risk:** a second resolution at display time can drift from send time. Pass the saved/requested
pair into the same function and label the result as a preview.

## 9. Approval card v2

**Why:** the card is the product's trust surface; it must read as a document, not a modal with a
paragraph.

**Tasks:**

1. Put the exact command first, the blast radius second, the scope third and the snapshot state
   last, in `ui/src/session.tsx`.
2. Reuse the existing `BlastRadius` data; a `cannot_determine` row is visually distinct and never
   collapsed.
3. One primary action; deny and "ask me later" are secondary. No auto-focus on approve.
4. Add the card to the eight-state/keyboard harness and test with a keyboard-only path.
5. Record the decision through the existing approval bus; do not add a second channel.

**Done when:** the card passes the audit, a keyboard user can deny without touching the mouse, and
the transcript records the exact scope.

**Risk:** the card is used by the window and the CLI. Change the shared data shape only if both
surfaces can render it; the CLI's plain-text question is the fallback.

## 10. Eight-state and keyboard audits

**Why:** Hallmark's rule is that every interactive element has default, hover, focus, active,
disabled, loading, error and success states, and that the core flow works without a mouse.

**Tasks:**

1. Add an audit table to `docs/qa/` listing every control in the session, approval, picker and
   settings surfaces and the eight states; a missing state is a row with an owner.
2. Centralise the states in `ui/src/shell.css` with tokens; no inline colour.
3. Add `ui/tests/a11y.spec.ts` with axe on the session and approval screens, and a keyboard-only
   path through open project → send → approve → diff.
4. Fix every failure; do not add a suppression without a dated reason.

**Done when:** the audit table has no unexplained gaps, axe is clean on the two flows, and the
keyboard path passes in CI.

**Risk:** axe cannot judge whether a focus ring is visible. Keep the keyboard path and a contrast
check as separate assertions.

## Order and parallelisation

1. **Week 1:** WP4 (credential guards), WP5 (redaction), WP6 (file jail) in parallel; WP3 (CI lane)
   lands first so the others are gated.
2. **Week 2:** WP7 (limiter/cancellation) and WP8 (route inspector) in parallel; WP2 (migration
   rehearsal) starts once WP3 is green.
3. **Week 3:** WP1 (native driver) and WP10 (audits) in parallel; WP9 (approval card) follows WP10.
4. **Close:** run the exit criteria, record the evidence in the sprint issue, and hand the deferred
   items listed above to their Wave 2 and Wave 3 owners.

## Evidence to attach at close

- The window test log and the deliberately-broken run.
- The migration rehearsal output (every prefix, final versions).
- The supply-chain job URL and the gitleaks fixture result.
- The credential-shape test table.
- The redaction corpus result.
- The file-jail case table (pass/skip with reason).
- The limiter concurrency log and cancellation bill.
- The route inspector screenshot and CLI JSON.
- The eight-state table, axe output and keyboard run.

## Research check

All 35 repositories in [roadmap-0.3.md](roadmap-0.3.md) returned HTTP 200 from GitHub on 6 Nov
2026. The sprint uses Tauri/WebdriverIO for WP1, SQLx/SQLite for WP2, RustSec/cargo-deny/gitleaks
for WP3, Tokio for WP7, Playwright/axe for WP10, and Radix's focus-management patterns for WP9.

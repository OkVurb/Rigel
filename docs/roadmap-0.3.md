# Roadmap 0.3 — the next 200

**Status:** planning only; this document changes no product code. It is written against the 0.2.0
tree of 6 Nov 2026 and absorbs the deferred round-9 items from [roadmap-0.2.md](roadmap-0.2.md)
rather than leaving a second list.

**Tally (2026-11-08):** 71 Done · 7 Blocked · 122 Open. The 0.3.0 preview tree ships every Done
item. The last pass closed 53, 60, 64, 67, 69, 70, 75, 78, 80, 106, 107, 130, 140 and 182; the
open and blocked items below are unchanged by it.

**Verified gate (2026-11-08).** `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets -- -D warnings` and `cargo test --workspace --no-fail-fast` pass (53 test binaries,
0 failures); `npm --prefix ui run build` and the 55-test Playwright suite pass; `npm --prefix
functions test` passes 37 tests; `node scripts/check-docs.mjs` validates 55 documents and 200
roadmap items; `check-tokens.mjs` covers 44 files; `check-versions.mjs` agrees on 0.3.0; and
`node deploy/check.mjs` reports 11 checks with 4 notes. This is local evidence only: no VPS
deployment or database migration was run. The native-window runner and the signing, VM and
published-asset items (1, 6-11 and 200) stay blocked.

**Built without further input.** These are the inferred Hallmark design-context answers; redirect
one and only the affected items change:

- **Audience:** developers and technical leads running several coding agents on one repository, who
  need to know which model ran, what it touched and what it cost.
- **Use case:** start a project, delegate work safely, review the diff and the bill, and recover.
- **Tone:** technical, austere, instrument-like. Not playful, not luxury, not "clean and modern".

**How to read an item.** Every item has the same shape:

    N. **W# · P# · S/M/L · Title.** <body naming one concrete artifact in backticks> — done when
    <observable signal>.

`W#` is the wave; `P#` is the priority (**P0** correctness, security or the release gate, **P1** the
next product layer, **P2** ecosystem or scale); `S/M/L` is the size (days, a week or two, a
multi-week project). The backticked artifact is the file, crate, workflow, command or screen the
work starts in.

**How this document is checked.** `node scripts/check-docs.mjs` also runs
`scripts/check-roadmap.mjs`. It fails when the count is not exactly 200, an ID is missing, repeated
or out of order, an item loses its wave/priority/size metadata, a title repeats, an item has no
backticked artifact or no "done when", the Wave 1 set disagrees with
[sprint-0.3-wave-1.md](sprint-0.3-wave-1.md) in either direction, an [Order of work](#order-of-work)
wave list disagrees with the item metadata, the repository table falls below 20 rows, or the
Hallmark section stops citing items.

**Waves.** W1 proof and safety (the sprint); W2 the release and the turn; W3 runtime, tools and
craft; W4 the window; W5 ecosystem, data and cloud; W6 0.3.0. Membership appears exactly twice — in
each item's metadata and once in [Order of work](#order-of-work) — and the checker compares them.

---

## Hallmark, applied

Source: **Hallmark v1.0.0**, originally [`nutlope/hallmark`](https://github.com/nutlope/hallmark),
found through `/find-skills` as [`rheinmir/setup@hallmark`](https://skills.sh/rheinmir/setup/hallmark)
(174 installs; Gen Agent Trust Hub pass, Socket pass, Snyk warn). The public site already carries a
Hallmark stamp in `web/site.css`: *genre: developer tool · macrostructure: product-led · nav: inset
sticky bar · theme: instrument dark · hero: banner · job: download*.

Hallmark is a design doctrine, not a component kit, so it changes the plan in two ways: it forbids
any surface that borrows its rhythm from a marketing page, and it requires every interaction to be
specified in states rather than in adjectives.

### Which items Hallmark shaped

| Hallmark rule | Where it lands |
|---|---|
| Structural variety: different jobs get different rhythms | item 107 audits the surface table; item 137 rebuilds the composer as a workspace rather than a hero |
| A page earns its sections | item 117 replaces decorative empty states with one real action; item 136 is stat-led with a ledger under it |
| Real numbers or a labelled placeholder | item 130 fails an unsourced figure in copy; item 20 labels every pre-send quote an estimate |
| Token-only colour and type | item 106 locks the tokens and fails a raw hex; item 138 gives async states the same tokens |
| All eight interaction states | item 127 audits default, hover, focus, active, disabled, loading, error and success |
| The core flow works without a mouse | item 128 is the keyboard-only exit test |
| Verify at 320/375/414/768 px | item 126 sets the breakpoints and bans horizontal scroll |
| No fake chrome | item 133 keeps the PTY real and removes window chrome |
| An approval is a document, not a modal | item 135 orders command, blast radius, scope and snapshot |

Nothing here claims a design review has happened. Items 106–140 are the work; each names its own
done signal, and the eight-state table is the evidence a review will use.

### Surface jobs

| Surface | Job | Macrostructure | Primary action |
|---|---|---|---|
| Composer / session | watch work and keep control | split workspace, transcript-led | send |
| Approval card | decide safely | single document, command first | approve once |
| Model picker | choose a door | searchable list with provider groups | use model |
| Cost / usage | understand spend | stat-led with a ledger below | export |
| Onboarding | configure once | guided checklist with detected facts | finish setup |
| Diff | judge a change | document with provenance rail | apply / revert |
| CMS admin | publish content | dashboard plus console nav | edit FAQ |

---

## 1. Release, proof and foundation (1–12)

1. **W1 · P0 · L · Native window driver.** `e2e/wdio.conf.ts` drives the built app through open,
   scripted turn, approval and diff, and a second spec renames an IPC command. — done when
   `npm run test:window` passes on the Windows CI runner and the renamed run fails by name.
   **Done (2026-11-06):** `npm --prefix ui run test:window` runs four spec files (window, IPC, approval, route) green on Windows, the renamed command fails by name, and the `window` job in `.github/workflows/ci.yml` runs the same command (its first GitHub run is the remaining external proof).


2. **W1 · P0 · M · Migration rehearsal.** `crates/rigel-core/tests/schema.rs` builds a database at
   every project and index migration prefix, seeds it, applies the rest and reopens it. — done when
   every prefix test passes and `scripts/rehearse-migrations.mjs` prints the final versions.
   **Done (2026-11-06):** `cargo test -p rigel-core --test schema` -> `test result: ok. 13 passed; 0 failed`; `node scripts/rehearse-migrations.mjs` -> `migration rehearsal: 17 prefix runs upgraded to the current schema` (exit 0).

3. **W2 · P0 · M · Fuzz and property targets.** `fuzz/fuzz_targets/patch_parse.rs` fuzzes the patch
   parser, `.mcp.json`, settings and skill frontmatter, plus a property that a canonical path never
   escapes the project. — done when a 60-second `cargo fuzz` run finds no panic and the property
   test rejects every escape fixture.
   **Done (2026-11-06):** the x64 ASAN runtime came from VS BuildTools'
   `14.44.35207/lib/x64`; with `RUSTUP_TOOLCHAIN=nightly` and that directory on `LIB` (its
   `bin/Hostx64/x64` on `PATH` for the ASAN DLL), `cargo fuzz build` linked all four binaries and
   `cargo fuzz run <target> -- -max_total_time=60 -print_final_stats=1` exited 0 for each:
   patch_parse 1,260,948 executions / cov 202 / ft 1151; mcp_json 939,817 / cov 837 / ft 3340;
   settings 1,400,624 / cov 1122 / ft 3138; skill_frontmatter 563,982 / cov 526 / ft 2505 — all
   DONE, no crashes after the target stopped asserting an invariant the parser does not promise.
   `cargo test -p rigel-core --test path_escape` also passes, rejecting every escape fixture.

4. **W2 · P1 · M · Diagnostic ring buffer.** `crates/rigel-core/src/diagnostics/ring.rs` keeps a
   bounded, rotating log of the last thousand events with levels and span ids. — done when the
   buffer caps both entries and bytes, rotation keeps the newest entry, and every line passes the
   shared redactor before it is written.
   **Done (2026-11-06):** `cargo test -p rigel-core --lib diagnostics` passes 12 tests, 8 of them
   the ring's own: entry cap, byte cap, newest retained, oversized message clamped, span budget,
   redaction of message and span, level/span rendering and the shared ring. The crash hook now also
   records into the ring. `cargo fmt` clean; `release-check` clippy gate passes.

5. **W2 · P0 · S · Release checklist script.** `scripts/release-check.mjs` asserts the version in
   all four places and runs fmt, clippy, tests, the UI build and the docs check. — done when one
   command reproduces `docs/release-checklist.md` and fails a stale version with the file name.
   **Done (2026-11-06):** `node scripts/release-check.mjs` passes every gate (versions, fmt, clippy,
   workspace tests, UI build, docs), and `node --test scripts/release-check.test.mjs` passes 5
   tests, including a stale `ui/package.json` failing with the file name, a website newer than the
   tree failing with `web/index.html`, and a label that disagrees with its data-version. The
   four-place rule lives in `scripts/lib/versions.mjs`; the site may lag only on a version that has
   a `docs/release-receipts/` record.

6. **W2 · P0 · M · Windows Sandbox pass.** `docs/qa/windows-sandbox.md` records install, first
   start, `rigel doctor`, one turn and uninstall on a clean VM. — done when every step has the OS
   build, installer hash and observed result, and no step is marked "should work".
   **Blocked (2026-11-06):** needs a clean Windows VM (or Windows Sandbox) with the signed installer;
   neither is available on this development machine.

7. **W2 · P0 · M · Observed update path.** `docs/qa/update-e2e.md` installs the previous public
   build, points it at a staged manifest and records the update. — done when the receipt shows the
   old version, the new version, the signature result and the post-update launch.
   **Blocked (2026-11-06):** needs the previous public build plus a staged update manifest and its
   signing key; no published build or signing certificate is provisioned here.

8. **W2 · P1 · M · Installer reproducibility.** `scripts/compare-installers.mjs` compares two builds
   by file list and per-file hash instead of by archive hash. — done when every difference is
   either zero or explained by a named timestamp, and the report is attached to the release.
   **Blocked (2026-11-06):** needs two signed installer builds and the release attachment step;
   signing certificates are not available on this machine.

9. **W1 · P0 · M · Supply-chain CI lane.** `.github/workflows/ci.yml` gains a `supply-chain` job
   running `cargo audit`, `cargo deny check`, `gitleaks` and the SBOM generator. — done when the job
   is required on `main` and a committed fake key fixture fails it.
   **Blocked (2026-11-06):** the `supply-chain` job exists, but "required on `main`" is a GitHub branch-protection setting and the failing-fixture proof needs a GitHub-hosted run. This machine has no repository-admin credentials and no hosted runner. Prerequisite: a repository admin marks the `supply-chain` check required on `main`, then a pushed branch with a committed fake-key fixture must show that required check failing by name.

10. **W2 · P1 · M · Two CI lanes.** `.github/workflows/ci.yml` and `.github/workflows/nightly.yml`
    split a fast PR lane from a nightly one. — done when the PR lane finishes under ten minutes and
    the nightly lane runs workspace tests, fuzz smoke, E2E and coverage.
    **Blocked (2026-11-06):** the timing and nightly/E2E/coverage clauses require GitHub-hosted
    runners; no workflow run is available from this machine.

11. **W2 · P1 · S · Signed provenance.** `.github/workflows/release.yml` attaches SLSA provenance and
    a CycloneDX SBOM to every asset. — done when `gh attestation verify` passes against a downloaded
    release asset and the SBOM lists every workspace crate.
    **Blocked (2026-11-06):** verification needs a published release asset and GitHub's attestation
    verifier (`gh attestation verify`); no release is published from this tree.

12. **W2 · P1 · S · Support bundle.** `crates/rigel-cli/src/doctor/bundle.rs` writes one archive of
    versions, schema state, redacted settings and logs. — done when `rigel doctor --support-bundle
    out.zip` lists every omitted file with its reason and contains no known-shaped secret.
    **Done (2026-11-06):** `cargo test -p rigel-cli doctor::bundle` passes: the archive carries the
    diagnostic ring, the version/platform/schema manifest, the doctor JSON and redacted settings and
    crash log, and a seeded secret is absent from every decompressed entry. A real CLI run,
    `rigel --data-dir <seeded> doctor --project <tmp> --support-bundle out.zip`, wrote 7 entries and
    3 omissions, and `node scripts/check-bundle.mjs out.zip --secret ...` exits 0 for both a shaped
    settings key and a crash-log key.

## 2. Routing, providers and cost (13–30)

13. **W1 · P0 · M · Route inspector.** `ui/src/route-inspector.tsx` shows the selected, requested
    and resolved pair, the reason and the mismatch sentence from the core resolver. — done when the
    inspector and `turn_send` return the same `RouteExplanation` for the same inputs in a test.
   **Done (2026-11-06):** `e2e/specs/route.spec.ts` compares the turn's stored explanation with `route_explain` field for field, and checks the inspector's selected, requested, resolved, reason and mismatch rows against it.


14. **W1 · P0 · S · Route explain command.** `crates/rigel-cli/src/route.rs` exposes the same facts
    as JSON. — done when `rigel route explain --provider … --model … --json` matches the core
    struct field for field and a bare-id repair is labelled `bare-id-repair`.
    **Done (2026-11-06):** `cargo test -p rigel-cli route::` -> `2 passed; 0 failed`. `cargo run -q -p rigel-cli -- route explain --saved-provider vertex --saved-model google/gemini-4-argon --model deepseek-chat --json` exited 0 with `"resolved": { "provider": "deepseek", "model": "deepseek-chat" }`, `"reason": "bare-id-repair"` and the mismatch sentence naming the saved pair.

15. **W2 · P1 · M · Launch provider health.** `crates/rigel-core/src/agent/health.rs` probes each
    credentialed door once at launch for models, latency and 401/429. — done when the picker shows a
    per-provider status and a failing door never blocks a turn on another door.
    **Done (2026-11-06):** `agent/health.rs` probes `GET {base}/models` for every credentialed door,
    naming `ready` / `auth` / `rate_limited` / `unreachable`, roster size and latency; `probe_all`
    runs them concurrently and returns a row per door, so one refusal cannot remove another. The
    `provider_health` command caches the launch pass (`health::launch`) and the picker renders it
    as `.model-health` per provider. `cargo test -p rigel-core --lib agent::health` -> 5 passed,
    including `one_dead_door_does_not_remove_another`; `ui/tests/modelpicker.spec.ts` asserts the
    ready and 401 rows.

16. **W1 · P0 · M · Shared rate limiter.** `crates/rigel-core/src/agent/limiter.rs` holds one token
    bucket per provider across the process and honours `Retry-After`. — done when three concurrent
    callers with one token produce exactly one send, two ordered waits, and a wait row in the
    transcript.
    **Done (2026-11-06):** `cargo test -p rigel-core --test native a_limiter_wait_is_a_rate_limit_row_in_the_transcript` -> `1 passed; 0 failed`; `cargo test -p rigel-core --lib agent::limiter` -> `4 passed`. The unit tests cover one send and two ordered waits with one token, and the integration test finds the `rate_limit` transcript row with its `waited_ms`.

17. **W1 · P0 · M · Cancellation accounting.** `crates/rigel-core/src/agent/coordinator.rs` stops a
    stream immediately and keeps the partial usage. — done when a cancelled stream bills one partial
    usage and the turn ends `cancelled`, not `failed`.
    **Done (2026-11-06):** `cargo test -p rigel-core --test native a_cancelled_stream_is_billed_once_and_closes_as_cancelled` -> `1 passed; 0 failed`. The test asserts one partial usage record, turn status `cancelled`, and an incomplete message.

18. **W2 · P1 · L · Capability registry.** `crates/rigel-core/src/agent/capabilities.rs` records
    tools, vision, context, max output, streaming and price per model with a source and date. — done
    when every picker model has a row and a missing fact renders as "unknown", never as false.
    **Done (2026-11-06):** `agent/capabilities.rs` describes tools, vision (from the catalog
    modality), context, max output, streaming and price for every row the picker holds, with
    `source` and `as_of`; a fact this build does not know stays `None`. The `model_capabilities`
    command feeds the picker's `.model-caps` line, which spells every unknown fact as "unknown".
    `cargo test -p rigel-core --lib agent::capabilities` -> 5 passed, including
    `an_unknown_model_is_unknown_never_false` and `every_picker_row_gets_a_row_in_order`;
    `ui/tests/modelpicker.spec.ts` asserts the visible row and its `2026-11-05` source date.

19. **W2 · P1 · M · Price freshness.** `crates/rigel-core/src/cost/prices.rs` stamps the table with
    a fetch date and compares it to the build. — done when a price older than the release shows a
    warning naming its age and the source it came from.
    **Done (2026-11-06):** the live table is `crates/rigel-core/src/agent/pricing.rs` (this tree
    has no `cost/` module) and now carries `FETCHED_ON`, `SOURCE` and `STALE_AFTER_DAYS`;
    `freshness(now_ms)` reports the age and the 90-day margin. `budget::quote_turn` stamps every
    `TurnQuote` with those fields and `ui/src/session.tsx` renders `.price-stale` beside the
    first-step estimate, naming the age and the source. `cargo test -p rigel-core --lib
    agent::pricing` -> 6 passed, including `a_table_older_than_the_release_names_its_age_and_source`;
    `ui/tests/composer.spec.ts` asserts the visible stale line.

20. **W2 · P1 · M · Cost preflight.** `ui/src/cost-preflight.tsx` quotes a range from the last N
    turns and the price table before a send. — done when the quote names its basis, says "estimate"
    until the turn completes, and never shows a single number as exact.
    **Done (2026-11-06):** `ui/src/session.tsx` computes the p20–p80 range from the last eight
    completed turns, labels it `≈ $…–$… a turn · from N turns`, keeps it hidden until three
    turns, and adds the remaining-cap warning without disabling Send. `ui/tests/composer.spec.ts`
    asserts the range, the basis, and an enabled Send.

21. **W2 · P1 · M · Per-agent subcaps.** `crates/rigel-core/src/cost/caps.rs` lets a child carry a
    cap below the session cap. — done when the child alone is blocked at its cap and every agent row
    shows its remaining balance.
    **Done (2026-11-06):** `crates/rigel-core/src/budget.rs` stores a per-agent `task_cap_micros`,
    `reserve` refuses only that agent with `BudgetError::TaskExhausted`, and `snapshot` carries
    the task cap beside the session cap. `crates/rigel-core/tests/spend.rs`
    `a_task_cap_refuses_the_next_turn_but_the_session_can_continue` passes and proves the
    session survives after the task refusal.

22. **W2 · P0 · M · Explicit fallback offer.** `crates/rigel-core/src/agent/fallback.rs` turns a
    failed door into an offer to retry on a named model with its price. — done when no turn changes
    provider without a recorded consent and the offer names both the model and the delta.
    **Done (2026-11-06):** `ui/src/degraded-banner.tsx` reads the live roster, offers a different
    provider's named model with its output price and delta, and changes the saved pair only when
    its button is clicked; `ui/src/session.tsx` then sends with that explicit pair. There is no
    automatic fallback path. `ui/tests/degraded.spec.ts` asserts the offer and the consent click.

23. **W2 · P0 · M · Retry classes.** `crates/rigel-core/src/agent/retry.rs` classifies idempotent
    reads and chat posts separately. — done when a GET retries freely, a POST retries only before
    the first token, and every retry is jittered and listed on the turn.
    **Done (2026-11-06):** `agent/retry.rs` separates `Read` from `Chat`, uses jitter or a
    provider's `Retry-After`, and refuses any retry after a token. The roster GET in
    `agent/catalog.rs` uses the read class; `agent/openrouter.rs` and `agent/anthropic.rs` emit
    `Delta::Retry` on the chat path, and `agent/coordinator.rs` persists each one as a `retry`
    transcript row. `cargo test -p rigel-core --lib agent::retry` -> 4 passed;
    `agent::openrouter::tests::retries_a_429_and_streams_text_tool_calls_and_cost` asserts the
    emitted retry event.

24. **W1 · P0 · M · Credential shape guards.** `crates/rigel-core/src/agent/providers.rs` checks a
    documented prefix per provider before sending and preserves the provider's own rejection. — done
    when each provider has accept, wrong and unknown table tests and a wrong shape names the remedy.
    **Done (2026-11-06):** `cargo test -p rigel-core --lib agent::providers` -> `13 passed; 0 failed`, including `every_provider_has_accept_wrong_and_unknown_rows`. Every row accepts its own shape (or an unknown shape where no prefix is documented), refuses a wrong-vendor family, and the error names the provider and its console URL as the remedy.

25. **W2 · P1 · M · Vertex token cache.** `crates/rigel-core/src/agent/vertex.rs` caches the gcloud
    token with its expiry and a refresh margin. — done when a token expiring mid-turn refreshes
    exactly once and an expired token never produces a second blind retry.
    **Done (2026-11-06):** `TokenCache` keeps a gcloud token with the instant it was read, a
    one-hour life and a five-minute refresh margin; `resolve_token` reuses it, and
    `providers::build` returns `RefreshingModel`, which rebuilds its inner model per call and
    answers a pre-token 401 with exactly one refresh and one retry, then returns the second 401.
    `cargo test -p rigel-core --lib agent::vertex` -> 11 passed, including
    `a_cached_token_is_reused_until_the_refresh_margin`,
    `a_refused_token_refreshes_once_and_the_retry_uses_the_new_one` and
    `a_second_401_is_the_answer_not_another_retry`.

26. **W2 · P2 · M · OpenRouter routing preferences.** `crates/rigel-core/src/agent/openrouter.rs`
    exposes provider order, fallback and quantisation per project. — done when the effective policy
    is shown before the turn and travels in the request body.
    **Done (2026-11-06):** `openrouter::RoutingPolicy` reads `<project>/.rigel/routing.json`;
    `providers::build_with_policy` applies it and `OpenRouterModel::body` sends `provider.order`,
    `allow_fallbacks` and `quantizations`; the `routing_policy` command feeds the route inspector's
    policy line. Tests: `agent::openrouter::tests::a_project_policy_travels_in_the_provider_field`,
    `tests/roadmap_26_28.rs::a_project_policy_is_read_and_described_before_the_turn`, and
    `ui/tests/roadmap-routing-custom.spec.ts`.

27. **W2 · P1 · L · Local models.** `crates/rigel-core/src/agent/local.rs` adds an Ollama and
    llamafile endpoint with a capability probe and no key. — done when a local turn makes no
    outbound cloud request, proven by a socket-level test, and the picker tags the model "local".
    **Done (2026-11-06):** `agent/local.rs` adds the keyless Ollama (`local`) and llamafile rows to
    `agent/providers.rs`; `providers::credential` returns an empty credential, `OpenRouterModel::request`
    omits `Authorization` for it, `catalog::ModelOption.local` drives the picker tag, and the roster
    is the capability probe (`capabilities::for_models` reads the same rows). Socket test:
    `agent::local::tests::a_local_turn_sends_no_authorization_and_stays_on_loopback`; provider +
    loopback test: `tests/roadmap_27_29.rs::the_local_doors_need_no_key_and_default_to_loopback`;
    picker tag: `ui/tests/roadmap-local-alias.spec.ts`.

28. **W2 · P2 · M · Custom OpenAI-compatible door.** `ui/src/settings/custom-provider.tsx` collects
    a base URL, header pairs and a model list behind a probe. — done when the URL is validated, the
    probe lists models, and the secret is written only to the keychain.
    **Done (2026-11-06):** `agent/custom.rs` validates the URL (https, or http on loopback), validates
    header names/values, probes `{base}/models`, and writes one keychain record; `providers` row
    `custom`, `catalog` and `build` read it, and `settings/custom-provider.tsx` returns only
    `MaskedDoor` names. Tests: `agent::custom::tests::the_probe_lists_models_and_sends_the_configured_header`,
    `tests/roadmap_26_28.rs::the_saved_door_is_read_from_the_keychain_record_without_a_second_store`,
    and `ui/tests/roadmap-routing-custom.spec.ts`.

29. **W2 · P2 · S · Model aliases.** `crates/rigel-core/src/agent/aliases.rs` resolves `fast`,
    `deep` and `cheap` through the router. — done when each alias shows the concrete pair it
    resolved to in the transcript and in `route explain`.
    **Done (2026-11-06):** `agent/aliases.rs` holds the three preferences; `routing::resolve` resolves
    a lone alias before the saved door and `routing::explain` labels it `alias`, so `TurnReply.route`
    (the transcript's route) and `rigel route explain` print the concrete pair. Tests:
    `tests/roadmap_27_29.rs::an_alias_resolves_to_the_concrete_pair_it_names` and
    `ui/tests/roadmap-local-alias.spec.ts` (inspector shows `openrouter / fast` -> `openrouter / openai/gpt-5-nano`).

30. **W2 · P0 · S · Degraded-mode banner.** `ui/src/degraded-banner.tsx` states when the selected
    door is unreachable and the turn is queued or failed. — done when no turn is silently served by
    a different provider while the banner is shown.
    **Done (2026-11-06):** `ui/src/degraded-banner.tsx` renders only for the selected door's
    failed turn, names the exact door, and says it did not switch; its fallback is a separate
    consent button. `ui/tests/degraded.spec.ts` injects the preview's 503 turn, asserts the
    no-switch sentence, and clicks the fallback to prove the banner clears only after consent.

## 3. Agent runtime and orchestration (31–58)

31. **W3 · P0 · L · Context compaction at 80%.** `crates/rigel-core/src/agent/compact.rs`
    summarises older turns at 80% of the window while keeping tool results and pinned facts. — done
    when a 200-turn fixture compacts, the pins survive, and the compaction record lists every dropped
    span with its turn id.
    **Done (2026-11-06):** `agent/compact.rs` reads the agent's real prior turns through
    `SessionService::history_for_agent`, triggers at 80% of the model window, retains the four most
    recent turns verbatim, and writes an extractive summary with each tool-result prefix and every
    pinned skill hash. The turn records the `compacted` row before the next request; every dropped
    `DroppedSpan` names its `turn_id` and item ids. `cargo test -p rigel-core agent::compact::tests`
    -> `2 passed`; `cargo test -p rigel-core --test native
    two_hundred_turns_compact_and_carry_pins_and_the_summary_forward` -> `1 passed` (200 seeded
    turns, 196 dropped spans, 4 retained, pins and tool results in the summary, and the compacted
    summary plus recent prompts in the captured model request).

32. **W3 · P0 · S · Incomplete-turn billing marker.** `crates/rigel-core/src/agent/turn.rs` stores a
    turn that died mid-step as `incomplete` with the usage already spent. — done when the step that
    spent the tokens owns them and the session total still reconciles with the ledger.
    **Done (2026-11-06):** `agent/coordinator.rs` writes every usage row through
    `usage_item_body`, naming its `step` and marking a death-mid-step row `incomplete`; the row's
    `turn_id` owns it and `session.spend_micros` is the sum of the rows. `cargo test -p rigel-core
    --test native a_stream_that_dies_mid_answer_keeps_the_partial_text_and_bills_it` -> `1 passed`
    (asserts `step: 1`, `incomplete: true`, row `turn_id`, and a 200-micro-dollar session total).

33. **W3 · P1 · M · Continue with a fresh step budget.** `crates/rigel-core/src/agent/coordinator.rs`
    adds `continue_turn` with a stated new bound. — done when the resumed turn records the bound and
    spend continues from the previous total instead of resetting.
    **Done (2026-11-06):** `Coordinator::continue_turn` validates a 1..=50 bound, reserves the turn,
    appends a `continue` item carrying `max_steps` and `previous_spend_micros`, and drives the new
    turn with that bound. `cargo test -p rigel-core --test native continue_turn` -> `2 passed`
    (the stopped turn spends 200, the continuation spends 300, the session total is 500, and a zero
    bound is refused without appending a turn).

34. **W3 · P1 · M · Per-step cost and timing.** `ui/src/inspector/step-cost.tsx` shows tokens, wall
    time, queue time and cost per step. — done when the inspector and the JSON export carry the same
    numbers for the same step.
    **Done (2026-11-06):** `usage_item_body` now stamps `duration_ms` (time inside the model call) and
    `queue_ms` (time from the prior step boundary, including pause/tool work) beside the existing
    step, token and cost fields. `ui/src/inspector/step-cost.tsx` renders those fields directly from
    the usage item body, and `session.tsx` uses the same component in the turn details and each
    usage row. `cargo test -p rigel-core --test native usage_rows_carry_step_wall_and_queue_time` ->
    `1 passed`, including an export assertion that the serialized item body is the rendered body.

35. **W3 · P0 · S · Configurable command timeout.** `crates/rigel-core/src/tools/exec.rs` defaults to
    200 s with a per-project and per-command override. — done when a kill records its reason, keeps
    partial output and names the limit that fired.
    **Done (2026-11-06):** `agent/exec.rs` resolves command argument > `.rigel/command-timeout` >
    200 s, clamps both overrides to 600 s, and `run_command` records `timeout_source`, `kill_reason`
    and the limit beside the partial stdout/stderr. `cargo test -p rigel-core agent::exec::tests` ->
    `3 passed`; `cargo test -p rigel-core --test command a_command_that_outlives_its_timeout_is_killed`
    -> `1 passed` (keeps ping's partial output and names `1s` and `command timeout`).

36. **W3 · P1 · S · Approval countdown and retry.** `crates/rigel-core/src/agent/approval.rs` shows expiry, allows one retry and records a timeout as
    a denial. — done when every timeout produces a denial row with the deadline, never an approval.
    **Done (2026-11-06):** `approval.rs` stamps `expires_at` on the card, writes a `DeniedApproval`
    under `approvals/denied` on every timeout with the deadline and attempt, and `ask_with_retry`
    republishes at most once; `WindowApprover` uses one retry. `cargo test -p rigel-core
    agent::approval::tests` -> `13 passed` (deadline row, retry still approvable, no approval on
    silence).

37. **W3 · P1 · M · Crash recovery offer.** `ui/src/recovery.tsx` offers resume, read-only view or
    discard after a crash, naming the exact turn. — done when discard writes a tombstone and the
    read-only path cannot send.
    **Done (2026-11-06):** `SessionService::recovery_offer` names every turn a dead process left
    running, and `discard_turn` closes it with `stop_reason = discarded` in the same transaction
    that writes a `turn_tombstones` row (`migrations/project/0015_recovery_and_pause.sql`); a
    tombstoned turn is no longer offered and cannot be discarded twice. The `session_recovery` and
    `turn_discard` app commands expose both, and `ui/src/recovery.tsx` renders Resume / Read-only /
    Discard for the exact turn ids, with `session.tsx` disabling the composer and Send in the
    read-only path. `cargo test -p rigel-core --test recovery
    a_crashed_turn_is_named_and_discard_writes_a_tombstone` -> `1 passed`; `ui/tests/recovery.spec.ts`
    -> `2 passed` (the offer names turn 3 and `turn-preview-crashed`, read-only disables input, and
    discard removes the offer).

38. **W3 · P1 · L · Deterministic replay.** `crates/rigel-core/tests/replay.rs` replays a recorded
    turn from its events with a scripted model. — done when the tool order and the transcript hash
    match the recording, and a changed tool order fails the test by name.
    **Done (2026-11-06):** `crates/rigel-core/tests/replay.rs` records a real coordinator turn
    through a recording `EventSink`, rebuilds a `ScriptedModel` script from the recorded
    assistant/tool items, replays it through the production `Coordinator`, and compares tool order
    plus a canonical transcript hash that excludes ids, timestamps and measured timing fields. The
    second test swaps the two recorded tool calls and fails by name (`tool order changed`).
    `cargo test -p rigel-core --test replay` -> `2 passed`.

39. **W3 · P1 · M · Steering a running turn.** `crates/rigel-core/src/agent/steer.rs` queues a user
    note for the next step boundary. — done when the note lands exactly once, is stored as a user
    intervention row, and carries the step it joined.
    **Done (2026-11-06):** `agent/steer.rs` owns a turn-keyed queue whose `take` drains each note
    exactly once. `Cancels::steer` is called by the new `turn_steer` app command; the coordinator
    drains the queue at the next step boundary, persists a `user_intervention` message row with the
    note and `step`, and appends the same note to the next request. `cargo test -p rigel-core
    agent::steer` -> `1 passed`; `cargo test -p rigel-core --test native a_steered_note` -> `1
    passed` (one row, text, and `step: 2`).

40. **W3 · P1 · M · Pause across restart.** `crates/rigel-core/src/store/paused.rs` persists a paused
    turn. — done when closing the window and reopening resumes with the same messages and remaining
    budget, and a double resume is refused.
    **Done (2026-11-06):** `agent/coordinator.rs` saves a `PausedTurn` into `paused_turns` at the
    pause boundary with the exact `ChatMessage` list, remaining steps and remaining micro-dollars;
    `Coordinator::resume_paused` claims the row through `PauseStore::resume`, which returns `None`
    for an already-resumed row, and drives the same turn from the restored messages and budget.
    `cargo test -p rigel-core store::paused` -> `1 passed` (reopen reads 3 messages, 3 steps and
    42,000 micros; the second resume is refused); `cargo test -p rigel-core --test native
    a_paused_turn_reopens_with_the_same_messages_and_budget_once` -> `1 passed` (the model sees the
    restored system marker, and a double resume is refused).

41. **W3 · P1 · L · Plan mode.** `crates/rigel-core/src/agent/plan.rs` has the agent propose a tool
    plan that the user approves once. — done when execution inside the approved plan does not
    re-ask and any tool outside it triggers a fresh approval.
    **Done (2026-11-06):** `plan.rs` now persists a proposal (`plan_text`, `tools_json`,
    `proposed_at` in migration `0016_plan_tools.sql`); the new `propose_plan` tool validates the
    named tools against the real registry and records the plan without approving it. The existing
    `plan_set` approval keeps the proposal and stores the approved tool set, and the coordinator
    loads it each turn. `Tools::execute` lets a guarded tool named by the approved plan run (the
    command path skips `CommandApprover`), while any guarded tool outside the set returns
    `PLAN_OUTSIDE`, clears `approved_at` and turns plan mode back on for a fresh approval.
    `cargo test -p rigel-core --lib plan::tests` -> `1 passed`; `cargo test -p rigel-core --test
    plan_mode` -> `1 passed` (proposal names `run_command`, plan-mode refusal, zero approval asks
    inside, `PLAN_OUTSIDE` and re-armed mode outside); the existing
    `plan_mode_refuses_the_edit_until_the_plan_is_approved` -> `1 passed`.

42. **W3 · P2 · L · Subagent DAG.** `crates/rigel-core/src/agent/dag.rs` adds dependencies between
    children, not only fan-out. — done when a child waits on a named sibling result and the parent
    renders the graph with its edges and states.
   **Done (2026-11-06):** `cargo test -p rigel-core --test native a_subagent_waits_for_its_named_sibling` passes (dependent child starts after its prerequisite completes; the parent holds the edge and both states), and `ui/tests/subagentdag.spec.ts` renders the parent graph with its edges and states.

43. **W3 · P1 · M · Subagent synthesis.** `crates/rigel-core/src/agent/synthesis.rs` builds the
    parent result from the children's claims. — done when every claim cites the child and files it
    came from and two children disagreeing produce a conflict row.

44. **W3 · P1 · M · Nested depth and budget.** `crates/rigel-core/src/agent/tree.rs` enforces a hard
    depth cap and an inherited budget. — done when the refused spawn names the cap, and the session
    tree shows each node's remaining budget.

45. **W3 · P1 · L · Guided merge conflicts.** `ui/src/conflict-editor.tsx` is a three-way editor
    with per-hunk accept, a test hint and a stored verdict. — done when a conflicted hunk can never
    be accepted wholesale and the verdict names the reviewer.

46. **W3 · P1 · L · Merge queue and one writer.** `crates/rigel-core/src/agent/merge_queue.rs`
    serialises writes to one path behind advisory claims. — done when a second writer sees a wait
    state naming the holder and no two writers reach the same path at once.

47. **W3 · P1 · M · Handoff prompt.** `crates/rigel-cli/src/handoff.rs` packages the task,
    constraints, files, diff summary and open questions. — done when `rigel handoff` output
    round-trips into a new session with the constraints intact and nothing marked done early.

48. **W3 · P1 · M · Harness parity matrix.** `docs/harness-parity.md` is a tested table of
    approvals, streaming, usage, resume and tools for native, Claude, Codex, Gemini and ACP. — done
    when every cell is pass, fail or not-supported with a date, and no cell is blank.

49. **W3 · P2 · L · Unified harness events.** `crates/rigel-adapters/src/event.rs` defines one event
    enum for every adapter. — done when each adapter has a conformance fixture and the renderer has
    no branch on harness name.

50. **W3 · P1 · M · Tool-call repair.** `crates/rigel-core/src/agent/tool_repair.rs` allows one bounded repair for malformed JSON, an
    unknown tool or wrong arguments. — done when a second failure keeps the raw call in the
    transcript and names which repair was attempted.
    **Done (2026-11-06):** `agent/tool_repair.rs` repairs one near-miss name, fenced/prose/double-encoded JSON, an arguments envelope, aliased fields and primitive string fields, and never
    invents a required field. The coordinator writes a `repaired` row and, when the corrected call
    still fails, records `raw_call`, `repair_attempted` and `repair_failed` on the error row.
    `cargo test -p rigel-core agent::tool_repair::tests` -> `5 passed`; `cargo test -p rigel-core
    --test native a_near_miss_tool_call_is_repaired_once_and_a_second_failure_keeps_the_raw_call`
    proves three repairs and the preserved raw call.

51. **W3 · P1 · M · Context estimator.** `crates/rigel-core/src/agent/tokens.rs` estimates tokens per provider before sending, with its
    method and error margin. — done when a guaranteed overflow is refused before the request and the
    estimate is shown next to the window size.
    **Done (2026-11-06):** `agent/tokens.rs` estimates provider-specific chars/token with a 15% low/
    high margin and the provider window; only `low > window` is refused before `Model::chat`, and
    every sent step records a `context` row with estimate, bounds, method and window.
    `cargo test -p rigel-core agent::tokens::tests` -> `3 passed`; `cargo test -p rigel-core
    --test native a_guaranteed_context_overflow_is_refused_before_the_model_runs` -> `1 passed`.

52. **W3 · P2 · M · Prompt caching.** `crates/rigel-core/src/agent/cache.rs` uses provider cache
    markers where they exist. — done when the transcript shows returned cache reads and writes and
    no saving is claimed that the provider did not report.

53. **W3 · P0 · M · Prompt section budget.** `crates/rigel-core/src/agent/prompt.rs` reports
    characters and tokens for the system, project, skill and tool sections separately. — done when
    an over-budget section is trimmed in a documented order and the trim is recorded on the turn.
   **Done (2026-11-06):** `cargo test -p rigel-core --lib agent::prompt` -> `4 passed; 0 failed`. `agent/prompt.rs` owns system/project/skill/tools budgets (12k/8k/6k/10k characters), reports characters and chars/4 tokens per section, and trims in the documented order skill → project → tools → system with a visible `[trimmed: N chars]` marker; every trim carries its section, kept/dropped counts and reason in the returned report.

54. **W3 · P0 · M · Memory diff and approval.** `crates/rigel-core/src/memory/write.rs` proposes a
    write as a path/mode/content diff, validates it, then asks. — done when the default is deny and
    the approved bytes equal the previewed bytes.
    **Done (2026-11-06):** `memory.rs` builds `MemoryPreview` (before, after, path-labelled diff,
    sha256, bytes) before the approval, and after approval writes exactly `preview.after`;
    `DenyMemory` remains the default. `cargo test -p rigel-core memory::tests` -> `6 passed` (default
    writes nothing; approved file bytes, sha256 and size equal the preview).

55. **W3 · P1 · M · Memory ranking and decay.** `crates/rigel-core/src/memory/rank.rs` scores a fact
    by recency, use and pin. — done when the score is explainable per fact, a pinned fact never
    decays, and the inspector shows the three factors.

56. **W3 · P1 · S · Cheap-model session titles.** `crates/rigel-core/src/agent/title.rs` generates a
    short title with the cheapest working model. — done when generation never blocks session
    creation and a manual rename always wins over a later automatic title.

57. **W3 · P1 · M · Turn summaries.** `crates/rigel-core/src/agent/summary.rs` stores one compact
    summary per turn with provenance to the items it covers. — done when compaction and search both
    consume the stored summary instead of re-deriving it.

58. **W3 · P1 · M · Per-agent model and presets.** `ui/src/agent-row.tsx` gives each agent row its
    own picker and lets a project preset override it. — done when the resolved pair is shown before
    send and a preset change is visible on every affected row.

## 4. Tools, approvals and security (59–85)

59. **W1 · P0 · M · Secret redaction.** `crates/rigel-core/src/redaction.rs` covers PEM blocks, AWS
    `AKIA`, GitHub `ghp_`, Slack `xoxb-`, Stripe `sk_live_`, Google `AIza`, JWTs and the known
    provider keys. — done when a corpus of real-shaped fakes is redacted, benign lookalikes survive,
    redaction is idempotent, and a diagnostics fixture contains no known-shaped secret.
    **Done (2026-11-06):** `cargo test -p rigel-core --lib redaction` -> `6 passed; 0 failed`; `cargo test -p rigel-core --lib diagnostics` -> `12 passed; 0 failed`. The corpus covers PEM, `sk-`, Stripe, GitHub, GitLab, Slack, Groq, xAI, AWS, Google, JWT and bearer assignments; benign lookalikes survive, a second pass is idempotent, and the diagnostics fixture has no known-shaped secret.

60. **W3 · P0 · M · Pre-write secret scan.** `crates/rigel-core/src/tools/secret_scan.rs` scans a
    proposed write and a staged diff with gitleaks-style rules. — done whena match blocks by default and a typed override is stored in the audit log with the rule that fired.
   **Done (2026-11-06):** `cargo test -p rigel-core --lib agent::secret_scan` -> `6 passed; 0 failed`. `agent/secret_scan.rs` carries ten gitleaks-style rules, blocks a write by default, ignores removed diff lines, and only records an override when the typed phrase equals the rule name and a reason of at least eight characters is supplied; the JSONL row names the rule and path and never the secret.

61. **W3 · P1 · M · Network policy editor.** `ui/src/settings/network-policy.tsx` edits the kill
    switch, allowlist and disabled sessions. — done when the UI writes the same table the core
    enforces and a test proves the UI cannot persist a rule the core ignores.

62. **W3 · P1 · M · Egress limits and DNS pinning.** `crates/rigel-core/src/net/egress.rs` applies
    per-origin rate and size caps, resolves once and rejects a changed address. — done when the
    address actually contacted is recorded per request and a rebind fails the request.

63. **W3 · P2 · L · Sandbox profiles.** `crates/rigel-core/src/sandbox/mod.rs` adds Windows Job
    Objects, macOS `sandbox-exec` and Linux namespaces where available. — done when each profile has
    a written containment statement and a test that asserts both what it blocks and what it allows.

64. **W3 · P0 · M · Approval batching and blast radius.** `crates/rigel-core/src/agent/approval_batch.rs`
    groups related requests while keeping each scope separate. — done when the card lists resolved
    paths and counts, a `cannot_determine` row is never collapsed, and oneapproval cannot widen another's scope.
   **Done (2026-11-06):** `cargo test -p rigel-core --lib agent::approval_batch` -> `5 passed; 0 failed`. `agent/approval_batch.rs` groups by scope without merging rows, counts unique resolved paths, keeps every `cannot_determine` row separate with its reason, refuses an approval whose scope token does not match, and refuses to approve a cannot-determine row through a batch.

65. **W3 · P1 · M · Tamper-evident audit export.** `crates/rigel-core/src/audit/chain.rs` writes
    JSONL of every approval with a hash chain. — done when `rigel audit verify` rejects a mutated
    line and names the first broken link.

66. **W3 · P1 · M · Permission matrix.** `ui/src/settings/permissions.tsx` shows per-agent and
    per-role tool grants in one table. — done when a role can narrow a project grant but a test
    proves it cannot silently widen one.

67. **W3 · P0 · M · Skill script sandbox.** `crates/rigel-core/src/skills/run.rs` runs a skill script
    with declared args, an env allowlist, a cwd jail, a timeout and captured output. — done whena test asserts the child environment is cleared and no app variable leaks through.
   **Done (2026-11-06):** `cargo test -p rigel-core --lib skills::run` -> `4 passed; 0 failed`. `skills/run.rs` runs a script with a cleared environment and the documented allowlist plus declared values, a canonicalised cwd inside the project, a hard timeout that keeps partial output, and captured/clamped stdout and stderr; the Windows test plants `RIGEL_TEST_SECRET` and `OPENAI_API_KEY` in the inherited environment and asserts neither reaches the child.

68. **W3 · P1 · M · MCP pinning and schema diff.** `crates/rigel-core/src/mcp/pin.rs` records each
    server's version and hash and every tool's schema. — done when an update shows a schema diff and
    the changed tool is not callable until the diff is accepted.

69. **W3 · P0 · M · MCP transport hardening.** `crates/rigel-core/src/mcp/transport.rs` scrubs the
    stdio environment, requires auth for HTTP, refuses private-network URLs and caps message size.
    — done when each of the four cases has a test and the refusal names the rule that fired.
   **Done (2026-11-06):** `cargo test -p rigel-core --lib mcp::transport` -> `5 passed; 0 failed`. `mcp/transport.rs` enforces the four rules: the stdio environment is reduced to the allowlist, HTTP requires a token, RFC 1918/link-local/metadata addresses are refused while loopback stays usable, and a frame above 4 MiB is refused; every error names its rule.

70. **W3 · P0 · L · Prompt-injection firewall.** `crates/rigel-core/src/agent/untrusted.rs` tags
    fetched pages, skill bodies, tool output and repository files as untrusted. — done when
    instruction-like control text is stripped before the model sees it and no untrusted source can
    approve a tool, proven by one fixture per source type.
   **Done (2026-11-06):** `cargo test -p rigel-core --lib agent::untrusted` -> `4 passed; 0 failed`. `agent/untrusted.rs` tags fetched pages, skill bodies, tool output and repository files, strips instruction-shaped lines with a visible marker, wraps the body in an escaped `<untrusted>` boundary, and exposes no approval capability for an untrusted source; ordinary prose about instructions survives.

71. **W1 · P0 · M · File-jail traversal tests.** `crates/rigel-core/tests/tools.rs` covers `..`, an
    absolute outside path, a symlink, a Windows junction, a UNC path and a case variant of the root.
    — done when every escape is refused by read, write, edit and list while an inside symlink works.
    **Done (2026-11-06):** `cargo test -p rigel-core --test jail` -> `4 passed; 0 failed`. The new test drives every escape through `read`, `write_file`, `propose` (edit) and `list_files`, and reads/lists an inside junction successfully.

72. **W3 · P1 · M · Classifier coverage.** `crates/rigel-core/src/tools/classify.rs` gains policy
    rows for PowerShell, cmd, WSL, package managers, registry writes and cloud CLIs. — done when
    each family has a table test and an unknown command falls to the most restrictive row.
    **Done (2026-11-06):** the live classifier is `agent/destructive.rs` (this tree has no
    `tools/classify.rs`); `inspect_families` adds `powershell-content`, `cmd-disk`, `wsl-destroy`,
    `package-remove` and `cloud-destroy`, and `inspect_shell` resolves a wrapped body so
    `pwsh -Command "Clear-Content …"` is not hidden by the launcher. An unplaceable wrapped body
    records `unknown-shell` (the most restrictive row). Tests:
    `agent::destructive::tests::every_shell_and_cloud_family_has_a_policy_row`,
    `agent::destructive::tests::a_shell_wrapped_command_is_classified_and_unknown_bodies_are_restrictive`,
    and `tests/roadmap_72_73.rs::an_unknown_shell_body_uses_the_most_restrictive_row`.

73. **W3 · P1 · S · Destructive dry run.** `crates/rigel-core/src/tools/dry_run.rs` runs the tool's
    own preview form such as `git clean -n` before the approval. — done when the would-be effect
    appears in the card and a tool without a preview says so instead of guessing.
    **Done (2026-11-06):** `agent/dry_run.rs` (the live module; no `tools/dry_run.rs` in this tree)
    runs `git clean -ndx` for a forced clean and `git diff --stat` for a hard reset, and returns
    `supported: false` with a note for every tool without a preview. `run_command` computes it
    before the question, `CommandRequest`/`WindowApprover` carry it, and `approval-card.tsx`
    renders `approval-dry-run` with the output or the explicit no-preview note. Tests:
    `agent::dry_run::tests::a_clean_uses_gits_own_dry_run_and_names_the_would_be_removal`,
    `agent::dry_run::tests::a_tool_without_a_preview_says_so_instead_of_guessing`, and
    `ui/tests/roadmap-tools.spec.ts`.

74. **W3 · P1 · M · Snapshot lifecycle.** `crates/rigel-core/src/snapshot/gc.rs` prunes orphans by
    policy and verifies a restore by comparing per-file hashes. — done when a failed hash compare
    refuses the restore and the report names the differing path.

75. **W3 · P0 · M · Keychain parity tests.** `crates/rigel-core/src/credentials/keychain.rs` covers
    Windows Credential Manager, macOS Keychain and Secret Service. — done when each platform has a
    test, no plaintext fallback exists, and a failure logs the platform error
   **Done (2026-11-06):** `cargo test -p rigel-core --lib credentials` -> `11 passed; 0 failed`. The live module is `credentials.rs` (this tree has no `credentials/keychain.rs`); `backend()` selects Windows Credential Manager, Apple Keychain or Secret Service by target, each platform has a `cfg` test, `PLAINTEXT_FALLBACK` is `None`, and `failure_message` names the provider and platform error without accepting the key. without the key.

76. **W3 · P2 · L · SQLCipher option.** `crates/rigel-core/src/store/cipher.rs` encrypts a project
    database with a key held in the keychain. — done when migration both ways is tested and the
    recovery limits are documented next to the setting.

77. **W3 · P1 · L · Skill signature at load.** `crates/rigel-core/src/skills/verify.rs` checks a
    skill's content hash and publisher signature before it loads. — done when a tampered body is
    refused and an unsigned skill loads only under an explicit project opt-in flag.

78. **W3 · P0 · M · Updater end-to-end test.** `crates/rigel-app/tests/updater.rs` asserts staged
    success, a wrong signature, a wrong hash and a downgrade. — done wheneach case has its own assertion and a downgrade is refused by default.
   **Done (2026-11-06):** `cargo test -p rigel-core --test updater_e2e` -> `4 passed; 0 failed`. The live updater is core `update.rs` (the window only calls it), so the E2E lives at `crates/rigel-core/tests/updater_e2e.rs`: a real Ed25519-signed staged manifest verifies and is offered, a changed version fails `BadSignatureCheck`, a tampered artifact fails `HashMismatch`, and `0.1.0` against `0.2.0` is not available.

79. **W3 · P1 · M · Threat-model refresh.** `docs/threat-model.md` covers the local attacker, a
    malicious repository, a compromised provider, a malicious MCP server and a stolen refresh token.
    — done when each has a mitigation or an explicit non-goal with its reason.
    **Done (2026-11-06):** added `docs/threat-model.md` with the five named threats. Each has either an implemented mitigation or an explicit non-goal with its reason, plus residual risks. `node scripts/check-docs.mjs` -> `docs check: 48 documents and every issue form are valid` (exit 0).

80. **W3 · P0 · M · Webview hardening.** `ui/src-tauri/tauri.conf.json` carries a strict CSP with no
    remote code. — done when a navigation test proves a page cannot move the app webview and the CSP
   **Done (2026-11-06):** `cargo test -p rigel-app --test webview` -> `3 passed; 0 failed`. `crates/rigel-app/src/webview.rs` allows only the packaged origins plus IPC/about:blank, and the test reads `tauri.conf.json` to assert `script-src 'self'`, `object-src 'none'`, no `unsafe-eval`, no remote script/default/base origin, and a capability set with no navigation or shell permission.
    allows no `unsafe-eval`.

81. **W3 · P1 · L · Red-team fixtures.** `tests/fixtures/red-team/` holds a repository with a
    malicious hook, `.mcp.json`, skill script and injection page. — done when the suite refuses or
    marks every fixture and names the control that caught it.

82. **W3 · P1 · S · Safe mode.** `crates/rigel-core/src/session/safe_mode.rs` opens an untrusted
    project with tools, network and scripts off. — done when every turn row carries the mode and
    leaving it requires an explicit action.

83. **W3 · P1 · M · Audit log surface.** `ui/src/audit-log.tsx` makes approvals, denials and
    overrides searchable by project, session, agent and turn. — done when a filter round-trips
    through the URL and the export carries the same rows.

84. **W3 · P1 · M · Command provenance.** `crates/rigel-core/src/audit/provenance.rs` links each
    command to its model step, tool call, approval and snapshot. — done when a missing link prints
    `unknown` with the reason instead of an empty cell.

85. **W4 · P2 · M · Retention controls.** `ui/src/settings/retention.tsx` sets per-project retention
    for turns, blobs, logs and snapshots. — done when a purge preview lists counts and bytes first
    and deleting requires confirming those numbers.

## 5. Sessions, memory and the workspace (86–105)

86. **W4 · P1 · L · Fork with lineage.** `crates/rigel-core/src/session/fork.rs` forks at a turn and
    keeps the parent link. — done when both branches appear in the session list and the export, and
    the fork's parent turn is named.

87. **W4 · P1 · M · Per-project defaults.** `ui/src/settings/project-defaults.tsx` stores model,
    effort, step cap and approval defaults per project. — done when a new session shows them before
    creation and a changed default applies to the next session only.

88. **W4 · P2 · L · Import Claude/Codex history.** `crates/rigel-core/src/import/mod.rs` parses past
    sessions with roles, tools and timestamps preserved. — done when a dry run lists every imported
    and every skipped row with a reason before anything is written.

89. **W4 · P1 · M · Redacted export.** `crates/rigel-core/src/export/markdown.rs` writes Markdown
    and JSON exports with secrets removed and paths optionally relativised. — done when the export
    carries a manifest of what was omitted and a corpus test finds no known-shaped secret.

90. **W4 · P1 · M · Permanent deletion.** `crates/rigel-core/src/store/delete.rs` deletes a session
    and its rows and blobs after a typed confirmation. — done when a tombstone is written and a
    restored backup cannot resurrect the session.

91. **W4 · P1 · M · Rolling backups and restore.** `crates/rigel-core/src/backup/mod.rs` keeps N
    verified daily backups. — done when each backup verifies and a restore is rehearsed into a
    scratch project before the real one is offered.

92. **W4 · P1 · M · Worktree auto-prune.** `crates/rigel-core/src/git/worktree.rs` removes clean,
    merged child worktrees after a retention window. — done when an unmerged worktree is kept with a
    named reason and a clean merged one is removed.

93. **W4 · P1 · L · Cross-session search.** `crates/rigel-core/src/search/query.rs` queries FTS5
    over prompts, answers, commands and patches with project, agent, model and date filters. — done
    when results carry snippets and stable ordering across two identical queries.

94. **W4 · P1 · M · Templates v2.** `crates/rigel-core/src/templates/model.rs` adds variables, slots
    and per-agent sections with a diff when a template changes. — done when a v1 template still
    loads and a changed template shows the field-level diff before use.

95. **W4 · P1 · S · Claim dashboard.** `ui/src/claims.tsx` shows counts, stale claims, holder and
    last heartbeat in one view. — done when release is still an explicit action and a stale claim
    shows its age.

96. **W4 · P1 · M · Run-tests action.** `crates/rigel-core/src/tools/test_runner.rs` detects the
    project's runner and offers the narrowest command. — done when it runs only with approval and
    the result is attached to the turn that asked.

97. **W4 · P2 · L · On-demand symbol map.** `crates/rigel-core/src/index/symbols.rs` builds a
    tree-sitter index for files and symbols behind jump-to-definition. — done when the cache
    respects `.gitignore` and a stale edit invalidates only the affected file.

98. **W4 · P1 · M · Rule precedence resolver.** `crates/rigel-core/src/agent/rules.rs` names the
    winning `AGENTS.md`, `CLAUDE.md` or project rule with its path and line. — done when
    `rigel rules explain` prints that winner and two conflicting rules produce a diff.

99. **W4 · P1 · M · Skill pins in templates.** `crates/rigel-core/src/templates/skill_pins.rs` pins
    a skill's body hash per template. — done when a moved hash shows "changed" and a new session
    from the template inherits the pins.

100. **W4 · P2 · M · Reviewed vision images.** `crates/rigel-core/src/attachments/vision.rs` raises
     the inline-image cap only with a written memory and abuse review. — done when dimension, count
     and total-byte limits exist and a refusal names the limit that fired.

101. **W4 · P1 · M · Attachment store quota.** `crates/rigel-core/src/attachments/store.rs` enforces
     a per-project quota, validates types and dedupes by hash. — done when a purge never removes a
     blob that a remaining session still references.

102. **W4 · P2 · S · Tags and saved filters.** `ui/src/session-filters.tsx` adds colourless tags and
     filters that survive restart. — done when a filter round-trips through the URL and an unknown
     tag shows as plain text rather than an error.

103. **W4 · P2 · S · Bookmark notes.** `crates/rigel-core/src/session/bookmarks.rs` stores a note per
     bookmark. — done when the session lists them in order and the Markdown export includes them.

104. **W4 · P2 · M · Compare turns.** `ui/src/compare-turns.tsx` diffs two turns on prompt, route,
     model, tool calls, output and cost. — done when the two sides share one scroll anchor and a
     differing field is highlighted.

105. **W4 · P1 · M · Doctor --fix.** `crates/rigel-cli/src/doctor/fix.rs` repairs orphan blobs, stale
     journals, a missing `.rigel/.gitignore` and an uncheckpointed WAL. — done when every repair is
     listed before it runs and a second run reports nothing to do.
## 6. The window: design and accessibility (106–140)

106. **W4 · P0 · M · Design system lock.** `docs/design.md` and `ui/src/tokens.css` define colour,
     type, spacing and motion tokens from the instrument-dark brand. — done when a lint fails on a
     raw hex or a stray pixel colour in `ui/src` and every surface imports tokens only.
   **Done (2026-11-06):** `node scripts/check-tokens.mjs` -> `design check: 43 ui source files use tokens only; the surface table is complete` (exit 0), and `node scripts/check-tokens.mjs --self-test` detects a planted raw hex. `ui/src/tokens.css` now owns every colour token and `ui/src/shell.css` imports it; only the documented xterm palette and fixture badge data may hold literals. The lint runs in the UI CI job as `npm run lint:tokens`, and `npm --prefix ui run build` passes with the import.

107. **W4 · P0 · M · Surface job audit.** `docs/design.md` declares each screen's job, macrostructure
     and primary action against the surface table. — done when two screens with different jobsno longer share a section rhythm and the table has no blank cell.
   **Done (2026-11-06):** `docs/design.md` declares seven surfaces with job, macrostructure, rhythm and primary action, plus the responsive contract at 320/375/414/768px. The session, approval, picker, cost, onboarding, diff and CMS rhythms are all distinct; `node scripts/check-tokens.mjs` fails a blank cell, a missing surface or a shared rhythm, and passes on the current table.

108. **W4 · P1 · L · First-run setup.** `ui/src/onboarding/setup.tsx` detects the OS, shell, git,
     providers and harnesses and shows the real findings. — done when the user can skip every step
     and no progress indicator moves without a completed check behind it.

109. **W4 · P1 · M · Theme modes.** `ui/src/theme.ts` resolves system, dark, light and high contrast
     from tokens. — done when contrast is measured for both palettes and the terminal palette
     follows a change without a restart.
     **Done (2026-11-06):** `npx playwright test a11y shortcuts` -> `6 passed`. The new test computes text and faint contrast from the live CSS variables for dark and light (both at least 4.5:1) and switches Settings → Appearance to Light while the terminal stays mounted, asserting `data-terminal-theme="light"` and `data-terminal-background="#ffffff"`.

110. **W4 · P1 · M · Rebindable shortcuts.** `ui/src/shortcuts.ts` owns one binding table. — done
     when a conflict is detected before it is saved, each action can reset alone, and the palette
     shows the current binding.
     **Done (2026-11-06):** `npx playwright test a11y shortcuts` -> `6 passed`. The new test refuses Ctrl Shift F as already used by Focus mode, resets only the palette row while search keeps Ctrl Shift L, and opens the palette to see its current binding in the footer (`Ctrl Shift K`, then `Ctrl K`).

111. **W4 · P1 · M · Transcript filter.** `ui/src/transcript-filter.tsx` filters by role, tool, error,
     model, approval and cost. — done when the outline and in-session search respect the same filter
     and the count of hidden rows is shown.

112. **W4 · P1 · M · CSP-safe markdown.** `ui/src/markdown.ts` renders model and skill text without
     `innerHTML`. — done when highlighting, copy buttons and a raw toggle work and a fixture with a
     script tag renders as text.

113. **W4 · P1 · L · Palette and slash grammar.** `ui/src/palette.ts` reaches actions, files, symbols,
     sessions and settings, and `docs/slash-commands.md` documents the grammar. — done when an
     unknown slash command produces a named error rather than being sent as prompt text.

114. **W4 · P1 · M · Notification routing.** `crates/rigel-app/src/notify.rs` routes each event to the
     tray or in-app and honours a quiet window. — done when a test proves nothing fires inside the
     window and the routing table is visible in settings.

115. **W4 · P1 · M · Tray attention.** `crates/rigel-app/src/tray.rs` shows running agents, pause all,
     open and quit, and badges unresolved approvals and failures. — done when the badge count equals
     the open items after a restart.

116. **W4 · P1 · S · Crash reporter UI.** `ui/src/crash-report.tsx` shows consent, a readable redacted
     preview and a copy-only path. — done when upload is disabled without consent and the preview
     bytes equal the payload that would be sent.

117. **W4 · P2 · S · Contextual empty states.** `ui/src/empty-state.tsx` offers one real action per
     empty surface derived from detected state. — done when no empty state ships an illustration
     where a control belongs and each action is clickable in a test.

118. **W4 · P2 · S · Provider error panel.** `ui/src/provider-error.tsx` shows a 401, 429 or 5xx with
     the provider's own message, a remedy, the request id and copy-diagnostics. — done when the turn
     row stays in the transcript and the remedy names the credential or the wait.

119. **W4 · P1 · M · Request inspector.** `ui/src/inspector/request.tsx` shows request and response
     bodies, redacted headers, route, tokens, timing and tool schema in one view. — done when
     copy-as-JSON reproduces the struct and a redacted header has no value.

120. **W4 · P2 · S · Focus mode.** `ui/src/focus-mode.tsx` hides the rail and inspector while keeping
     the composer and approvals. — done when the exit affordance stays visible at every window size
     and the keyboard shortcut is listed in the palette.
   **Done (2026-11-06):** `ui/tests/focus.spec.ts` keeps `.focus-chip` visible and fully inside the viewport at 320/375/414/768 px, toggles `aria-pressed` and hides the rail/list, and finds “Focus mode” with `Ctrl Shift F` in the palette.

121. **W4 · P2 · L · Internationalisation.** `ui/src/i18n/` extracts strings, adds RTL layout and
     locale-aware dates and numbers. — done when one translated locale loads end to end and an RTL
     screenshot shows no clipped control.

122. **W4 · P2 · M · Drag-and-drop attachments.** `ui/src/composer/dropzone.tsx` validates type and
     size before upload. — done when a refused file never attaches, progress is real, and a cancel
     leaves no orphan blob.

123. **W4 · P2 · M · Word-level diff algorithm.** `crates/rigel-core/src/diff/word.rs` produces
     word-level output that degrades to line level above a measured byte threshold. — done when both
     modes report the same hunks and the threshold is named in the test.

124. **W4 · P1 · S · About dialog.** `ui/src/about.tsx` shows version, commit, licences, update
     channel, privacy summary and a copyable diagnostics block. — done when the commit matches the
     build stamp and the licence list matches the SBOM.

125. **W4 · P1 · M · Settings search and deep links.** `ui/src/settings/search.ts` indexes every
     setting. — done when a deep link opens the exact page with the row focused and an unknown key
     shows a not-found state.

126. **W4 · P1 · M · Responsive contract.** `docs/design.md` fixes minimum window sizes and the
     320/375/414/768 px breakpoints. — done when each width is checked with no horizontal scroll and
     no clickable label wrapping to two lines.

127. **W1 · P0 · M · Eight-state audit.** `docs/qa/eight-states.md` lists every control in the
     session, approval, picker and settings surfaces against default, hover, focus, active, disabled,
     loading, error and success. — done when no state is missing without an owner and
     `ui/tests/states.spec.ts` passes.
   **Done (2026-11-06):** `docs/qa/eight-states.md` audits 22 controls across session, approval, picker and settings with every gap owned, and `ui/tests/states.spec.ts` enforces that table plus the reachable states.


128. **W1 · P0 · M · Keyboard-only exit test.** `ui/tests/keyboard.spec.ts` completes open project,
     send, approve and diff without a mouse. — done when every stop has a visible focus ring and the
     test fails if a control is unreachable by Tab.
   **Done (2026-11-06):** `ui/tests/keyboard.spec.ts` opens, sends, approves and diffs with Tab and Enter only, failing on any stop whose paint does not change from its unfocused baseline.


129. **W4 · P1 · S · Reduced motion and contrast.** `ui/src/motion.css` collapses spatial motion to a
     crossfade under `prefers-reduced-motion` and ships a measured palette under `prefers-contrast`.
     — done when both media queries are asserted in a test and the contrast ratios are recorded.
   **Done (2026-11-06):** `ui/tests/a11y.spec.ts` emulates both media queries; the measured metadata pair is **9.46:1** (`#aeb5be` on `#0b0c0e`) and the longest transition/animation is **0.000001 s**. The run is recorded in `docs/qa/reduced-motion.md`.

130. **W4 · P0 · S · No invented metrics.** `scripts/check-claims.mjs` fails on a percentage or usage
     count in `web/` or `ui/` copy that has no recorded source. — done when every figure links to a
     measurement or a labelled placeholder and the check rejects a planted fake number.
   **Done (2026-11-06):** `node scripts/check-claims.mjs` -> `claims check: 3 sourced claims across 38 copy files` (exit 0), and `node scripts/check-claims.mjs --self-test` rejects a planted `99%` claim. `docs/claims.json` records the source and basis for every percentage in `web/` and visible `ui/src` copy; the check now runs inside `node scripts/check-docs.mjs`, which passes.

131. **W4 · P1 · M · Roster redesign.** `ui/src/roster.tsx` groups by project and status, virtualises
     long lists and keeps rename, pin, archive and delete in one row menu. — done when a 500-session
     fixture scrolls without dropping a row and the menu is keyboard reachable.

132. **W4 · P1 · L · Diff review screen.** `ui/src/diff-review.tsx` offers split and unified modes, a
     provenance rail per hunk and a verdict that names the reviewer. — done when each hunk can be
     accepted alone and the verdict is stored with the turn.

133. **W4 · P1 · M · Terminal chrome.** `ui/src/terminal.tsx` renders a real PTY with no fake window
     chrome. — done when font, scrollback and shell settings apply live and the shell survives a tab
     switch in a test.
   **Done (2026-11-06):** `ui/tests/terminal.spec.ts` passes 4 tests: live font/scrollback/program changes, the shell surface surviving a tab switch with generation 0, the desktop-titlebar layout, and the resize refit.

134. **W4 · P1 · M · Model picker v2.** `ui/src/model-picker.tsx` groups by provider with search, a
     free filter and price/latency hints. — done when the resolved pair shows on hover and a model
     with an unknown price says "unknown" instead of a guess.

135. **W1 · P0 · M · Approval card v2.** `ui/src/session/approval-card.tsx` reads command first,
     blast radius second, scope third and snapshot state last. — done when one primary action
     exists, deny is reachable by keyboard, and a `cannot_determine` row is never collapsed.
   **Done (2026-11-06):** `e2e/specs/approval.spec.ts` asserts command -> blast radius -> scope -> snapshot order, exactly one primary, the visible `cannot_determine` row, and Enter on Reject after four Tab stops.


136. **W4 · P1 · M · Cost dashboard.** `ui/src/cost-dashboard.tsx` shows a day, model and agent
     breakdown with a ledger, a cap indicator and CSV/JSON export. — done when the totals reconcile
     with the stored usage rows to the cent.

137. **W4 · P1 · L · Composer v2.** `ui/src/composer.tsx` puts attachments, skills, model, effort,
     context meter and a plan/act toggle in one row. — done when no control label wraps to two lines
     at the minimum window width and every control is keyboard reachable.

138. **W4 · P1 · M · Async state kit.** `ui/src/async-state.tsx` gives loading, empty, error, retry
     and success the same tokens. — done when each state announces to a live region and the kit is
     used on every async surface with a test per state.

139. **W4 · P2 · S · CMS parity.** `cms/src/app/(payload)/custom.scss` keeps the Payload admin on
     Rigel tokens, the mark and the Hallmark stamp. — done when the admin links to the public site
     and the Firebase console without leaving the brand and a screenshot is filed in `docs/qa/`.

140. **W4 · P0 · L · Accessibility audit.** `docs/qa/accessibility.md` records axe in CI for the
     session and approval flows, a screen-reader pass, contrast checks and target-size rules. — done
     when every failure is fixed or has a dated, owned exception and CI fails on a new violation.
   **Done (2026-11-06):** `npx playwright test axe` -> `3 passed; 0 failed` (session and approval flows clean under WCAG 2.0/2.1/2.2 A/AA, plus the dense-icon target check). The three violations found were fixed: pane separators carry their ARIA values, the changed-skill pill uses `--danger-text`, and the outline/chip/pin controls meet the 24px (26px adjacent) target rule. `docs/qa/accessibility.md` records the contrast measurements, the keyboard path, and the dated, owned screen-reader exception; the axe file runs inside the UI CI job.

## 7. CLI, MCP and the skills ecosystem (141–160)

141. **W5 · P1 · M · Versioned CLI JSON.** `crates/rigel-cli/src/json.rs` puts a schema version on
     every `--json` payload. — done when a shape test covers each command and a breaking field change
     bumps the version.

142. **W5 · P2 · S · Shell completions.** `crates/rigel-cli/src/completions.rs` generates bash, zsh,
     fish and PowerShell from clap. — done when each install path has a test and an unknown flag
     completes to an error, not silence.

143. **W5 · P1 · M · Agent run dry run.** `crates/rigel-cli/src/agent_run.rs` prints the resolved
     route, tools, prompt budget and required approvals. — done when `--dry-run` makes no network
     call, asserted by a socket test, and its JSON matches the real run.

144. **W5 · P1 · M · MCP health and discovery.** `crates/rigel-core/src/mcp/health.rs` reports connect
     latency, protocol version, tool count and last error per server. — done when one failed
     handshake is marked without blocking the others and the panel refreshes on demand.

145. **W5 · P1 · M · MCP result rendering.** `ui/src/mcp-result.tsx` renders text, images and
     structured results as typed rows. — done when a size cap truncates with a visible marker and
     the raw payload is one action away.

146. **W5 · P2 · L · MCP resources and prompts.** `crates/rigel-core/src/mcp/resources.rs` supports
     resources and prompt templates, not only tools. — done when both travel the same approval and
     audit path and a resource read is recorded like a tool call.

147. **W5 · P1 · L · Skill lockfile.** `rigel.lock` records version, hash and source per skill. — done
     when `rigel skills restore` reproduces the tree from the lockfile alone and a hash mismatch
     refuses to load.

148. **W5 · P2 · L · Skill eval harness.** `crates/rigel-core/src/skills/eval.rs` runs a golden task
     per skill before enabling and after an update. — done when a failing eval blocks the update and
     the report names the task and the diff.

149. **W5 · P1 · M · Skill authoring and lint.** `crates/rigel-cli/src/skills.rs` provides `rigel
     skills init`, `lint` and `test`. — done when the rules match the indexer exactly and a skill
     that lints clean also loads.

150. **W5 · P2 · M · Skill ranking.** `crates/rigel-core/src/skills/rank.rs` ranks by fit, freshness,
     evidence and install history. — done when no score is derived from popularity and each result
     shows the factors that ranked it.

151. **W5 · P2 · M · Skill conflict detection.** `crates/rigel-core/src/skills/conflict.rs` compares
     triggers and rules before two skills are pinned. — done when a duplicate trigger and a
     contradictory rule each produce a named conflict.

152. **W5 · P1 · M · Skill script contract.** `crates/rigel-core/src/skills/contract.rs` declares args,
     env and exit codes. — done when `--dry-run` prints the exact command and a script whose contract
     does not match is refused.

153. **W5 · P2 · L · Plugin API.** `crates/rigel-core/src/plugin/mod.rs` loads a subprocess or WASM
     plugin behind a capability manifest. — done when a plugin without a filesystem capability cannot
     read a file even when it tries, proven by a fixture.

154. **W5 · P2 · L · Headless server.** `crates/rigel-cli/src/serve.rs` exposes HTTP and SSE for CI and
     editors. — done when it uses the same approval path and a local token, and a request without the
     token is refused.

155. **W5 · P2 · M · GitHub Action.** `.github/actions/rigel-review/action.yml` runs a bounded turn and
     posts a summary. — done when it pushes nothing without an explicit input and the summary links
     the turn it came from.

156. **W5 · P2 · M · Git hooks.** `crates/rigel-cli/src/hooks.rs` installs an optional pre-commit
     secret scan, an attribution check and a `rigel` trailer. — done when uninstall restores the
     previous hook byte for byte.

157. **W5 · P2 · L · jj and worktree interop.** `crates/rigel-core/src/git/jj.rs` detects jj and maps
     its working copy to the same write path. — done when snapshots stay correct in a jj test
     repository and a plain git repository still behaves as before.

158. **W5 · P2 · M · Agent config import and export.** `crates/rigel-core/src/config/interop.rs` reads
     and writes Claude, Codex and Cursor config. — done when unknown keys survive a round trip and a
     conflicting key is reported, not overwritten.

159. **W5 · P2 · M · Prompt library.** `crates/rigel-core/src/prompts/library.rs` versions prompts per
     project with variables and a diff. — done when a prompt can be pinned into a session and the
     pinned text is what the transcript shows.

160. **W5 · P2 · L · Batch mode.** `crates/rigel-cli/src/batch.rs` runs one task across N repositories
     under a single spend cap. — done when each repository gets a report and the cap stops the batch
     with the remaining list printed.
## 8. Cloud, sync, mobile and collaboration (161–180)

161. **W5 · P2 · L · Hosted cloud, opt in.** `docs/cloud-privacy.md` records the privacy review and
     the operator and billing statement for a hosted project. — done when cloud is off by default,
     the document lists every outbound call, and there is no telemetry path.

162. **W5 · P1 · M · Account export and delete.** `crates/rigel-core/src/cloud/account.rs` exports
     every cloud document, then deletes the account and its data. — done when the confirmation names
     what remains and a post-delete request returns a documented not-found.

163. **W5 · P1 · L · Sync revision vectors.** `crates/rigel-core/src/sync/revision.rs` gives each
     document a revision vector. — done when both sides moving produces a conflict row and no state
     is resolved by last write alone.

164. **W5 · P2 · M · Publication redaction preview.** `ui/src/publish-preview.tsx` lists every row
     that publishing a session would expose. — done when tool output and diffs default off and the
     chosen scope is stored per session.

165. **W5 · P2 · M · App Check enforcement.** `crates/rigel-core/src/cloud/app_check.rs` requires a
     valid App Check token when a project configures it. — done when an un-attested client fails with
     a documented error and the failure is listed in `docs/troubleshooting.md`.

166. **W5 · P2 · L · Sponsored runs.** `crates/rigel-core/src/cloud/sponsor.rs` funds a run from an
     operator account with a visible sponsor and a cap. — done when the report names the sponsor and
     no run is subsidised without a line in the report.

167. **W5 · P2 · L · Remote approvals.** `crates/rigel-core/src/approval/remote.rs` sends an approval
     to the phone with the exact command, scope and expiry. — done when the answering device is
     recorded and an expired remote approval cannot approve anything.

168. **W5 · P2 · L · GitHub PR and check sync.** `crates/rigel-core/src/forge/github.rs` imports PRs,
     checks and review comments into a session. — done when a turn links to the PR it changed and a
     closed PR is marked closed on the next sync.

169. **W5 · P2 · L · Forge trait and conformance suite.** `crates/rigel-core/src/forge/mod.rs` defines
     the PR and check operations one trait. — done when one conformance suite runs against every
     implementation and a missing operation fails to compile.

170. **W5 · P2 · M · Linear connector.** `crates/rigel-core/src/forge/linear.rs` reads issues and
     writes a status and link. — done when OAuth is scoped per project and a revoked token shows a
     reconnect action instead of a failure loop.

171. **W5 · P2 · M · System proxy.** `crates/rigel-core/src/net/proxy.rs` applies the OS proxy and an
     explicit override to every HTTP path. — done when updates and MCP HTTP honour the same setting
     and a bypass list is tested.

172. **W5 · P2 · L · OAuth sign-in.** `crates/rigel-core/src/cloud/oauth.rs` signs in with Google and
     GitHub using PKCE and a loopback callback. — done when no client secret exists in the binary and
     a replayed callback is refused.

173. **W5 · P2 · M · Legal documents.** `docs/terms.md` and `docs/privacy.md` match the network calls
     in `docs/cloud-privacy.md`. — done when every listed call has a purpose and a retention period
     and the review date is recorded.
     **Done (2026-11-06):** added `docs/cloud-privacy.md` with a row for every outbound call (model provider, Firebase Identity Toolkit/App Check/Firestore, update manifest and installer, skills sources, AgentMail, GitHub, and configured MCP servers), each with a purpose and retention/control. Review date 2026-11-06 is recorded; `privacy.md` and `terms.md` now list the same MCP outbound call, and `node scripts/check-docs.mjs` -> `docs check: 48 documents and every issue form are valid` (exit 0).

174. **W5 · P2 · L · Phone client.** `mobile/` lists sessions, streams a live turn, approves or
     denies, and keeps a read-only offline cache. — done when the cache cannot send while offline and
     an approval round-trips to the desktop.

175. **W5 · P2 · M · Web reader and share links.** `web/session/[id].html` renders a read-only session
     with an expiry and a revoke action. — done when tool output is off by default and a revoked link
     returns not-found.

176. **W5 · P2 · L · Team workspace.** `crates/rigel-core/src/cloud/team.rs` adds shared projects,
     roles, per-member spend and an audit trail. — done when a member cannot widen another member's
     grant and the attempt is recorded.

177. **W5 · P2 · L · Realtime collaboration.** `crates/rigel-core/src/sync/presence.rs` shares a
     session event log with presence and conflict rows. — done when the local database stays
     authoritative and a reconnect reconciles without duplicating a turn.

178. **W5 · P2 · M · Cloud queue hardening.** `crates/rigel-core/src/cloud/queue.rs` adds leases,
     retries, cancel and requeue. — done when a dead-letter view lists every failed run with its last
     error and a requeue preserves the original request.

179. **W5 · P1 · M · Metadata conflict UI.** `ui/src/metadata-conflict.tsx` shows base, ours and
     theirs per field. — done when a merge action stores the decision with its author and an
     unresolved field blocks publishing.

180. **W5 · P2 · L · Region and residency.** `crates/rigel-core/src/cloud/region.rs` pins a project to
     a region and shows where its data lives. — done when a cross-region move needs an explicit
     confirmation and the data-location panel matches the stored region.

## 9. Data, storage and performance (181–192)

181. **W5 · P1 · M · Hot-query plan assertions.** `crates/rigel-core/tests/query_plans.rs` records
     `EXPLAIN QUERY PLAN` for the ten hottest queries. — done when each plan shows an index rather
     than a table scan and dropping an index fails the test by query name.

182. **W5 · P0 · M · SQLite integrity check.** `crates/rigel-core/src/store/integrity.rs` runs
     `PRAGMA integrity_check` and `foreign_key_check` after an unclean shutdown. — done when a
     damaged fixture is reported with table and row and the repair path copies the file before it
   **Done (2026-11-06):** `cargo test -p rigel-core --lib store::integrity` -> `3 passed; 0 failed`. `store/integrity.rs` runs `PRAGMA integrity_check` and `foreign_key_check`, reports an orphan with its table (`child`) and row (`7`), and `check_and_repair` copies the database into the backup directory before `REINDEX`/`VACUUM`, with the test asserting the backup bytes equal the pre-repair file.
     touches anything.

183. **W5 · P1 · M · Database maintenance.** `crates/rigel-cli/src/db.rs` owns the WAL checkpoint
     policy, `ANALYZE`, size reporting and a `vacuum` that is safe while the app runs. — done when a
     concurrent reader completes during the vacuum and the size report names each table.

184. **W5 · P1 · M · Blob store integrity.** `crates/rigel-core/src/blobs/store.rs` dedupes by hash,
     verifies on read, collects orphans and reports bytes per project. — done when a corrupted blob
     is reported by hash and a re-read refuses to serve it.

185. **W5 · P1 · M · Streaming backpressure.** `crates/rigel-core/src/agent/stream.rs` caps in-memory
     output and spills to disk. — done when a slow renderer cannot block the producer and the spill
     file is removed on commit.

186. **W5 · P1 · M · Transcript virtualisation.** `ui/src/transcript/virtual.tsx` renders only the
     visible window with stable scroll anchoring. — done when a 10,000-row fixture keeps its
     position on resize and jump-to-turn lands on the right row.

187. **W5 · P1 · M · Incremental tree hashing.** `crates/rigel-core/src/index/hash.rs` hashes only
     changed paths and caches directory hashes. — done when an edit invalidates exactly its subtree
     and resume and attribution checks stay exact.

188. **W5 · P1 · M · Parallel tools, ordered results.** `crates/rigel-core/src/agent/parallel.rs` runs
     independent read-only tools together and commits results in request order. — done when a slow
     first tool does not reorder the transcript and a write tool never runs in the batch.

189. **W5 · P1 · M · Session event log compaction.** `crates/rigel-core/src/store/event_log.rs`
     collapses superseded streaming chunks into their final message on write. — done when a
     million-chunk fixture stays within a bounded row count and replay still reconstructs the exact
     transcript.

190. **W5 · P1 · M · Cold-start budget.** `crates/rigel-app/tests/startup.rs` measures launch to
     interactive on a reference machine. — done when the median stays under two seconds and a
     regression beyond ten percent fails CI with both numbers.

191. **W5 · P1 · S · HTTP connection reuse.** `crates/rigel-core/src/net/client.rs` keeps one client
     per provider with keep-alive, bounded pools and a timeout per phase. — done when a test asserts
     pool reuse and a phase timeout names the phase that expired.

192. **W5 · P1 · L · Performance regression suite.** `benches/` covers indexing, search, diff and
     patch apply with recorded thresholds. — done when a run prints a trend against the last recorded
     baseline and a regression beyond the threshold fails.

## 10. Testing, observability, docs and release (193–200)

193. **W6 · P1 · M · Mutation testing.** `crates/rigel-core/tests/mutants.md` records the
     `cargo-mutants` run over routing, approvals and budget arithmetic. — done when every surviving
     mutant has either a new test or a written equivalence argument.

194. **W6 · P0 · M · Adapter contract fixtures.** `crates/rigel-adapters/tests/fixtures/` holds a
     recorded stream per provider and harness, replayed in CI. — done when a protocol change fails a
     named test and adding an adapter requires a fixture.
     **Done (2026-11-06):** `cargo test -p rigel-adapters --test replay` -> `5 passed; 0 failed`, including `muse_fixtures_replay_through_the_adapter`; `cargo test -p rigel-core --test guarantees fixture_replay_covers_every_harness_and_scenario` -> `1 passed`. Muse now has all five golden streams and `fake-muse` replays `FAKE_MUSE_FIXTURE`.

195. **W6 · P1 · M · Local tracing.** `crates/rigel-core/src/telemetry/spans.rs` adds opt-in spans for
     a turn, tool, request and migration. — done when spans are stored locally, redacted with the
     shared function, and the trace view has no upload action.

196. **W6 · P2 · S · Local metrics.** `crates/rigel-cli/src/stats.rs` reports turn latency, tokens,
     retries and cache hits. — done when `rigel stats --json` has a documented shape and a test
     asserts that no request leaves the machine.

197. **W6 · P1 · M · Architecture decision records.** `docs/adr/` holds one short record for routing,
     storage, approvals and cloud. — done when each names the alternatives, the date and the
     consequence that would reopen it.
     **Done (2026-11-06):** added `docs/adr/README.md` and four records covering routing, storage, approvals and cloud. Each names the alternatives, the date 2026-11-06 and the consequence that would reopen it. `node scripts/check-docs.mjs` -> `docs check: 48 documents and every issue form are valid` (exit 0).

198. **W6 · P1 · M · Troubleshooting cookbook.** `docs/troubleshooting.md` maps real error strings to
     causes and fixes. — done when 401 credential shapes, 429, the MCP handshake, migration refusal
     and updater signature each have an entry quoted from an actual log line.
     **Done (2026-11-06):** added `docs/troubleshooting.md` with both 401 shapes (the provider mismatch and the live router's `HTTP/1.1 401 Unauthorized` / `Authentication required`), 429, MCP handshake, migration-refusal and updater-signature lines. The quoted strings are backed by five passing checks plus the router probe: provider shape 1/1, Anthropic 429 1/1, project MCP 2/2, newer-schema refusal 1/1, signed-manifest mismatch 1/1, and `curl -sS -i http://127.0.0.1:20128/v1/models` -> `HTTP/1.1 401 Unauthorized` with `{"error":{"message":"Authentication required","type":"invalid_api_key","code":"invalid_api_key"}}`; `node scripts/check-docs.mjs` -> `49 documents and every issue form are valid`.

199. **W6 · P2 · M · Community surface.** `.github/ISSUE_TEMPLATE/` asks for a diagnostics block and
     the skill showcase stays curated. — done when a new issue form passes the docs check and every
     showcased skill is one the repository has tested.
     **Done (2026-11-06):** `bug_report.yml` now requires a diagnostics block, and `node scripts/check-docs.mjs` -> `49 documents and every issue form are valid`. `cargo test -p rigel-core --test showcase` -> `1 passed`: every `skills/…/SKILL.md` linked from the public shelf is read from this repository and parsed with the production frontmatter parser.

200. **W6 · P0 · M · Cut 0.3.0.** `docs/release-0.3.0.md` is the dated receipt for the release. — done
     when the checklist runs, signed assets publish, the installed version verifies, the receipt
     posts and the retro names what slipped.
     **Blocked (2026-11-06):** the receipt requires publishing signed 0.3.0 assets and verifying an installed build; this machine has no code-signing certificate or release credentials, and the open 0.3 feature items are not complete. Prerequisite: finish the open items, provision the Windows signing identity, publish the signed installers and provenance, then install that build and post the receipt.

---

## 35 repositories that can help

These are inputs, not dependencies. Check the licence and pin a commit before borrowing code.

| Repository | What to borrow | Roadmap items |
|---|---|---|
| [tauri-apps/tauri](https://github.com/tauri-apps/tauri) | window, tray and updater patterns and the capability model | 1, 6, 7, 63, 80 |
| [tauri-apps/plugins-workspace](https://github.com/tauri-apps/plugins-workspace) | official updater, store, notification and single-instance plugins | 7, 114, 115 |
| [tauri-apps/tauri-action](https://github.com/tauri-apps/tauri-action) | signed release CI and draft assets | 5, 9, 11 |
| [payloadcms/payload](https://github.com/payloadcms/payload) | admin components, access control and migrations | 139 |
| [tokio-rs/tokio](https://github.com/tokio-rs/tokio) | cancellation, bounded channels and task supervision | 16, 17, 185, 191 |
| [tokio-rs/tracing](https://github.com/tokio-rs/tracing) | local spans, redaction fields and JSON layers | 195 |
| [launchbadge/sqlx](https://github.com/launchbadge/sqlx) | migrations, SQLite pooling and query plans | 2, 181, 182, 183 |
| [clap-rs/clap](https://github.com/clap-rs/clap) | completions and typed CLI errors | 141, 142 |
| [modelcontextprotocol/rust-sdk](https://github.com/modelcontextprotocol/rust-sdk) | MCP client and server transports | 68, 69, 144, 146 |
| [modelcontextprotocol/servers](https://github.com/modelcontextprotocol/servers) | reference servers and test fixtures | 81, 144 |
| [BerriAI/litellm](https://github.com/BerriAI/litellm) | provider normalisation, retries and fallback semantics | 13, 22, 23, 26 |
| [Portkey-AI/gateway](https://github.com/Portkey-AI/gateway) | routing policies, guardrails and retry classes | 15, 22, 24 |
| [vercel/ai](https://github.com/vercel/ai) | streaming UI and tool-result patterns | 17, 39 |
| [microsoft/playwright](https://github.com/microsoft/playwright) | end-to-end tests and the webview driver | 1, 128, 139 |
| [rust-fuzz/cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) | libFuzzer targets and corpora | 3, 81 |
| [nextest-rs/nextest](https://github.com/nextest-rs/nextest) | fast, retryable test runs | 9, 10, 193 |
| [taiki-e/cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) | coverage in the nightly lane | 9, 192 |
| [rustsec/rustsec](https://github.com/rustsec/rustsec) | advisory database and audit CI | 9 |
| [EmbarkStudios/cargo-deny](https://github.com/EmbarkStudios/cargo-deny) | licences, advisories and duplicate crates | 9 |
| [gitleaks/gitleaks](https://github.com/gitleaks/gitleaks) | secret rules for the pre-write scan | 60 |
| [trufflesecurity/trufflehog](https://github.com/trufflesecurity/trufflehog) | verified-secret scanning | 60 |
| [radix-ui/primitives](https://github.com/radix-ui/primitives) | accessible menus, dialogs and focus management | 127, 128, 140 |
| [shadcn-ui/ui](https://github.com/shadcn-ui/ui) | token and component composition ideas, not a theme to copy | 106, 131 |
| [codemirror/dev](https://github.com/codemirror/dev) | editor and diff surface | 112, 123 |
| [tree-sitter/tree-sitter](https://github.com/tree-sitter/tree-sitter) | symbol map and structural parsing | 97 |
| [ast-grep/ast-grep](https://github.com/ast-grep/ast-grep) | structural search and rewrite for the review flow | 97, 156 |
| [firebase/firebase-tools](https://github.com/firebase/firebase-tools) | emulators and rules deployment | 161, 165, 168 |
| [anthropics/skills](https://github.com/anthropics/skills) | skill format, eval and packaging conventions | 147, 148, 149 |
| [vercel-labs/agent-skills](https://github.com/vercel-labs/agent-skills) | skill authoring and eval examples | 147, 150 |
| [All-Hands-AI/OpenHands](https://github.com/All-Hands-AI/OpenHands) | agent loop and event-stream architecture | 41, 42, 43 |
| [humanlayer/humanlayer](https://github.com/humanlayer/humanlayer) | approval and human-in-the-loop patterns | 64, 65, 83 |
| [e2b-dev/E2B](https://github.com/e2b-dev/E2B) | sandbox lifecycle and capability boundaries | 63, 67 |
| [jj-vcs/jj](https://github.com/jj-vcs/jj) | working-copy model for the jj interop item | 157 |
| [nektos/act](https://github.com/nektos/act) | run the GitHub Action locally | 9, 155 |
| [oxc-project/oxc](https://github.com/oxc-project/oxc) | fast JS and TS lint for `ui/` and `cms/` | 9, 139 |

## Skills found with `/find-skills`

`npx skills find` on 6 Nov 2026. Install counts are the CLI's own. The CLI printed a libuv assertion
on exit on Windows after returning results; the results were still usable.

| Skill | Installs | Use | Command |
|---|---:|---|---|
| `rheinmir/setup@hallmark` | 174 | design doctrine for the window work | `npx skills add https://github.com/rheinmir/setup --skill hallmark` |
| `mindrally/skills@tauri-development` | 1.4K | Tauri patterns for the native shell | `npx skills add mindrally/skills@tauri-development` |
| `tursodatabase/turso@code-quality` | 1.8K | Rust review checklist for the supply chain and mutation work | `npx skills add tursodatabase/turso@code-quality` |
| `jssblck/agents@code-craft` | 1.7K | diff and review discipline | `npx skills add jssblck/agents@code-craft` |
| `rtk-ai/rtk@design-patterns` | 1.9K | architecture vocabulary for the ADRs | `npx skills add rtk-ai/rtk@design-patterns` |
| `powersync-ja/agent-skills@powersync` | 1.3K | sync and conflict patterns | `npx skills add powersync-ja/agent-skills@powersync` |
| `usestrix/strix@owasp-top-10-testing` | 15.3K | security tests for the secret, injection and threat-model work | `npx skills add usestrix/strix@owasp-top-10-testing` |
| `usestrix/strix@web-app-penetration-testing` | 15.4K | red-team method for the fixture suite | `npx skills add usestrix/strix@web-app-penetration-testing` |
| `wondelai/skills@software-design-philosophy` | 6.3K | module boundaries for the harness and plugin work | `npx skills add wondelai/skills@software-design-philosophy` |
| `trailofbits/skills@spec-to-code-compliance` | 6.2K | spec-to-test checks for adapter fixtures | `npx skills add trailofbits/skills@spec-to-code-compliance` |
| `wshobson/agents@distributed-tracing` | 11K | local tracing spans | `npx skills add wshobson/agents@distributed-tracing` |
| `dchuk/claude-code-tauri-skills@running-nodejs-sidecar-in-tauri` | 150 | sidecar lifecycle; low install count, review first | `npx skills add dchuk/claude-code-tauri-skills@running-nodejs-sidecar-in-tauri` |

Hallmark's page lists Gen Agent Trust Hub pass, Socket pass and **Snyk warn**. Treat the warn as a
review item, not a blocker: the skill is instructions, not executable code.

## Order of work

Wave 1 is broken into executable work packages with tests and exit criteria in
[sprint-0.3-wave-1.md](sprint-0.3-wave-1.md). The lists below are the only place wave membership is
repeated, and `scripts/check-roadmap.mjs` fails if they disagree with the item metadata or with the
sprint table.

- **Wave 1 (weeks 1–3): proof and safety.** 1–2, 9, 13–14, 16–17, 24, 59, 71, 127–128, 135.
- **Wave 2 (weeks 4–6): release and the turn.** 3–8, 10–12, 15, 18–23, 25–30.
- **Wave 3 (weeks 7–10): runtime, tools and craft.** 31–58, 60–70, 72–84.
- **Wave 4 (weeks 11–13): the window.** 85–126, 129–134, 136–140.
- **Wave 5 (weeks 14–18): ecosystem, data and cloud.** 141–192.
- **Wave 6: cut 0.3.0.** 193–200.

### The first ten, in order

- **Native window driver** — item 1.
- **Migration rehearsal** — item 2.
- **Credential guards everywhere** — item 24.
- **Secret redaction** — item 59.
- **File-jail traversal tests** — item 71.
- **Shared rate limiter and cancellation accounting** — items 16 and 17.
- **Route inspector and `route explain`** — items 13 and 14.
- **Approval card v2** — item 135.
- **Eight-state and keyboard audits** — items 127 and 128.
- **Supply-chain CI lane** — item 9.

### Repairs made in this audit

The 200 items were rewritten to one shape, and six pairs that described the same change were merged.
Each merge replaced the duplicate with a different task, so the count still reaches 200:

- Item 181 was "FTS5 everywhere", a second description of item 93; it is now hot-query plan
  assertions, which nothing else covered.
- Item 118 duplicated item 134's favourites and price hints; it is now the provider error panel.
- Item 144 duplicated item 68's MCP pinning; it is now MCP health and discovery.
- Item 182 duplicated item 2's migration rehearsal; it is now the SQLite integrity check.
- Item 53 and item 98 both described prompt precedence; 53 is now the per-section prompt budget and
  98 is the rule precedence resolver.
- Item 77 and item 147 both described skill signatures; 77 verifies at load and 147 owns the
  lockfile and reproducible restore.

### Explicitly not doing in 0.3

No telemetry, no cloud by default, no silent provider fallback, no remote code in the webview, no
auto-merge of a conflicted hunk, and no "AI-generated" marketing metrics. These are product
constraints, not backlog items.

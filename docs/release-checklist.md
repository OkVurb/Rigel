# Release checklist

The release script is the only publisher. This checklist is the evidence it must be allowed to
run. A box that cannot be ticked is a blocker, not a note.

## 0.1.7 — public unsigned beta

### Scope and tree

- [ ] `git status` is clean after the release commit.
- [ ] `docs/grilling-round-9.md` contains Q216–Q335 and the `0.1.7`/`0.2` disposition.
- [ ] `docs/roadmap-0.2.md` records every deferred `0.2` item.
- [ ] `README.md`, `docs/PLATFORMS.md`, `docs/what-works.md`, `CHANGELOG.md`, `SECURITY.md`, and
      this file are current.
- [x] Version is `0.1.7` in `Cargo.toml`, `crates/rigel-app/tauri.conf.json`, `ui/package.json` and
      `web/index.html`.

### Code gate

- [x] `cargo fmt --all -- --check` — clean on 4 Nov 2026 after normalizing the committed tree.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` — clean; the MSRV move to 1.88
      activated `manual_is_multiple_of`, applied with `cargo clippy --fix`.
- [x] `cargo test --workspace` — 39 suites, 0 failures.
- [x] `npm --prefix ui run build` — 50 modules, built.
- [x] `node scripts/check-docs.mjs` — 37 documents and every issue form valid.
- [x] `node scripts/check-versions.mjs` — 0.1.6 agrees across 3 files before the bump; rerun at 0.1.7.
- [x] `node scripts/handoff-status.mjs` — 82/84, Phase 7 7/9.
- [x] Focused regression test for the settings repair — `an_older_settings_file_gains_the_chat_filter`.
- [x] Cargo and npm audits recorded — see below.
- [x] SBOM generated — 611 components; attached by the release workflow.

#### Audit record, 4 Nov 2026

`cargo audit` reports **0 vulnerabilities** after four dependency moves: `tokio` 1.54.0 → 1.54.1
(yanked, RUSTSEC-2026-0384), and — through `plist` 1.8.0 → 1.10.3 — `quick-xml` 0.38.4 → 0.42.0
(RUSTSEC-2026-0194 and -0195, both high) and `time` 0.3.45 → 0.3.55 (RUSTSEC-2026-0009). Those
versions need Rust 1.88, so `rust-version` moves from 1.85 to 1.88 and the README and CONTRIBUTING
statements move with it. Warnings without a fix path through Tauri remain: `proc-macro-error` and
the `unic-*` crates are unmaintained, and glib's unsoundness advisory is Linux-only. Neither is a
vulnerability on Windows. `npm --prefix ui audit --omit=dev` reports 0 vulnerabilities.

### Installer gate

- [x] Build from `crates/rigel-app` with `cargo tauri build` — two clean builds, exit 0 each.
- [x] MSI and NSIS produced — `Rigel_0.1.7_x64_en-US.msi`, `Rigel_0.1.7_x64-setup.exe`.
- [x] `node scripts/check-installer-branding.mjs --dist` — 42 checks passed.
- [x] Build twice and compare — compared; **not byte-identical** (WiX and makensis stamp build
      metadata). Recorded as a reproducibility gap in `docs/qa/p7-acceptance.md`.
- [ ] Windows Sandbox install/start/doctor/uninstall pass recorded in `docs/qa/p7-acceptance.md`
      — not run for this cut; the section above records the unapplied boot requirement.
- [ ] Windows 10 1809 and SmartScreen recorded as manual gaps.
- [ ] MSI and NSIS SHA-256 hashes recorded in the release receipt.
- [ ] `latest.json` Ed25519 signature verifies against the committed public key.

### Publish gate

- [x] `node scripts/release.mjs 0.1.7` creates the version commit and tag.
- [x] Tag and branch push — `v0.1.7` on `54f64f6`, the commit whose CI is green.
- [x] Release workflow passes its CI gate and publishes `latest.json`, MSI, NSIS, and `sbom.cdx.json`.
- [x] Release notes use the curated `docs/release-notes/0.1.7.md`.
- [x] Published assets are downloaded and their hashes match the receipt — both artifacts match
      `latest.json`.
- [ ] The published MSI installs on this machine — **not executed**: the elevation prompt was
      cancelled and the machine still reports 0.1.6.
- [ ] The 0.1.6 update path is checked against the published `latest.json` — blocked by the install
      check above; the manifest itself verifies.
- [ ] The app reports `0.1.7` after installation — blocked by the same install check.
- [x] Post-release receipt is posted — `docs/release-receipts/0.1.7.md`, with the gaps above.

### Rollback

- [ ] The v0.1.6 MSI and NSIS artifacts and hashes are retained.
- [ ] `docs/PLATFORMS.md` explains manual rollback.
- [ ] A failed startup is treated as a release blocker, not an automatic rollback (Q228).

## Future releases

Copy this section into a versioned file under `docs/release-notes/` and keep the same gates.

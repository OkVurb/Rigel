# sqlx-sqlite 0.8.6 (locally patched)

Source: the `sqlx-sqlite` 0.8.6 crate from crates.io, dual-licensed MIT OR
Apache-2.0 (the licence files are alongside this note).

This copy changes exactly one thing: `libsqlite3-sys` is pinned to 0.39.1
instead of 0.30.1. `libsqlite3-sys` 0.39 bundles SQLite 3.54.1 and fixes
RUSTSEC-2026-0459 / CVE-2026-119375. SQLx main made the same dependency-bound
change in commit `10462fa4414c`; no `sqlx-sqlite` Rust source changed.

Remove this vendor directory and the `[patch.crates-io]` entry in the workspace
`Cargo.toml` once a released SQLx version allows `libsqlite3-sys` 0.39 or newer.

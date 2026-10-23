## What and why

<!-- The change in a paragraph: what it does, and the problem it solves. Link the issue it closes. -->

## How it was checked

<!-- The commands you ran. Say plainly if something could not run, and why. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `(cd ui && npx tsc --noEmit && npm run build)`

## Checklist

- [ ] A failing test is fixed by fixing the cause: no `#[ignore]`, no weakened assertion, no lint silenced with an `allow` attribute.
- [ ] If this touches the schema: a new numbered file in `migrations/` plus a `Migration` entry, and no applied migration edited.
- [ ] If this changes a trust weight: `rigel trust explain` output and the pinned arithmetic test move in the same pull request.
- [ ] If this changes behaviour a document describes: that document is updated here, not afterwards.
- [ ] No new dependency for something the standard library does in twenty lines. If one is genuinely needed, the Cargo.toml comment says why.

## Notes for review

<!-- A decision you took, a trade-off, or something deliberately left out. Security issues: security@rigel.invalid, never a pull request description. -->

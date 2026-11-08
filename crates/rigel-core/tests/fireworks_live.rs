//! Live, keyed check of Fireworks' model roster and our local price coverage.
//!
//! This is deliberately opt-in: an ordinary `cargo test` must not spend a network request or
//! depend on a developer's key. With a key in the keychain (Settings → Providers, or
//! `rigel keys set fireworks`) or `FIREWORKS_API_KEY` set, run:
//!
//! ```text
//! RIGEL_FIREWORKS_SMOKE=1 cargo test -p rigel-core --test fireworks_live -- --nocapture
//! ```
//!
//! It prints the roster ids and fails if a benchmark model is missing from the roster or has no
//! local price row, which is exactly the condition that would make a FireRouter turn unpriced.

use rigel_core::agent::{catalog, pricing};
use rigel_core::credentials::KeychainCredentialStore;

#[tokio::test]
async fn the_live_fireworks_roster_contains_our_priced_models() {
    if std::env::var("RIGEL_FIREWORKS_SMOKE").as_deref() != Ok("1") {
        eprintln!(
            "skipped: set RIGEL_FIREWORKS_SMOKE=1 (with a Fireworks key) to run the live roster check"
        );
        return;
    }

    let store = KeychainCredentialStore;
    let rows = catalog::catalog("fireworks", &store).await.expect(
        "Fireworks roster: store a key with `rigel keys set fireworks` or set FIREWORKS_API_KEY",
    );

    println!("Fireworks returned {} models", rows.len());
    for row in &rows {
        println!(
            "  {}  {}",
            row.id,
            if row.tools { "tools" } else { "no-tools" }
        );
    }

    for wanted in ["glm-5p3-flash", "deepseek-v4p1-flash", "kimi-k3p1-pro"] {
        let row = rows
            .iter()
            .find(|row| row.id == wanted || row.id.ends_with(&format!("/{wanted}")))
            .unwrap_or_else(|| {
                panic!(
                    "the Fireworks roster did not include {wanted}; the live ids were:\n{}",
                    rows.iter()
                        .map(|row| row.id.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            });
        assert!(
            pricing::is_listed(&row.id),
            "{} has no local price row; add it to pricing.rs before selecting it",
            row.id
        );
        assert!(row.tools, "{} does not advertise tool calls", row.id);
    }

    assert!(
        rows.iter()
            .any(|row| row.id == "firerouter" || row.id.ends_with("/firerouter")),
        "FireRouter must be selectable even when /models omits the routing id"
    );
}

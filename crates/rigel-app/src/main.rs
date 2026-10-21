//! `rigel-app` — the Tauri 2 shell.
//!
//! Phase 1 deliberately keeps this a plain binary: the window, IPC commands and the
//! WebView2 build arrive with P1.6 and P1.7. Wiring Tauri before the storage contract is
//! settled would mean building the shell twice.

fn main() {
    println!(
        "rigel-app {} — shell lands with P1.6/P1.7; the core and CLI are usable now.",
        env!("CARGO_PKG_VERSION")
    );
}

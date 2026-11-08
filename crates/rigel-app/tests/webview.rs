//! Webview hardening proof (roadmap 80).
//!
//! The configuration is read as data, so a change that re-enables eval or remote script
//! fails here before a build starts. The navigation policy is the production function the
//! window uses.

use rigel_app::webview::navigation_allowed;
use serde_json::Value;

fn config() -> Value {
    serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json parses")
}

#[test]
fn the_csp_allows_no_remote_code_and_no_unsafe_eval() {
    let config = config();
    let csp = &config["app"]["security"]["csp"];
    assert_eq!(csp["script-src"], "'self'");
    assert_eq!(csp["default-src"], "'self'");
    assert_eq!(csp["object-src"], "'none'");
    assert_eq!(csp["base-uri"], "'self'");
    assert_eq!(csp["form-action"], "'none'");
    let rendered = csp.to_string();
    assert!(!rendered.contains("unsafe-eval"), "the CSP allows eval");
    // Inline style is the only permitted relaxation; no directive may load remote code.
    for directive in ["script-src", "object-src", "base-uri", "default-src"] {
        let value = csp[directive].to_string();
        assert!(
            !value.contains("http://") && !value.contains("https://"),
            "{directive} allows a remote origin: {value}"
        );
    }
    assert_eq!(csp["connect-src"], "ipc: http://ipc.localhost");
}

#[test]
fn a_page_cannot_move_the_app_webview() {
    assert!(navigation_allowed("http://tauri.localhost/index.html"));
    for url in [
        "https://example.com",
        "http://192.168.1.10/admin",
        "file:///C:/Windows/System32/cmd.exe",
        "javascript:fetch('https://example.com')",
    ] {
        assert!(!navigation_allowed(url), "{url} was allowed");
    }
}

#[test]
fn the_capability_grants_no_navigation_or_shell_permission() {
    let capability: Value =
        serde_json::from_str(include_str!("../capabilities/default.json")).expect("capability");
    let permissions = capability["permissions"].to_string();
    for forbidden in ["shell:", "allow-navigate", "allow-open-url", "webview:"] {
        assert!(
            !permissions.contains(forbidden),
            "the default capability grants {forbidden}"
        );
    }
}

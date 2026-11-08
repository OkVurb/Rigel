//! Webview navigation policy (roadmap 80).
//!
//! The main window only ever loads the packaged frontend. A link in model output or a
//! fetched page may not move the app webview to a remote origin; an external URL belongs in
//! the user's browser, not in the shell that holds the IPC bridge.

/// The origins the packaged frontend is served from. Tauri uses `tauri://localhost` on
/// Windows and the https variant on a platform that serves assets over the custom protocol.
pub const APP_ORIGINS: &[&str] = &[
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// Whether the app webview may navigate to `url`. Internal IPC and the packaged frontend
/// are allowed; every other scheme and origin is refused.
pub fn navigation_allowed(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    if lower == "about:blank" {
        return true;
    }
    if lower.starts_with("ipc://") || lower.starts_with("http://ipc.localhost") {
        return true;
    }
    APP_ORIGINS
        .iter()
        .any(|origin| lower == *origin || lower.starts_with(&format!("{origin}/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_packaged_frontend_and_ipc_are_allowed() {
        assert!(navigation_allowed("http://tauri.localhost/index.html"));
        assert!(navigation_allowed("tauri://localhost/"));
        assert!(navigation_allowed("ipc://localhost"));
        assert!(navigation_allowed("about:blank"));
    }

    #[test]
    fn a_remote_or_file_page_cannot_move_the_webview() {
        for url in [
            "https://example.com/phish",
            "http://127.0.0.1:8080/",
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
        ] {
            assert!(!navigation_allowed(url), "{url} was allowed");
        }
    }
}

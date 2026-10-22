//! P4.1: probe every external harness once and cache the answer in `adapters.json`.
//!
//! The Settings screen and `rigel doctor` ask the same question — which harnesses exist, what
//! version, and whether each one has an API key — so they share one probe and one file. The file
//! lives beside `index.db` and `settings.toml` in the app's data directory, which is what makes it
//! a cache rather than a second source of truth: a probe that cannot write it still returns its
//! result.
//!
//! A missing harness is a row, not an error. The UI has to render "not installed" with the command
//! that fixes it, and a probe that fails as a whole would turn four reports into none.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{
    AcpConfig, Adapter, AdapterError, AuthMode, ClaudeAdapter, ClaudeConfig, CodexAdapter,
    CodexConfig, Harness, HarnessInfo, credential_spec, registry,
};

/// The cache's file name under the data directory.
pub const FILE_NAME: &str = "adapters.json";

#[derive(Debug, Clone, Serialize)]
pub struct AdapterCache {
    /// Unix milliseconds. Informational: nothing expires on it yet, because a probe is cheap and
    /// the screen re-probes on open.
    pub probed_at: i64,
    pub harnesses: Vec<HarnessInfo>,
}

/// Where the cache lives for a data directory (`%APPDATA%\Rigel`, or `RIGEL_INDEX_DIR`).
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// Probe all four external harnesses. Each probe is independent and bounded by its own timeout, so
/// one hung binary cannot hold the other three.
pub async fn probe_all(cwd: &Path) -> Vec<HarnessInfo> {
    let codex = CodexAdapter::new(CodexConfig::router(cwd));
    let claude = ClaudeAdapter::new(ClaudeConfig::new(cwd));
    let cursor = AcpConfig::cursor(cwd);
    let cline = AcpConfig::cline(cwd);

    let (codex, claude, cursor, cline) =
        tokio::join!(codex.probe(), claude.probe(), cursor.probe(), cline.probe());

    vec![
        report(Harness::Codex, codex),
        report(Harness::Claude, claude),
        report(Harness::Cursor, cursor),
        report(Harness::Cline, cline),
    ]
}

/// Probe, then write the cache. A write failure does not fail the probe: a read-only data
/// directory must not hide the harnesses that are installed.
pub async fn refresh(dir: &Path, cwd: &Path) -> Vec<HarnessInfo> {
    let harnesses = probe_all(cwd).await;
    let cache = AdapterCache {
        probed_at: rigel_core::migrate::now_ms(),
        harnesses: harnesses.clone(),
    };
    let _ = write(dir, &cache).await;
    harnesses
}

pub async fn write(dir: &Path, cache: &AdapterCache) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let text = serde_json::to_string_pretty(cache)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    tokio::fs::write(path(dir), text + "\n").await
}

/// Turn a probe result into a row. `NotInstalled` is the normal missing case and keeps the install
/// hint; any other failure says so instead of pretending the harness is absent.
fn report(harness: Harness, result: Result<HarnessInfo, AdapterError>) -> HarnessInfo {
    match result {
        Ok(info) => info,
        Err(error) => HarnessInfo {
            harness,
            installed: false,
            version: None,
            path: None,
            auth_mode: AuthMode::None,
            capabilities: registry::capabilities(harness, None),
            provider: credential_spec(harness, None).map(|spec| spec.provider),
            install_hint: Some(match error {
                AdapterError::NotInstalled { .. } => registry::install_hint(harness),
                other => format!("probe failed: {other}"),
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_cache_is_written_beside_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let cache = AdapterCache {
            probed_at: 1,
            harnesses: vec![HarnessInfo {
                harness: Harness::Cursor,
                installed: false,
                version: None,
                path: None,
                auth_mode: AuthMode::None,
                capabilities: registry::capabilities(Harness::Cursor, None),
                provider: Some("cursor".to_string()),
                install_hint: Some(registry::install_hint(Harness::Cursor)),
            }],
        };
        write(dir.path(), &cache).await.unwrap();

        let text = std::fs::read_to_string(path(dir.path())).unwrap();
        assert!(text.contains("\"harness\": \"cursor\""), "{text}");
        assert!(text.contains("install"), "{text}");
        assert!(text.contains("\"probed_at\": 1"), "{text}");
    }

    #[test]
    fn a_missing_harness_becomes_a_row_with_a_hint() {
        let info = report(
            Harness::Codex,
            Err(AdapterError::NotInstalled {
                harness: Harness::Codex,
            }),
        );
        assert!(!info.installed);
        assert_eq!(info.auth_mode, AuthMode::None);
        assert_eq!(
            info.install_hint.as_deref(),
            Some(registry::install_hint(Harness::Codex).as_str())
        );
        assert_eq!(info.provider.as_deref(), Some("openai"));
    }

    #[test]
    fn a_failed_probe_is_not_reported_as_merely_missing() {
        let info = report(
            Harness::Claude,
            Err(AdapterError::Other("version check timed out".into())),
        );
        assert!(
            info.install_hint
                .as_deref()
                .is_some_and(|hint| hint.contains("probe failed")),
            "{info:?}"
        );
    }
}

//! Which harness CLIs exist on this machine, and how each one authenticates (P4.1).
//!
//! The probe answers four questions per harness — installed, version, absolute path, auth mode —
//! and caches them in `adapters.json`. The cache path is an argument, not a policy: the app and
//! the CLI decide where their state directory is (`rigel_core::default_index_dir()`), and this
//! crate never invents one.
//!
//! A missing binary is not an error. `HarnessInfo { installed: false, .. }` is what lets Settings
//! show an install hint instead of an empty row.
//!
//! Auth classification follows §4.4 and is decided from **presence only**: a keychain (or
//! environment) key means `ApiKey`; a vendor login file means `Subscription`; neither means
//! `None`. No credential file is ever opened. Cline's key is `rigel/cline` for the same reason
//! Cursor's is `rigel/cursor`: its ACP child has its own key path.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rigel_core::credentials::{CredentialStore, present_in};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::claude::ClaudeConfig;
use crate::codex::{CodexConfig, auth_mode};
use crate::{AuthMode, Capabilities, Harness, HarnessInfo, credential_spec};

/// The cache file name; the caller supplies the directory.
pub const CACHE_FILE: &str = "adapters.json";

const CACHE_VERSION: u32 = 1;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// The five external harnesses, in the order Settings lists them.
pub const HARNESSES: [Harness; 5] = [
    Harness::Codex,
    Harness::Claude,
    Harness::Cursor,
    Harness::Cline,
    Harness::Gemini,
];

pub fn display_name(harness: Harness) -> &'static str {
    match harness {
        Harness::Native => "Rigel",
        Harness::Codex => "Codex",
        Harness::Claude => "Claude Code",
        Harness::Cursor => "Cursor",
        Harness::Cline => "Cline",
        Harness::Gemini => "Gemini CLI",
    }
}

/// Cursor's documented binary is `agent`; `cursor-agent` is the older name (§4.3).
pub fn binary_names(harness: Harness) -> &'static [&'static str] {
    match harness {
        Harness::Codex => &["codex"],
        Harness::Claude => &["claude"],
        Harness::Cursor => &["agent", "cursor-agent"],
        Harness::Cline => &["cline"],
        Harness::Gemini => &["gemini"],
        Harness::Native => &[],
    }
}

/// A copy-paste install command, shown only while the binary is missing.
pub fn install_hint(harness: Harness) -> String {
    match harness {
        Harness::Codex => "npm install -g @openai/codex".to_string(),
        Harness::Claude => "npm install -g @anthropic-ai/claude-code".to_string(),
        Harness::Cursor if cfg!(windows) => {
            // PowerShell syntax wrapped so it also runs from cmd or a Git Bash shell.
            "powershell -NoProfile -Command \"irm 'https://cursor.com/install?win32=true' | iex\""
                .to_string()
        }
        Harness::Cursor => "curl https://cursor.com/install -fsS | bash".to_string(),
        Harness::Cline => "npm install -g cline".to_string(),
        Harness::Gemini => "npm install -g @google/gemini-cli".to_string(),
        Harness::Native => String::new(),
    }
}

/// The vendor login artefact that makes a harness subscription-authenticated. Checked for
/// existence only. Cline has none: its ACP child takes a `CLINE_API_KEY`, not a login file.
pub fn vendor_login(harness: Harness) -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    match harness {
        Harness::Codex => Some(home.join(".codex").join("auth.json")),
        Harness::Claude => Some(home.join(".claude").join(".credentials.json")),
        // Cursor keeps the CLI's login in its own config directory. A machine without it simply
        // reports `None`; the probe never guesses a subscription from the app's presence.
        Harness::Cursor => Some(home.join(".cursor").join("cli-config.json")),
        // Gemini CLI's Google-account login. Current builds migrate this file into a hybrid token
        // store and remove it, so a signed-in CLI can still report `None` here; it is only used to
        // classify an auth mode, and the key path is what Rigel actually spawns with (D21).
        Harness::Gemini => Some(home.join(".gemini").join("oauth_creds.json")),
        Harness::Cline | Harness::Native => None,
    }
}

/// Auth classification, kept pure so tests never touch a real keychain or login file.
pub fn auth_from_presence(key_present: bool, login_present: bool) -> AuthMode {
    if key_present {
        AuthMode::ApiKey
    } else if login_present {
        AuthMode::Subscription
    } else {
        AuthMode::None
    }
}

/// Capability flags from the §4.2 matrix. Claude's subagent visibility is version-gated
/// (v2.1.219+); an unknown version reports `false` rather than promising what it cannot see.
pub fn capabilities(harness: Harness, version: Option<&str>) -> Capabilities {
    match harness {
        Harness::Codex => Capabilities {
            acp: false,
            json_stream: true,
            programmatic_approval: true,
            worktree: true,
            nested_agents: false,
        },
        Harness::Claude => Capabilities {
            acp: false,
            json_stream: true,
            programmatic_approval: true,
            // Claude's worktrees are host-managed, not a CLI flag.
            worktree: false,
            nested_agents: version.is_some_and(|version| at_least(version, [2, 1, 219])),
        },
        Harness::Cursor => Capabilities {
            acp: true,
            json_stream: true,
            programmatic_approval: true,
            worktree: true,
            // ACP child sessions need the client to advertise the capability; P4.3 does not yet.
            nested_agents: false,
        },
        Harness::Cline => Capabilities {
            acp: true,
            json_stream: true,
            programmatic_approval: true,
            worktree: true,
            nested_agents: false,
        },
        Harness::Gemini => Capabilities {
            acp: true,
            json_stream: true,
            programmatic_approval: true,
            // Gemini CLI documents no worktree flag of its own, so Rigel does not promise one.
            worktree: false,
            nested_agents: false,
        },
        Harness::Native => Capabilities::default(),
    }
}

/// The first `major.minor[.patch]` token in a `--version` banner. Banners vary wildly
/// (`codex-cli 0.162.1`, `2.1.251 (Claude Code)`), so nothing here assumes a prefix.
pub fn parse_version(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|token| {
            let trimmed = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.');
            trimmed
                .strip_prefix('v')
                .or_else(|| trimmed.strip_prefix('V'))
                .unwrap_or(trimmed)
        })
        .find(|token| {
            let core = token.split(['-', '+']).next().unwrap_or_default();
            let parts: Vec<&str> = core.split('.').collect();
            parts.len() >= 2
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        })
        .map(str::to_string)
}

/// Probe every harness. Codex and Claude report through their adapter's configured credentials;
/// the ACP harnesses (Cursor, Cline, Gemini) report version plus auth classification. Their turn adapter
/// is the shared `AcpClient`, which refuses to start without the matching key.
pub async fn probe_all(store: &dyn CredentialStore) -> Vec<HarnessInfo> {
    let mut infos = Vec::with_capacity(HARNESSES.len());
    for harness in HARNESSES {
        infos.push(probe(harness, store).await);
    }
    infos
}

pub async fn probe(harness: Harness, store: &dyn CredentialStore) -> HarnessInfo {
    if harness == Harness::Codex {
        let mut config = CodexConfig::router(".");
        config.inject_credential(store);
        return probe_codex(&config).await;
    }

    let path = locate(harness);
    let version = match &path {
        Some(path) => version_of(path).await,
        None => None,
    };
    let spec = credential_spec(harness, None);
    let key_present = spec.is_some_and(|spec| present_in(store, spec.provider));
    let login_present = vendor_login(harness).is_some_and(|path| path.is_file());

    let flags = capabilities(harness, version.as_deref());
    HarnessInfo {
        harness,
        installed: path.is_some(),
        version,
        path: path.clone(),
        auth_mode: auth_from_presence(key_present, login_present),
        capabilities: flags,
        provider: spec.map(|spec| spec.provider.to_string()),
        install_hint: path.is_none().then(|| install_hint(harness)),
    }
}

/// Codex's probe lives with its adapter because its auth depends on the configured endpoint
/// (router → `rigel/omniroute`, direct → `rigel/openai`).
pub async fn probe_codex(config: &CodexConfig) -> HarnessInfo {
    // Resolve the same way the other harnesses do, so an npm `codex.cmd` shim is probed (and
    // later spawned) instead of reported missing just because CreateProcess cannot run batch files.
    let exe = resolve_codex_exe(&config.exe);
    let mut command = command_for(&exe);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(PROBE_TIMEOUT, command.output()).await;

    let (installed, version) = match output {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            (
                true,
                parse_version(&stdout).or_else(|| parse_version(&stderr)),
            )
        }
        _ => (false, None),
    };

    let path = installed.then_some(exe);

    HarnessInfo {
        harness: Harness::Codex,
        installed,
        version: version.clone(),
        path,
        auth_mode: auth_mode(config),
        capabilities: capabilities(Harness::Codex, version.as_deref()),
        provider: credential_spec(Harness::Codex, Some(&config.base_url))
            .map(|spec| spec.provider.to_string()),
        install_hint: (!installed).then(|| install_hint(Harness::Codex)),
    }
}

/// Claude's probe lives with its adapter for the same reason as Codex's: auth is decided by the
/// adapter's own injected environment, not by a keychain lookup performed here.
pub async fn probe_claude(config: &ClaudeConfig) -> HarnessInfo {
    let exe = resolve_exe(Harness::Claude, &config.exe);
    let installed = exe.is_file();
    let version = if installed {
        version_of(&exe).await
    } else {
        None
    };

    HarnessInfo {
        harness: Harness::Claude,
        installed,
        version: version.clone(),
        path: installed.then_some(exe),
        auth_mode: crate::claude::auth_mode(config),
        capabilities: capabilities(Harness::Claude, version.as_deref()),
        provider: credential_spec(Harness::Claude, None).map(|spec| spec.provider.to_string()),
        install_hint: (!installed).then(|| install_hint(Harness::Claude)),
    }
}

/// A bare `codex`/`codex.exe` is looked up on `PATH` with the same extension rules as every other
/// harness; an explicit path is used as given.
pub(crate) fn resolve_codex_exe(exe: &Path) -> PathBuf {
    resolve_exe(Harness::Codex, exe)
}

/// The same lookup for any harness. An explicit path is used as given, so a test can point an
/// adapter at a fixture without touching `PATH`.
pub fn resolve_exe(harness: Harness, exe: &Path) -> PathBuf {
    if exe.components().count() > 1 {
        return exe.to_path_buf();
    }
    locate(harness).unwrap_or_else(|| exe.to_path_buf())
}

pub(crate) async fn version_of(path: &Path) -> Option<String> {
    let mut command = command_for(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(PROBE_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // A binary that answers but prints no number still proves it is installed; the caller treats
    // a `None` version as "installed, version unread".
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_version(&stdout).or_else(|| parse_version(&stderr))
}

/// Resolve a harness binary on `PATH`. Windows installs from npm are `.cmd` shims, which
/// `CreateProcess` cannot execute directly, so the extension list comes from `PATHEXT` and
/// [`command_for`] wraps batch files in `cmd /C`.
pub fn locate(harness: Harness) -> Option<PathBuf> {
    binary_names(harness)
        .iter()
        .find_map(|name| resolve_executable(name))
}

fn resolve_executable(name: &str) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.components().count() > 1 {
        return direct.is_file().then(|| direct.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| pick_in_dir(&dir, name))
}

/// Choose one command in one directory. On Windows the `PATHEXT` names are tried **before** the
/// bare name, because npm writes a POSIX shell shim with no extension beside the real `cline.cmd`
/// (and `codex.cmd`, and so on). `CreateProcess` cannot run the shim, so preferring it makes an
/// installed harness look installed and then fail to spawn.
fn pick_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    for extension in windows_extensions() {
        let candidate = dir.join(format!("{name}{extension}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let candidate = dir.join(name);
    candidate.is_file().then_some(candidate)
}

#[cfg(windows)]
fn windows_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .map(|extension| extension.trim().to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
        .collect()
}

pub(crate) fn command_for(path: &Path) -> Command {
    #[cfg(windows)]
    {
        let batch = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
            });
        if batch {
            let mut command = Command::new("cmd");
            command.arg("/C").arg(path);
            return command;
        }
    }
    Command::new(path)
}

fn at_least(version: &str, floor: [u64; 3]) -> bool {
    let mut parts = version
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0));
    let parsed = [
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    ];
    parsed >= floor
}

/* ------------------------------- the cache -------------------------------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedHarness {
    #[serde(flatten)]
    pub info: HarnessInfo,
    /// The binary's mtime when it was probed; `None` when the binary was missing.
    #[serde(default)]
    pub binary_mtime: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterCache {
    pub version: u32,
    pub probed_at: i64,
    pub harnesses: Vec<CachedHarness>,
}

impl AdapterCache {
    /// Probe every harness and stamp the binary mtimes that make the cache invalidatable.
    pub async fn probe(store: &dyn CredentialStore) -> Self {
        let mut harnesses = Vec::with_capacity(HARNESSES.len());
        for info in probe_all(store).await {
            let binary_mtime = info.path.as_deref().and_then(mtime);
            harnesses.push(CachedHarness { info, binary_mtime });
        }
        Self {
            version: CACHE_VERSION,
            probed_at: now(),
            harnesses,
        }
    }

    pub fn infos(&self) -> Vec<HarnessInfo> {
        self.harnesses
            .iter()
            .map(|cached| cached.info.clone())
            .collect()
    }

    /// Recompute only the auth classification, from the credential store and login-file presence.
    /// A key added in Settings must not wait for a binary to change before the card updates.
    /// Returns true when anything changed.
    pub fn refresh_auth(&mut self, store: &dyn CredentialStore) -> bool {
        let mut changed = false;
        for cached in &mut self.harnesses {
            let info = &mut cached.info;
            let login_present = vendor_login(info.harness).is_some_and(|path| path.is_file());
            let key_present = info
                .provider
                .as_deref()
                .is_some_and(|provider| present_in(store, provider));
            let mode = auth_from_presence(key_present, login_present);
            if mode != info.auth_mode {
                info.auth_mode = mode;
                changed = true;
            }
        }
        changed
    }

    /// True while no binary has been installed, removed or replaced since the probe.
    pub fn is_fresh(&self) -> bool {
        self.version == CACHE_VERSION
            && self.harnesses.iter().all(|cached| {
                cached.info.path.as_deref().and_then(mtime) == cached.binary_mtime
                    && locate(cached.info.harness) == cached.info.path
            })
    }

    pub fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let cache: Self = serde_json::from_slice(&bytes).ok()?;
        (cache.version == CACHE_VERSION).then_some(cache)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        std::fs::write(path, bytes)
    }
}

/// Load `adapters.json` when it is still accurate, otherwise re-probe and rewrite it.
pub async fn probe_cached(
    path: &Path,
    store: &dyn CredentialStore,
    force: bool,
) -> std::io::Result<AdapterCache> {
    if !force {
        if let Some(mut cache) = AdapterCache::load(path) {
            if cache.is_fresh() {
                if cache.refresh_auth(store) {
                    cache.save(path)?;
                }
                return Ok(cache);
            }
        }
    }
    let cache = AdapterCache::probe(store).await;
    cache.save(path)?;
    Ok(cache)
}

fn mtime(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(
        modified
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or_default(),
    )
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_banners_parse_to_a_number() {
        assert_eq!(
            parse_version("codex-cli 0.162.1"),
            Some("0.162.1".to_string())
        );
        assert_eq!(
            parse_version("2.1.251 (Claude Code)"),
            Some("2.1.251".to_string())
        );
        assert_eq!(
            parse_version("agent 3.24.12\n"),
            Some("3.24.12".to_string())
        );
        assert_eq!(parse_version("cline v3.0.73"), Some("3.0.73".to_string()));
        assert_eq!(
            parse_version("1.2.3-beta.4"),
            Some("1.2.3-beta.4".to_string())
        );
        assert_eq!(parse_version("no version here"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn claude_subagents_need_the_documented_version() {
        assert!(!capabilities(Harness::Claude, Some("2.1.218")).nested_agents);
        assert!(capabilities(Harness::Claude, Some("2.1.219")).nested_agents);
        assert!(capabilities(Harness::Claude, Some("2.2.0")).nested_agents);
        assert!(!capabilities(Harness::Claude, None).nested_agents);
    }

    #[cfg(windows)]
    #[test]
    fn an_npm_shim_loses_to_the_runnable_batch_file() {
        let dir = tempfile::tempdir().unwrap();
        // Exactly what npm leaves behind for `cline`: a sh script named `cline`, a `cline.cmd`
        // CreateProcess can run, and a `cline.ps1` that PATHEXT does not list here.
        std::fs::write(
            dir.path().join("cline"),
            "#!/bin/sh
exit 0
",
        )
        .unwrap();
        std::fs::write(dir.path().join("cline.cmd"), "@echo off").unwrap();
        let picked = pick_in_dir(dir.path(), "cline").expect("one of them is chosen");
        assert_eq!(
            picked.file_name().and_then(|name| name.to_str()),
            Some("cline.cmd"),
            "the bare shim must not win over the runnable shim"
        );
    }

    #[test]
    fn auth_classification_prefers_a_key_and_never_invents_one() {
        assert_eq!(auth_from_presence(true, true), AuthMode::ApiKey);
        assert_eq!(auth_from_presence(false, true), AuthMode::Subscription);
        assert_eq!(auth_from_presence(false, false), AuthMode::None);
    }

    #[test]
    fn cline_probes_as_its_own_provider_with_no_login_file() {
        let spec = credential_spec(Harness::Cline, None).expect("Cline has a key path");
        assert_eq!(spec.provider, rigel_core::credentials::CLINE);
        assert_eq!(spec.env_var, "CLINE_API_KEY");
        assert_eq!(vendor_login(Harness::Cline), None);
        assert!(capabilities(Harness::Cline, None).acp);
    }

    #[test]
    fn gemini_has_its_own_key_and_login_rule() {
        let spec = credential_spec(Harness::Gemini, None).expect("Gemini has a key path");
        assert_eq!(spec.provider, rigel_core::credentials::GEMINI);
        assert_eq!(spec.env_var, "GEMINI_API_KEY");
        let login = vendor_login(Harness::Gemini).expect("a login artefact path");
        assert_eq!(
            login.file_name().and_then(|name| name.to_str()),
            Some("oauth_creds.json")
        );
        let flags = capabilities(Harness::Gemini, None);
        assert!(flags.acp);
        assert!(flags.programmatic_approval);
        assert!(!flags.worktree, "no vendor worktree flag exists to promise");
    }

    #[tokio::test]
    async fn a_missing_binary_is_reported_not_errored() {
        let store = rigel_core::credentials::MemoryCredentialStore::default();
        let info = probe(Harness::Cline, &store).await;
        if info.installed {
            // Cline is installed on this machine; the invariant still holds.
            assert!(info.path.is_some());
            assert!(info.install_hint.is_none());
        } else {
            assert!(info.version.is_none());
            assert!(info.path.is_none());
            assert_eq!(info.auth_mode, AuthMode::None);
            assert!(info.install_hint.is_some());
        }
    }

    #[tokio::test]
    async fn the_cache_round_trips_and_goes_stale_when_a_binary_disappears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CACHE_FILE);
        let store = rigel_core::credentials::MemoryCredentialStore::default();

        let cache = probe_cached(&path, &store, true).await.unwrap();
        assert_eq!(cache.harnesses.len(), HARNESSES.len());
        assert!(path.is_file());

        let loaded = AdapterCache::load(&path).unwrap();
        assert_eq!(loaded.harnesses.len(), HARNESSES.len());
        assert_eq!(loaded.infos().len(), HARNESSES.len());

        // A cached entry that claims a path which no longer exists is not fresh.
        let mut poisoned = loaded;
        poisoned.harnesses[0].info.path = Some(dir.path().join("gone"));
        poisoned.harnesses[0].binary_mtime = Some(1);
        assert!(!poisoned.is_fresh());
    }

    #[test]
    fn a_new_key_updates_the_cached_auth_mode_without_reprobing() {
        let info = HarnessInfo {
            harness: Harness::Native,
            installed: true,
            version: None,
            path: None,
            auth_mode: AuthMode::None,
            capabilities: Capabilities::default(),
            provider: Some("test-provider".to_string()),
            install_hint: None,
        };
        let mut cache = AdapterCache {
            version: CACHE_VERSION,
            probed_at: 0,
            harnesses: vec![CachedHarness {
                info,
                binary_mtime: None,
            }],
        };
        let store = rigel_core::credentials::MemoryCredentialStore::default();
        assert!(!cache.refresh_auth(&store));

        store.insert("test-provider", "k");
        assert!(cache.refresh_auth(&store));
        assert_eq!(cache.harnesses[0].info.auth_mode, AuthMode::ApiKey);
        assert!(!cache.refresh_auth(&store));
    }

    #[test]
    fn an_explicit_binary_path_wins_over_the_path_scan() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir
            .path()
            .join(if cfg!(windows) { "agent.exe" } else { "agent" });
        std::fs::write(&exe, b"stub").unwrap();
        assert_eq!(resolve_codex_exe(&exe), exe);
    }

    #[test]
    fn install_hints_are_real_commands() {
        for harness in HARNESSES {
            let hint = install_hint(harness);
            assert!(!hint.is_empty(), "{harness:?} has no install hint");
            assert!(
                hint.contains("install") || hint.contains("cursor.com"),
                "{harness:?} hint is not an install command: {hint}"
            );
        }
    }
}

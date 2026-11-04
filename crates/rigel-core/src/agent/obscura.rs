//! Rigel b is the browser every Rigel agent uses.
//!
//! It is Rigel's fork of Obscura, shipped beside the app as `rigel-b`. External harnesses reach
//! it through its stdio MCP server (`rigel-b mcp`); the native agent
//! gets the same engine through the `browser_open` tool. The command is resolved once per call
//! so a packaged app can find the sidecar even when its own process PATH predates it, and so a
//! relocated binary can be named with `RIGEL_B_BIN`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

pub const SERVER_NAME: &str = "obscura";
pub const COMMAND: &str = "rigel-b";
pub const DOCS_URL: &str = "https://www.obscura.sh";
pub const INSTALL_HINT: &str = "Rigel b ships with Rigel; rebuild the app if `rigel-b` is missing.";

/// The binary path if it can be found. `RIGEL_B_BIN` wins, then the sidecar beside this
/// executable, then a development build in the source tree, then PATH and the legacy Obscura
/// install (so an older checkout still works).
pub fn locate() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("RIGEL_B_BIN") {
        let configured = PathBuf::from(configured);
        if configured.is_file() {
            return Some(configured);
        }
        if configured.components().count() == 1
            && let Some(found) = search_path(&configured.to_string_lossy())
        {
            return Some(found);
        }
    }
    sidecar().or_else(dev_build).or_else(|| search_path(COMMAND)).or_else(legacy_install)
}

fn binary_name() -> &'static str {
    if cfg!(windows) { "rigel-b.exe" } else { "rigel-b" }
}

fn sidecar() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join(binary_name());
    candidate.is_file().then_some(candidate)
}

fn dev_build() -> Option<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent()?.parent()?;
    ["release", "debug"]
        .iter()
        .map(|profile| {
            root.join("vendor")
                .join("rigel-b")
                .join("target")
                .join(profile)
                .join(binary_name())
        })
        .find(|candidate| candidate.is_file())
}

fn search_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        {
            let names = if program.to_ascii_lowercase().ends_with(".exe") {
                vec![program.to_string()]
            } else {
                vec![format!("{program}.exe"), program.to_string()]
            };
            for name in names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        #[cfg(not(windows))]
        {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn legacy_install() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let candidate = PathBuf::from(local)
        .join("Programs")
        .join("Obscura")
        .join("obscura.exe");
    candidate.is_file().then_some(candidate)
}

#[cfg(not(windows))]
fn legacy_install() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    [".local/bin/obscura", ".cargo/bin/obscura"]
        .iter()
        .map(|relative| PathBuf::from(&home).join(relative))
        .find(|candidate| candidate.is_file())
}

/// What goes in an MCP config. The absolute path is used when it is known, because a harness
/// started by a long-running Rigel process may have inherited a PATH from before the install.
pub fn command() -> String {
    locate()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| COMMAND.to_string())
}

/// Where one session's browser captures live. The session id is part of a path, so it is
/// reduced to characters a path segment can always carry; the timestamp keeps two captures in
/// the same millisecond apart, and the pid separates two Rigel processes sharing an index.
pub fn capture_path(index_dir: &Path, session: &str) -> PathBuf {
    let safe = safe_segment(session);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    index_dir
        .join("browser")
        .join(safe)
        .join(format!("{stamp}-{}.png", std::process::id()))
}

pub fn mcp_server() -> Value {
    json!({ "command": command(), "args": ["mcp"] })
}

/// The exact-capture variant Claude's config gets. The fork writes this file itself after
/// every successful page action, so the inspector never has to render the URL separately.
pub fn mcp_server_for(session: &str, agent: &str, index_dir: &Path) -> Value {
    let capture = harness_capture_path(index_dir, session, agent);
    if let Some(parent) = capture.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let space = format!("{}--{}", safe_segment(session), safe_segment(agent));
    json!({
        "command": command(),
        "args": ["mcp", "--capture", capture, "--space", space],
    })
}

/// Where one harness process writes its exact page frame. The inspector polls this path for
/// the agent that made the browser call.
pub fn harness_capture_path(index_dir: &Path, session: &str, agent: &str) -> PathBuf {
    index_dir
        .join("browser")
        .join("external")
        .join(safe_segment(session))
        .join(format!("{}.png", safe_segment(agent)))
}

fn safe_segment(value: &str) -> String {
    let safe: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() { "session".to_string() } else { safe }
}

/// ACP's `session/new` uses a named stdio server with an `env` array.
pub fn acp_server() -> Value {
    json!({
        "name": SERVER_NAME,
        "command": command(),
        "args": ["mcp"],
        "env": []
    })
}

/// Codex reads MCP servers from its TOML config; `-c` pairs are the per-turn override.
pub fn codex_overrides() -> Vec<String> {
    let command = serde_json::to_string(&command()).unwrap_or_else(|_| format!("\"{COMMAND}\""));
    vec![
        format!("mcp_servers.{SERVER_NAME}.command={command}"),
        format!("mcp_servers.{SERVER_NAME}.args=[\"mcp\"]"),
    ]
}

/// Add Rigel b to a project's `.mcp.json`, which agentchattr merges into Claude, Codex, Gemini
/// and Kimi. An existing `obscura` entry belongs to the user and is left alone; every other
/// server is preserved. Returns whether the file changed.
pub fn ensure_project_mcp(root: &Path) -> Result<bool, String> {
    if !root.is_dir() {
        return Err(format!("{} is not a project directory", root.display()));
    }
    let path = root.join(".mcp.json");
    let mut config = if path.exists() {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&text)
                .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))?
        }
    } else {
        json!({})
    };

    let object = config
        .as_object_mut()
        .ok_or_else(|| format!("{} must contain a JSON object", path.display()))?;
    let servers = object
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{} has a non-object `mcpServers`", path.display()))?;
    if servers.contains_key(SERVER_NAME) {
        return Ok(false);
    }
    servers.insert(SERVER_NAME.to_string(), mcp_server());

    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&config)
            .map_err(|error| format!("could not serialize .mcp.json: {error}"))?
    );
    std::fs::write(&path, text)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mcp_entry_is_a_stdio_server() {
        let server = mcp_server();
        assert_eq!(server["command"], command());
        assert_eq!(server["args"], json!(["mcp"]));
    }

    #[test]
    fn claude_gets_an_exact_capture_path_for_the_session_and_agent() {
        let dir = tempfile::tempdir().unwrap();
        let server = mcp_server_for("session/one", "agent one", dir.path());
        assert_eq!(server["args"][0], "mcp");
        assert_eq!(server["args"][1], "--capture");
        let capture = std::path::PathBuf::from(server["args"][2].as_str().unwrap());
        assert_eq!(server["args"][3], "--space");
        assert_eq!(server["args"][4], "session_one--agent_one");
        assert_eq!(
            capture,
            dir.path()
                .join("browser")
                .join("external")
                .join("session_one")
                .join("agent_one.png")
        );
        assert!(capture.parent().unwrap().is_dir());
    }

    #[test]
    fn acp_names_the_server_and_gives_it_an_env_array() {
        let server = acp_server();
        assert_eq!(server["name"], SERVER_NAME);
        assert_eq!(server["args"], json!(["mcp"]));
        assert_eq!(server["env"], json!([]));
    }

    #[test]
    fn codex_gets_both_command_and_args() {
        let overrides = codex_overrides();
        assert!(
            overrides
                .iter()
                .any(|pair| pair.starts_with("mcp_servers.obscura.command="))
        );
        assert!(
            overrides
                .iter()
                .any(|pair| pair == "mcp_servers.obscura.args=[\"mcp\"]")
        );
    }

    #[test]
    fn the_project_file_keeps_other_servers_and_adds_obscura_once() {
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join(".mcp.json");
        std::fs::write(&path, r#"{"mcpServers":{"keep":{"command":"keep"}}}"#).unwrap();

        assert!(ensure_project_mcp(project.path()).unwrap());
        let config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["keep"]["command"], "keep");
        assert_eq!(config["mcpServers"]["obscura"], mcp_server());
        assert!(!ensure_project_mcp(project.path()).unwrap());
    }
}

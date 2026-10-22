//! The one trust step Claude Code gates project MCP servers behind.
//!
//! `claude -p --mcp-config …` sees every rigel tool, but an interactive session only exposes the
//! servers the project has been told to trust. Two files carry that decision (QA, 22 Oct):
//! `enableAllProjectMcpServers` (plus the server names) in `.claude/settings.local.json`, and a
//! per-project `enabledMcpjsonServers` entry in `~/.claude.json`. Hand-editing both before every
//! fresh session is the friction this command removes.
//!
//! It is a command rather than a silent side effect on purpose: enabling a project's MCP servers
//! is exactly the kind of consent D15 says Rigel never assumes. Nothing here runs unless asked,
//! and `--dry-run` prints the two targets without touching either.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct TrustReport {
    pub settings: String,
    pub settings_changed: bool,
    pub global: Option<String>,
    pub global_changed: bool,
    pub servers: Vec<String>,
    pub dry_run: bool,
    pub notes: Vec<String>,
}

/// Read the project's `.mcp.json` and trust its servers for Claude Code.
pub fn trust_claude(project: &Path, dry_run: bool) -> Result<TrustReport, String> {
    let project = project
        .canonicalize()
        .map_err(|error| format!("cannot resolve {}: {error}", project.display()))?;
    let project = super::acp::plain_path(project);
    let servers = servers_in(&project)?;
    let mut notes = Vec::new();

    let settings_path = project.join(".claude").join("settings.local.json");
    let settings_before = read_object(&settings_path)?;
    let settings_after = merge_project_settings(settings_before.clone(), &servers);
    let settings_changed = settings_after != settings_before;
    if !dry_run && settings_changed {
        write_json(&settings_path, &settings_after)?;
    }

    let mut global = None;
    let mut global_changed = false;
    match home_dir() {
        Some(home) => {
            let path = home.join(".claude.json");
            if path.exists() {
                let before = read_object(&path)?;
                let after = merge_global(before.clone(), &project.display().to_string(), &servers)?;
                global_changed = after != before;
                if !dry_run && global_changed {
                    write_json(&path, &after)?;
                }
                global = Some(path.display().to_string());
            } else {
                notes.push(format!(
                    "{} does not exist yet — open Claude once, then re-run this to add the project entry",
                    path.display()
                ));
            }
        }
        None => notes.push(
            "no USERPROFILE/HOME, so ~/.claude.json was skipped; the project settings were still written"
                .to_string(),
        ),
    }

    notes.push("restart any running Claude session: trust is read at startup".to_string());
    Ok(TrustReport {
        settings: settings_path.display().to_string(),
        settings_changed,
        global,
        global_changed,
        servers,
        dry_run,
        notes,
    })
}

/// The server names a project declares. No file, no servers: say so rather than writing a trust
/// entry for a project that has nothing to trust.
fn servers_in(project: &Path) -> Result<Vec<String>, String> {
    let path = project.join(".mcp.json");
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "no {} — nothing to trust (Rigel's own MCP wiring is gitignored; regenerate it first)",
            path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))?;
    let mut servers: Vec<String> = value
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    servers.sort();
    if servers.is_empty() {
        return Err(format!(
            "{} lists no servers under `mcpServers`",
            path.display()
        ));
    }
    Ok(servers)
}

/// `enableAllProjectMcpServers` plus the names, merged over whatever the file already says. The
/// names are a union: a server the user trusted earlier is never dropped by a later run.
fn merge_project_settings(existing: Value, servers: &[String]) -> Value {
    let mut map = existing.as_object().cloned().unwrap_or_default();
    map.insert("enableAllProjectMcpServers".to_string(), Value::Bool(true));
    map.insert(
        "enabledMcpjsonServers".to_string(),
        json!(union(
            map.get("enabledMcpjsonServers")
                .and_then(Value::as_array)
                .map(|rows| rows
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect())
                .unwrap_or_default(),
            servers
        )),
    );
    Value::Object(map)
}

/// The per-project entry in `~/.claude.json`. A malformed file is refused, never rewritten: it may
/// hold every other project's settings and the user's account state.
fn merge_global(existing: Value, project_key: &str, servers: &[String]) -> Result<Value, String> {
    let mut root = existing
        .as_object()
        .cloned()
        .ok_or("~/.claude.json is not a JSON object; refusing to rewrite it")?;
    let projects = root
        .entry("projects")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("`projects` in ~/.claude.json is not an object; refusing to rewrite it")?;
    let entry = projects
        .entry(project_key.to_string())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("the project entry in ~/.claude.json is not an object; refusing to rewrite it")?;
    let before: Vec<String> = entry
        .get("enabledMcpjsonServers")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    entry.insert(
        "enabledMcpjsonServers".to_string(),
        json!(union(before, servers)),
    );
    Ok(Value::Object(root))
}

fn union(mut existing: Vec<String>, extra: &[String]) -> Vec<String> {
    for name in extra {
        if !existing.iter().any(|known| known == name) {
            existing.push(name.clone());
        }
    }
    existing.sort();
    existing
}

fn read_object(path: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid JSON: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(value)
        .map_err(|error| format!("cannot encode {}: {error}", path.display()))?;
    text.push('\n');
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::{merge_global, merge_project_settings, servers_in};
    use serde_json::json;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn project_settings_keep_what_is_there_and_add_the_servers() {
        let before = json!({
            "theme": "dark",
            "enabledMcpjsonServers": ["earlier"],
        });
        let after = merge_project_settings(before, &names(&["rigel-claude", "rigel-codex"]));
        assert_eq!(after["theme"], json!("dark"));
        assert_eq!(after["enableAllProjectMcpServers"], json!(true));
        assert_eq!(
            after["enabledMcpjsonServers"],
            json!(["earlier", "rigel-claude", "rigel-codex"])
        );
    }

    #[test]
    fn project_settings_are_idempotent() {
        let once = merge_project_settings(json!({}), &names(&["rigel-claude"]));
        let twice = merge_project_settings(once.clone(), &names(&["rigel-claude"]));
        assert_eq!(once, twice);
    }

    #[test]
    fn the_global_entry_leaves_other_projects_alone() {
        let before = json!({
            "numStartups": 12,
            "projects": { "C:/elsewhere": { "enabledMcpjsonServers": ["theirs"] } },
        });
        let after = merge_global(before, "C:\rigel", &names(&["rigel-claude"]))
            .expect("a well-formed file is merged");
        assert_eq!(after["numStartups"], json!(12));
        assert_eq!(
            after["projects"]["C:/elsewhere"]["enabledMcpjsonServers"],
            json!(["theirs"])
        );
        assert_eq!(
            after["projects"]["C:\rigel"]["enabledMcpjsonServers"],
            json!(["rigel-claude"])
        );
    }

    #[test]
    fn a_malformed_global_file_is_refused_not_rewritten() {
        let error = merge_global(json!({ "projects": "nope" }), "C:\rigel", &names(&["x"]))
            .expect_err("projects must be an object");
        assert!(error.contains("refusing to rewrite"), "{error}");
    }

    #[test]
    fn servers_come_from_the_project_mcp_config() {
        let root = std::env::temp_dir().join(format!("rigel-trust-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp project");
        std::fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"rigel-claude":{"command":"rigel"},"rigel-codex":{"command":"rigel"}}}"#,
        )
        .expect("write config");

        let found = servers_in(&root).expect("two servers");
        assert_eq!(
            found,
            vec!["rigel-claude".to_string(), "rigel-codex".to_string()]
        );

        std::fs::remove_file(root.join(".mcp.json")).expect("cleanup");
        let error = servers_in(&root).expect_err("no config, nothing to trust");
        assert!(error.contains("nothing to trust"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }
}

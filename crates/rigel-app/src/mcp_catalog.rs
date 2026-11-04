//! The MCP catalogue, embedded from the repository snapshot.
//!
//! The catalogue is build-time data, not a network service: the shelf has to answer offline,
//! and a first search should not wait for a registry. Only entries with a launch command this
//! app can name without guessing are marked installable. Installing writes one named server to
//! the open project's `.mcp.json`, the file Claude Code and Rigel's Claude path already read.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const CATALOG_JSON: &str = include_str!("../../../data/mcp-catalog.json");

#[derive(Debug, Deserialize)]
struct Catalog {
    #[serde(default)]
    servers: Vec<McpServer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub qualified: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub transport: String,
    #[serde(default)]
    pub install: String,
    #[serde(default)]
    pub version: String,
    /// Derived while loading: the catalogue has a launch recipe this app understands.
    #[serde(default)]
    pub installable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallReport {
    pub id: String,
    pub name: String,
    /// The key written under `mcpServers`.
    pub entry: String,
    pub path: PathBuf,
    pub created: bool,
}

static CATALOG: OnceLock<Result<Vec<McpServer>, String>> = OnceLock::new();

fn catalog() -> Result<&'static [McpServer], String> {
    match CATALOG.get_or_init(|| {
        let mut catalog: Catalog = serde_json::from_str(CATALOG_JSON)
            .map_err(|error| format!("could not parse the MCP catalogue: {error}"))?;
        for server in &mut catalog.servers {
            server.installable = launch_spec(server).is_some();
        }
        Ok(catalog.servers)
    }) {
        Ok(servers) => Ok(servers.as_slice()),
        Err(error) => Err(error.clone()),
    }
}

fn matches(server: &McpServer, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    [
        &server.name,
        &server.qualified,
        &server.description,
        &server.repo,
    ]
    .iter()
    .any(|value| value.to_lowercase().contains(query))
}

fn score(server: &McpServer, query: &str) -> u8 {
    let name = server.name.to_lowercase();
    if name == query {
        100
    } else if name.starts_with(query) {
        80
    } else if name.contains(query) {
        60
    } else if server.qualified.to_lowercase().contains(query)
        || server.repo.to_lowercase().contains(query)
    {
        40
    } else {
        20
    }
}

/// One page of the catalogue. The scan is over 39k rows and runs in memory; the catalogue is
/// parsed once, so repeated queries do not pay the JSON cost.
pub fn search(query: &str, limit: usize, offset: usize) -> Result<Vec<McpServer>, String> {
    let needle = query.trim().to_lowercase();
    let mut hits: Vec<&McpServer> = catalog()?
        .iter()
        .filter(|server| matches(server, &needle))
        .collect();

    hits.sort_by(|left, right| {
        let order = if needle.is_empty() {
            Ordering::Equal
        } else {
            score(right, &needle).cmp(&score(left, &needle))
        };
        order
            .then_with(|| right.installable.cmp(&left.installable))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(hits
        .into_iter()
        .skip(offset)
        .take(limit.min(500))
        .cloned()
        .collect())
}

pub fn count(query: &str) -> Result<i64, String> {
    let needle = query.trim().to_lowercase();
    Ok(catalog()?
        .iter()
        .filter(|server| matches(server, &needle))
        .count() as i64)
}

pub fn find(id: &str) -> Result<McpServer, String> {
    catalog()?
        .iter()
        .find(|server| server.id == id)
        .cloned()
        .ok_or_else(|| format!("no MCP server with id {id} in the catalogue"))
}

/// The JSON key shown to agents. Qualified names are the registry's stable identity, so the
/// last segment is both readable and less likely to collide than a display name.
fn entry_name(server: &McpServer) -> String {
    let raw = server
        .qualified
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(&server.name);
    let mut slug = String::new();
    let mut separator = false;
    for character in raw.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            separator = false;
        } else if !separator && !slug.is_empty() {
            slug.push('-');
            separator = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "mcp".to_string()
    } else {
        slug
    }
}

/// A launch recipe only when the catalogue gives enough information to start the server.
/// Remote endpoints with credential placeholders are deliberately not installable: writing a
/// URL that cannot authenticate would look successful and fail at the first tool call.
fn launch_spec(server: &McpServer) -> Option<Value> {
    let install = server.install.trim();
    if let Some(package) = install.strip_prefix("npm:") {
        let package = package.trim();
        if package.is_empty() {
            return None;
        }
        return Some(if cfg!(windows) {
            json!({ "command": "cmd", "args": ["/c", "npx", "-y", package] })
        } else {
            json!({ "command": "npx", "args": ["-y", package] })
        });
    }
    if let Some(package) = install.strip_prefix("pypi:") {
        let package = package.trim();
        if package.is_empty() {
            return None;
        }
        return Some(json!({ "command": "uvx", "args": [package] }));
    }
    if matches!(
        server.transport.as_str(),
        "streamable-http" | "sse" | "remote" | "http"
    ) {
        let url = server.homepage.trim();
        if !url.starts_with("http") || url.contains('{') || url.contains('<') {
            return None;
        }
        let kind = if server.transport == "sse" {
            "sse"
        } else {
            "http"
        };
        return Some(json!({ "type": kind, "url": url }));
    }
    None
}

/// Install one catalogue entry into `<project>/.mcp.json`, preserving every other server.
pub fn install(project: &str, id: &str) -> Result<InstallReport, String> {
    let root = Path::new(project);
    if project.trim().is_empty() || !root.is_dir() {
        return Err("Open a project before installing an MCP server.".to_string());
    }

    let server = find(id)?;
    let spec = launch_spec(&server).ok_or_else(|| {
        format!(
            "{} has no launch command this app can install without guessing.",
            server.name
        )
    })?;
    let entry = entry_name(&server);
    let path = root.join(".mcp.json");
    let mut config = if path.exists() {
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        serde_json::from_str::<Value>(&text)
            .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))?
    } else {
        json!({})
    };

    let object = config
        .as_object_mut()
        .ok_or_else(|| format!("{} must contain a JSON object", path.display()))?;
    if !object.contains_key("mcpServers") {
        object.insert("mcpServers".to_string(), json!({}));
    }
    let servers = object
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{} has a non-object `mcpServers`", path.display()))?;

    if let Some(existing) = servers.get(&entry) {
        if existing == &spec {
            return Ok(InstallReport {
                id: server.id,
                name: server.name,
                entry,
                path,
                created: false,
            });
        }
        return Err(format!(
            "{entry} is already in {} with different settings; remove it first",
            path.display()
        ));
    }

    servers.insert(entry.clone(), spec);
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&config)
            .map_err(|error| format!("could not serialize .mcp.json: {error}"))?
    );
    fs::write(&path, text)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;

    Ok(InstallReport {
        id: server.id,
        name: server.name,
        entry,
        path,
        created: true,
    })
}

/// Whether this exact server is already in the project's `.mcp.json`.
pub fn installed(project: &str, id: &str) -> Result<bool, String> {
    let root = Path::new(project);
    if project.trim().is_empty() || !root.is_dir() {
        return Ok(false);
    }
    let server = find(id)?;
    let Some(spec) = launch_spec(&server) else {
        return Ok(false);
    };
    let path = root.join(".mcp.json");
    if !path.exists() {
        return Ok(false);
    }
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let config: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))?;
    Ok(config
        .get("mcpServers")
        .and_then(Value::as_object)
        .and_then(|servers| servers.get(&entry_name(&server)))
        .is_some_and(|existing| existing == &spec))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(transport: &str, install: &str, homepage: &str) -> McpServer {
        McpServer {
            id: "test".to_string(),
            name: "Test Server".to_string(),
            qualified: "example/Test Server".to_string(),
            description: String::new(),
            repo: String::new(),
            homepage: homepage.to_string(),
            source: "registry".to_string(),
            sources: vec!["registry".to_string()],
            transport: transport.to_string(),
            install: install.to_string(),
            version: "1.0.0".to_string(),
            installable: false,
        }
    }

    #[test]
    fn package_and_remote_entries_get_a_launch_spec() {
        let npm = launch_spec(&server("package", "npm:chrome-devtools-mcp", "")).unwrap();
        let args = npm["args"].as_array().unwrap();
        assert_eq!(
            args.last().and_then(Value::as_str),
            Some("chrome-devtools-mcp")
        );

        let remote = launch_spec(&server("sse", "", "https://example.test/sse")).unwrap();
        assert_eq!(remote["type"], "sse");
        assert_eq!(remote["url"], "https://example.test/sse");
        assert!(launch_spec(&server("sse", "", "https://example.test/mcp?key={key}")).is_none());
    }

    #[test]
    fn install_merges_without_touching_other_servers() {
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join(".mcp.json");
        fs::write(&path, r#"{"mcpServers":{"keep":{"command":"keep"}}}"#).unwrap();

        let report = install(
            project.path().to_str().unwrap(),
            "registry:io.github.Wolfe-Jam/faf-mcp",
        )
        .unwrap();

        assert_eq!(report.entry, "faf-mcp");
        assert!(report.created);
        assert!(
            installed(
                project.path().to_str().unwrap(),
                "registry:io.github.Wolfe-Jam/faf-mcp"
            )
            .unwrap()
        );
        let config: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["keep"]["command"], "keep");
        assert!(config["mcpServers"]["faf-mcp"].is_object());
    }

    #[test]
    fn a_second_install_is_idempotent() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path().to_str().unwrap();
        let first = install(root, "registry:io.github.Wolfe-Jam/faf-mcp").unwrap();
        let second = install(root, "registry:io.github.Wolfe-Jam/faf-mcp").unwrap();
        assert!(first.created);
        assert!(!second.created);
    }
}

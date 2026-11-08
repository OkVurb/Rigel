//! Project MCP servers declared in `.mcp.json`, read only far enough to ask before first use.
//!
//! A project server is someone else's process. Rigel never treats the declaration as consent:
//! the MCP permission bus starts the server once to read its real `tools/list`, publishes that
//! list on the same card the native agent uses, and refuses the first tool call if nobody
//! answers. The server is then allowed per tool; the server grant is not a tool grant.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// One stdio server declared under `mcpServers`. Remote-only entries are reported as
/// unsupported rather than guessed at: Rigel cannot list their tools without making a network
/// request, and this gate is deliberately local.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSpec {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl ServerSpec {
    pub fn command_line(&self) -> String {
        std::iter::once(self.command.clone())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The servers a project declares. A missing or malformed file is an empty map for the caller;
/// the `rigel_approve` path reports that distinctly when a name looked like a project tool.
pub fn servers(root: &Path) -> Result<HashMap<String, ServerSpec>, String> {
    let path = root.join(".mcp.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Parse a `.mcp.json` body without touching the filesystem, so the fuzz target and the tests
/// exercise the same path the app uses.
pub fn parse(text: &str) -> Result<HashMap<String, ServerSpec>, String> {
    let config: Value =
        serde_json::from_str(text).map_err(|error| format!("is not valid JSON: {error}"))?;
    let Some(entries) = config.get("mcpServers").and_then(Value::as_object) else {
        return Ok(HashMap::new());
    };
    let mut servers = HashMap::new();
    for (name, entry) in entries {
        // A remote entry has a `url` and no command. It is kept out of the map so the caller
        // can say why it cannot be listed instead of starting a child with an empty command.
        let Some(command) = entry.get("command").and_then(Value::as_str) else {
            continue;
        };
        if command.trim().is_empty() {
            continue;
        }
        let args = entry
            .get("args")
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let env = entry
            .get("env")
            .and_then(Value::as_object)
            .map(|env| {
                env.iter()
                    .filter_map(|(key, value)| {
                        value.as_str().map(|value| (key.clone(), value.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        servers.insert(
            name.clone(),
            ServerSpec {
                name: name.clone(),
                command: command.to_string(),
                args,
                env,
            },
        );
    }
    Ok(servers)
}

/// Whether this permission name is `mcp__server__tool`, and the two parts.
pub fn tool_ref(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server.to_string(), tool.to_string()))
}

/// Ask the server for its real tool list over stdio. This is the one launch before approval;
/// it is bounded, it only completes the MCP handshake, and the child is killed when the
/// handshake ends. A server that does not answer is not silently trusted.
pub async fn list_tools(server: &ServerSpec, wait: Duration) -> Result<Vec<Value>, String> {
    let mut command = Command::new(&server.command);
    command
        .args(&server.args)
        .envs(server.env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", server.name))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("{} has no stdin", server.name))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{} has no stdout", server.name))?;
    let name = server.name.clone();
    let handshake = async move {
        send(
            &mut stdin,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "rigel", "version": env!("CARGO_PKG_VERSION") }
                }
            }),
        )
        .await?;

        let mut lines = BufReader::new(stdout).lines();
        let mut initialized = false;
        while let Some(line) = next_line(&mut lines, &name).await? {
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message.get("id") == Some(&json!(1)) {
                if message.get("error").is_some() {
                    return Err(format!("{name} refused initialize"));
                }
                initialized = true;
                break;
            }
        }
        if !initialized {
            return Err(format!("{name} closed before initialize"));
        }

        send(
            &mut stdin,
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )
        .await?;
        send(
            &mut stdin,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        )
        .await?;

        while let Some(line) = next_line(&mut lines, &name).await? {
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message.get("id") != Some(&json!(2)) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(format!("{name} refused tools/list: {error}"));
            }
            let tools = message
                .get("result")
                .and_then(|result| result.get("tools"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            return Ok(tools
                .into_iter()
                .filter(|tool| tool.get("name").and_then(Value::as_str).is_some())
                .take(64)
                .collect());
        }
        Err(format!("{name} closed before tools/list"))
    };

    let result = tokio::time::timeout(wait, handshake)
        .await
        .map_err(|_| format!("{} did not answer tools/list in time", server.name))?;
    let _ = child.start_kill();
    let _ = child.wait().await;
    result
}

/// Write one JSON-RPC line to the child. `writeln!` does not work on a tokio `ChildStdin`,
/// so the newline is appended to the serialized message before the async write.
async fn send(stdin: &mut tokio::process::ChildStdin, message: Value) -> Result<(), String> {
    let mut line = message.to_string();
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|error| format!("could not write to the server: {error}"))?;
    stdin
        .flush()
        .await
        .map_err(|error| format!("could not flush to the server: {error}"))
}

async fn next_line(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    name: &str,
) -> Result<Option<String>, String> {
    lines
        .next_line()
        .await
        .map_err(|error| format!("could not read from {name}: {error}"))
}

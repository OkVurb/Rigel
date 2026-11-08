//! A scripted stdio MCP server for the Q242 gate tests. It never touches the network.
//!
//! `initialize` and `tools/list` complete the handshake; `tools/call` echoes one argument so a
//! future proxy test can prove the same process can run a tool after approval.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn main() {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = message.get("id").cloned() else {
            // `notifications/initialized` and friends need no answer.
            continue;
        };
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let result = match method {
            "initialize" => json!({
                "protocolVersion": "2025-06-18",
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "fake-project", "version": "1" }
            }),
            "tools/list" => json!({
                "tools": [
                    {
                        "name": "echo",
                        "description": "Echo one line",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "text": { "type": "string" } }
                        }
                    },
                    {
                        "name": "dangerous_write",
                        "description": "Write a file in the project",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "path": { "type": "string" } }
                        }
                    }
                ]
            }),
            "tools/call" => {
                let text = message
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .and_then(|arguments| arguments.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("ok");
                json!({ "content": [{ "type": "text", "text": text }] })
            }
            _ => {
                println!(
                    "{}",
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "unknown" } })
                );
                let _ = std::io::stdout().flush();
                continue;
            }
        };
        println!(
            "{}",
            json!({ "jsonrpc": "2.0", "id": id, "result": result })
        );
        let _ = std::io::stdout().flush();
    }
}

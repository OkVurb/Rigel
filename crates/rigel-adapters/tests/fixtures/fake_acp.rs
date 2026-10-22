//! A scripted stand-in for an ACP agent (`agent acp` / `cline --acp`), used by `tests/acp.rs`.
//!
//! Modes, chosen by `FAKE_ACP_MODE`:
//! - unset / `ok`: initialize, create a session, stream two message chunks, ask permission for a
//!   tool call, wait for the decision, then finish with `end_turn`;
//! - `reject-only`: offer only `reject_once`, which the client must answer with `cancelled`;
//! - `wrong-version`: answer with protocol version 99, which the client must refuse;
//! - `wait-cancel`: stream one chunk, then wait for `session/cancel` and answer `cancelled`.
//!
//! `FAKE_ACP_RECORD` names a file that receives what the client sent: its argv, whether the
//! injected key was visible, and the permission outcome it received.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{Value, json};

fn record(path: &Option<PathBuf>, line: &str) {
    if let Some(path) = path {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open record file");
        writeln!(file, "{line}").expect("write record");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version") {
        println!("mock-agent 1.4.2");
        return;
    }
    let mode = std::env::var("FAKE_ACP_MODE").unwrap_or_default();
    let record_path = std::env::var_os("FAKE_ACP_RECORD").map(PathBuf::from);
    record(&record_path, &format!("argv={}", args.join(" ")));
    // Each ACP harness has its own key variable; record both so a test can prove which one the
    // client injected instead of only that *a* key arrived.
    record(
        &record_path,
        &format!("cursor-key={}", presence("CURSOR_API_KEY")),
    );
    record(
        &record_path,
        &format!("cline-key={}", presence("CLINE_API_KEY")),
    );

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    while let Some(Ok(line)) = lines.next() {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        match message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "initialize" => {
                let version = if mode == "wrong-version" { 99 } else { 1 };
                respond(
                    &id,
                    json!({
                        "protocolVersion": version,
                        "agentCapabilities": {},
                        "agentInfo": { "name": "fake-acp", "version": "1.4.2" }
                    }),
                );
            }
            "session/new" => {
                let cwd = message["params"]["cwd"]
                    .as_str()
                    .unwrap_or("<missing>")
                    .to_string();
                record(&record_path, &format!("cwd={cwd}"));
                let absolute = std::path::Path::new(&cwd).is_absolute();
                respond(
                    &id,
                    json!({ "sessionId": if absolute { "sess-1" } else { "relative-cwd" } }),
                );
            }
            "session/prompt" => {
                let prompt_id = id.clone();
                if mode == "wait-cancel" {
                    update(
                        "agent_message_chunk",
                        json!({ "content": { "type": "text", "text": "working" } }),
                    );
                    loop {
                        match lines.next() {
                            Some(Ok(line)) => {
                                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                                    continue;
                                };
                                if message.get("method").and_then(Value::as_str)
                                    == Some("session/cancel")
                                {
                                    record(&record_path, "cancel=received");
                                    respond(&prompt_id, json!({ "stopReason": "cancelled" }));
                                    return;
                                }
                            }
                            _ => return,
                        }
                    }
                }
                run_turn(&prompt_id, &mode, &record_path, &mut lines);
            }
            "session/cancel" => record(&record_path, "cancel=received"),
            _ => {}
        }
    }
}

fn run_turn(
    prompt_id: &Value,
    mode: &str,
    record_path: &Option<PathBuf>,
    lines: &mut std::io::Lines<std::io::StdinLock<'_>>,
) {
    update(
        "agent_message_chunk",
        json!({ "content": { "type": "text", "text": "po" } }),
    );
    update(
        "agent_message_chunk",
        json!({ "content": { "type": "text", "text": "ng" } }),
    );

    let options = if mode == "reject-only" {
        json!([
            { "optionId": "deny", "name": "Reject", "kind": "reject_once" }
        ])
    } else {
        json!([
            { "optionId": "allow", "name": "Allow once", "kind": "allow_once" },
            { "optionId": "deny", "name": "Reject", "kind": "reject_once" }
        ])
    };
    let permission_id = 900;
    emit(json!({
        "jsonrpc": "2.0",
        "id": permission_id,
        "method": "session/request_permission",
        "params": {
            "sessionId": "sess-1",
            "toolCall": {
                "toolCallId": "call-1",
                "title": "Run the test suite",
                "kind": "execute",
                "rawInput": { "command": "cargo test" }
            },
            "options": options
        }
    }));

    let outcome = loop {
        match lines.next() {
            Some(Ok(line)) => {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if message.get("id").and_then(Value::as_u64) == Some(permission_id) {
                    break message["result"]["outcome"].clone();
                }
            }
            _ => return,
        }
    };
    record(record_path, &format!("outcome={outcome}"));

    update(
        "tool_call_update",
        json!({
            "toolCallId": "call-1",
            "status": "completed",
            "content": [{ "type": "content", "content": { "type": "text", "text": "done" } }]
        }),
    );
    update(
        "usage_update",
        json!({ "used": 5300, "size": 200000, "cost": { "amount": 0.0045, "currency": "USD" } }),
    );
    // A valid update from a newer agent: it must reach the caller as `Unmapped`, never be
    // dropped on the floor (P4.8).
    update("future_update_from_a_newer_agent", json!({}));
    respond(prompt_id, json!({ "stopReason": "end_turn" }));
}

fn presence(name: &str) -> &'static str {
    if std::env::var(name).is_ok() {
        "set"
    } else {
        "unset"
    }
}

fn respond(id: &Value, result: Value) {
    emit(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
}

fn update(kind: &str, fields: Value) {
    let mut update = json!({ "sessionUpdate": kind, "sessionId": "sess-1" });
    if let (Some(target), Some(source)) = (update.as_object_mut(), fields.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    emit(json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": { "sessionId": "sess-1", "update": update }
    }));
}

fn emit(value: Value) {
    println!("{value}");
    let _ = std::io::stdout().flush();
}

//! A scripted stand-in for an ACP agent (`agent acp` / `cline --acp`), used by `tests/acp.rs`.
//!
//! Modes, chosen by `FAKE_ACP_MODE`:
//! - unset / `ok`: initialize, create a session, stream two message chunks, ask permission for a
//!   tool call, wait for the decision, then finish with `end_turn`;
//! - `reject-only`: offer only `reject_once`, which the client must answer with `cancelled`;
//! - `wrong-version`: answer with protocol version 99, which the client must refuse;
//! - `wait-cancel`: stream one chunk, then wait for `session/cancel` and answer `cancelled`;
//! - `permission-hang`: ask permission and never resolve it, so the client's cancel must answer
//!   the pending request with `cancelled`.
//!
//! `FAKE_ACP_RECORD` names a file that receives what the client sent: its argv, whether the
//! injected key was visible, and the permission outcome it received.
//!
//! `FAKE_ACP_FIXTURE` replays a golden fixture as the agent's side of a turn: `$prompt` is
//! replaced by the `session/prompt` request id, `$permission` by a fixed id this binary then waits
//! for, and a fixture without its own prompt response is closed with `end_turn`.

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
    record(
        &record_path,
        &format!(
            "key={}",
            if std::env::var("CURSOR_API_KEY").is_ok() {
                "set"
            } else {
                "unset"
            }
        ),
    );
    record(
        &record_path,
        &format!(
            "cline_key={}",
            if std::env::var("CLINE_API_KEY").is_ok() {
                "set"
            } else {
                "unset"
            }
        ),
    );

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    while let Some(Ok(line)) = lines.next() {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = message.get("id").cloned();
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
                let prompt_id = id.clone().expect("session/prompt needs an id");
                if let Some(path) = std::env::var_os("FAKE_ACP_FIXTURE") {
                    replay_turn(&prompt_id, &path, &record_path, &mut lines);
                    return;
                }
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
                                    respond_value(&prompt_id, json!({ "stopReason": "cancelled" }));
                                    return;
                                }
                            }
                            _ => return,
                        }
                    }
                }
                if mode == "permission-hang" {
                    update(
                        "agent_message_chunk",
                        json!({ "content": { "type": "text", "text": "waiting for consent" } }),
                    );
                    emit(json!({
                        "jsonrpc": "2.0",
                        "id": 901,
                        "method": "session/request_permission",
                        "params": {
                            "sessionId": "sess-1",
                            "toolCall": { "toolCallId": "call-2", "title": "Delete build", "kind": "delete" },
                            "options": [
                                { "optionId": "allow", "name": "Allow", "kind": "allow_once" },
                                { "optionId": "deny", "name": "Deny", "kind": "reject_once" }
                            ]
                        }
                    }));
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
                                }
                                if message.get("id").and_then(Value::as_u64) == Some(901) {
                                    record(
                                        &record_path,
                                        &format!("outcome={}", message["result"]["outcome"]),
                                    );
                                    respond_value(&prompt_id, json!({ "stopReason": "cancelled" }));
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

/// The recorded stream, replayed into the client. A fixture line that is a request (an id and a
/// method) is answered by the client, and that answer is recorded so a test can assert the
/// permission outcome without trusting this binary to report it.
fn replay_turn(
    prompt_id: &Value,
    path: &std::ffi::OsStr,
    record_path: &Option<PathBuf>,
    lines: &mut std::io::Lines<std::io::StdinLock<'_>>,
) {
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("fake-acp: cannot read fixture {}", path.to_string_lossy());
        std::process::exit(2);
    };
    let prompt = prompt_id.to_string();
    let mut answered = false;
    for raw in text.lines().filter(|line| !line.trim().is_empty()) {
        let line = raw
            .replace("\"$prompt\"", &prompt)
            .replace("\"$permission\"", "901");
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").cloned();
        match (id.clone(), method) {
            (Some(id), Some(_)) => {
                emit(message);
                loop {
                    match lines.next() {
                        Some(Ok(line)) => {
                            let Ok(reply) = serde_json::from_str::<Value>(&line) else {
                                continue;
                            };
                            if reply.get("id") == Some(&id) {
                                record(
                                    record_path,
                                    &format!("outcome={}", reply["result"]["outcome"]),
                                );
                                break;
                            }
                        }
                        _ => return,
                    }
                }
            }
            (Some(_), None) => {
                emit(message);
                answered = true;
            }
            _ => emit(message),
        }
    }
    if !answered {
        respond_value(prompt_id, json!({ "stopReason": "end_turn" }));
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
    // The Phase 4 exit test asks each harness to run the same diagnostic command; the default
    // stays the test suite so the other fixtures read like a real turn.
    let (title, command) = if mode == "doctor" {
        ("Run rigel doctor", "rigel doctor --json")
    } else {
        ("Run the test suite", "cargo test")
    };
    // A real agent announces the call before asking to run it, which is what gives the turn a
    // command item carrying the text the user approves.
    update(
        "tool_call",
        json!({
            "toolCallId": "call-1",
            "title": title,
            "kind": "execute",
            "status": "pending",
            "rawInput": { "command": command }
        }),
    );
    let permission_id = 900;
    emit(json!({
        "jsonrpc": "2.0",
        "id": permission_id,
        "method": "session/request_permission",
        "params": {
            "sessionId": "sess-1",
            "toolCall": {
                "toolCallId": "call-1",
                "title": title,
                "kind": "execute",
                "rawInput": { "command": command }
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
    update("future_update_from_a_newer_agent", json!({}));
    update(
        "usage_update",
        json!({ "used": 5300, "size": 200000, "cost": { "amount": 0.0045, "currency": "USD" } }),
    );
    respond_value(prompt_id, json!({ "stopReason": "end_turn" }));
}

fn respond(id: &Option<Value>, result: Value) {
    emit(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
}

fn respond_value(id: &Value, result: Value) {
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

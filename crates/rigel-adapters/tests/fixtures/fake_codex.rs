//! A scripted stand-in for `codex`, used by `tests/codex.rs`.
//!
//! It speaks just enough of both transports: `--version`, `exec --json` (JSONL) and
//! `app-server` (JSON-RPC over stdio). App-server mode exits 9 when a `FAIL_APP_SERVER`
//! file exists in the working directory, which is how the fallback test forces the
//! pre-turn failure path without touching the real Codex installation.
//!
//! `FAKE_CODEX_FIXTURE` replays a golden fixture as the agent's side of the conversation: in
//! `app-server` mode the fixture holds the notifications a turn emits, and the JSON-RPC replies
//! to `initialize` / `thread/start` / `turn/start` are still answered here. In `exec` mode the
//! fixture is the whole JSONL stream.

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version") {
        println!("codex-cli 9.9.9");
        return;
    }

    match args.first().map(String::as_str) {
        Some("app-server") => app_server(),
        Some("exec") => exec(),
        other => {
            eprintln!("fake-codex: unsupported mode {other:?}");
            std::process::exit(2);
        }
    }
}

fn exec() {
    if let Some(path) = std::env::var_os("FAKE_CODEX_FIXTURE") {
        replay(&path);
        return;
    }
    println!(
        "{}",
        json!({
            "type": "item.completed",
            "item": {
                "type": "error",
                "message": format!(
                    "OPENAI_API_KEY={}",
                    if std::env::var_os("OPENAI_API_KEY").is_some() { "set" } else { "unset" }
                )
            }
        })
    );
    println!(
        "{}",
        json!({ "type": "thread.started", "thread_id": "th-exec" })
    );
    println!(
        "{}",
        json!({
            "type": "item.completed",
            "item": { "type": "error", "message": "ignoring one unrecognized setting" }
        })
    );
    println!(
        "{}",
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "pong" }
        })
    );
    println!(
        "{}",
        json!({
            "type": "turn.completed",
            "usage": { "input_tokens": 10, "cached_input_tokens": 4, "output_tokens": 2 }
        })
    );
}

fn app_server() {
    std::fs::write("SPAWNED", b"").expect("record fake process start");
    if Path::new("FAIL_APP_SERVER").exists() {
        std::process::exit(9);
    }
    if Path::new("SUBSCRIPTION_EVENT").exists() {
        notify("account/updated", json!({ "authMode": "subscription" }));
        std::thread::sleep(Duration::from_secs(30));
        return;
    }

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    while let Some(Ok(line)) = lines.next() {
        let line = line;
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        match message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "initialize" => respond(&id, json!({ "userAgent": "fake-codex" })),
            "thread/start" => respond(&id, json!({ "threadId": "th-1", "model": "fake" })),
            "turn/start" => {
                respond(&id, json!({ "turnId": "tn-1" }));
                if let Some(path) = std::env::var_os("FAKE_CODEX_FIXTURE") {
                    replay(&path);
                    return;
                }
                // `APPROVAL_COMMAND` asks the fake to pause on an approval, like a Codex that
                // needs the user before it runs a shell command.
                if let Ok(command) = std::fs::read_to_string("APPROVAL_COMMAND") {
                    request(
                        41,
                        "item/commandExecution/requestApproval",
                        json!({
                            "threadId": "th-1",
                            "turnId": "tn-1",
                            "itemId": "item-9",
                            "reason": "needs to write outside the sandbox",
                            "command": ["bash", "-lc", command.trim()]
                        }),
                    );
                    let decision = answer(&mut lines);
                    notify(
                        "item/agentMessage/delta",
                        json!({ "delta": format!("decision={decision}") }),
                    );
                    notify(
                        "turn/completed",
                        json!({ "turn": { "status": "completed" } }),
                    );
                    return;
                }
                notify(
                    "item/completed",
                    json!({
                        "item": {
                            "type": "error",
                            "message": format!(
                                "OPENAI_API_KEY={}",
                                if std::env::var_os("OPENAI_API_KEY").is_some() { "set" } else { "unset" }
                            )
                        }
                    }),
                );
                notify("item/agentMessage/delta", json!({ "delta": "po" }));
                notify("item/agentMessage/delta", json!({ "delta": "ng" }));
                notify(
                    "item/completed",
                    json!({ "item": { "type": "agentMessage", "text": "pong" } }),
                );
                notify(
                    "turn/completed",
                    json!({
                        "turn": { "status": "completed" },
                        "usage": { "input_tokens": 10, "output_tokens": 2 }
                    }),
                );
                return;
            }
            _ => {}
        }
    }
}

/// A recorded stream is the whole script: print it as it was recorded, then return. Exiting
/// without a `turn/completed` is how an interrupted turn looks from the adapter's side.
fn replay(path: &std::ffi::OsStr) {
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("fake-codex: cannot read fixture {}", path.to_string_lossy());
        std::process::exit(2);
    };
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        println!("{line}");
    }
    let _ = std::io::stdout().flush();
}

/// Read the client's reply to request `41`, or report that none came.
fn answer(lines: &mut std::io::Lines<std::io::StdinLock<'_>>) -> String {
    let Some(Ok(line)) = lines.next() else {
        return "no-answer".to_string();
    };
    let Ok(message) = serde_json::from_str::<Value>(&line) else {
        return "unreadable".to_string();
    };
    if message.get("id").and_then(Value::as_u64) != Some(41) {
        return "wrong-id".to_string();
    }
    message["result"]["decision"]
        .as_str()
        .unwrap_or("no-decision")
        .to_string()
}

/// A request from the server: it carries both an `id` and a `method`, which is what makes it
/// different from a response.
fn request(id: u64, method: &str, params: Value) {
    println!(
        "{}",
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    );
    let _ = std::io::stdout().flush();
}

fn respond(id: &Value, result: Value) {
    println!(
        "{}",
        json!({ "jsonrpc": "2.0", "id": id, "result": result })
    );
}

fn notify(method: &str, params: Value) {
    println!(
        "{}",
        json!({ "jsonrpc": "2.0", "method": method, "params": params })
    );
    let _ = std::io::stdout().flush();
}

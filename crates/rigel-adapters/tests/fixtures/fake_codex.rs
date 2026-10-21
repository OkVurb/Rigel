//! A scripted stand-in for `codex`, used by `tests/codex.rs`.
//!
//! It speaks just enough of both transports: `--version`, `exec --json` (JSONL) and
//! `app-server` (JSON-RPC over stdio). App-server mode exits 9 when a `FAIL_APP_SERVER`
//! file exists in the working directory, which is how the fallback test forces the
//! pre-turn failure path without touching the real Codex installation.

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
    for line in stdin.lock().lines() {
        let Ok(line) = line else { return };
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

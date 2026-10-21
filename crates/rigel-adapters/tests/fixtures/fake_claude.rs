//! A scripted stand-in for `claude`, used by `tests/claude.rs`.
//!
//! It prints the same NDJSON shapes the real CLI printed on 21 Oct 2026 (2.1.251):
//! `system/init`, `stream_event` deltas, a final `assistant` message and `result`.
//!
//! Behaviour is chosen by `FAKE_CLAUDE_MODE`:
//! - unset / `ok`: a normal one-word answer;
//! - `subscription`: an init line that reports a non-API-key source, then silence (the adapter
//!   must kill it);
//! - `echo-key`: the answer contains the injected key, to prove redaction;
//! - `error`: a `result` with `is_error`, the way a blocked tool call reports;
//! - `silent`: print nothing and exit 3;
//! - `partial`: an answer, then exit 0 without the closing `result` line.
//!
//! `FAKE_CLAUDE_ARGS` names a file that receives the argv, so a test can assert what the adapter
//! passed without guessing.

use std::io::Write;

use serde_json::json;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version") {
        println!("2.1.251 (Claude Code)");
        return;
    }
    if let Some(path) = std::env::var_os("FAKE_CLAUDE_ARGS") {
        let _ = std::fs::write(path, args.join("\n"));
    }

    match std::env::var("FAKE_CLAUDE_MODE")
        .unwrap_or_default()
        .as_str()
    {
        "subscription" => subscription(),
        "echo-key" => echo_key(),
        "error" => error(),
        "silent" => std::process::exit(3),
        // A truncated stream: an answer, then exit 0 with no `result` line.
        "partial" => {
            init("ANTHROPIC_API_KEY");
            assistant("half an answer");
        }
        _ => ok(),
    }
}

fn init(api_key_source: &str) {
    emit(json!({
        "type": "system",
        "subtype": "init",
        "session_id": "cs-1",
        "model": "free-stack",
        "apiKeySource": api_key_source
    }));
}

fn emit(value: serde_json::Value) {
    println!("{value}");
    let _ = std::io::stdout().flush();
}

fn delta(text: &str) {
    emit(json!({
        "type": "stream_event",
        "event": {
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text }
        }
    }));
}

fn assistant(text: &str) {
    emit(json!({
        "type": "assistant",
        "message": { "content": [{ "type": "text", "text": text }] }
    }));
}

fn result(text: &str) {
    emit(json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "result": text,
        "session_id": "cs-1",
        "total_cost_usd": 0.00616,
        "usage": { "input_tokens": 1207, "cache_read_input_tokens": 0, "output_tokens": 5 }
    }));
}

fn ok() {
    init("ANTHROPIC_API_KEY");
    delta("po");
    delta("ng");
    assistant("pong");
    result("pong");
}

fn subscription() {
    init("none");
    std::thread::sleep(std::time::Duration::from_secs(30));
}

fn echo_key() {
    let key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| "no-key".to_string());
    init("ANTHROPIC_API_KEY");
    delta(&format!("the key is {key}"));
    assistant(&format!("the key is {key}"));
    result(&format!("the key is {key}"));
}

fn error() {
    init("ANTHROPIC_API_KEY");
    emit(json!({
        "type": "result",
        "subtype": "error_during_execution",
        "is_error": true,
        "result": "tool permission denied",
        "session_id": "cs-1"
    }));
}

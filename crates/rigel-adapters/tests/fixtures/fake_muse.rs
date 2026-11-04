//! A stand-in for `muse exec --json`, for the adapter's spawn-and-parse test.
//!
//! It reads the prompt file it was given and emits the published envelope: a configured model,
//! two answer deltas, one tool result, and the terminal text. The shapes come from Meta's
//! `muse-code/extending` page and two independent parsers of the same journal; Muse itself is not
//! installed on this machine, so this binary is the fixture, not a recording.

use serde_json::json;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut prompt_file = None;
    while let Some(arg) = args.next() {
        if arg == "--prompt-file" {
            prompt_file = args.next();
        }
    }
    let prompt = prompt_file
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    if prompt.trim() != "say pong" {
        eprintln!("fake-muse: unexpected prompt {prompt:?}");
        std::process::exit(2);
    }

    for line in [
        json!({
            "schema_version": 1,
            "payload_type": "run.model.configured",
            "payload_schema_version": 1,
            "payload": { "model_id": "muse-spark-1.3" }
        }),
        json!({
            "schema_version": 1,
            "payload_type": "run.output.delta",
            "payload_schema_version": 1,
            "payload": { "text": "po" }
        }),
        json!({
            "schema_version": 1,
            "payload_type": "tool.result",
            "payload_schema_version": 1,
            "payload": {
                "correlation_facts": { "tool_name": "bash", "outcome": "success" },
                "result": "{\"command\":\"cargo test\",\"exit_code\":0,\"output\":\"ok\"}"
            }
        }),
        json!({
            "schema_version": 1,
            "payload_type": "run.output.delta",
            "payload_schema_version": 1,
            "payload": { "text": "ng" }
        }),
        json!({
            "schema_version": 1,
            "payload_type": "run.terminal.completed",
            "payload_schema_version": 1,
            "payload": { "text": "pong", "terminal": "completed" }
        }),
    ] {
        println!("{line}");
    }
}

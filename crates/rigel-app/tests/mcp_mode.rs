//! The window binary's card channel, without the window.
//!
//! `rigel-app mcp` is what Claude calls when it wants to act. The approval it publishes is the
//! same file-backed question the window's card reads and answers, so this test plays the card:
//! it starts the real binary, asks through `rigel_approve`, answers on the bus and checks the
//! harness is told yes. Without this, "the window sends a turn to Claude" would only be a claim.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rigel_core::agent::approval::ApprovalBus;
use rigel_core::{ApprovalScope, SessionService};
use serde_json::{Value, json};

/// Killed on drop: a failed assertion must not leave a server waiting on a question nobody will
/// answer, which would hold this test's stdout open until the ten-minute timeout.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn line(reader: &mut BufReader<std::process::ChildStdout>) -> Value {
    let mut text = String::new();
    reader.read_line(&mut text).expect("a response line");
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("not JSON: {text:?} ({error})"))
}

#[test]
fn a_claude_tool_call_is_answered_by_the_card_channel() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let index = dir.path().join("index");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&index).unwrap();

    // The fixture's runtime is dropped before the child starts: sqlite takes a write lock for a
    // migration, and a pool still open in this process would leave the child waiting for it.
    let (session, agent) = {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let service = SessionService::open(&project).await.unwrap();
            let session = service.start(Some("claude"), None).await.unwrap();
            let agent = service
                .register_agent_with(&session.id, "claude", Some("claude"), Some("claude-sonnet"))
                .await
                .unwrap();
            (session, agent)
        })
    };

    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_rigel-app"))
            .args([
                "mcp",
                "--data-dir",
                &index.display().to_string(),
                "--project",
                &project.display().to_string(),
                "--session",
                &session.id,
                "--agent",
                &agent.id,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the window binary starts in mcp mode"),
    );

    let mut stdin = server.0.stdin.take().unwrap();
    let mut stdout = BufReader::new(server.0.stdout.take().unwrap());

    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})
    )
    .unwrap();
    stdin.flush().unwrap();
    let init = line(&mut stdout);
    assert_eq!(init["result"]["serverInfo"]["name"], "rigel");

    // The question the harness asks. It blocks until something answers, so the card is played
    // from this thread while the child waits.
    let request = json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"rigel_approve","arguments":{
            "tool_name":"Bash",
            "input":{"command":"cargo test"},
            "tool_use_id":"toolu_1"
        }}
    });
    writeln!(stdin, "{request}").unwrap();
    stdin.flush().unwrap();

    let bus = ApprovalBus::new(&index);
    let deadline = Instant::now() + Duration::from_secs(30);
    let pending = loop {
        if let Some(question) = bus.pending().into_iter().next() {
            break question;
        }
        assert!(Instant::now() < deadline, "nothing reached the card");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        pending.kind, "claude",
        "the card is labelled with the harness that asked"
    );
    assert_eq!(
        pending.title, "cargo test",
        "the human sees the command itself, not just the tool name"
    );

    bus.answer(&pending.id, Some(ApprovalScope::Session))
        .expect("the card answers");

    let decided = line(&mut stdout);
    let text = decided["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text in {decided}"));
    let decision: Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        decision["behavior"], "allow",
        "an answer of `session` is a grant: {decision}"
    );
    assert_eq!(decision["scope"], "session");
}

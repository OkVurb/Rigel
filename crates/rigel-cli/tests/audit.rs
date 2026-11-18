//! `rigel audit` as a program: the verify command catches a mutated ledger, and provenance
//! prints a named reason for every link the journal cannot supply.

use std::path::Path;
use std::process::Command;

use rigel_core::SessionService;
use rigel_core::audit;
use serde_json::{Value, json};
use tempfile::TempDir;

fn rigel(data: &Path, args: &[&str]) -> (i32, String) {
    let exe = env!("CARGO_BIN_EXE_rigel");
    let output = Command::new(exe)
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .expect("rigel runs");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[tokio::test]
async fn a_mutated_ledger_is_rejected_by_the_cli_with_the_first_broken_line() {
    let data = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let service = SessionService::open(project.path()).await.unwrap();
    let session = service.start(Some("audit"), None).await.unwrap();
    let agent = service
        .register_agent_with(&session.id, "native", Some("commander"), Some("test/model"))
        .await
        .unwrap();
    service
        .record_approval(
            &session.id,
            &agent.id,
            None,
            "command",
            &json!({ "command": "echo audited" }),
            "allowed",
            Some("once"),
            1_700_000_000_000,
        )
        .await
        .unwrap();

    let args = [
        "audit",
        "verify",
        "--project",
        project.path().to_str().unwrap(),
        "--json",
    ];
    let (code, stdout) = rigel(data.path(), &args);
    assert_eq!(code, 0, "{stdout}");
    let report: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report["valid"], true);
    assert_eq!(report["records"], 1);

    let path = audit::ledger_path(project.path());
    let mut lines: Vec<String> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    let line = lines.pop().unwrap();
    lines.push(line.replacen("\"decision\":\"allowed\"", "\"decision\":\"denied\"", 1));
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();

    let (code, stdout) = rigel(data.path(), &args);
    assert_eq!(code, 2, "{stdout}");
    let report: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report["valid"], false);
    assert_eq!(report["first_bad"]["line"], 1);
    assert!(
        report["first_bad"]["reason"]
            .as_str()
            .unwrap()
            .contains("hash does not match")
    );
}

#[tokio::test]
async fn provenance_prints_unknown_and_its_reason_for_an_unrecorded_chain() {
    let data = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let service = SessionService::open(project.path()).await.unwrap();
    let session = service.start(Some("provenance"), None).await.unwrap();
    let agent = service
        .register_agent_with(&session.id, "native", Some("commander"), None)
        .await
        .unwrap();
    let turn = service.append_turn(&agent.id, Some("old")).await.unwrap();
    let item = service
        .append_item(
            &turn.id,
            "command",
            "error",
            &json!({
                "tool": "run_command",
                "result": { "ok": false, "command": "echo old", "destructive": [] }
            }),
            &json!({}),
        )
        .await
        .unwrap();

    let (code, stdout) = rigel(
        data.path(),
        &[
            "audit",
            "provenance",
            "--project",
            project.path().to_str().unwrap(),
            "--item",
            &item.id,
            "--json",
        ],
    );
    assert_eq!(code, 0, "{stdout}");
    let chain: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(chain["step"]["status"], "unknown");
    assert!(chain["step"]["reason"].as_str().is_some());
    assert_eq!(chain["tool_call"]["status"], "unknown");
    assert_eq!(chain["approval"]["status"], "unknown");
    assert_eq!(chain["snapshot"]["status"], "unknown");
    assert_eq!(
        chain["snapshot"]["reason"],
        "no snapshot was taken: the command was not destructive"
    );
}

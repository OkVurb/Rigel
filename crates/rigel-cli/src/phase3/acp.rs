//! `rigel agent acp` — one Cursor, Cline or Gemini turn through the shared ACP client (P4.3).
//!
//! The ACP client already speaks the protocol and routes `session/request_permission`. This module
//! is the other half: it puts the turn in a session, gives the request somewhere to go, and records
//! what happened as transcript items so the same data reaches the window and `rigel agent list`.
//!
//! **Approvals.** The default surface is a prompt on stdin/stderr: the tool's title, kind and raw
//! input are printed, then a one-letter answer. Under `--json` the default becomes `deny`, because
//! a machine-readable run has no one to ask; pass `--approval allow` to run unattended with
//! pre-consent. Anything unrecognised, and end-of-input, is a rejection — never an upgrade.
//!
//! **What is recorded.** Deltas are not one item each (a turn can emit hundreds); they accumulate
//! into one `message` item, with reasoning in its own. Tool calls, plans, usage, and both sides of
//! every approval are separate items. An unmapped ACP update is kept as a `message` item with an
//! `unmapped` key rather than dropped (P4.8 builds on that).
//!
//! **What is not claimed.** ACP reports context `used`/`size`, not billed input/output tokens, so
//! `tokens_in`/`tokens_out` stay zero and the reported cost is what is charged. Inventing a split
//! would make the bill look more precise than it is.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use clap::ValueEnum;
use rigel_adapters::acp::{AcpConfig, AcpEvent, ApprovalDecision, ApprovalRequest, Approver};
use rigel_adapters::{AcpClient, AdapterError, Harness};
use rigel_core::{
    ApprovalScope, BudgetService, CostSource, ItemKind, Scopes, SessionService, Usage, credentials,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Which ACP harness this turn drives. Each has its own key variable — Cursor reads
/// `CURSOR_API_KEY`, Cline reads `CLINE_API_KEY`, Gemini reads `GEMINI_API_KEY` — and the spawn
/// refuses without one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HarnessChoice {
    Cursor,
    Cline,
    Gemini,
}

impl HarnessChoice {
    fn harness(self) -> Harness {
        match self {
            Self::Cursor => Harness::Cursor,
            Self::Cline => Harness::Cline,
            Self::Gemini => Harness::Gemini,
        }
    }

    fn config(self, cwd: &Path) -> AcpConfig {
        match self {
            Self::Cursor => AcpConfig::cursor(cwd),
            Self::Cline => AcpConfig::cline(cwd),
            Self::Gemini => AcpConfig::gemini(cwd),
        }
    }
}

/// How a permission request is answered when there is no rendered approval card yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ApprovalChoice {
    /// Prompt on stdin. Any answer other than an explicit allow is a rejection.
    Ask,
    /// Answer `allow_once` without asking.
    Allow,
    /// Answer `reject_once` without asking.
    Deny,
}

#[derive(Debug, Clone)]
pub struct AcpArgs {
    pub project: PathBuf,
    pub session: Option<String>,
    pub agent: String,
    pub harness: HarnessChoice,
    pub prompt: String,
    pub json: bool,
    pub timeout_secs: u64,
    pub approval: Option<ApprovalChoice>,
    /// A harness installed somewhere `PATH` does not see.
    pub exe: Option<PathBuf>,
}

/// Ask on a terminal, pre-answer when told to, and fail closed otherwise.
///
/// An `Always` answer is also kept in the scope store, so a second identical request is answered
/// without a prompt even when the agent only ever offers `allow_once` (P4.10d).
pub struct CliApprover {
    mode: ApprovalChoice,
    events: mpsc::UnboundedSender<AcpEvent>,
    scopes: Scopes,
}

impl CliApprover {
    fn new(mode: ApprovalChoice, events: mpsc::UnboundedSender<AcpEvent>, scopes: Scopes) -> Self {
        Self {
            mode,
            events,
            scopes,
        }
    }
}

/// The kind, the exact title and the raw input, so a standing grant covers one request shape and
/// not every request from the same tool.
fn approval_key(request: &ApprovalRequest) -> String {
    format!(
        "acp:{}:{}:{}",
        request.kind.as_deref().unwrap_or("tool"),
        request.title,
        request
            .raw_input
            .as_ref()
            .map(|input| input.to_string())
            .unwrap_or_default()
    )
}

fn decision_for(scope: ApprovalScope) -> ApprovalDecision {
    match scope {
        ApprovalScope::Once => ApprovalDecision::AllowOnce,
        // ACP's strongest answer is its own session grant; project and always are Rigel's.
        ApprovalScope::Session | ApprovalScope::Project | ApprovalScope::Always => {
            ApprovalDecision::AllowAlways
        }
    }
}

#[async_trait]
impl Approver for CliApprover {
    async fn approve(&self, request: &ApprovalRequest) -> ApprovalDecision {
        let _ = self.events.send(AcpEvent::PermissionRequested {
            request: request.clone(),
        });
        let key = approval_key(request);
        let decision = match self.mode {
            ApprovalChoice::Allow => ApprovalDecision::AllowOnce,
            ApprovalChoice::Deny => ApprovalDecision::RejectOnce,
            // A standing answer is replayed without asking again.
            ApprovalChoice::Ask => match self.scopes.granted(&key) {
                Some(scope) => decision_for(scope),
                None => {
                    let decision = ask(request).await;
                    if decision == ApprovalDecision::AllowAlways {
                        // A store that cannot write is not permission to widen: this answer
                        // still holds for this request only.
                        let _ = self.scopes.remember(&key, ApprovalScope::Session);
                    }
                    decision
                }
            },
        };
        let _ = self.events.send(AcpEvent::PermissionResolved {
            tool_call_id: request.tool_call_id.clone(),
            decision,
        });
        decision
    }
}

/// Print what the agent wants and read one answer from stdin. Runs on the blocking pool: the rest
/// of the process is async and a stdin read must not hold the runtime.
async fn ask(request: &ApprovalRequest) -> ApprovalDecision {
    let request = request.clone();
    let prompt = format_request(&request);
    eprintln!("{prompt}");
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => ApprovalDecision::RejectOnce,
            Ok(_) => parse_decision(&line),
        }
    })
    .await
    .unwrap_or(ApprovalDecision::RejectOnce)
}

/// The question, exactly as the CLI prints it. Split out, and pure, so a test can assert that the
/// raw input is shown before consent (N2).
fn format_request(request: &ApprovalRequest) -> String {
    let kind = request.kind.as_deref().unwrap_or("tool");
    let input = request
        .raw_input
        .as_ref()
        .map(|value| serde_json::to_string(value).unwrap_or_default())
        .unwrap_or_default();
    format!(
        "\nThe harness wants to run a {kind} tool: {}\n  input: {input}\n  [a]llow once  [A]lways allow  [r]eject  (anything else rejects):",
        request.title
    )
}

/// One line to one decision. Anything not explicitly allowing rejects.
fn parse_decision(line: &str) -> ApprovalDecision {
    match line.trim() {
        "a" | "y" | "yes" => ApprovalDecision::AllowOnce,
        "A" | "always" => ApprovalDecision::AllowAlways,
        _ => ApprovalDecision::RejectOnce,
    }
}

pub async fn run(args: AcpArgs) -> ExitCode {
    let cwd = match std::fs::canonicalize(&args.project) {
        Ok(cwd) => plain_path(cwd),
        Err(error) => {
            eprintln!("rigel: {}: {error}", args.project.display());
            return ExitCode::from(1);
        }
    };
    let service = match SessionService::open(&cwd).await {
        Ok(service) => service,
        Err(error) => return crate::fail(error),
    };
    let session = match resolve_session(&service, args.session.as_deref()).await {
        Ok(session) => session,
        Err(error) => {
            eprintln!("rigel: {error}");
            return ExitCode::from(1);
        }
    };

    let harness = args.harness.harness();
    let role = Some(args.agent.as_str());
    let agent = match service.agents(&session.id).await.ok().and_then(|agents| {
        agents.into_iter().find(|candidate| {
            candidate.harness == harness.key() && candidate.role.as_deref() == role
        })
    }) {
        Some(agent) => agent,
        None => match service
            .register_agent_with(&session.id, harness.key(), role, None)
            .await
        {
            Ok(agent) => agent,
            Err(error) => return crate::fail(error),
        },
    };

    let mut config = args.harness.config(&cwd);
    config.timeout = Duration::from_secs(args.timeout_secs.max(1));
    if let Some(exe) = &args.exe {
        config.exe = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.clone());
    }
    // Resolve before spawning so a missing harness is reported as such — with the install hint —
    // rather than as an opaque "could not start" from the process layer.
    config.exe = rigel_adapters::registry::resolve_exe(harness, &config.exe);
    if !config.exe.is_file() {
        return harness_error(harness, AdapterError::NotInstalled { harness });
    }
    // P4.2's path: environment first, then the OS keychain, and no other source.
    if !config.inject_credential(&credentials::KeychainCredentialStore) {
        report_missing_key(harness, args.json);
        return ExitCode::from(3);
    }

    let (events, outgoing) = mpsc::unbounded_channel();
    let approval = args.approval.unwrap_or(if args.json {
        ApprovalChoice::Deny
    } else {
        ApprovalChoice::Ask
    });
    // The approver and the client's own callback write to one channel, so the recorder sees the
    // round trip in order: request, decision, then the turn's next update.
    let approver = Arc::new(CliApprover::new(
        approval,
        events.clone(),
        Scopes::new(&cwd, rigel_core::default_index_dir()),
    ));
    let mut client = match AcpClient::spawn(config, approver).await {
        Ok(client) => client,
        Err(error) => return harness_error(harness, error),
    };
    if let Err(error) = client.new_session().await {
        client.kill().await;
        return harness_error(harness, error);
    }

    let turn = match service.append_turn(&agent.id, Some(&args.prompt)).await {
        Ok(turn) => turn,
        Err(error) => {
            client.kill().await;
            return crate::fail(error);
        }
    };

    let recorder = {
        let service = service.clone();
        let turn_id = turn.id.clone();
        let json = args.json;
        tokio::spawn(async move {
            let mut state = State::default();
            let mut incoming = outgoing;
            while let Some(event) = incoming.recv().await {
                state.apply(&service, &turn_id, event, json).await;
            }
            state
        })
    };

    let outcome = {
        let sink = events.clone();
        client
            .prompt(&args.prompt, move |event| {
                let _ = sink.send(event);
            })
            .await
    };

    client.kill().await;
    drop(client);
    drop(events);

    // The recorder closes once the client's reader and the approver are gone. A dead child closes
    // stdout, and the timeout only guards a harness that somehow stays alive after `kill`.
    let state = match tokio::time::timeout(Duration::from_secs(5), recorder).await {
        Ok(Ok(state)) => state,
        _ => State::default(),
    };

    let usage = usage_of(state.usage);
    let budget = BudgetService::with_null_interrupt(service.pool().clone());
    let _ = budget.account(&agent.id, &turn.id, &usage).await;

    match outcome {
        Ok(turn_result) => {
            if !state.text.trim().is_empty() {
                record(
                    &service,
                    &turn.id,
                    "message",
                    "completed",
                    json!({ "text": state.text }),
                )
                .await;
            }
            if !state.reasoning.trim().is_empty() {
                record(
                    &service,
                    &turn.id,
                    "reasoning",
                    "completed",
                    json!({ "text": state.reasoning }),
                )
                .await;
            }
            if let Err(error) = service
                .finish_turn_with_reason(&turn.id, "completed", Some(&turn_result.stop_reason))
                .await
            {
                return crate::fail(error);
            }

            if args.json {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "session": session.id,
                        "agent": agent.id,
                        "turn": turn.id,
                        "harness": harness.key(),
                        "status": "completed",
                        "stop": turn_result.stop_reason,
                        "text": turn_result.text,
                        "used": state.usage.map(|usage| usage.used),
                        "size": state.usage.map(|usage| usage.size),
                        "cost_micros": state.usage.and_then(|usage| usage.cost_micros),
                    })
                );
            } else {
                println!(
                    "\n{} · {} · ${:.4}{}",
                    turn_result.stop_reason,
                    state
                        .usage
                        .map(|usage| format!("{}/{} tok context", usage.used, usage.size))
                        .unwrap_or_else(|| "no usage reported".to_string()),
                    usage.cost_micros as f64 / 1_000_000.0,
                    if state.usage.and_then(|usage| usage.cost_micros).is_none() {
                        " (the harness reported no cost)"
                    } else {
                        ""
                    }
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            let message = error.to_string();
            record(
                &service,
                &turn.id,
                "error",
                "failed",
                json!({ "message": message }),
            )
            .await;
            let _ = service
                .finish_turn_with_reason(&turn.id, "failed", Some("harness_error"))
                .await;
            if args.json {
                println!(
                    "{}",
                    json!({
                        "ok": false,
                        "session": session.id,
                        "agent": agent.id,
                        "turn": turn.id,
                        "harness": harness.key(),
                        "status": "failed",
                        "error": message,
                    })
                );
            }
            harness_error(harness, error)
        }
    }
}

#[derive(Default)]
struct State {
    text: String,
    reasoning: String,
    usage: Option<rigel_adapters::AcpUsage>,
    /// The item kind of each open tool call, so its completion updates the same kind instead of
    /// defaulting every update back to `command` (P4.8).
    kinds: HashMap<String, ItemKind>,
}

impl State {
    async fn apply(
        &mut self,
        service: &SessionService,
        turn_id: &str,
        event: AcpEvent,
        json: bool,
    ) {
        match event {
            AcpEvent::MessageDelta { text } => {
                self.text.push_str(&text);
                if !json {
                    print!("{text}");
                }
            }
            AcpEvent::ThoughtDelta { text } => self.reasoning.push_str(&text),
            AcpEvent::ToolCall {
                tool_call_id,
                title,
                kind,
                item_kind,
                operation,
                status,
                raw_input,
            } => {
                if !json {
                    println!("\n  [{}] {title}", kind.as_deref().unwrap_or("tool"));
                }
                // P4.8: the ACP `kind` picks the item kind, so an edit is a file change and a
                // search is a web search instead of everything being a command.
                self.kinds.insert(tool_call_id.clone(), item_kind);
                record(
                    service,
                    turn_id,
                    item_kind.as_str(),
                    status.as_deref().unwrap_or("running"),
                    json!({
                        "tool_call_id": tool_call_id,
                        "title": title,
                        "kind": kind,
                        "operation": operation,
                        "raw_input": raw_input,
                    }),
                )
                .await;
            }
            AcpEvent::ToolCallUpdate {
                tool_call_id,
                status,
                text,
            } => {
                if !json {
                    println!(
                        "  [{}] {}",
                        status.as_deref().unwrap_or("update"),
                        text.as_deref().unwrap_or_default()
                    );
                }
                let kind = self
                    .kinds
                    .get(&tool_call_id)
                    .copied()
                    .unwrap_or(ItemKind::Command);
                record(
                    service,
                    turn_id,
                    kind.as_str(),
                    status.as_deref().unwrap_or("completed"),
                    json!({ "tool_call_id": tool_call_id, "text": text }),
                )
                .await;
            }
            AcpEvent::Plan { entries } => {
                record(
                    service,
                    turn_id,
                    "plan",
                    "completed",
                    json!({ "entries": entries }),
                )
                .await;
            }
            AcpEvent::Usage {
                used,
                size,
                cost_micros,
            } => {
                self.usage = Some(rigel_adapters::AcpUsage {
                    used,
                    size,
                    cost_micros,
                });
                record(
                    service,
                    turn_id,
                    "usage",
                    "completed",
                    json!({ "used": used, "size": size, "cost_micros": cost_micros }),
                )
                .await;
            }
            AcpEvent::Notice { message } => {
                record(
                    service,
                    turn_id,
                    "message",
                    "completed",
                    json!({ "notice": message }),
                )
                .await;
            }
            AcpEvent::PermissionRequested { request } => {
                record(
                    service,
                    turn_id,
                    "approval",
                    "requested",
                    json!({
                        "tool_call_id": request.tool_call_id,
                        "title": request.title,
                        "kind": request.kind,
                        "raw_input": request.raw_input,
                        "options": request.options.iter().map(|option| json!({
                            "option_id": option.option_id,
                            "name": option.name,
                            "kind": option.kind,
                        })).collect::<Vec<_>>(),
                    }),
                )
                .await;
            }
            AcpEvent::PermissionResolved {
                tool_call_id,
                decision,
            } => {
                record(
                    service,
                    turn_id,
                    "approval",
                    "completed",
                    json!({ "tool_call_id": tool_call_id, "decision": decision_name(decision) }),
                )
                .await;
            }
            AcpEvent::Unmapped { kind } => {
                record(
                    service,
                    turn_id,
                    "message",
                    "completed",
                    json!({ "unmapped": kind }),
                )
                .await;
            }
        }
    }
}

fn decision_name(decision: ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::AllowOnce => "allow_once",
        ApprovalDecision::AllowAlways => "allow_always",
        ApprovalDecision::RejectOnce => "reject_once",
        ApprovalDecision::RejectAlways => "reject_always",
        ApprovalDecision::Cancel => "cancelled",
    }
}

async fn record(service: &SessionService, turn_id: &str, kind: &str, status: &str, body: Value) {
    let _ = service
        .append_item(turn_id, kind, status, &body, &body)
        .await;
}

fn report_missing_key(harness: Harness, json: bool) {
    let provider = match harness {
        Harness::Cursor => rigel_core::credentials::CURSOR,
        Harness::Cline => rigel_core::credentials::CLINE,
        Harness::Gemini => rigel_core::credentials::GEMINI,
        _ => "provider",
    };
    let variable = rigel_core::credentials::provider_environment_variable(provider)
        .unwrap_or("the harness's API key variable");
    let message = format!(
        "{} needs an API key before Rigel will spawn it (D21).\n  set one with: rigel keys set {provider} (reads stdin), or {variable} in the environment",
        rigel_adapters::registry::display_name(harness)
    );
    if json {
        println!("{}", json!({ "ok": false, "error": message }));
    }
    eprintln!("rigel: {message}");
}

fn harness_error(harness: Harness, error: AdapterError) -> ExitCode {
    match error {
        AdapterError::NotInstalled { .. } => {
            eprintln!(
                "rigel: {} is not installed.\n  {}",
                rigel_adapters::registry::display_name(harness),
                rigel_adapters::registry::install_hint(harness)
            );
            ExitCode::from(4)
        }
        AdapterError::RequiresApiKey { .. } => {
            report_missing_key(harness, false);
            ExitCode::from(3)
        }
        other => {
            eprintln!("rigel: {other}");
            ExitCode::from(4)
        }
    }
}

/// Windows canonicalisation returns the verbatim form (`\\?\C:\work`), which not every agent or
/// tool accepts as a working directory. The prefix makes a path unambiguous to Win32, but an
/// ordinary absolute path is unambiguous too, and it is what the harness's own CLI examples use.
pub(crate) fn plain_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy().to_string();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

async fn resolve_session(
    service: &SessionService,
    requested: Option<&str>,
) -> Result<rigel_core::Session, rigel_core::SessionError> {
    if let Some(id) = requested {
        return service.get(id).await;
    }
    let sessions = service.list().await?;
    if let Some(found) = sessions
        .into_iter()
        .find(|session| session.status != rigel_core::SessionStatus::Ended)
    {
        return Ok(found);
    }
    service.start(Some("acp"), None).await
}

/// Provider-less summary of what the turn cost. `cost_micros` is the harness's own report; tokens
/// stay out of `Usage` because ACP reports context size, not billed tokens.
fn usage_of(usage: Option<rigel_adapters::AcpUsage>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    Usage {
        cost_micros: usage.cost_micros.unwrap_or(0),
        provider_micros: usage.cost_micros,
        source: if usage.cost_micros.is_some() {
            CostSource::Provider
        } else {
            CostSource::Estimated
        },
        ..Usage::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rigel_adapters::acp::{AcpEvent, ApprovalOption};

    #[cfg(windows)]
    #[test]
    fn windows_verbatim_paths_become_ordinary_absolute_paths() {
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\C:\work")),
            PathBuf::from(r"C:\work")
        );
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\UNC\server\share\work")),
            PathBuf::from(r"\\server\share\work")
        );
        assert_eq!(
            plain_path(PathBuf::from(r"C:\work")),
            PathBuf::from(r"C:\work")
        );
    }

    #[test]
    fn an_answer_that_is_not_an_allow_rejects() {
        assert_eq!(parse_decision("a\n"), ApprovalDecision::AllowOnce);
        assert_eq!(parse_decision("yes"), ApprovalDecision::AllowOnce);
        assert_eq!(parse_decision("A"), ApprovalDecision::AllowAlways);
        assert_eq!(parse_decision("always"), ApprovalDecision::AllowAlways);
        assert_eq!(parse_decision("r"), ApprovalDecision::RejectOnce);
        assert_eq!(parse_decision(""), ApprovalDecision::RejectOnce);
        assert_eq!(parse_decision("yolo"), ApprovalDecision::RejectOnce);
    }

    #[test]
    fn the_prompt_shows_the_tool_and_its_input_before_asking() {
        let request = ApprovalRequest {
            tool_call_id: "call-1".into(),
            title: "Run the test suite".into(),
            kind: Some("execute".into()),
            raw_input: Some(json!({ "command": "cargo test" })),
            options: vec![ApprovalOption {
                option_id: "allow".into(),
                name: "Allow once".into(),
                kind: "allow_once".into(),
            }],
        };
        let text = format_request(&request);
        assert!(text.contains("Run the test suite"), "{text}");
        assert!(text.contains("execute"), "{text}");
        assert!(text.contains("cargo test"), "{text}");
        assert!(text.contains("[a]llow once"), "{text}");
        assert!(text.contains("[r]eject"), "{text}");
    }

    #[test]
    fn a_turn_with_no_reported_cost_is_not_billed_an_invented_one() {
        let usage = usage_of(None);
        assert_eq!(usage.cost_micros, 0);
        assert_eq!(usage.provider_micros, None);
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);

        let reported = usage_of(Some(rigel_adapters::AcpUsage {
            used: 5300,
            size: 200_000,
            cost_micros: Some(4500),
        }));
        assert_eq!(reported.cost_micros, 4500);
        assert_eq!(reported.source, CostSource::Provider);
        // ACP's `used` is context size, not billed input tokens.
        assert_eq!(reported.input_tokens, 0);
    }

    #[test]
    fn every_acp_event_has_a_decision_name_for_the_audit_row() {
        for decision in [
            ApprovalDecision::AllowOnce,
            ApprovalDecision::AllowAlways,
            ApprovalDecision::RejectOnce,
            ApprovalDecision::RejectAlways,
            ApprovalDecision::Cancel,
        ] {
            assert!(!decision_name(decision).is_empty());
        }
        // One sanity check on the shape the transcript stores.
        assert_eq!(decision_name(ApprovalDecision::AllowOnce), "allow_once");
    }

    #[tokio::test]
    async fn the_approver_emits_both_sides_of_a_permission_round_trip() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let dir = tempfile::tempdir().unwrap();
        let approver = CliApprover::new(
            ApprovalChoice::Allow,
            tx,
            Scopes::new(dir.path(), dir.path().join("index")),
        );
        let request = ApprovalRequest {
            tool_call_id: "call-2".into(),
            title: "Remove build".into(),
            kind: Some("delete".into()),
            raw_input: None,
            options: Vec::new(),
        };
        let decision = approver.approve(&request).await;
        assert_eq!(decision, ApprovalDecision::AllowOnce);
        assert!(matches!(
            rx.try_recv(),
            Ok(AcpEvent::PermissionRequested { .. })
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(AcpEvent::PermissionResolved {
                decision: ApprovalDecision::AllowOnce,
                ..
            })
        ));
    }
}

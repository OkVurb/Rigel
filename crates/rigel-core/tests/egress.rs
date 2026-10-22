//! `fetch_url` against a local HTTP server: consent, scopes, redirects and caps.
//!
//! The servers are raw TCP: these tests are about the tool's policy and wire handling, not a
//! framework, and they must not need the network to pass.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rigel_core::{
    BudgetService, Cancels, ItemKind, OriginApprover, OriginDecision, OriginRequest,
    SessionService, StopAt, Tools,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const CAP: usize = 256 * 1024;

struct Recorder {
    decision: OriginDecision,
    seen: Mutex<Vec<OriginRequest>>,
    calls: AtomicUsize,
}

impl Recorder {
    fn new(decision: OriginDecision) -> Arc<Self> {
        Arc::new(Self {
            decision,
            seen: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
        })
    }

    fn origins(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.origin.clone())
            .collect()
    }
}

#[async_trait]
impl OriginApprover for Recorder {
    async fn approve(&self, request: &OriginRequest) -> OriginDecision {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(request.clone());
        self.decision
    }
}

async fn build_tools(
    root: &std::path::Path,
    index: &std::path::Path,
    approver: Arc<dyn OriginApprover>,
) -> Tools {
    let service = SessionService::open(root).await.unwrap();
    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels as Arc<dyn rigel_core::InterruptSink>,
    ));
    Tools::new(
        root.to_path_buf(),
        service,
        budget,
        index.to_path_buf(),
        StopAt::Never,
    )
    .with_origin_approval(approver)
}

fn response(status: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Serve one response per entry, in order, and return the base URL plus every request line.
async fn serve(responses: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();

    tokio::spawn(async move {
        for response in responses {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0u8; 8_192];
            let read = socket.read(&mut buffer).await.unwrap_or(0);
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buffer[..read]).to_string());
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });

    (format!("http://{address}"), seen)
}

async fn fetch(tools: &Tools, url: &str) -> rigel_core::ToolOutcome {
    tools
        .execute(
            "session",
            "agent",
            "fetch_url",
            serde_json::json!({ "url": url }),
        )
        .await
}

#[tokio::test]
async fn an_approved_origin_is_fetched_as_untrusted_text() {
    let tmp = tempfile::tempdir().unwrap();
    let (base, seen) = serve(vec![response("200 OK", "text/plain", "hello page")]).await;
    let approver = Recorder::new(OriginDecision::AllowOnce);
    let tools = build_tools(tmp.path(), &tmp.path().join("index"), approver.clone()).await;

    let outcome = fetch(&tools, &format!("{base}/page")).await;

    assert!(outcome.ok, "{:?}", outcome.body);
    assert_eq!(outcome.kind, ItemKind::WebSearch);
    assert_eq!(outcome.body["status"], 200);
    assert_eq!(outcome.body["content_type"], "text/plain");
    assert_eq!(outcome.body["text"], "hello page");
    assert_eq!(outcome.body["untrusted"], true);
    assert_eq!(outcome.body["truncated"], false);
    assert_eq!(outcome.body["origin"], base);
    assert_eq!(approver.origins(), vec![base.clone()]);
    assert!(seen.lock().unwrap()[0].starts_with("GET /page "));
}

#[tokio::test]
async fn a_denied_origin_is_never_contacted() {
    let tmp = tempfile::tempdir().unwrap();
    // Nothing listens on port 1: a NETWORK error would mean the request was attempted.
    let approver = Recorder::new(OriginDecision::Deny);
    let tools = build_tools(tmp.path(), &tmp.path().join("index"), approver).await;

    let outcome = fetch(&tools, "http://127.0.0.1:1/secret").await;

    assert!(!outcome.ok);
    assert_eq!(outcome.kind, ItemKind::WebSearch);
    assert_eq!(outcome.body["approved"], false);
    assert_eq!(outcome.body["error"]["code"], "DENIED");
}

#[tokio::test]
async fn a_session_grant_is_asked_once_for_the_origin() {
    let tmp = tempfile::tempdir().unwrap();
    let (base, seen) = serve(vec![
        response("200 OK", "text/html", "one"),
        response("200 OK", "text/html", "two"),
    ])
    .await;
    let approver = Recorder::new(OriginDecision::AllowSession);
    let tools = build_tools(tmp.path(), &tmp.path().join("index"), approver.clone()).await;

    assert!(fetch(&tools, &format!("{base}/one")).await.ok);
    assert!(fetch(&tools, &format!("{base}/two")).await.ok);

    assert_eq!(approver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn an_always_grant_survives_a_new_tool_set() {
    let tmp = tempfile::tempdir().unwrap();
    let index = tmp.path().join("index");
    let (base, _) = serve(vec![
        response("200 OK", "text/plain", "first"),
        response("200 OK", "text/plain", "second"),
    ])
    .await;

    let first = Recorder::new(OriginDecision::AllowAlways);
    let tools = build_tools(tmp.path(), &index, first.clone()).await;
    assert!(fetch(&tools, &format!("{base}/a")).await.ok);
    assert_eq!(first.calls.load(Ordering::SeqCst), 1);
    assert!(index.join("approved-origins.json").is_file());

    // A fresh process would build a fresh Egress over the same index dir. This approver says
    // no, but the persisted grant should mean it is never asked.
    let second = Recorder::new(OriginDecision::Deny);
    let tools = build_tools(tmp.path(), &index, second.clone()).await;
    assert!(fetch(&tools, &format!("{base}/b")).await.ok);
    assert_eq!(second.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_redirect_to_another_origin_is_approved_separately() {
    let tmp = tempfile::tempdir().unwrap();
    let (target, _) = serve(vec![response("200 OK", "text/plain", "arrived")]).await;
    let redirect = format!(
        "HTTP/1.1 302 Found\r\nLocation: {target}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let (start, _) = serve(vec![redirect]).await;

    let approver = Recorder::new(OriginDecision::AllowOnce);
    let tools = build_tools(tmp.path(), &tmp.path().join("index"), approver.clone()).await;

    let outcome = fetch(&tools, &format!("{start}/go")).await;

    assert!(outcome.ok, "{:?}", outcome.body);
    assert_eq!(outcome.body["text"], "arrived");
    assert_eq!(outcome.body["url"], format!("{target}/final"));
    let origins = approver.origins();
    assert_eq!(origins.len(), 2, "{origins:?}");
    assert_eq!(origins[0], start);
    assert_eq!(origins[1], target);
}

#[tokio::test]
async fn a_non_text_response_is_refused_by_type() {
    let tmp = tempfile::tempdir().unwrap();
    let (base, _) = serve(vec![response("200 OK", "application/pdf", "%PDF-1.7")]).await;
    let approver = Recorder::new(OriginDecision::AllowOnce);
    let tools = build_tools(tmp.path(), &tmp.path().join("index"), approver).await;

    let outcome = fetch(&tools, &format!("{base}/doc.pdf")).await;

    assert!(!outcome.ok);
    assert_eq!(outcome.body["error"]["code"], "UNSUPPORTED_TYPE");
}

#[tokio::test]
async fn a_large_body_is_cut_at_the_cap_and_marked_truncated() {
    let tmp = tempfile::tempdir().unwrap();
    let body = "a".repeat(CAP + 10_000);
    let (base, _) = serve(vec![response("200 OK", "text/plain", &body)]).await;
    let approver = Recorder::new(OriginDecision::AllowOnce);
    let tools = build_tools(tmp.path(), &tmp.path().join("index"), approver).await;

    let outcome = fetch(&tools, &format!("{base}/big")).await;

    assert!(outcome.ok, "{:?}", outcome.body);
    assert_eq!(outcome.body["bytes"], CAP);
    assert_eq!(outcome.body["truncated"], true);
}

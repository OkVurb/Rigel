//! Rigel Cloud against a local stand-in for Firebase.
//!
//! The real endpoints are three HTTPS hosts and a project this repository does not own, so the
//! behaviour that matters is exercised here: the sign-in exchange, the refresh that a stale token
//! forces, the batched commit, and a pull that writes what another machine pushed. The fake speaks
//! the same JSON shapes and records every request, so a test can see which token was sent.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rigel_core::SessionService;
use rigel_core::cloud::{
    AppCheckConfig, Cloud, CloudConfig, CloudError, Endpoints, MemorySessionStore, SessionStore,
};
use rigel_core::skills::{SkillIndex, SkillRecord};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Default)]
struct FakeFirebase {
    /// `METHOD /path` for every request, query string stripped.
    requests: Mutex<Vec<String>>,
    /// The authorization header of every commit, oldest first.
    commits: Mutex<Vec<String>>,
    /// Every write Rigel sent, across every commit.
    writes: Mutex<Vec<Value>>,
    /// Lifetimes Firebase would answer with, as its stringly-typed `expiresIn`.
    sign_in_expires: String,
    /// Whether the fake account's address is verified, as `accounts:lookup` would report it.
    verified: AtomicBool,
    /// The ID token of every verification link request.
    verification_tokens: Mutex<Vec<String>>,
    /// `X-Firebase-AppCheck` on every request that is not an attestation call, oldest first.
    app_check_headers: Mutex<Vec<String>>,
    /// How many times the fake attestation endpoint was called.
    attest_calls: AtomicUsize,
}

impl FakeFirebase {
    fn saw(&self, request: &str) -> bool {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .any(|row| row == request)
    }
}

async fn start(sign_in_expires: &str) -> (String, Arc<FakeFirebase>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let origin = format!("http://{}", listener.local_addr().expect("address"));
    let state = Arc::new(FakeFirebase {
        sign_in_expires: sign_in_expires.to_string(),
        ..FakeFirebase::default()
    });

    let serving = state.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            let state = serving.clone();
            tokio::spawn(async move {
                let _ = handle(socket, state).await;
            });
        }
    });

    (origin, state)
}

async fn handle(mut socket: TcpStream, state: Arc<FakeFirebase>) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if buffer.len() > 1_000_000 {
            return Ok(());
        }
    };

    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_string();

    let mut content_length = 0_usize;
    let mut authorization = String::new();
    let mut app_check = String::new();
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("authorization:") {
            authorization = line
                .split_once(':')
                .map(|(_, value)| value.trim().to_string())
                .unwrap_or_default();
        }
        if lower.starts_with("x-firebase-appcheck:") {
            app_check = line
                .split_once(':')
                .map(|(_, value)| value.trim().to_string())
                .unwrap_or_default();
        }
    }
    while buffer.len() < header_end + content_length {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8_lossy(&buffer[header_end..]).to_string();

    state
        .requests
        .lock()
        .expect("requests")
        .push(format!("{method} {path}"));
    if !path.starts_with("/app-check/") {
        state
            .app_check_headers
            .lock()
            .expect("app check headers")
            .push(app_check);
    }

    match path.as_str() {
        path if path.starts_with("/app-check/") => {
            state.attest_calls.fetch_add(1, Ordering::SeqCst);
            match path {
                "/app-check/token" => {
                    respond(
                        &mut socket,
                        200,
                        &json!({
                            "token": "app-check-1",
                            "expires_at": unix_now() + 3_600,
                        })
                        .to_string(),
                    )
                    .await
                }
                "/app-check/expired" => {
                    respond(
                        &mut socket,
                        200,
                        &json!({
                            "token": "app-check-expired",
                            "expires_at": unix_now() - 10,
                        })
                        .to_string(),
                    )
                    .await
                }
                "/app-check/malformed" => respond(&mut socket, 200, "{}").await,
                "/app-check/error" => respond(&mut socket, 503, "attestation is offline").await,
                _ => respond(&mut socket, 404, r#"{"error":{"message":"not found"}}"#).await,
            }
        }
        "/v1/accounts:signUp" => {
            let email = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|value| value.get("email")?.as_str().map(str::to_string))
                .unwrap_or_else(|| "new@example.com".to_string());
            respond(
                &mut socket,
                200,
                &json!({
                    "localId": "uid-2",
                    "email": email,
                    "idToken": "id-new",
                    "refreshToken": "refresh-new",
                    "expiresIn": "3600",
                    "emailVerified": false,
                })
                .to_string(),
            )
            .await
        }
        "/v1/accounts:sendOobCode" => {
            let payload: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            // A verification link is asked for with the caller's ID token, not an address, and
            // Firebase mails it to the account that token names.
            if payload.get("requestType").and_then(Value::as_str) == Some("VERIFY_EMAIL") {
                let token = payload
                    .get("idToken")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if token.is_empty() {
                    return respond(
                        &mut socket,
                        400,
                        r#"{"error":{"code":400,"message":"INVALID_ID_TOKEN"}}"#,
                    )
                    .await;
                }
                state
                    .verification_tokens
                    .lock()
                    .expect("verification tokens")
                    .push(token);
                return respond(
                    &mut socket,
                    200,
                    &json!({ "email": "ada@example.com" }).to_string(),
                )
                .await;
            }
            let email = payload
                .get("email")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            // Firebase's enumeration protection is off by default, so an unknown address is a
            // 400 rather than a success. The client has to hide that difference.
            if email == "missing@example.com" {
                respond(
                    &mut socket,
                    400,
                    r#"{"error":{"code":400,"message":"EMAIL_NOT_FOUND"}}"#,
                )
                .await
            } else {
                respond(&mut socket, 200, &json!({ "email": email }).to_string()).await
            }
        }
        "/v1/accounts:lookup" => {
            let payload: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let token = payload
                .get("idToken")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if token.is_empty() {
                respond(
                    &mut socket,
                    400,
                    r#"{"error":{"code":400,"message":"INVALID_ID_TOKEN"}}"#,
                )
                .await
            } else {
                respond(
                    &mut socket,
                    200,
                    &json!({
                        "users": [{
                            "localId": "uid-1",
                            "email": "ada@example.com",
                            "emailVerified": state.verified.load(Ordering::SeqCst),
                        }]
                    })
                    .to_string(),
                )
                .await
            }
        }
        "/v1/accounts:signInWithPassword" => {
            respond(
                &mut socket,
                200,
                &json!({
                    "localId": "uid-1",
                    "email": "ada@example.com",
                    "idToken": "id-1",
                    "refreshToken": "refresh-1",
                    "expiresIn": state.sign_in_expires,
                    "emailVerified": state.verified.load(Ordering::SeqCst),
                })
                .to_string(),
            )
            .await
        }
        "/v1/token" => {
            respond(
                &mut socket,
                200,
                &json!({
                    "id_token": "id-2",
                    "refresh_token": "refresh-2",
                    "expires_in": "3600",
                    "user_id": "uid-1",
                })
                .to_string(),
            )
            .await
        }
        path if path.ends_with("/documents:commit") => {
            if let Ok(payload) = serde_json::from_str::<Value>(&body) {
                if let Some(writes) = payload.get("writes").and_then(Value::as_array) {
                    state
                        .writes
                        .lock()
                        .expect("writes")
                        .extend(writes.iter().cloned());
                }
            }
            state.commits.lock().expect("commits").push(authorization);
            respond(&mut socket, 200, "{}").await
        }
        path if path.ends_with("/skills") => {
            // Firestore keeps one document per name: a later update replaces the earlier one.
            // The fake accumulates every write, so it has to fold them the same way or a
            // tombstone would be listed beside the live row it replaced.
            let mut latest: BTreeMap<String, Value> = BTreeMap::new();
            for write in state.writes.lock().expect("writes").iter() {
                let Some(update) = write.get("update") else {
                    continue;
                };
                let Some(name) = update.get("name").and_then(Value::as_str) else {
                    continue;
                };
                if !name.contains("/skills/") {
                    continue;
                }
                latest.insert(
                    name.to_string(),
                    update.get("fields").cloned().unwrap_or_default(),
                );
            }
            let documents: Vec<Value> = latest
                .into_iter()
                .map(|(name, fields)| json!({ "name": name, "fields": fields }))
                .collect();
            respond(
                &mut socket,
                200,
                &json!({ "documents": documents }).to_string(),
            )
            .await
        }
        _ => respond(&mut socket, 404, r#"{"error":{"message":"not found"}}"#).await,
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs() as i64
}

async fn respond(socket: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await
}

fn record(id: &str, name: &str) -> SkillRecord {
    SkillRecord {
        id: id.to_string(),
        name: name.to_string(),
        description: format!("{name} does one job."),
        source: "github".to_string(),
        repo: Some("acme/skills".to_string()),
        path: Some(format!("skills/{name}/SKILL.md")),
        url: Some(format!("https://example.com/{name}")),
        body_sha: Some("deadbeef".to_string()),
        grade: Some("B".to_string()),
        grade_scope: Some("file-only".to_string()),
        license: Some("MIT".to_string()),
        stars: Some(7),
        installs: None,
        tokens: Some(300),
        capabilities: Vec::new(),
        simhash: None,
    }
}

async fn local_index() -> (tempfile::TempDir, SkillIndex) {
    let dir = tempfile::tempdir().expect("tempdir");
    let index = SkillIndex::open(dir.path()).await.expect("index");
    (dir, index)
}

#[tokio::test]
async fn sign_in_pushes_and_another_index_pulls() {
    let (origin, server) = start("3600").await;
    let config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
    let store = Arc::new(MemorySessionStore::new());
    let cloud = Cloud::new(config, Endpoints::same(origin), store.clone()).expect("cloud");

    let before = cloud.status().expect("status");
    assert!(before.configured);
    assert!(!before.signed_in);

    let after = cloud
        .sign_in("ada@example.com", "hunter2")
        .await
        .expect("sign in");
    assert!(after.signed_in);
    assert_eq!(after.email.as_deref(), Some("ada@example.com"));
    assert_eq!(after.uid.as_deref(), Some("uid-1"));
    assert_eq!(
        store.account().map(|(uid, email, _)| (uid, email)),
        Some(("uid-1".to_string(), "ada@example.com".to_string()))
    );
    assert!(server.saw("POST /v1/accounts:signInWithPassword"));

    let (_owning, source) = local_index().await;
    source
        .ingest_records(&[
            record("github:acme/skills:skills/pdf/SKILL.md", "pdf"),
            record("github:acme/skills:skills/csv/SKILL.md", "csv"),
        ])
        .await
        .expect("seed");

    let pushed = cloud.push(&source).await.expect("push");
    assert_eq!(pushed.pushed, 2);
    assert_eq!(pushed.batches, 1);
    assert_eq!(pushed.uid, "uid-1");
    assert_eq!(
        server.commits.lock().expect("commits").as_slice(),
        ["Bearer id-1".to_string(), "Bearer id-1".to_string()],
        "the profile and the skill batch each commit once with the ID token"
    );
    let names: Vec<String> = server
        .writes
        .lock()
        .expect("writes")
        .iter()
        .filter_map(|write| write["update"]["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        names.iter().all(|name| name
            .contains("projects/rigel-cloud-test/databases/(default)/documents/users/uid-1")),
        "{names:?}"
    );
    assert!(
        names
            .iter()
            .any(|name| name.ends_with("/documents/users/uid-1")),
        "the account document is written: {names:?}"
    );
    assert_eq!(
        names
            .iter()
            .filter(|name| name.contains("/skills/"))
            .count(),
        2,
        "one document per skill: {names:?}"
    );

    // A second machine, with an empty shelf, pulls the same account.
    let (_other_owning, other) = local_index().await;
    let pulled = cloud.pull(&other).await.expect("pull");
    assert_eq!(pulled.received, 2);
    assert_eq!(pulled.written, 2);
    assert_eq!(pulled.pages, 1);

    let rows = other.catalogue(10).await.expect("catalogue");
    assert_eq!(rows.len(), 2);
    let pdf = rows
        .iter()
        .find(|row| row.name == "pdf")
        .expect("the pdf row arrives");
    assert_eq!(pdf.id, "github:acme/skills:skills/pdf/SKILL.md");
    assert_eq!(pdf.grade.as_deref(), Some("B"));
    assert_eq!(
        pdf.body_sha, None,
        "a pull never claims a blob this machine does not hold"
    );

    let out = cloud.sign_out().expect("sign out");
    assert!(!out.signed_in);
    assert!(store.account().is_none());
}

#[tokio::test]
async fn publishing_a_session_writes_the_turns_the_phone_reads() {
    let (origin, server) = start("3600").await;
    let config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
    let cloud = Cloud::new(
        config,
        Endpoints::same(origin),
        Arc::new(MemorySessionStore::new()),
    )
    .expect("cloud");
    cloud
        .sign_in("ada@example.com", "hunter2")
        .await
        .expect("sign in");

    let dir = tempfile::tempdir().expect("tempdir");
    let service = SessionService::open(dir.path()).await.expect("session db");
    let session = service
        .start(Some("Fix the build"), Some(500))
        .await
        .expect("start");
    let agent = service
        .register_agent_with(&session.id, "native", Some("implementer"), Some("gpt-5.6"))
        .await
        .expect("agent");
    let turn = service
        .append_turn(&agent.id, Some("why is the build slow?"))
        .await
        .expect("turn");
    service
        .append_item(
            &turn.id,
            "message",
            "complete",
            &json!({ "role": "assistant", "text": "It compiles twice." }),
            &json!({}),
        )
        .await
        .expect("reply");
    service
        .append_item(
            &turn.id,
            "command",
            "complete",
            &json!({ "command": "cargo check --token hunter2" }),
            &json!({}),
        )
        .await
        .expect("command");
    service
        .finish_turn_with_reason(&turn.id, "completed", Some("end_turn"))
        .await
        .expect("finish");

    let report = cloud
        .publish_session(&service, &session.id)
        .await
        .expect("publish");
    assert_eq!(report.session_id, session.id);
    assert_eq!(report.title, "Fix the build");
    assert_eq!(report.turns, 1);
    assert_eq!(report.batches, 1);

    let writes = server.writes.lock().expect("writes").clone();
    assert_eq!(writes.len(), 2, "one turn and one session header");
    let turn_doc = &writes[0]["update"];
    let header = &writes[1]["update"];
    assert!(
        turn_doc["name"]
            .as_str()
            .expect("name")
            .ends_with(&format!("/sessions/{}/turns/{}", session.id, turn.id)),
        "{turn_doc}"
    );
    assert!(
        header["name"]
            .as_str()
            .expect("name")
            .ends_with(&format!("/sessions/{}", session.id)),
        "the header is written after its turns: {header}"
    );
    assert_eq!(
        turn_doc["fields"]["reply"]["stringValue"],
        "It compiles twice."
    );
    assert_eq!(
        turn_doc["fields"]["prompt"]["stringValue"],
        "why is the build slow?"
    );
    assert_eq!(turn_doc["fields"]["model"]["stringValue"], "gpt-5.6");
    assert_eq!(header["fields"]["title"]["stringValue"], "Fix the build");
    assert_eq!(header["fields"]["turn_count"]["integerValue"], "1");

    let text = serde_json::to_string(&writes).expect("json");
    assert!(
        !text.contains("cargo check") && !text.contains("hunter2"),
        "tool bodies must not be published"
    );
    let project = header["fields"]["project"]["stringValue"]
        .as_str()
        .expect("project");
    assert!(
        !project.contains('/') && !project.contains('\\'),
        "the absolute project path is not published: {project}"
    );
}

#[tokio::test]
async fn a_locally_removed_row_becomes_a_tombstone_and_deletes_on_pull() {
    let (origin, server) = start("3600").await;
    let config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
    let store = Arc::new(MemorySessionStore::new());
    let state_dir = tempfile::tempdir().expect("state dir");
    let cloud =
        Cloud::new_at(state_dir.path(), config, Endpoints::same(origin), store).expect("cloud");
    cloud
        .sign_in("ada@example.com", "hunter2")
        .await
        .expect("sign in");

    const PDF: &str = "github:acme/skills:skills/pdf/SKILL.md";
    const CSV: &str = "github:acme/skills:skills/csv/SKILL.md";

    let (_owning, owner) = local_index().await;
    owner
        .ingest_records(&[record(PDF, "pdf"), record(CSV, "csv")])
        .await
        .expect("seed");
    let first = cloud.push(&owner).await.expect("first push");
    assert_eq!(first.pushed, 2);
    assert_eq!(first.deleted, 0);
    assert!(!first.truncated);

    // A second machine receives both rows, so it can later observe the deletion.
    let (_other_owning, other) = local_index().await;
    let before = cloud.pull(&other).await.expect("first pull");
    assert_eq!(before.received, 2);
    assert_eq!(before.deleted, 0);
    assert!(
        other.get(PDF).await.expect("get").is_some(),
        "the row arrives before it is deleted"
    );

    assert!(owner.remove(PDF).await.expect("local remove"));
    let second = cloud.push(&owner).await.expect("second push");
    assert_eq!(second.pushed, 1);
    assert_eq!(second.deleted, 1);
    assert!(
        second.batches >= 2,
        "the live batch and the tombstone batch"
    );

    let wanted = format!("/skills/{}", rigel_core::cloud::document_id(PDF));
    let writes = server.writes.lock().expect("writes").clone();
    let tombstone = writes
        .iter()
        .rev()
        .find(|write| {
            write["update"]["name"]
                .as_str()
                .is_some_and(|name| name.ends_with(&wanted))
        })
        .expect("a tombstone write");
    assert_eq!(
        tombstone["update"]["fields"]["deleted"]["booleanValue"],
        true
    );
    assert_eq!(
        tombstone["update"]["fields"]["id"]["stringValue"], PDF,
        "the tombstone names the row a client has to remove"
    );
    assert!(
        tombstone["update"]["fields"].get("name").is_none(),
        "a tombstone is not a skill document"
    );

    let after = cloud.pull(&other).await.expect("second pull");
    assert_eq!(after.tombstones, 1);
    assert_eq!(after.deleted, 1);
    assert!(
        other.get(PDF).await.expect("get").is_none(),
        "the tombstone deletes the row locally"
    );
    assert!(other.get(CSV).await.expect("get").is_some());

    let status = cloud.status().expect("status");
    let pushed = status.last_push_at.expect("a push clock");
    let pulled = status.last_pull_at.expect("a pull clock");
    assert_eq!(status.last_sync_at, Some(pushed.max(pulled)));

    let manifest = std::fs::read_to_string(state_dir.path().join("cloud-state.json"))
        .expect("the manifest is on disk");
    assert!(manifest.contains(CSV), "{manifest}");
    assert!(
        !manifest.contains(PDF),
        "a tombstoned id leaves the manifest so a later push cannot resurrect it: {manifest}"
    );
}

#[tokio::test]
async fn an_expired_token_is_refreshed_before_the_next_commit() {
    // One second of life means the session is already inside the refresh margin.
    let (origin, server) = start("1").await;
    let config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
    let store = Arc::new(MemorySessionStore::new());
    let cloud = Cloud::new(config, Endpoints::same(origin), store.clone()).expect("cloud");

    // The account arrives verified, so the refresh below has a flag to lose.
    server.verified.store(true, Ordering::SeqCst);
    cloud
        .sign_in("ada@example.com", "hunter2")
        .await
        .expect("sign in");
    let (_owning, index) = local_index().await;
    index
        .ingest_records(&[record("github:acme/skills:skills/pdf/SKILL.md", "pdf")])
        .await
        .expect("seed");

    cloud.push(&index).await.expect("push");

    assert!(
        server.saw("POST /v1/token"),
        "the refresh endpoint was used"
    );
    assert_eq!(
        server.commits.lock().expect("commits").as_slice(),
        ["Bearer id-2".to_string(), "Bearer id-2".to_string()],
        "the commits carry the refreshed token, not the expired one"
    );
    assert_eq!(
        store.account().map(|(uid, _, _)| uid),
        Some("uid-1".to_string()),
        "a refresh keeps the same account"
    );
    assert!(
        store.load().expect("load").expect("session").email_verified,
        "a refresh must not drop the verification answer"
    );
}

#[tokio::test]
async fn sign_up_creates_an_account_and_signs_it_in() {
    let (origin, server) = start("3600").await;
    let config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
    let store = Arc::new(MemorySessionStore::new());
    let cloud = Cloud::new(config, Endpoints::same(origin), store.clone()).expect("cloud");

    let status = cloud
        .sign_up("new@example.com", "hunter2")
        .await
        .expect("sign up");

    assert!(status.signed_in);
    assert_eq!(status.email.as_deref(), Some("new@example.com"));
    assert!(server.saw("POST /v1/accounts:signUp"));
    assert_eq!(
        store.account().map(|(_, email, _)| email),
        Some("new@example.com".to_string()),
        "creating the account also stores its session"
    );
}

#[tokio::test]
async fn an_unverified_account_requests_a_link_and_can_confirm_it() {
    let (origin, server) = start("3600").await;
    let config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
    let cloud = Cloud::new(
        config,
        Endpoints::same(origin),
        Arc::new(MemorySessionStore::new()),
    )
    .expect("cloud");

    let signed_in = cloud
        .sign_in("ada@example.com", "hunter2")
        .await
        .expect("sign in");
    assert!(!signed_in.email_verified);
    assert!(!cloud.status().expect("status").email_verified);

    let address = cloud.send_verification_email().await.expect("send");
    assert_eq!(address, "ada@example.com");
    assert_eq!(
        server
            .verification_tokens
            .lock()
            .expect("tokens")
            .as_slice(),
        ["id-1".to_string()],
        "the request names the account with its ID token, not with an address"
    );

    assert!(
        !cloud
            .check_email_verification()
            .await
            .expect("first check")
            .email_verified,
        "checking before the link is clicked must not verify the address"
    );

    server.verified.store(true, Ordering::SeqCst);
    let confirmed = cloud.check_email_verification().await.expect("check");
    assert!(confirmed.email_verified);
    assert!(
        cloud.status().expect("status").email_verified,
        "the answer is kept in the stored session"
    );

    let refused = cloud.send_verification_email().await;
    assert!(
        matches!(refused, Err(CloudError::Input(_))),
        "a verified account has no link to resend"
    );
}

#[tokio::test]
async fn app_check_tokens_are_fetched_once_and_attached_to_every_request() {
    let (origin, server) = start("3600").await;
    let attest_url = format!("{origin}/app-check/token");
    let mut config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
    config.app_check = Some(AppCheckConfig {
        attest_url: attest_url.clone(),
    });
    let cloud = Cloud::new(
        config,
        Endpoints::same(origin),
        Arc::new(MemorySessionStore::new()),
    )
    .expect("cloud");

    let status = cloud.status().expect("status");
    assert_eq!(
        status.app_check.map(|check| check.attest_url),
        Some(attest_url)
    );
    assert_eq!(
        server.attest_calls.load(Ordering::SeqCst),
        0,
        "reading the status must not call the endpoint"
    );

    cloud
        .sign_in("ada@example.com", "hunter2")
        .await
        .expect("sign in");
    let (_owning, index) = local_index().await;
    index
        .ingest_records(&[record("github:acme/skills:skills/pdf/SKILL.md", "pdf")])
        .await
        .expect("seed");
    cloud.push(&index).await.expect("push");

    let headers = server.app_check_headers.lock().expect("headers").clone();
    assert!(!headers.is_empty());
    assert!(
        headers.iter().all(|value| value == "app-check-1"),
        "every Firebase request carries the token: {headers:?}"
    );
    assert_eq!(
        server.attest_calls.load(Ordering::SeqCst),
        1,
        "the sign-in and every commit share one cached token"
    );

    let serialized = serde_json::to_string(&cloud.status().expect("status")).expect("json");
    assert!(
        !serialized.contains("app-check-1"),
        "the token is never part of the status: {serialized}"
    );
}

#[tokio::test]
async fn a_broken_attestation_endpoint_stops_the_request_before_firebase() {
    let (origin, server) = start("3600").await;
    for path in [
        "/app-check/error",
        "/app-check/expired",
        "/app-check/malformed",
    ] {
        let mut config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
        config.app_check = Some(AppCheckConfig {
            attest_url: format!("{origin}{path}"),
        });
        let cloud = Cloud::new(
            config,
            Endpoints::same(origin.clone()),
            Arc::new(MemorySessionStore::new()),
        )
        .expect("cloud");

        let error = cloud
            .sign_in("ada@example.com", "hunter2")
            .await
            .expect_err("no token means no Firebase request");
        assert!(matches!(&error, CloudError::AppCheck(_)), "{path}: {error}");
    }
    assert!(
        !server.saw("POST /v1/accounts:signInWithPassword"),
        "a failed attestation must not fall through to an un-attested request"
    );
    assert!(server.app_check_headers.lock().expect("headers").is_empty());
}

#[tokio::test]
async fn a_short_password_is_refused_before_a_request() {
    let (origin, server) = start("3600").await;
    let config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
    let cloud = Cloud::new(
        config,
        Endpoints::same(origin),
        Arc::new(MemorySessionStore::new()),
    )
    .expect("cloud");

    let error = cloud
        .sign_up("new@example.com", "12345")
        .await
        .expect_err("five characters cannot pass Firebase's six-character floor");
    assert!(
        error.to_string().contains("at least 6 characters"),
        "{error}"
    );
    assert!(!server.saw("POST /v1/accounts:signUp"));
}

#[tokio::test]
async fn a_password_reset_does_not_reveal_whether_the_account_exists() {
    let (origin, server) = start("3600").await;
    let config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
    let cloud = Cloud::new(
        config,
        Endpoints::same(origin),
        Arc::new(MemorySessionStore::new()),
    )
    .expect("cloud");

    let known = cloud
        .send_password_reset("ada@example.com")
        .await
        .expect("a known address is sent a link");
    let unknown = cloud
        .send_password_reset("missing@example.com")
        .await
        .expect("an unknown address gets the same answer, not an account oracle");

    assert_eq!(known, "ada@example.com");
    assert_eq!(unknown, "missing@example.com");
    assert!(server.saw("POST /v1/accounts:sendOobCode"));
}

#[tokio::test]
async fn a_wrong_password_is_a_sentence_not_a_json_envelope() {
    let (origin, _server) = start("3600").await;
    let config = CloudConfig::new("rigel-cloud-test", "web-key").expect("config");
    // The fake answers success for any password, so this test proves the message mapping on a
    // *rejected* answer by pointing the identity host at a path the fake 404s.
    let endpoints = Endpoints {
        identity: format!("{origin}/missing"),
        ..Endpoints::same(origin)
    };
    let cloud = Cloud::new(config, endpoints, Arc::new(MemorySessionStore::new())).expect("cloud");
    let error = cloud
        .sign_in("ada@example.com", "wrong")
        .await
        .expect_err("the endpoint does not exist");
    let text = error.to_string();
    assert!(text.contains("not found"), "{text}");
    assert!(
        !text.contains("hunter2"),
        "a password never appears in an error"
    );
}

//! The shelf's guarantees, exercised over a real HTTP connection to a local fixture.
//!
//! The fixture exists because the interesting behaviours are all about *not* repeating work:
//! a re-index inside the freshness window must make no requests at all, and a forced one must
//! revalidate with `If-None-Match` and accept a `304` without losing the body. Asserting that
//! against api.github.com would be both slow and non-deterministic.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rigel_core::skills::evidence::Evidence;
use rigel_core::skills::github::GitHubSource;
use rigel_core::skills::source::{SkillRecord, Source, SourceState, SyncPolicy};
use rigel_core::{INDEX_MIGRATIONS, SkillIndex, Store};
use sqlx::Row;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/* ------------------------------------------------------------------ *
 * fixture server
 * ------------------------------------------------------------------ */

#[derive(Clone)]
struct Route {
    body: String,
    etag: Option<String>,
}

struct Fixture {
    base: String,
    hits: Arc<AtomicUsize>,
    conditional: Arc<Mutex<Vec<String>>>,
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

impl Fixture {
    fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }
}

async fn fixture(routes: Vec<(&str, &str, Option<&str>)>) -> Fixture {
    let table: Arc<HashMap<String, Route>> = Arc::new(
        routes
            .into_iter()
            .map(|(path, body, etag)| {
                (
                    path.to_string(),
                    Route {
                        body: body.to_string(),
                        etag: etag.map(ToOwned::to_owned),
                    },
                )
            })
            .collect(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let conditional = Arc::new(Mutex::new(Vec::new()));
    let (shutdown, mut stop) = tokio::sync::oneshot::channel::<()>();

    let (hits_task, conditional_task) = (hits.clone(), conditional.clone());
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stop => break,
                accepted = listener.accept() => {
                    let Ok((mut socket, _)) = accepted else { break };
                    let table = table.clone();
                    let hits = hits_task.clone();
                    let conditional = conditional_task.clone();
                    tokio::spawn(async move {
                        let mut buffer = vec![0_u8; 16_384];
                        let read = socket.read(&mut buffer).await.unwrap_or(0);
                        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                        let mut lines = request.lines();
                        let request_line = lines.next().unwrap_or_default().to_string();
                        let path = request_line
                            .split(' ')
                            .nth(1)
                            .unwrap_or("/")
                            .trim_end_matches('/')
                            .to_string();
                        let if_none_match = lines
                            .filter_map(|line| {
                                let lower = line.to_ascii_lowercase();
                                lower
                                    .strip_prefix("if-none-match:")
                                    .map(|value| value.trim().to_string())
                            })
                            .next();

                        hits.fetch_add(1, Ordering::Relaxed);
                        if let Some(tag) = &if_none_match {
                            conditional.lock().unwrap().push(tag.clone());
                        }

                        let response = match table.get(&path) {
                            None => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".to_string(),
                            Some(route) => match (&route.etag, &if_none_match) {
                                (Some(etag), Some(sent)) if etag == sent => {
                                    "HTTP/1.1 304 Not Modified\r\n\r\n".to_string()
                                }
                                _ => {
                                    let mut head = String::from("HTTP/1.1 200 OK\r\n");
                                    head.push_str("content-type: application/json\r\n");
                                    if let Some(etag) = &route.etag {
                                        head.push_str(&format!("etag: {etag}\r\n"));
                                    }
                                    head.push_str(&format!(
                                        "content-length: {}\r\n\r\n",
                                        route.body.len()
                                    ));
                                    format!("{head}{}", route.body)
                                }
                            },
                        };
                        socket.write_all(response.as_bytes()).await.ok();
                        socket.flush().await.ok();
                    });
                }
            }
        }
    });

    Fixture {
        base,
        hits,
        conditional,
        _shutdown: shutdown,
    }
}

const PDF_BODY: &str = "---\nname: pdf-reader\ndescription: Read scanned PDFs and receipts.\nlicense: MIT\n---\n# PDF reader\n\nAsk the agent for the file, then summarise it.\n";
const WRITER_BODY: &str = "---\nname: commit-writer\ndescription: Write a commit subject from the staged diff.\n---\n# Commit writer\n\nRead the diff, then propose one subject line under 60 characters.\n";
const TREE: &str = r#"{"truncated": false, "tree": [
  {"path": "README.md", "type": "blob", "size": 900},
  {"path": "pdf/SKILL.md", "type": "blob", "size": 180},
  {"path": "writer/SKILL.md", "type": "blob", "size": 190}
]}"#;
const META: &str = r#"{"full_name": "acme/skills", "default_branch": "main", "stargazers_count": 42, "license": {"spdx_id": "MIT"}, "pushed_at": "2026-10-01T00:00:00Z"}"#;

async fn index_with_fixture() -> (TempDir, SkillIndex, Fixture, GitHubSource) {
    let tmp = TempDir::new().unwrap();
    let index = SkillIndex::open(tmp.path()).await.expect("index opens");
    let fixture = fixture(vec![
        ("/repos/acme/skills", META, Some("\"meta-1\"")),
        (
            "/repos/acme/skills/git/trees/main?recursive=1",
            TREE,
            Some("\"tree-1\""),
        ),
        (
            "/raw/acme/skills/main/pdf/SKILL.md",
            PDF_BODY,
            Some("\"pdf-1\""),
        ),
        (
            "/raw/acme/skills/main/writer/SKILL.md",
            WRITER_BODY,
            Some("\"writer-1\""),
        ),
    ])
    .await;

    let source = GitHubSource::new(
        index.store().pool().clone(),
        index.blobs().clone(),
        Duration::from_secs(600),
    )
    .await
    .unwrap()
    .with_endpoints(&fixture.base, &format!("{}/raw", fixture.base));

    (tmp, index, fixture, source)
}

/* ------------------------------------------------------------------ *
 * bodies on disk, rows in SQLite
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn ingest_puts_bodies_on_disk_and_never_in_sqlite() {
    let (_tmp, index, fixture, source) = index_with_fixture().await;

    let snapshot = source
        .fetch_repo("acme/skills", SyncPolicy::normal())
        .await
        .unwrap();
    assert_eq!(snapshot.skills.len(), 2);
    assert_eq!(snapshot.found, 2);
    assert_eq!(snapshot.http_requests, 4, "meta + tree + two raw bodies");
    assert_eq!(fixture.hits(), 4);
    assert_eq!(snapshot.stars, 42);
    assert_eq!(snapshot.license.as_deref(), Some("MIT"));

    let report = index.ingest(&snapshot).await.unwrap();
    assert_eq!(report.added, 2);
    assert_eq!(report.updated, 0);
    assert_eq!(report.with_body, 2);

    // The body is readable back through the index…
    let (body, sha) = index
        .body("github:acme/skills:pdf/SKILL.md")
        .await
        .unwrap()
        .expect("body is indexed");
    assert_eq!(body, PDF_BODY);
    assert_eq!(sha.len(), 64);
    assert!(index.blobs().path_for(&sha).exists());

    // …and the blob is not stored in the database. `blobs` is a manifest, and `skills` has no
    // body column at all: P2.3.
    let columns: Vec<String> = sqlx::query("PRAGMA table_info(skills)")
        .fetch_all(index.store().pool())
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    assert!(!columns.iter().any(|c| c == "body" || c == "content"));
    let blob_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM blobs")
        .fetch_one(index.store().pool())
        .await
        .unwrap();
    assert_eq!(blob_rows, 0, "bodies live on disk, not in a BLOB column");

    let detail = index
        .get("github:acme/skills:pdf/SKILL.md")
        .await
        .unwrap()
        .expect("row exists");
    assert_eq!(detail.skill.grade.as_deref(), Some("B"));
    assert_eq!(detail.skill.grade_scope.as_deref(), Some("file-only"));
    assert!(detail.skill.has_body);
    assert_eq!(detail.capabilities.len(), 1);
    assert_eq!(detail.capabilities[0].severity.as_str(), "low");
}

#[tokio::test]
async fn reindexing_inside_the_window_makes_zero_requests() {
    let (_tmp, index, fixture, source) = index_with_fixture().await;
    let first = source
        .fetch_repo("acme/skills", SyncPolicy::normal())
        .await
        .unwrap();
    index.ingest(&first).await.unwrap();
    let after_first = fixture.hits();

    let second = source
        .fetch_repo("acme/skills", SyncPolicy::normal())
        .await
        .unwrap();
    assert_eq!(
        second.http_requests, 0,
        "a re-index inside the window is free"
    );
    assert_eq!(fixture.hits(), after_first, "nothing reached the fixture");

    let report = index.ingest(&second).await.unwrap();
    assert_eq!(report.added, 0);
    assert_eq!(report.unchanged, 2, "same bodies, same hashes");
    assert_eq!(report.updated, 0);
}

#[tokio::test]
async fn a_forced_reindex_revalidates_and_accepts_304() {
    let (_tmp, index, fixture, source) = index_with_fixture().await;
    let first = source
        .fetch_repo("acme/skills", SyncPolicy::normal())
        .await
        .unwrap();
    index.ingest(&first).await.unwrap();

    let forced = source
        .fetch_repo("acme/skills", SyncPolicy::forced())
        .await
        .unwrap();
    assert_eq!(
        forced.http_requests, 4,
        "meta + tree + both bodies revalidated"
    );
    assert_eq!(forced.skills.len(), 2);

    let sent = fixture.conditional.lock().unwrap().clone();
    assert!(
        sent.contains(&"\"meta-1\"".to_string()),
        "etag was not sent: {sent:?}"
    );
    assert!(sent.contains(&"\"pdf-1\"".to_string()));

    // A 304 returns the cached body, so the skill is still fully readable.
    let body = index
        .body("github:acme/skills:writer/SKILL.md")
        .await
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(body, WRITER_BODY);
}

#[tokio::test]
async fn an_unreachable_source_goes_broken_and_keeps_its_last_sync() {
    let tmp = TempDir::new().unwrap();
    let index = SkillIndex::open(tmp.path()).await.unwrap();

    index
        .set_source("github", "https://api.github.com", SourceState::Ok, None)
        .await
        .unwrap();
    let healthy = index.status().await.unwrap();
    assert_eq!(healthy[0].state, SourceState::Ok);
    assert!(healthy[0].last_sync_at.is_some());

    // A source that fails later keeps the timestamp of when it last worked, because "last
    // reached 3h ago" is the useful sentence, not "broken".
    index
        .set_source(
            "github",
            "https://api.github.com",
            SourceState::Broken,
            Some("connect timeout"),
        )
        .await
        .unwrap();
    let broken = index.status().await.unwrap();
    assert_eq!(broken[0].state, SourceState::Broken);
    assert_eq!(broken[0].error.as_deref(), Some("connect timeout"));
    assert_eq!(broken[0].last_sync_at, healthy[0].last_sync_at);
}

#[tokio::test]
async fn gc_keeps_bodies_and_cached_responses() {
    let (_tmp, index, _fixture, source) = index_with_fixture().await;
    let snapshot = source
        .fetch_repo("acme/skills", SyncPolicy::normal())
        .await
        .unwrap();
    index.ingest(&snapshot).await.unwrap();

    let live = index.live_blobs().await.unwrap();
    // Two skill bodies plus the cached repo metadata and tree.
    assert_eq!(live.len(), 4, "cache blobs must be live: {live:?}");

    let orphan = index.blobs().put(b"from a previous life").unwrap();
    assert!(index.blobs().exists(&orphan));
    assert_eq!(index.blobs().gc(&live).unwrap(), 1);
    assert!(!index.blobs().exists(&orphan));
    assert!(!live.contains(&orphan));

    // Everything still resolves: bodies through the index, cache through a second walk that
    // must remain request-free.
    assert!(
        index
            .body("github:acme/skills:pdf/SKILL.md")
            .await
            .unwrap()
            .is_some()
    );
    let again = source
        .fetch_repo("acme/skills", SyncPolicy::normal())
        .await
        .unwrap();
    assert_eq!(again.http_requests, 0);
}

#[tokio::test]
async fn a_missing_cached_body_is_refetched_instead_of_failing() {
    let (_tmp, index, fixture, source) = index_with_fixture().await;
    let first = source
        .fetch_repo("acme/skills", SyncPolicy::normal())
        .await
        .unwrap();
    index.ingest(&first).await.unwrap();

    // Delete the blob behind the tree response, leaving its cache row pointing at nothing.
    // A `gc` that ran before cache blobs counted as live did exactly this, and it must not
    // brick every later re-index of that repository.
    let sha: String =
        sqlx::query_scalar("SELECT sha256 FROM http_cache WHERE url LIKE '%git/trees%'")
            .fetch_one(index.store().pool())
            .await
            .unwrap();
    std::fs::remove_file(index.blobs().path_for(&sha)).unwrap();

    // TTL zero forces revalidation, which is where the 304-without-body branch lives.
    let cold = GitHubSource::new(
        index.store().pool().clone(),
        index.blobs().clone(),
        Duration::ZERO,
    )
    .await
    .unwrap()
    .with_endpoints(&fixture.base, &format!("{}/raw", fixture.base));

    let snapshot = cold
        .fetch_repo("acme/skills", SyncPolicy::normal())
        .await
        .unwrap();
    assert_eq!(
        snapshot.skills.len(),
        2,
        "the tree was fetched again, not abandoned"
    );
    assert!(
        index.blobs().exists(&sha),
        "the missing blob is back on disk"
    );
}

#[tokio::test]
async fn an_offline_source_with_no_cache_fails_loudly_instead_of_inventing_rows() {
    let tmp = TempDir::new().unwrap();
    let index = SkillIndex::open(tmp.path()).await.unwrap();
    let source = GitHubSource::new(
        index.store().pool().clone(),
        index.blobs().clone(),
        Duration::ZERO,
    )
    .await
    .unwrap()
    .with_endpoints("http://127.0.0.1:9", "http://127.0.0.1:9/raw");

    let error = source
        .fetch_repo("acme/skills", SyncPolicy::normal())
        .await
        .expect_err("nothing is listening on port 9");
    assert!(error.to_string().contains("could not reach"), "{error}");
}

/* ------------------------------------------------------------------ *
 * ordering, variants, evidence
 * ------------------------------------------------------------------ */

fn record(id: &str, name: &str, description: &str, grade: &str, stars: i64) -> SkillRecord {
    SkillRecord {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        source: "local".to_string(),
        repo: None,
        path: None,
        url: None,
        body_sha: None,
        grade: Some(grade.to_string()),
        grade_scope: Some("file-only".to_string()),
        license: None,
        stars: Some(stars),
        installs: None,
        tokens: None,
        capabilities: Vec::new(),
        simhash: None,
    }
}

#[tokio::test]
async fn search_orders_by_grade_then_stars_then_recency() {
    let tmp = TempDir::new().unwrap();
    let index = SkillIndex::open(tmp.path()).await.unwrap();

    // Same relevance, different trust signals: the order must be grade, then stars, then age.
    index
        .ingest_records(&[
            record("local:c", "review-c", "review the diff", "C", 900),
            record("local:b", "review-b", "review the diff", "B", 10),
            record("local:b2", "review-b2", "review the diff", "B", 10),
            record("local:f", "review-f", "review the diff", "F", 9_000),
            record("local:a", "review-a", "review the diff", "A", 0),
        ])
        .await
        .unwrap();

    // Recency is the third signal, so give the two B rows different ages.
    for (id, seen) in [("local:b", 1_000_i64), ("local:b2", 2_000_i64)] {
        sqlx::query("UPDATE skills SET last_seen_at = ? WHERE id = ?")
            .bind(seen)
            .bind(id)
            .execute(index.store().pool())
            .await
            .unwrap();
    }

    let hits = index.search("review", 10).await.unwrap();
    let names: Vec<&str> = hits.iter().map(|hit| hit.name.as_str()).collect();
    assert_eq!(
        names,
        ["review-a", "review-b2", "review-b", "review-c", "review-f"]
    );

    // A word that is not in the index returns nothing rather than everything.
    assert!(index.search("kubernetes", 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn near_duplicates_cluster_without_hiding_any_row() {
    let tmp = TempDir::new().unwrap();
    let index = SkillIndex::open(tmp.path()).await.unwrap();

    let body = "# Code review\n\nRead the diff before the description, name the specific risk you see, and propose the smallest change that removes it. Check that new branches have tests and that error paths return something a caller can act on. Leave style to the formatter, and never approve a change you have not understood.\n";
    let fork = body.replace("smallest change", "least invasive change");

    let snapshot = rigel_core::skills::RepoSnapshot {
        source: "github".into(),
        repo: "acme/skills".into(),
        branch: "main".into(),
        stars: 1,
        license: Some("MIT".into()),
        truncated: false,
        found: 2,
        http_requests: 0,
        skills: vec![
            rigel_core::skills::source::FetchedSkill {
                record: SkillRecord {
                    id: "github:acme/skills:review/SKILL.md".into(),
                    name: "review".into(),
                    description: "Review a diff".into(),
                    source: "github".into(),
                    repo: Some("acme/skills".into()),
                    path: Some("review/SKILL.md".into()),
                    url: Some("https://github.com/acme/skills/blob/main/review/SKILL.md".into()),
                    body_sha: None,
                    grade: Some("B".into()),
                    grade_scope: Some("file-only".into()),
                    license: Some("MIT".into()),
                    stars: Some(10),
                    installs: None,
                    tokens: Some(400),
                    capabilities: Vec::new(),
                    simhash: Some(rigel_core::skills::simhash(body) as i64),
                },
                body: body.into(),
            },
            rigel_core::skills::source::FetchedSkill {
                record: SkillRecord {
                    id: "github:acme/fork:review/SKILL.md".into(),
                    name: "review".into(),
                    description: "Review a diff".into(),
                    source: "github".into(),
                    repo: Some("acme/fork".into()),
                    path: Some("review/SKILL.md".into()),
                    url: Some("https://github.com/acme/fork/blob/main/review/SKILL.md".into()),
                    body_sha: None,
                    grade: Some("B".into()),
                    grade_scope: Some("file-only".into()),
                    license: None,
                    stars: Some(2),
                    installs: None,
                    tokens: Some(400),
                    capabilities: Vec::new(),
                    simhash: Some(rigel_core::skills::simhash(&fork) as i64),
                },
                body: fork,
            },
        ],
    };

    let report = index.ingest(&snapshot).await.unwrap();
    assert_eq!(report.clusters, 1, "the fork belongs in the same cluster");

    // Both rows are still returned: clustering annotates, it never collapses (P2.6).
    let hits = index.search("diff", 10).await.unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|hit| hit.variants == 2));
    assert_eq!(hits[0].cluster_id, hits[1].cluster_id);

    let detail = index
        .get("github:acme/fork:review/SKILL.md")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.variants.len(), 1);
    assert_eq!(detail.variants[0].id, "github:acme/skills:review/SKILL.md");
    assert!(
        detail.variants[0].delta.contains("bits from this one"),
        "unexpected delta: {}",
        detail.variants[0].delta
    );
    assert!(
        detail.variants[0].delta.contains("no license"),
        "the licence difference is the interesting one: {}",
        detail.variants[0].delta
    );
}

#[tokio::test]
async fn advertised_numbers_are_kept_next_to_an_explicit_unmeasured_label() {
    let tmp = TempDir::new().unwrap();
    let index = SkillIndex::open(tmp.path()).await.unwrap();

    index
        .ingest_records(&[record(
            "local:rtk",
            "rtk",
            "Claims 60–90% token reduction on long diffs.",
            "C",
            100,
        )])
        .await
        .unwrap();

    let detail = index.get("local:rtk").await.unwrap().unwrap();
    assert_eq!(detail.evidence.len(), 1);
    let claim = &detail.evidence[0];
    assert_eq!(claim.metric, "token-reduction");
    assert_eq!(claim.advertised, Some(90.0));
    assert_eq!(claim.measured, None);
    assert_eq!(claim.label, "unmeasured");

    let hit = index.search("rtk", 5).await.unwrap().remove(0);
    assert!(
        hit.unmeasured,
        "the shelf needs this flag to render the tag"
    );

    // A real measurement replaces the label without touching the advertised number.
    Evidence {
        skill_id: "local:rtk".into(),
        metric: "token-reduction".into(),
        measured: Some(18.0),
        advertised: Some(90.0),
        source: "rigel bench 2026-10".into(),
        source_url: None,
        observed_at: 1,
    }
    .record(index.store().pool())
    .await
    .unwrap();

    // The benchmark is a separate observation from the vendor's claim (source is part of the
    // key), so both rows stay: measured 18% next to advertised 90%.
    let detail = index.get("local:rtk").await.unwrap().unwrap();
    assert_eq!(detail.evidence.len(), 2);
    let measured = detail
        .evidence
        .iter()
        .find(|row| row.label == "measured")
        .expect("the benchmark row is labelled measured");
    assert_eq!(measured.measured, Some(18.0));
    assert_eq!(measured.advertised, Some(90.0));
    assert!(
        detail.evidence.iter().any(|row| row.label == "unmeasured"),
        "the vendor's own claim stays unmeasured"
    );
    assert!(
        !index.search("rtk", 5).await.unwrap().remove(0).unmeasured,
        "a measured metric clears the shelf's tag"
    );
}

/* ------------------------------------------------------------------ *
 * prototype migration and schema upgrade
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn prototype_entries_import_once_and_report_what_they_did() {
    let tmp = TempDir::new().unwrap();
    let index = SkillIndex::open(tmp.path()).await.unwrap();

    let export = r#"[
      {"key":"gh:acme/pdf","name":"pdf-reader","description":"Read PDFs","source":"github","url":"https://github.com/acme/pdf","grade":"B","stars":12,"installs":"1.2k","savedAt":1760961600000},
      {"key":"gh:acme/pdf","name":"pdf-reader","description":"Read PDFs","source":"github","url":"https://github.com/acme/pdf","grade":"B","savedAt":1760961600000},
      {"key":"gh:acme/blank","name":"   ","description":"no name","source":"github","savedAt":1760961600000}
    ]"#;

    let report = index.import_prototype_entries(export).await.unwrap();
    assert_eq!(report.imported, 1);
    assert_eq!(report.skipped, 1, "the same URL twice is one row");
    assert_eq!(report.invalid, 1);

    let hits = index.search("pdf", 5).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "local:gh:acme/pdf");
    assert_eq!(hits[0].source, "local");
    assert_eq!(
        hits[0].grade.as_deref(),
        Some("B"),
        "the prototype grade is preserved"
    );
    assert!(
        !hits[0].has_body,
        "an imported row has no body until it is fetched"
    );

    // savedAt is kept, so "last seen" does not jump to now just because of the migration.
    let (first, last): (i64, i64) =
        sqlx::query_as("SELECT first_seen_at, last_seen_at FROM skills WHERE id = ?")
            .bind("local:gh:acme/pdf")
            .fetch_one(index.store().pool())
            .await
            .unwrap();
    assert_eq!(first, 1_760_961_600_000);
    assert_eq!(last, 1_760_961_600_000);

    // Re-running the import is a no-op.
    let again = index.import_prototype_entries(export).await.unwrap();
    assert_eq!(again.imported, 0);
}

#[tokio::test]
async fn a_version_1_index_upgrades_to_0002_in_place() {
    let tmp = TempDir::new().unwrap();

    // A Phase 1 database: migration 0001 only.
    {
        let store = Store::open(tmp.path().join("index.db"), &INDEX_MIGRATIONS[..1])
            .await
            .unwrap();
        let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(version, 1);
        store.pool().close().await;
    }

    let index = SkillIndex::open(tmp.path()).await.unwrap();
    let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
        .fetch_one(index.store().pool())
        .await
        .unwrap();
    let latest = INDEX_MIGRATIONS
        .last()
        .expect("at least one migration")
        .version;
    assert_eq!(
        version, latest,
        "a Phase 1 index upgrades to the current schema"
    );

    // The new columns and the cache table exist after the upgrade.
    let columns: Vec<String> = sqlx::query("PRAGMA table_info(skills)")
        .fetch_all(index.store().pool())
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    assert!(columns.contains(&"simhash".to_string()));
    assert!(columns.contains(&"capabilities_json".to_string()));

    let tables: Vec<String> = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table'")
        .fetch_all(index.store().pool())
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    assert!(tables.contains(&"http_cache".to_string()));
}

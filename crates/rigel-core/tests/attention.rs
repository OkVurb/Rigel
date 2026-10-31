//! D154: only unresolved work reaches the Attention view, and resolution is a recorded row,
//! not a timer. These tests drive each kind and then clear it.

use rigel_core::agent::approval::PendingApproval;
use rigel_core::attention::Attention;
use rigel_core::safety::{ReviewInput, Verdict};
use rigel_core::{ClaimService, MergeReviews, SessionService, attention::attention};

#[tokio::test]
async fn a_failed_turn_clears_when_the_agent_completes_a_later_one() {
    let tmp = tempfile::tempdir().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("attention"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("writer"))
        .await
        .unwrap();

    let failed = service.append_turn(&agent.id, Some("first")).await.unwrap();
    service.finish_turn(&failed.id, "failed").await.unwrap();

    let view = attention(service.pool(), rigel_core::migrate::now_ms())
        .await
        .unwrap();
    assert_eq!(view.total, 1, "{view:?}");
    assert_eq!(view.groups.len(), 1);
    assert_eq!(view.groups[0].session_id, session.id);
    assert_eq!(view.groups[0].items[0].kind, "failed_turn");
    assert_eq!(
        view.groups[0].items[0].turn_id.as_deref(),
        Some(failed.id.as_str())
    );

    let retry = service.append_turn(&agent.id, Some("again")).await.unwrap();
    service.finish_turn(&retry.id, "completed").await.unwrap();
    let after = attention(service.pool(), rigel_core::migrate::now_ms())
        .await
        .unwrap();
    assert_eq!(
        after.total, 0,
        "a later completed turn is the resolution: {after:?}"
    );
}

#[tokio::test]
async fn a_budget_stop_is_its_own_kind_and_also_clears_on_retry() {
    let tmp = tempfile::tempdir().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("attention"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("writer"))
        .await
        .unwrap();

    let stopped = service.append_turn(&agent.id, Some("spend")).await.unwrap();
    service
        .finish_turn_with_reason(&stopped.id, "completed", Some("budget_exceeded"))
        .await
        .unwrap();

    let view = attention(service.pool(), rigel_core::migrate::now_ms())
        .await
        .unwrap();
    assert_eq!(view.total, 1);
    assert_eq!(view.groups[0].items[0].kind, "budget_stop");
    assert!(view.groups[0].items[0].detail.contains("cap"));

    let retry = service
        .append_turn(&agent.id, Some("smaller"))
        .await
        .unwrap();
    service.finish_turn(&retry.id, "completed").await.unwrap();
    assert_eq!(
        attention(service.pool(), rigel_core::migrate::now_ms())
            .await
            .unwrap()
            .total,
        0
    );
}

#[tokio::test]
async fn a_stale_claim_lists_until_it_is_released() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), "one\n").unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("attention"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("writer"))
        .await
        .unwrap();
    let claims = ClaimService::new(service.pool().clone(), tmp.path());
    claims
        .acquire(&agent.id, &["notes.txt".to_string()], "work")
        .await
        .unwrap();

    let fresh = attention(service.pool(), rigel_core::migrate::now_ms())
        .await
        .unwrap();
    assert_eq!(
        fresh.total, 0,
        "a fresh heartbeat is not attention: {fresh:?}"
    );

    let old = rigel_core::migrate::now_ms() - 120_000;
    sqlx::query("UPDATE claims SET heartbeat_at = ? WHERE agent_id = ?")
        .bind(old)
        .bind(&agent.id)
        .execute(service.pool())
        .await
        .unwrap();

    let stale = attention(service.pool(), rigel_core::migrate::now_ms())
        .await
        .unwrap();
    assert_eq!(stale.total, 1, "{stale:?}");
    assert_eq!(stale.groups[0].items[0].kind, "stale_claim");
    assert!(stale.groups[0].items[0].label.contains("notes.txt"));

    claims
        .release(&agent.id, &["notes.txt".to_string()], "done")
        .await
        .unwrap();
    assert_eq!(
        attention(service.pool(), rigel_core::migrate::now_ms())
            .await
            .unwrap()
            .total,
        0
    );
}

#[tokio::test]
async fn reviews_wait_until_merged_and_a_block_is_its_own_kind() {
    let tmp = tempfile::tempdir().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("attention"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("writer"))
        .await
        .unwrap();
    let reviews = MergeReviews::new(service.pool().clone());
    let files = vec!["notes.txt".to_string()];

    let blocked = reviews
        .record(ReviewInput {
            session_id: &session.id,
            agent_id: &agent.id,
            reviewer_model: "test-reviewer",
            verdict: Verdict::Blocked,
            reason: Some("needs a test"),
            diff: "--- a/notes.txt\n+++ b/notes.txt\n",
            files: &files,
            override_note: None,
        })
        .await
        .unwrap();
    let approved = reviews
        .record(ReviewInput {
            session_id: &session.id,
            agent_id: &agent.id,
            reviewer_model: "test-reviewer",
            verdict: Verdict::Approved,
            reason: None,
            diff: "--- a/notes.txt\n+++ b/notes.txt\n",
            files: &files,
            override_note: None,
        })
        .await
        .unwrap();

    let open = attention(service.pool(), rigel_core::migrate::now_ms())
        .await
        .unwrap();
    assert_eq!(open.total, 2, "{open:?}");
    assert!(
        open.groups[0]
            .items
            .iter()
            .any(|item| item.kind == "blocked_merge")
    );
    assert!(
        open.groups[0]
            .items
            .iter()
            .any(|item| item.kind == "merge_review")
    );

    // Merging one clears exactly that row; the blocked review still needs a decision.
    reviews.mark_merged(&approved.id).await.unwrap();
    let after = attention(service.pool(), rigel_core::migrate::now_ms())
        .await
        .unwrap();
    assert_eq!(after.total, 1);
    assert_eq!(
        after.groups[0].items[0].kind, "blocked_merge",
        "the remaining row is the block, and the merged one is gone: {after:?}"
    );
    assert_ne!(after.groups[0].items[0].at, blocked.created_at - 1);
}

#[test]
fn approvals_lead_the_view_and_keep_the_turn_that_asked() {
    let here = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let known = PendingApproval::now("command", "command:cargo test", "cargo test")
        .in_project(here.path())
        .in_session("session-1", "agent-1")
        .on_turn("turn-1");
    let legacy = PendingApproval::now("mail", "mail:a@b:c", "Send email to a@b");
    let other = PendingApproval::now("command", "command:ls", "ls")
        .in_project(elsewhere.path())
        .in_session("session-2", "agent-2");

    let view = Attention {
        groups: Vec::new(),
        total: 0,
    }
    .with_approvals(&[known, other, legacy], here.path());
    assert_eq!(view.total, 2);
    assert_eq!(view.groups.len(), 1);
    assert_eq!(view.groups[0].items.len(), 2);
    assert_eq!(view.groups[0].items[0].label, "cargo test");
    assert_eq!(view.groups[0].items[1].label, "Send email to a@b");
    let group = &view.groups[0];
    assert_eq!(group.title, "Waiting on you");
    assert!(group.session_id.is_empty());

    let first = &group.items[0];
    assert_eq!(first.kind, "approval");
    assert_eq!(first.session_id, "session-1");
    assert_eq!(first.agent_id.as_deref(), Some("agent-1"));
    assert_eq!(first.turn_id.as_deref(), Some("turn-1"));
    assert_eq!(first.detail, "command · waiting for an answer");

    // A surface that recorded no turn still opens its session rather than guessing one.
    let second = &group.items[1];
    assert!(second.session_id.is_empty());
    assert_eq!(second.turn_id, None);

    let none = Attention {
        groups: Vec::new(),
        total: 0,
    }
    .with_approvals(&[], here.path());
    assert_eq!(none.total, 0);
    assert!(none.groups.is_empty());
}

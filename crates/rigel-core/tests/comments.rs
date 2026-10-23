//! D88: a comment attaches to one hunk, names that hunk's author, and is delivered to them as
//! a turn. The patch itself never changes.

use rigel_core::{HunkComments, PatchPipeline, SessionService};

fn lines(count: usize) -> String {
    (1..=count).map(|index| format!("line {index}\n")).collect()
}

#[tokio::test]
async fn a_comment_names_the_author_and_stays_pending_until_it_is_delivered() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), lines(20)).unwrap();

    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("comments"), None).await.unwrap();
    let writer = service
        .register_agent(&session.id, "native", Some("writer"))
        .await
        .unwrap();
    let pipeline = PatchPipeline::new(tmp.path(), service.pool().clone());

    let (text, base) = pipeline.read("notes.txt").unwrap();
    let edited = text.replace("line 2\n", "line two, by the writer\n");
    let patch = pipeline
        .propose(&session.id, &writer.id, "notes.txt", &base, &edited)
        .await
        .unwrap();
    pipeline.apply(&patch.id).await.unwrap();

    let hunks = pipeline.attributed_hunks(&session.id).await.unwrap();
    let hunk = hunks.first().expect("the patch has a hunk");

    let comments = HunkComments::new(service.pool().clone());
    let comment = comments
        .add(&hunk.id, "  why this line?  ", "user")
        .await
        .unwrap();

    assert_eq!(
        comment.agent_id, writer.id,
        "addressed to the hunk's author"
    );
    assert_eq!(comment.patch_id, patch.id);
    assert_eq!(comment.path, "notes.txt");
    assert_eq!(comment.body, "why this line?", "the body is trimmed");
    assert!(comment.pending());
    assert!(
        comment.prompt().contains(&comment.header),
        "the prompt quotes the header so the agent knows which lines: {}",
        comment.prompt()
    );

    let pending = comments.pending_for(&writer.id).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert!(
        comments
            .pending_for("somebody-else")
            .await
            .unwrap()
            .is_empty()
    );

    let delivered = comments.deliver(&comment.id, "turn-1").await.unwrap();
    assert_eq!(delivered.delivered_turn_id.as_deref(), Some("turn-1"));
    assert!(delivered.delivered_at.is_some());
    assert!(comments.pending_for(&writer.id).await.unwrap().is_empty());

    // A second delivery keeps the first turn: the id is the evidence, not a counter.
    let again = comments.deliver(&comment.id, "turn-2").await.unwrap();
    assert_eq!(again.delivered_turn_id.as_deref(), Some("turn-1"));

    // Commenting is read-only: the patch still says applied and its diff is untouched.
    let after = pipeline.get(&patch.id).await.unwrap().expect("still there");
    assert_eq!(after.status, patch.status);
    assert_eq!(after.diff, patch.diff);

    let listed = comments.for_session(&session.id).await.unwrap();
    assert_eq!(listed.len(), 1, "the Diff tab reads a session at a time");
    assert_eq!(listed[0].id, comment.id);
}

#[tokio::test]
async fn an_empty_comment_or_an_unknown_hunk_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let comments = HunkComments::new(service.pool().clone());

    let error = comments
        .add("no-such-hunk", "hello", "user")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no hunk"), "{error}");

    let error = comments
        .add("no-such-hunk", "   \n  ", "user")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("needs a sentence"), "{error}");
}

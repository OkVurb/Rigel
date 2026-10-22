//! Per-hunk attribution (P5.6): the merged diff can name an agent for every block, not one per
//! file. The rows are written with the patch and survive a revert, because attribution is
//! history rather than ownership.

use rigel_core::patches::parse_diff_hunks;
use rigel_core::{PatchPipeline, SessionService};

fn lines(count: usize) -> String {
    (1..=count).map(|index| format!("line {index}\n")).collect()
}

#[test]
fn headers_parse_with_and_without_counts() {
    let parsed = parse_diff_hunks("@@ -1 +1 @@\n-a\n+b\n@@ -10,3 +12,4 @@ context\n");
    assert_eq!(parsed.len(), 2);
    assert_eq!(
        (parsed[0].old_start, parsed[0].old_lines),
        (1, 1),
        "a missing count means one line"
    );
    assert_eq!((parsed[1].old_start, parsed[1].old_lines), (10, 3));
    assert_eq!((parsed[1].new_start, parsed[1].new_lines), (12, 4));
    assert_eq!(parsed[1].header, "@@ -10,3 +12,4 @@ context");
}

#[tokio::test]
async fn two_agents_leave_two_attributed_hunks_in_one_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), lines(20)).unwrap();

    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("hunks"), None).await.unwrap();
    let first = service
        .register_agent(&session.id, "native", Some("writer"))
        .await
        .unwrap();
    let second = service
        .register_agent(&session.id, "native", Some("reviewer"))
        .await
        .unwrap();
    let pipeline = PatchPipeline::new(tmp.path(), service.pool().clone());

    let (text, base) = pipeline.read("notes.txt").unwrap();
    let edited = text.replace("line 2\n", "line two, by the writer\n");
    let one = pipeline
        .propose(&session.id, &first.id, "notes.txt", &base, &edited)
        .await
        .unwrap();
    pipeline.apply(&one.id).await.unwrap();

    let (text, base) = pipeline.read("notes.txt").unwrap();
    let edited = text.replace("line 18\n", "line eighteen, by the reviewer\n");
    let two = pipeline
        .propose(&session.id, &second.id, "notes.txt", &base, &edited)
        .await
        .unwrap();
    pipeline.apply(&two.id).await.unwrap();

    let hunks = pipeline.attributed_hunks(&session.id).await.unwrap();
    assert_eq!(hunks.len(), 2, "{hunks:?}");
    assert_eq!(hunks[0].agent_id, first.id);
    assert_eq!(hunks[0].path, "notes.txt");
    assert_eq!(hunks[0].status, "applied");
    assert_eq!(hunks[1].agent_id, second.id);
    assert!(hunks[1].old_start > hunks[0].old_start);
}

#[tokio::test]
async fn one_agent_editing_twice_produces_two_ordered_rows() {
    // `unified_diff` emits one block per patch (see the handoff: a single patch with two
    // distant edits is still one block). Two patches from one agent is therefore the case that
    // produces several rows, and they must keep the order they were applied in.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), lines(40)).unwrap();

    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("multi"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", None)
        .await
        .unwrap();
    let pipeline = PatchPipeline::new(tmp.path(), service.pool().clone());

    let (text, base) = pipeline.read("notes.txt").unwrap();
    let first = pipeline
        .propose(
            &session.id,
            &agent.id,
            "notes.txt",
            &base,
            &text.replace(
                "line 2
",
                "line two
",
            ),
        )
        .await
        .unwrap();
    pipeline.apply(&first.id).await.unwrap();

    let (text, base) = pipeline.read("notes.txt").unwrap();
    let second = pipeline
        .propose(
            &session.id,
            &agent.id,
            "notes.txt",
            &base,
            &text.replace(
                "line 38
",
                "line thirty-eight
",
            ),
        )
        .await
        .unwrap();
    pipeline.apply(&second.id).await.unwrap();

    let hunks = pipeline.attributed_hunks(&session.id).await.unwrap();
    assert_eq!(hunks.len(), 2, "{hunks:?}");
    assert_eq!(hunks[0].patch_id, first.id);
    assert_eq!(hunks[1].patch_id, second.id);
    assert_eq!((hunks[0].ordinal, hunks[1].ordinal), (0, 0));
    assert!(hunks[1].old_start > hunks[0].old_start);
    assert!(hunks.iter().all(|hunk| hunk.agent_id == agent.id));
}

#[tokio::test]
async fn a_reverted_patch_keeps_its_hunks_and_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), "one\ntwo\n").unwrap();

    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("revert"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", None)
        .await
        .unwrap();
    let pipeline = PatchPipeline::new(tmp.path(), service.pool().clone());
    let (text, base) = pipeline.read("notes.txt").unwrap();
    let patch = pipeline
        .propose(
            &session.id,
            &agent.id,
            "notes.txt",
            &base,
            &text.replace("two", "three"),
        )
        .await
        .unwrap();
    pipeline.apply(&patch.id).await.unwrap();
    pipeline.revert(&patch.id).await.unwrap();

    let hunks = pipeline.attributed_hunks(&session.id).await.unwrap();
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].status, "reverted");
    assert_eq!(hunks[0].agent_id, agent.id);
}

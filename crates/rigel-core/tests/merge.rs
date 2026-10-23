//! The conflict view's three-way merge (D79).
//!
//! What is asserted here is the promise the Diff view makes: the three versions are captured with
//! the hashes the ledger recorded, the merge is computed before anything is written, and a
//! resolution writes the merged file through the same compare-and-swap path as an agent patch.

use rigel_core::{
    ClaimError, ClaimService, PatchError, PatchPipeline, SessionService, MERGE_AUTHOR,
};

const BASE: &str = "one\ntwo\nthree\nfour\nfive\n";

struct Fixture {
    tmp: tempfile::TempDir,
    claims: ClaimService,
    pipeline: PatchPipeline,
    session: String,
    claimant: String,
    writer: String,
}

impl Fixture {
    /// A project with one claimed file and two agents: the claimant, which writes first, and the
    /// writer, which writes over it.
    async fn new(path: &str) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(path), BASE).unwrap();
        let sessions = SessionService::open(tmp.path()).await.unwrap();
        let session = sessions.start(Some("merge"), None).await.unwrap();
        let claimant = sessions
            .register_agent(&session.id, "native", Some("worker"))
            .await
            .unwrap();
        let writer = sessions
            .register_agent(&session.id, "native", Some("reviewer"))
            .await
            .unwrap();
        let claims = ClaimService::new(sessions.pool().clone(), tmp.path());
        let pipeline = PatchPipeline::new(tmp.path(), sessions.pool().clone());
        claims
            .acquire(&claimant.id, &[path.to_string()], "edit it")
            .await
            .unwrap();
        Self {
            tmp,
            claims,
            pipeline,
            session: session.id,
            claimant: claimant.id,
            writer: writer.id,
        }
    }

    fn path(&self, path: &str) -> std::path::PathBuf {
        self.tmp.path().join(path)
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.path(path)).unwrap()
    }

    fn hash(&self, text: &str) -> String {
        rigel_core::patches::sha256_hex(text.as_bytes())
    }

    async fn write_as(&self, agent: &str, path: &str, text: &str) -> String {
        let (_, base) = self.pipeline.read(path).unwrap();
        self.pipeline
            .propose(&self.session, agent, path, &base, text)
            .await
            .unwrap();
        self.hash(text)
    }

    /// Replace the file without the patch pipeline: the writer an adapter or an editor would be.
    fn write_raw(&self, path: &str, text: &str) -> String {
        std::fs::write(self.path(path), text).unwrap();
        self.hash(text)
    }
}

#[tokio::test]
async fn the_three_sides_are_captured_with_the_recorded_hashes() {
    let path = "notes.txt";
    let fixture = Fixture::new(path).await;
    let base_hash = fixture.hash(BASE);

    let ours_text = BASE.replace("two\n", "TWO\n");
    let ours_hash = fixture.write_as(&fixture.claimant, path, &ours_text).await;
    let theirs_text = ours_text.replace("four\n", "FOUR\n");
    let theirs_hash = fixture.write_as(&fixture.writer, path, &theirs_text).await;

    let conflict = fixture
        .claims
        .record_conflict(
            path,
            Some(&fixture.claimant),
            Some(&fixture.writer),
            Some(base_hash.clone()),
            Some(theirs_hash.clone()),
        )
        .await
        .unwrap();

    let sides = fixture.claims.sides(&conflict.id).await.unwrap();
    assert_eq!(
        sides.base.as_ref().map(|side| side.hash.clone()),
        Some(base_hash)
    );
    assert_eq!(
        sides.base.as_ref().map(|side| side.text.clone()),
        Some(BASE.to_string())
    );
    assert_eq!(
        sides.ours.as_ref().map(|side| side.hash.clone()),
        Some(ours_hash)
    );
    assert_eq!(
        sides.ours.as_ref().map(|side| side.text.clone()),
        Some(ours_text)
    );
    assert_eq!(
        sides.theirs.as_ref().map(|side| side.hash.clone()),
        Some(theirs_hash)
    );
    assert_eq!(
        sides.theirs.as_ref().map(|side| side.text.clone()),
        Some(theirs_text)
    );
    assert_eq!(sides.note, None, "every side was rebuilt from the journal");
}

#[tokio::test]
async fn a_write_outside_the_pipeline_is_merged_with_the_claimants_version() {
    let path = "notes.txt";
    let fixture = Fixture::new(path).await;
    let base_hash = fixture.hash(BASE);
    let ours_text = BASE.replace("two\n", "TWO\n");
    fixture.write_as(&fixture.claimant, path, &ours_text).await;

    // An editor or an adapter writes over the claimant's work without a patch row. Its content
    // is based on the claim, so the claimant's edit is the part that would be lost.
    let theirs_text = format!("{BASE}appended by the writer\n");
    let theirs_hash = fixture.write_raw(path, &theirs_text);

    let conflict = fixture
        .claims
        .record_conflict(
            path,
            Some(&fixture.claimant),
            None,
            Some(base_hash),
            Some(theirs_hash),
        )
        .await
        .unwrap();

    let sides = fixture.claims.sides(&conflict.id).await.unwrap();
    assert_eq!(
        sides.base.as_ref().map(|side| side.text.clone()),
        Some(BASE.to_string()),
        "the claim captured the version it started from"
    );
    assert_eq!(
        sides.ours.as_ref().map(|side| side.text.clone()),
        Some(ours_text.clone()),
        "ours is the claimant's patch, not the file"
    );
    assert_eq!(
        sides.theirs.as_ref().map(|side| side.text.clone()),
        Some(theirs_text.clone())
    );
    assert_eq!(sides.note, None);

    let merged = fixture.claims.merge(&conflict.id).await.unwrap();
    assert!(merged.clean, "{merged:?}");
    assert_eq!(merged.conflicts, 0);
    assert_eq!(
        merged.merged, "one\nTWO\nthree\nfour\nfive\nappended by the writer\n",
        "both sides survive the merge"
    );

    let resolution = fixture
        .claims
        .resolve_with_merge(
            &fixture.pipeline,
            &fixture.session,
            &conflict.id,
            &merged.merged,
        )
        .await
        .unwrap();
    assert!(!resolution.unchanged);
    let patch = resolution
        .patch
        .expect("the merged text differs from the file");
    assert_eq!(patch.status, "applied");
    assert_eq!(patch.agent_id, MERGE_AUTHOR, "a merge is the human's write");
    assert_eq!(patch.path, path);
    assert_eq!(fixture.read(path), merged.merged);

    let sides = fixture.claims.sides(&conflict.id).await.unwrap();
    assert_eq!(sides.merged_patch_id.as_deref(), Some(patch.id.as_str()));
    assert_eq!(
        sides.merged_hash.as_deref(),
        Some(patch.result_hash.as_str())
    );

    let row = fixture.claims.conflict(&conflict.id).await.unwrap();
    assert!(row.resolved_at.is_some());
    assert_eq!(row.resolution.as_deref(), Some("merged the three versions"));

    // The patch is in the session's ledger, attributed to `user`; the Diff view suppresses the
    // comment box for it because there is no agent to send a comment to.
    let hunks = fixture
        .pipeline
        .attributed_hunks(&fixture.session)
        .await
        .unwrap();
    assert!(
        hunks
            .iter()
            .any(|hunk| hunk.patch_id == patch.id && hunk.agent_id == MERGE_AUTHOR),
        "{hunks:?}"
    );

    // Resolving again is refused rather than rewriting the file.
    assert!(matches!(
        fixture
            .claims
            .resolve_with_merge(&fixture.pipeline, &fixture.session, &conflict.id, "again\n")
            .await,
        Err(ClaimError::ConflictClosed(_))
    ));
}

#[tokio::test]
async fn a_linear_writer_that_built_on_ours_needs_no_write() {
    let path = "notes.txt";
    let fixture = Fixture::new(path).await;
    let base_hash = fixture.hash(BASE);
    let ours_text = BASE.replace("two\n", "TWO\n");
    fixture.write_as(&fixture.claimant, path, &ours_text).await;
    let theirs_text = ours_text.replace("four\n", "FOUR\n");
    let theirs_hash = fixture.write_as(&fixture.writer, path, &theirs_text).await;

    let conflict = fixture
        .claims
        .record_conflict(
            path,
            Some(&fixture.claimant),
            Some(&fixture.writer),
            Some(base_hash),
            Some(theirs_hash),
        )
        .await
        .unwrap();

    let merged = fixture.claims.merge(&conflict.id).await.unwrap();
    assert!(merged.clean, "{merged:?}");
    assert_eq!(merged.merged, theirs_text, "the writer built on our change");

    let resolution = fixture
        .claims
        .resolve_with_merge(
            &fixture.pipeline,
            &fixture.session,
            &conflict.id,
            &merged.merged,
        )
        .await
        .unwrap();
    assert!(resolution.unchanged, "nothing to write");
    assert!(resolution.patch.is_none());
    assert_eq!(fixture.read(path), theirs_text);
    let row = fixture.claims.conflict(&conflict.id).await.unwrap();
    assert!(row.resolved_at.is_some());
}

#[tokio::test]
async fn edits_to_the_same_line_are_written_with_their_markers() {
    let path = "notes.txt";
    let fixture = Fixture::new(path).await;
    let base_hash = fixture.hash(BASE);
    let ours_text = BASE.replace("three\n", "THREE from ours\n");
    fixture.write_as(&fixture.claimant, path, &ours_text).await;
    let theirs_text = ours_text.replace("THREE from ours\n", "THREE from theirs\n");
    let theirs_hash = fixture.write_as(&fixture.writer, path, &theirs_text).await;

    let conflict = fixture
        .claims
        .record_conflict(
            path,
            Some(&fixture.claimant),
            Some(&fixture.writer),
            Some(base_hash),
            Some(theirs_hash),
        )
        .await
        .unwrap();

    let merged = fixture.claims.merge(&conflict.id).await.unwrap();
    assert_eq!(merged.conflicts, 1);
    assert!(!merged.clean);
    assert!(merged.merged.contains("<<<<<<< ours"));
    assert!(merged.merged.contains("THREE from ours"));
    assert!(merged.merged.contains("======="));
    assert!(merged.merged.contains("THREE from theirs"));
    assert!(merged.merged.contains(">>>>>>> theirs"));

    // Writing the markers is a real choice: the file then says what is unresolved, and the patch
    // is the record of who left it that way.
    fixture
        .claims
        .resolve_with_merge(
            &fixture.pipeline,
            &fixture.session,
            &conflict.id,
            &merged.merged,
        )
        .await
        .unwrap();
    assert_eq!(fixture.read(path), merged.merged);
}

#[tokio::test]
async fn a_claim_without_captured_text_falls_back_to_the_ledger() {
    let path = "notes.txt";
    let fixture = Fixture::new(path).await;
    let base_hash = fixture.hash(BASE);

    // A claim recorded before 0010 has no base text. Its conflict can still be resolved, but
    // there is nothing to merge from when the write did not come through the journal.
    sqlx::query("UPDATE claims SET base_text = NULL")
        .execute(fixture.claims.pool())
        .await
        .unwrap();
    let observed = "written by something else\n";
    let observed_hash = fixture.write_raw(path, observed);

    let conflict = fixture
        .claims
        .record_conflict(
            path,
            Some(&fixture.claimant),
            None,
            Some(base_hash),
            Some(observed_hash),
        )
        .await
        .unwrap();

    let sides = fixture.claims.sides(&conflict.id).await.unwrap();
    assert_eq!(
        sides.theirs.as_ref().map(|side| side.text.clone()),
        Some(observed.to_string())
    );
    assert!(sides.ours.is_none());
    assert!(sides.base.is_none());
    let note = sides.note.unwrap_or_default();
    assert!(note.contains("not in the patch journal"), "{note:?}");

    assert!(matches!(
        fixture.claims.merge(&conflict.id).await,
        Err(ClaimError::SidesUnavailable(_))
    ));
}

#[tokio::test]
async fn a_write_after_the_capture_is_refused_instead_of_overwritten() {
    let path = "notes.txt";
    let fixture = Fixture::new(path).await;
    let base_hash = fixture.hash(BASE);
    let ours_text = BASE.replace("two\n", "TWO\n");
    fixture.write_as(&fixture.claimant, path, &ours_text).await;
    let theirs_text = ours_text.replace("four\n", "FOUR\n");
    let theirs_hash = fixture.write_as(&fixture.writer, path, &theirs_text).await;

    let conflict = fixture
        .claims
        .record_conflict(
            path,
            Some(&fixture.claimant),
            Some(&fixture.writer),
            Some(base_hash),
            Some(theirs_hash),
        )
        .await
        .unwrap();
    let merged = fixture.claims.merge(&conflict.id).await.unwrap();

    // A third writer lands between the merge preview and the write.
    let after = "one\nTWO\nthree\nFOUR\nfive\nsix\n";
    std::fs::write(fixture.path(path), after).unwrap();

    let outcome = fixture
        .claims
        .resolve_with_merge(
            &fixture.pipeline,
            &fixture.session,
            &conflict.id,
            &merged.merged,
        )
        .await;
    assert!(
        matches!(
            outcome,
            Err(ClaimError::Patch(PatchError::StaleBase { .. }))
        ),
        "{outcome:?}"
    );
    assert_eq!(fixture.read(path), after, "nothing was overwritten");
    let row = fixture.claims.conflict(&conflict.id).await.unwrap();
    assert!(
        row.resolved_at.is_none(),
        "a refused write leaves the row open"
    );
}

#[tokio::test]
async fn a_claimant_that_never_wrote_has_base_equal_to_ours() {
    let path = "notes.txt";
    let fixture = Fixture::new(path).await;
    let base_hash = fixture.hash(BASE);

    // Only the writer edits; the claimant only claimed.
    let theirs_text = BASE.replace("five\n", "FIVE\n");
    let theirs_hash = fixture.write_as(&fixture.writer, path, &theirs_text).await;
    let conflict = fixture
        .claims
        .record_conflict(
            path,
            Some(&fixture.claimant),
            Some(&fixture.writer),
            Some(base_hash),
            Some(theirs_hash),
        )
        .await
        .unwrap();

    let sides = fixture.claims.sides(&conflict.id).await.unwrap();
    assert_eq!(
        sides.base.as_ref().map(|side| side.text.clone()),
        Some(BASE.to_string())
    );
    assert_eq!(
        sides.ours.as_ref().map(|side| side.text.clone()),
        Some(BASE.to_string())
    );

    let merged = fixture.claims.merge(&conflict.id).await.unwrap();
    assert!(merged.clean);
    assert_eq!(merged.merged, theirs_text);

    // The merge is what the file already holds: no patch, but the row is answered.
    let resolution = fixture
        .claims
        .resolve_with_merge(
            &fixture.pipeline,
            &fixture.session,
            &conflict.id,
            &merged.merged,
        )
        .await
        .unwrap();
    assert!(resolution.unchanged);
    assert!(resolution.patch.is_none());
    let row = fixture.claims.conflict(&conflict.id).await.unwrap();
    assert!(row.resolved_at.is_some());
}

#[tokio::test]
async fn a_claim_collision_without_an_observed_hash_still_renders_the_file() {
    let path = "notes.txt";
    let fixture = Fixture::new(path).await;
    let base_hash = fixture.hash(BASE);

    // `acquire` records a collision with no observed hash: nobody wrote, two agents wanted the
    // file. The three versions are still the file and the claim, not an empty pane.
    let conflict = fixture
        .claims
        .record_conflict(
            path,
            Some(&fixture.claimant),
            Some(&fixture.writer),
            Some(base_hash),
            None,
        )
        .await
        .unwrap();

    let sides = fixture.claims.sides(&conflict.id).await.unwrap();
    assert_eq!(
        sides.theirs.as_ref().map(|side| side.text.clone()),
        Some(BASE.to_string())
    );
    assert_eq!(
        sides.base.as_ref().map(|side| side.text.clone()),
        Some(BASE.to_string())
    );
    assert_eq!(
        sides.ours.as_ref().map(|side| side.text.clone()),
        Some(BASE.to_string())
    );
    assert!(
        sides
            .note
            .unwrap_or_default()
            .contains("no observed version"),
        "the note explains why the row is not a lost write"
    );
}

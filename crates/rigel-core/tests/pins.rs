//! D141: a moved pin keeps both bodies and the decision is recorded on the session.
//!
//! The second test is the desktop start path without a webview: the template's skill list is
//! applied through the same core pin call the window uses, against a real index and blob store.

use std::path::Path;

use rigel_core::skills::{FetchedSkill, RepoSnapshot, SkillRecord};
use rigel_core::{SessionService, SessionTemplates, SkillIndex, TemplateSkill};

const SKILL_ID: &str = "github:acme/skills:pdf/SKILL.md";

const OLD_BODY: &str = "---\nname: pdf-reader\ndescription: Read scanned PDFs.\n---\n# PDF reader\n\nAsk for the file, then summarise it.\n";
const NEW_BODY: &str = "---\nname: pdf-reader\ndescription: Read scanned PDFs.\n---\n# PDF reader\n\nAsk for the file, wait for the scan, then summarise it.\n";

fn snapshot_for(id: &str, name: &str, body: &str) -> RepoSnapshot {
    RepoSnapshot {
        source: "github".to_string(),
        repo: "acme/skills".to_string(),
        branch: "main".to_string(),
        stars: 0,
        license: None,
        truncated: false,
        found: 1,
        http_requests: 0,
        skills: vec![FetchedSkill {
            record: SkillRecord {
                id: id.to_string(),
                name: name.to_string(),
                description: format!("{name} instructions."),
                source: "github".to_string(),
                repo: Some("acme/skills".to_string()),
                path: Some(format!("{name}/SKILL.md")),
                url: None,
                body_sha: None,
                grade: None,
                grade_scope: None,
                license: None,
                stars: None,
                installs: None,
                tokens: None,
                capabilities: Vec::new(),
                simhash: None,
            },
            body: body.to_string(),
        }],
    }
}

fn snapshot(body: &str) -> RepoSnapshot {
    snapshot_for(SKILL_ID, "pdf-reader", body)
}

async fn project_at(path: &Path) -> SessionService {
    std::fs::create_dir_all(path).unwrap();
    SessionService::open(path).await.unwrap()
}

#[tokio::test]
async fn a_changed_pin_shows_both_bodies_and_keeps_or_updates_with_a_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let index = SkillIndex::open(&tmp.path().join("index")).await.unwrap();
    index.ingest(&snapshot(OLD_BODY)).await.unwrap();
    let (old_body, old_sha) = index.body(SKILL_ID).await.unwrap().unwrap();
    assert_eq!(old_body, OLD_BODY);

    let project = tmp.path().join("project");
    let service = project_at(&project).await;
    let session = service.start(Some("pins"), None).await.unwrap();
    service
        .pin_skill(&session.id, SKILL_ID, &old_sha, false)
        .await
        .unwrap();

    // Re-index with a changed body. The old blob stays on disk, addressed by the pin's hash.
    index.ingest(&snapshot(NEW_BODY)).await.unwrap();
    let (new_body, new_sha) = index.body(SKILL_ID).await.unwrap().unwrap();
    assert_ne!(new_sha, old_sha);
    assert_eq!(new_body, NEW_BODY);

    let listed = rigel_core::pins::list(&service, &index, &session.id)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].changed);
    assert_eq!(listed[0].body_sha, old_sha);
    assert_eq!(
        listed[0].current_body_sha.as_deref(),
        Some(new_sha.as_str())
    );
    assert!(listed[0].pinned_body_available);

    let review = rigel_core::pins::review(&service, &index, &session.id, SKILL_ID)
        .await
        .unwrap();
    assert_eq!(review.pinned_body.as_deref(), Some(OLD_BODY));
    assert_eq!(review.current_body.as_deref(), Some(NEW_BODY));
    assert!(review.pin.changed);
    assert!(
        index.body_at("../../outside").unwrap().is_none(),
        "a template-supplied hash is not a path"
    );

    // Keep pin is a recorded intervention, and it does not move the hash.
    let kept = rigel_core::pins::keep(
        &service,
        &index,
        &session.id,
        SKILL_ID,
        "the old wording is the one this session tested",
    )
    .await
    .unwrap();
    assert!(kept.changed);
    assert_eq!(
        service.skill_pin(&session.id, SKILL_ID).await.unwrap(),
        Some(old_sha.clone())
    );
    let after_keep = service.get(&session.id).await.unwrap();
    assert_eq!(after_keep.interventions, 1);

    // Update resolves the current body in the core and records the reason there too.
    let updated = rigel_core::pins::update(
        &service,
        &index,
        &session.id,
        SKILL_ID,
        "the scan wait is part of the new instructions",
    )
    .await
    .unwrap();
    assert!(!updated.changed);
    assert_eq!(updated.body_sha, new_sha);
    assert_eq!(
        service.skill_pin(&session.id, SKILL_ID).await.unwrap(),
        Some(new_sha)
    );
    let after_update = service.get(&session.id).await.unwrap();
    assert_eq!(after_update.interventions, 2);

    let interventions: Vec<(String, String)> = sqlx::query_as(
        "SELECT kind, detail_json FROM interventions WHERE session_id = ? ORDER BY at, rowid",
    )
    .bind(&session.id)
    .fetch_all(service.pool())
    .await
    .unwrap();
    assert_eq!(interventions.len(), 2);
    assert!(interventions.iter().all(|(kind, _)| kind == "skill_pin"));
    assert!(interventions[0].1.contains("keep"));
    assert!(interventions[1].1.contains("update"));
    assert!(interventions[1].1.contains("scan wait"));
}

#[tokio::test]
async fn a_batch_update_moves_every_changed_pin_and_records_one_reason() {
    const SECOND_ID: &str = "github:acme/skills:review/SKILL.md";
    const SECOND_OLD: &str =
        "---\nname: reviewer\ndescription: Review a patch.\n---\n# Reviewer\n\nRead the diff.\n";
    const SECOND_NEW: &str = "---\nname: reviewer\ndescription: Review a patch.\n---\n# Reviewer\n\nRead the diff and the test output.\n";

    let tmp = tempfile::tempdir().unwrap();
    let index = SkillIndex::open(&tmp.path().join("index")).await.unwrap();
    index
        .ingest(&snapshot_for(SKILL_ID, "pdf-reader", OLD_BODY))
        .await
        .unwrap();
    index
        .ingest(&snapshot_for(SECOND_ID, "reviewer", SECOND_OLD))
        .await
        .unwrap();
    let (_, first_old) = index.body(SKILL_ID).await.unwrap().unwrap();
    let (_, second_old) = index.body(SECOND_ID).await.unwrap().unwrap();

    let project = tmp.path().join("project");
    let service = project_at(&project).await;
    let session = service.start(Some("batch"), None).await.unwrap();
    service
        .pin_skill(&session.id, SKILL_ID, &first_old, false)
        .await
        .unwrap();
    service
        .pin_skill(&session.id, SECOND_ID, &second_old, false)
        .await
        .unwrap();

    index
        .ingest(&snapshot_for(SKILL_ID, "pdf-reader", NEW_BODY))
        .await
        .unwrap();
    index
        .ingest(&snapshot_for(SECOND_ID, "reviewer", SECOND_NEW))
        .await
        .unwrap();
    let (_, first_new) = index.body(SKILL_ID).await.unwrap().unwrap();
    let (_, second_new) = index.body(SECOND_ID).await.unwrap().unwrap();

    let updated = rigel_core::pins::update_all(
        &service,
        &index,
        &session.id,
        "both instructions changed after review",
    )
    .await
    .unwrap();
    assert_eq!(updated.len(), 2);
    assert!(updated.iter().all(|pin| !pin.changed));
    assert_eq!(
        service.skill_pin(&session.id, SKILL_ID).await.unwrap(),
        Some(first_new)
    );
    assert_eq!(
        service.skill_pin(&session.id, SECOND_ID).await.unwrap(),
        Some(second_new)
    );

    let after = service.get(&session.id).await.unwrap();
    assert_eq!(after.interventions, 1, "one decision, not one per pin");
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT kind, detail_json FROM interventions WHERE session_id = ?")
            .bind(&session.id)
            .fetch_all(service.pool())
            .await
            .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "skill_pin");
    assert!(rows[0].1.contains("update_all"));
    assert!(rows[0].1.contains("both instructions changed after review"));
    assert!(rows[0].1.contains(SECOND_ID));
}

#[tokio::test]
async fn a_batch_keep_freezes_every_changed_pin_and_records_one_reason() {
    const SECOND_ID: &str = "github:acme/skills:review/SKILL.md";
    const SECOND_OLD: &str =
        "---\nname: reviewer\ndescription: Review a patch.\n---\n# Reviewer\n\nRead the diff.\n";
    const SECOND_NEW: &str = "---\nname: reviewer\ndescription: Review a patch.\n---\n# Reviewer\n\nRead the diff and the test output.\n";

    let tmp = tempfile::tempdir().unwrap();
    let index = SkillIndex::open(&tmp.path().join("index")).await.unwrap();
    index
        .ingest(&snapshot_for(SKILL_ID, "pdf-reader", OLD_BODY))
        .await
        .unwrap();
    index
        .ingest(&snapshot_for(SECOND_ID, "reviewer", SECOND_OLD))
        .await
        .unwrap();
    let (_, first_old) = index.body(SKILL_ID).await.unwrap().unwrap();
    let (_, second_old) = index.body(SECOND_ID).await.unwrap().unwrap();

    let project = tmp.path().join("project");
    let service = project_at(&project).await;
    let session = service.start(Some("batch keep"), None).await.unwrap();
    service
        .pin_skill(&session.id, SKILL_ID, &first_old, false)
        .await
        .unwrap();
    service
        .pin_skill(&session.id, SECOND_ID, &second_old, false)
        .await
        .unwrap();

    index
        .ingest(&snapshot_for(SKILL_ID, "pdf-reader", NEW_BODY))
        .await
        .unwrap();
    index
        .ingest(&snapshot_for(SECOND_ID, "reviewer", SECOND_NEW))
        .await
        .unwrap();

    let kept =
        rigel_core::pins::keep_all(&service, &index, &session.id, "keep the reviewed wording")
            .await
            .unwrap();
    assert_eq!(kept.len(), 2);
    assert!(kept.iter().all(|pin| pin.changed));
    assert_eq!(
        service
            .skill_pin(&session.id, SKILL_ID)
            .await
            .unwrap()
            .as_deref(),
        Some(first_old.as_str())
    );
    assert_eq!(
        service
            .skill_pin(&session.id, SECOND_ID)
            .await
            .unwrap()
            .as_deref(),
        Some(second_old.as_str())
    );

    let after = service.get(&session.id).await.unwrap();
    assert_eq!(after.interventions, 1, "one decision, not one per pin");
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT kind, detail_json FROM interventions WHERE session_id = ?")
            .bind(&session.id)
            .fetch_all(service.pool())
            .await
            .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "skill_pin");
    assert!(rows[0].1.contains("keep_all"));
    assert!(rows[0].1.contains("keep the reviewed wording"));
    assert!(rows[0].1.contains(SECOND_ID));
}

#[tokio::test]
async fn a_template_restores_a_real_indexed_pin_at_its_original_body() {
    let tmp = tempfile::tempdir().unwrap();
    let index = SkillIndex::open(&tmp.path().join("index")).await.unwrap();
    index.ingest(&snapshot(OLD_BODY)).await.unwrap();
    let (_, old_sha) = index.body(SKILL_ID).await.unwrap().unwrap();

    let project = tmp.path().join("project");
    let service = project_at(&project).await;
    let original = service.start(Some("original"), None).await.unwrap();
    service
        .pin_skill(&original.id, SKILL_ID, &old_sha, false)
        .await
        .unwrap();

    let templates = SessionTemplates::open(&project);
    let saved = templates
        .save(rigel_core::SessionTemplate {
            id: String::new(),
            name: "PDF review".to_string(),
            model: None,
            effort: None,
            max_steps: None,
            agents: Vec::new(),
            skills: vec![TemplateSkill {
                skill_id: SKILL_ID.to_string(),
                body_sha: old_sha.clone(),
            }],
            prompt: "Read the scan.".to_string(),
            created_at: 0,
        })
        .unwrap();

    // The index moves after the template was saved.
    index.ingest(&snapshot(NEW_BODY)).await.unwrap();

    // This is the start loop from `SessionView`: create, then apply each template skill by its
    // recorded hash. No current hash is substituted.
    let started = service.start(Some(&saved.name), None).await.unwrap();
    for skill in &saved.skills {
        service
            .pin_skill(&started.id, &skill.skill_id, &skill.body_sha, false)
            .await
            .unwrap();
    }

    let restored = service.skill_pins(&started.id).await.unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].body_sha, old_sha);
    let review = rigel_core::pins::review(&service, &index, &started.id, SKILL_ID)
        .await
        .unwrap();
    assert_eq!(review.pinned_body.as_deref(), Some(OLD_BODY));
    assert_eq!(review.current_body.as_deref(), Some(NEW_BODY));
    assert!(review.pin.changed, "the new session kept the older body");
}

//! D152: templates are project-local, atomic, and never guessed at when the file is missing.

use rigel_core::{SessionTemplates, TemplateAgent, TemplateError};

fn template(name: &str) -> rigel_core::SessionTemplate {
    rigel_core::SessionTemplate {
        id: String::new(),
        name: name.to_string(),
        model: Some("anthropic/claude-sonnet-4.5".to_string()),
        effort: Some("medium".to_string()),
        max_steps: Some(12),
        agents: vec![TemplateAgent {
            harness: "native".to_string(),
            role: Some("writer".to_string()),
            model: Some("openai/gpt-5.4-mini".to_string()),
        }],
        prompt: "Fix the flaky resume test.\n".to_string(),
        created_at: 0,
    }
}

#[test]
fn a_template_round_trips_and_survives_a_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionTemplates::open(tmp.path());
    assert!(
        store.list().unwrap().is_empty(),
        "a missing file is an empty list"
    );

    let saved = store.save(template("Windows resume")).unwrap();
    assert!(!saved.id.is_empty());
    assert_eq!(saved.created_at, 0.max(saved.created_at));
    assert_eq!(
        saved.prompt, "Fix the flaky resume test.",
        "the trailing newline is trimmed"
    );

    let reopened = SessionTemplates::open(tmp.path());
    let listed = reopened.list().unwrap();
    assert_eq!(listed, vec![saved.clone()]);
    assert!(reopened.path().ends_with(".rigel/templates.json"));

    // A save with the same id is an edit, not a second row.
    let edited = reopened
        .save(rigel_core::SessionTemplate {
            name: "Windows resume, now with a reviewer".to_string(),
            agents: vec![],
            ..saved.clone()
        })
        .unwrap();
    assert_eq!(reopened.list().unwrap(), vec![edited]);
}

#[test]
fn names_are_one_line_and_a_delete_must_match() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionTemplates::open(tmp.path());

    assert!(matches!(
        store.save(template("   ")),
        Err(TemplateError::InvalidName)
    ));

    let saved = store.save(template("first line\nsecond line")).unwrap();
    assert_eq!(saved.name, "first line");

    assert!(matches!(
        store.delete("missing"),
        Err(TemplateError::Unknown(_))
    ));
    store.delete(&saved.id).unwrap();
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn the_file_is_not_a_second_settings_surface() {
    let tmp = tempfile::tempdir().unwrap();
    let store = SessionTemplates::open(tmp.path());
    for index in 0..rigel_core::MAX_TEMPLATES {
        store.save(template(&format!("template {index}"))).unwrap();
    }
    assert!(matches!(
        store.save(template("one too many")),
        Err(TemplateError::TooMany)
    ));
}

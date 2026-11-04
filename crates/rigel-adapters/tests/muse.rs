//! The Muse adapter against a fake `muse exec --json`.
//!
//! This proves the parts a fixture can prove: the prompt file is written and read, the child gets
//! the Meta key, deltas stream, a tool result becomes a command item, and terminal text is the
//! answer. It is not a live Muse turn.

use std::path::PathBuf;

use rigel_adapters::{MuseAdapter, MuseConfig, MuseEvent};
use rigel_core::ItemKind;
use rigel_core::credentials::Secret;

#[tokio::test]
async fn a_muse_exec_turn_reads_the_prompt_and_the_journal() {
    let mut config = MuseConfig::new(env!("CARGO_MANIFEST_DIR"));
    config.exe = PathBuf::from(env!("CARGO_BIN_EXE_fake-muse"));
    config
        .env
        .insert("META_API_KEY".to_string(), Secret::new("test-key"));
    let adapter = MuseAdapter::new(config);

    let mut deltas = String::new();
    let mut items = Vec::new();
    let turn = adapter
        .run_turn("say pong", |event| match event {
            MuseEvent::Delta { text } => deltas.push_str(&text),
            MuseEvent::Item(item) => items.push(item),
            MuseEvent::Notice { .. } => {}
        })
        .await
        .expect("the fake answers");

    assert_eq!(turn.text, "pong", "terminal text wins over the deltas");
    assert_eq!(
        deltas, "pong",
        "both fragments streamed before the terminal"
    );
    assert_eq!(turn.model, "muse-spark-1.3", "the configured model is kept");
    assert!(!turn.session_id.is_empty());
    assert!(
        items
            .iter()
            .any(|item| item.kind == ItemKind::Command && item.body["command"] == "cargo test"),
        "the tool result is a command item: {items:?}"
    );
}

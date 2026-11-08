//! Roadmap 199: the skill showcase is curated, not a list of untested links.
//!
//! The public shelf page links only to skills in this repository. This test reads the page,
//! resolves every `skills/…/SKILL.md` link under the repository root, and parses it with the same
//! frontmatter parser the shelf uses. A new showcased skill must therefore arrive with a real
//! `name`, a description and a body, or this test fails.

use std::path::Path;

#[test]
fn every_showcased_skill_is_a_repository_skill_with_frontmatter() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let page = std::fs::read_to_string(root.join("web/features/shelf.html"))
        .expect("the shelf showcase page");

    let marker = "github.com/OkVurb/Rigel/blob/main/";
    let mut showcased = 0usize;
    for piece in page.split(marker).skip(1) {
        let path = piece.split('"').next().unwrap_or_default();
        if !path.starts_with("skills/") {
            continue;
        }
        showcased += 1;
        let body = std::fs::read_to_string(root.join(path))
            .unwrap_or_else(|error| panic!("{path} is linked but unreadable: {error}"));
        let parsed = rigel_core::skills::frontmatter::parse(&body);
        assert!(
            parsed.get("name").is_some_and(|name| !name.is_empty()),
            "{path} has no frontmatter name"
        );
        assert!(
            parsed
                .get("description")
                .is_some_and(|description| description.len() >= 40),
            "{path} has no useful frontmatter description"
        );
        assert!(
            parsed.body.contains("## "),
            "{path} has no readable body for the reviewer to test"
        );
    }

    assert!(
        showcased > 0,
        "the showcase page no longer links any repository skill; update this test with the new surface"
    );
}

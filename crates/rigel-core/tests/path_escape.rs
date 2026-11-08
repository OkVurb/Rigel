//! The property roadmap 3 names: a canonical path never escapes the project.
//!
//! `PatchPipeline::resolve` is the gate every agent write goes through, so the fixtures below
//! are the shapes an attacker or a confused model can actually produce: parent segments,
//! absolute paths, UNC paths, drive-absolute paths and an embedded NUL. A valid path in the
//! project must still resolve, or the rejections would prove nothing.

use std::fs;

use rigel_core::PatchPipeline;

#[tokio::test]
async fn every_escape_fixture_is_rejected_and_inside_paths_still_resolve() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("inside.txt"), "ok").unwrap();

    let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap();
    let pipeline = PatchPipeline::new(root, pool);

    assert!(pipeline.resolve("inside.txt").is_ok());
    assert!(pipeline.resolve("./inside.txt").is_ok());
    assert!(pipeline.resolve("sub/new.txt").is_ok());

    let escapes = [
        "../outside.txt",
        "sub/../../outside.txt",
        "sub/../..",
        "/etc/passwd",
        "//server/share/file",
        "C:\\Windows\\win.ini",
        "\\\\server\\share\\file",
        "a\0b",
        "",
    ];
    for fixture in escapes {
        assert!(
            pipeline.resolve(fixture).is_err(),
            "{fixture:?} escaped the project"
        );
    }
}

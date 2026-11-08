//! Item 71: the project jail holds against traversal, links, UNC paths and case variants.
//!
//! `PatchPipeline::resolve` is the single gate every read, write, edit and list goes through, so
//! these cases exercise the production function rather than a copy of its rules. A link that
//! cannot be created on the host is skipped rather than failed: Windows needs a privilege for a
//! file symlink, which is why the directory case uses a junction there.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use rigel_core::{
    BudgetService, Cancels, InterruptSink, PatchPipeline, SessionService, StopAt, Tools,
};
use serde_json::json;
use tempfile::TempDir;

async fn jail(root: &Path) -> PatchPipeline {
    let service = SessionService::open(root).await.unwrap();
    PatchPipeline::new(root, service.pool().clone())
}

async fn tools(root: &Path) -> Tools {
    let service = SessionService::open(root).await.unwrap();
    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels as Arc<dyn InterruptSink>,
    ));
    Tools::new(
        root.to_path_buf(),
        service,
        budget,
        root.join(".rigel/index"),
        StopAt::Never,
    )
}

#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) -> bool {
    Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn link_dir(_target: &Path, _link: &Path) -> bool {
    false
}

#[tokio::test]
async fn traversal_absolute_and_unc_paths_are_refused() {
    let root = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret\n").unwrap();
    let pipeline = jail(root.path()).await;

    for rel in [
        "../secret.txt",
        "..\\secret.txt",
        "nested/../../secret.txt",
        "\\\\server\\share\\secret.txt",
    ] {
        assert!(pipeline.resolve(rel).is_err(), "`{rel}` escaped the jail");
        assert!(
            pipeline.read(rel).is_err(),
            "`{rel}` was readable through the jail"
        );
    }

    let absolute = outside.path().join("secret.txt");
    let absolute = absolute.to_string_lossy();
    assert!(
        pipeline.resolve(&absolute).is_err(),
        "an absolute path outside the root was resolved: {absolute}"
    );
}

#[tokio::test]
async fn a_link_that_points_outside_is_refused_and_one_inside_is_allowed() {
    let root = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret\n").unwrap();
    std::fs::create_dir_all(root.path().join("real/inner")).unwrap();
    std::fs::write(root.path().join("real/inner/ok.txt"), "ok\n").unwrap();
    let pipeline = jail(root.path()).await;

    let escape = root.path().join("escape");
    if link_dir(outside.path(), &escape) {
        let error = pipeline
            .resolve("escape/secret.txt")
            .expect_err("a link outside the root must not resolve");
        assert!(
            error.to_string().contains("outside the project root"),
            "{error}"
        );
        assert!(
            pipeline.read("escape/secret.txt").is_err(),
            "a link outside the root was readable"
        );
    }

    // The control: a link that stays inside is a normal project path, not a refusal.
    let inside = root.path().join("inside");
    if link_dir(&root.path().join("real"), &inside) {
        assert!(
            pipeline.resolve("inside/inner/ok.txt").is_ok(),
            "an inside link must still resolve"
        );
    }
}

#[tokio::test]
async fn every_escape_is_refused_by_read_write_edit_and_list() {
    let root = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret\n").unwrap();
    std::fs::create_dir_all(root.path().join("real/inner")).unwrap();
    std::fs::write(root.path().join("real/inner/ok.txt"), "ok\n").unwrap();
    let pipeline = jail(root.path()).await;
    let tools = tools(root.path()).await;
    let absolute = outside
        .path()
        .join("secret.txt")
        .to_string_lossy()
        .into_owned();

    let mut escapes = vec![
        "../secret.txt".to_string(),
        "nested/../../secret.txt".to_string(),
        "\\\\server\\share\\secret.txt".to_string(),
        absolute,
    ];
    let escape = root.path().join("escape");
    if link_dir(outside.path(), &escape) {
        escapes.push("escape/secret.txt".to_string());
        escapes.push("escape".to_string());
    }

    for rel in &escapes {
        assert!(pipeline.read(rel).is_err(), "read escaped through `{rel}`");

        let write = tools
            .execute(
                "s",
                "a",
                "write_file",
                json!({ "path": rel, "content": "pwned" }),
            )
            .await;
        assert!(!write.ok, "write escaped through `{rel}`: {:?}", write.body);

        let edit = pipeline.propose("s", "a", rel, "stale", "pwned").await;
        assert!(edit.is_err(), "edit escaped through `{rel}`: {edit:?}");

        let list = tools
            .execute("s", "a", "list_files", json!({ "path": rel }))
            .await;
        assert!(!list.ok, "list escaped through `{rel}`: {:?}", list.body);
    }

    let inside = root.path().join("inside");
    if link_dir(&root.path().join("real"), &inside) {
        assert!(
            pipeline.read("inside/inner/ok.txt").is_ok(),
            "an inside link must stay readable"
        );
        let listed = tools
            .execute("s", "a", "list_files", json!({ "path": "inside" }))
            .await;
        assert!(
            listed.ok,
            "an inside link must stay listable: {:?}",
            listed.body
        );
    }
}

#[cfg(windows)]
#[tokio::test]
async fn a_case_variant_of_a_project_directory_stays_inside() {
    let root = TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "fn main() {}\n").unwrap();
    let pipeline = jail(root.path()).await;

    // Windows resolves the spelling to the same file; the jail must canonicalise it rather than
    // treat `SRC` as a second directory outside the root.
    let resolved = pipeline
        .resolve("SRC/lib.rs")
        .expect("a case variant of an inside path is still inside");
    assert!(resolved.ends_with("lib.rs"), "{}", resolved.display());
}

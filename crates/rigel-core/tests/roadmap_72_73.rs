//! Roadmap 0.3 items 72 (classifier coverage) and 73 (destructive dry run) through the public
//! agent API the command tool uses. Unit tables live beside each module; these prove the pair is
//! reachable from outside the crate, not only from `cfg(test)` helpers.

use std::path::Path;

use rigel_core::agent::{destructive, dry_run};

#[test]
fn an_unknown_shell_body_uses_the_most_restrictive_row() {
    let findings = destructive::classify(
        "pwsh -Command \"frobnicate --all\"",
        Path::new("/work/demo"),
        Path::new("/work/demo"),
    );
    assert!(
        findings.iter().any(|finding| finding.id == "unknown-shell"),
        "{findings:?}"
    );
}

#[test]
fn shell_and_cloud_families_are_reachable_through_the_public_classifier() {
    for (command, expected) in [
        (
            "pwsh -Command \"Clear-Content src/app.ts\"",
            "powershell-content",
        ),
        ("cmd /c \"format D: /q\"", "cmd-disk"),
        ("wsl --unregister Ubuntu", "wsl-destroy"),
        ("npm uninstall left-pad", "package-remove"),
        ("terraform destroy -auto-approve", "cloud-destroy"),
    ] {
        let findings =
            destructive::classify(command, Path::new("/work/demo"), Path::new("/work/demo"));
        assert!(
            findings.iter().any(|finding| finding.id == expected),
            "{command} should match {expected}: {findings:?}"
        );
    }
}

#[test]
fn a_tool_without_a_preview_says_so_instead_of_guessing() {
    let run = dry_run::preview("npm uninstall left-pad", Path::new("."));
    assert!(!run.supported);
    assert!(run.output.is_empty());
    assert!(
        run.note
            .as_deref()
            .unwrap_or_default()
            .contains("no built-in preview")
    );
}

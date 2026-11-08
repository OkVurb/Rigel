//! Read-only previews for destructive commands (roadmap 73).
//!
//! The approval card already carries a lexical blast radius. This module adds the command's own
//! preview form where one exists (`git clean -ndx` for a clean, `git diff --stat` for a reset),
//! and says so explicitly when a tool has no non-mutating form. It never guesses an effect and
//! never runs the approved command: every subprocess here is a read-only git query.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// What the card can show before the command runs. `supported: false` is an answer, not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryRun {
    /// The preview command that was run, or the approved command when there is no preview.
    pub command: String,
    /// Whether the output is the tool's own preview.
    pub supported: bool,
    /// Captured preview output, bounded for the card.
    pub output: String,
    /// Why no preview exists, or which preview failed.
    pub note: Option<String>,
}

/// The preview for one approved command. Only git forms have one; every other tool says so.
pub fn preview(command: &str, cwd: &Path) -> DryRun {
    let tokens: Vec<String> = command
        .split_whitespace()
        .map(|token| {
            token
                .trim_matches('"')
                .trim_matches('\'')
                .to_ascii_lowercase()
        })
        .collect();
    let git = tokens
        .iter()
        .position(|token| matches!(token.as_str(), "git" | "git.exe"));
    let Some(git) = git else {
        return unavailable(
            command,
            "this tool has no built-in preview; the card shows the exact command but cannot show its effect",
        );
    };
    let rest = &tokens[git + 1..];
    let Some(subcommand) = rest.first() else {
        return unavailable(command, "`git` without a subcommand has no preview");
    };

    if subcommand == "clean" {
        let mut args = vec!["clean".to_string(), "-nd".to_string()];
        let flags = rest
            .iter()
            .filter(|token| token.starts_with('-'))
            .collect::<Vec<_>>();
        if flags.iter().any(|flag| flag.contains('x')) {
            args[1].push('x');
        }
        return run(cwd, &args);
    }
    if subcommand == "reset" && rest.iter().any(|token| token == "--hard") {
        return run(cwd, &["diff".to_string(), "--stat".to_string()]);
    }
    if matches!(subcommand.as_str(), "checkout" | "restore") {
        return run(cwd, &["diff".to_string(), "--stat".to_string()]);
    }

    unavailable(
        command,
        "`git` has no non-mutating preview for this subcommand; the card names the exact command",
    )
}

fn unavailable(command: &str, note: &str) -> DryRun {
    DryRun {
        command: command.to_string(),
        supported: false,
        output: String::new(),
        note: Some(note.to_string()),
    }
}

fn run(cwd: &Path, args: &[String]) -> DryRun {
    let output = Command::new("git").args(args).current_dir(cwd).output();
    match output {
        Ok(output) if output.status.success() => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            DryRun {
                command: format!("git {}", args.join(" ")),
                supported: true,
                output: truncate(text.trim_end(), 4_000),
                note: None,
            }
        }
        Ok(output) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            DryRun {
                command: format!("git {}", args.join(" ")),
                supported: false,
                output: String::new(),
                note: Some(format!(
                    "the preview could not run: {}",
                    truncate(text.trim(), 400)
                )),
            }
        }
        Err(error) => DryRun {
            command: format!("git {}", args.join(" ")),
            supported: false,
            output: String::new(),
            note: Some(format!("git is not available: {error}")),
        },
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::preview;

    fn git(root: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn a_clean_uses_gits_own_dry_run_and_names_the_would_be_removal() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git(root, &["init", "-q"]);
        std::fs::write(root.join("leftover.txt"), b"temporary").unwrap();

        let run = preview("git clean -fdx", root);
        assert!(run.supported, "{run:?}");
        assert!(run.command.contains("clean -ndx"), "{}", run.command);
        assert!(
            run.output.contains("leftover.txt"),
            "the would-be removal is in the preview: {run:?}"
        );
        // The file is still there: a preview is not the command.
        assert!(root.join("leftover.txt").exists());
    }

    #[test]
    fn a_reset_previews_the_changes_it_would_discard() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);
        std::fs::write(root.join("tracked.txt"), b"one").unwrap();
        git(root, &["add", "tracked.txt"]);
        git(root, &["commit", "-qm", "one"]);
        std::fs::write(root.join("tracked.txt"), b"two").unwrap();

        let run = preview("git reset --hard HEAD", root);
        assert!(run.supported, "{run:?}");
        assert!(run.output.contains("tracked.txt"), "{run:?}");
    }

    #[test]
    fn a_tool_without_a_preview_says_so_instead_of_guessing() {
        let tmp = tempfile::tempdir().unwrap();
        let run = preview("npm uninstall left-pad", tmp.path());
        assert!(!run.supported);
        assert!(run.output.is_empty());
        assert!(
            run.note
                .as_deref()
                .unwrap_or_default()
                .contains("no built-in preview"),
            "{run:?}"
        );
    }
}

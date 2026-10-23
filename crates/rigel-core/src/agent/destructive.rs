//! The destructive-operation classifier (D32).
//!
//! Deliberation and snapshots are expensive, so they cannot run on every write. This module is
//! the deterministic trigger: it reads a command string and returns the policy rows it matches.
//! It is deliberately a classifier, not a sandbox or a parser — it recognises the shapes of the
//! operations that destroy work that has no other copy, and a human still approves the exact
//! text. Agents cannot vote a finding away.
//!
//! | policy | what it catches |
//! |---|---|
//! | `delete-outside-project` | `rm`/`del`/`Remove-Item` on an absolute or escaping path |
//! | `recursive-remove` | `rm -rf`, `rmdir /s`, `Remove-Item -Recurse` |
//! | `force-push` | `git push --force`, `-f`, or a `+refspec` |
//! | `git-reset` | `git reset --hard`, `git clean -f`, `git checkout -- .`, `git restore .` |
//! | `registry-write` | `reg add|delete|import`, writes through `HKLM:`/`HKCU:` |

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

/// One row of the policy table. The id is stable and is what the approval item records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Policy {
    pub id: &'static str,
    pub summary: &'static str,
}

pub const POLICIES: &[Policy] = &[
    Policy {
        id: "delete-outside-project",
        summary: "deletes a path outside the project root",
    },
    Policy {
        id: "recursive-remove",
        summary: "removes a directory tree",
    },
    Policy {
        id: "force-push",
        summary: "rewrites history on a remote",
    },
    Policy {
        id: "git-reset",
        summary: "discards uncommitted work",
    },
    Policy {
        id: "registry-write",
        summary: "writes to the Windows registry",
    },
];

/// A matched row. `detail` names the token that matched, for the approval prompt and the item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub id: &'static str,
    pub summary: &'static str,
    pub detail: String,
}

impl Finding {
    fn policy(policy: &'static Policy, detail: impl Into<String>) -> Self {
        Self {
            id: policy.id,
            summary: policy.summary,
            detail: detail.into(),
        }
    }
}

/// Every row this command matches, in policy order, at most one finding per row.
pub fn classify(command: &str, cwd: &Path, project_root: &Path) -> Vec<Finding> {
    let mut details: Vec<Option<String>> = vec![None; POLICIES.len()];
    for segment in segments(command) {
        if segment.is_empty() {
            continue;
        }
        inspect_git(&segment, &mut details);
        inspect_registry(&segment, &mut details);
        inspect_delete(&segment, cwd, project_root, &mut details);
    }
    POLICIES
        .iter()
        .zip(details)
        .filter_map(|(policy, detail)| detail.map(|detail| Finding::policy(policy, detail)))
        .collect()
}

fn row(id: &str) -> usize {
    POLICIES
        .iter()
        .position(|policy| policy.id == id)
        .expect("every row id in POLICIES")
}

fn record(details: &mut [Option<String>], id: &str, detail: impl Into<String>) {
    let slot = &mut details[row(id)];
    if slot.is_none() {
        *slot = Some(detail.into());
    }
}

/// Split on shell command separators, then on whitespace, and strip quotes. Quoted separators
/// stay inside their token. This is a heuristic: obfuscation is out of scope, and the human still
/// sees the exact text that will run.
fn segments(command: &str) -> Vec<Vec<String>> {
    let mut segments = Vec::new();
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote: Option<char> = None;
    for character in command.chars() {
        if let Some(open) = quote {
            if character == open {
                quote = None;
            } else {
                token.push(character);
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            ';' | '|' | '&' | '\n' | '\r' => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
                if !tokens.is_empty() {
                    segments.push(std::mem::take(&mut tokens));
                }
            }
            c if c.is_whitespace() => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            c => token.push(c),
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    if !tokens.is_empty() {
        segments.push(tokens);
    }
    segments
}

fn lower(token: &str) -> String {
    token.to_ascii_lowercase()
}

fn inspect_git(segment: &[String], details: &mut [Option<String>]) {
    let Some(index) = segment
        .iter()
        .position(|token| matches!(lower(token).as_str(), "git" | "git.exe"))
    else {
        return;
    };
    let rest = &segment[index + 1..];
    let Some(subcommand) = rest.iter().find(|token| !token.starts_with('-')) else {
        return;
    };
    let arguments = &rest[rest
        .iter()
        .position(|token| token == subcommand)
        .map(|position| position + 1)
        .unwrap_or(rest.len())..];
    let subcommand = lower(subcommand);
    match subcommand.as_str() {
        "push" => {
            if let Some(force) = arguments.iter().find(|argument| {
                let argument = lower(argument);
                argument == "-f"
                    || argument == "--force"
                    || argument == "--force-with-lease"
                    || argument == "--force-if-includes"
                    || argument.starts_with('+')
            }) {
                record(details, "force-push", force.clone());
            }
        }
        "reset" => {
            if let Some(hard) = arguments
                .iter()
                .find(|argument| lower(argument) == "--hard")
            {
                record(details, "git-reset", hard.clone());
            }
        }
        "clean" => {
            if let Some(force) = arguments.iter().find(|argument| {
                let argument = lower(argument);
                argument == "--force"
                    || (argument.starts_with('-')
                        && !argument.starts_with("--")
                        && argument.contains('f'))
            }) {
                record(details, "git-reset", force.clone());
            }
        }
        "checkout" | "restore" => {
            // `git checkout -- .` and `git restore .` throw away every uncommitted change.
            let worktree = arguments.iter().any(|argument| argument == ".")
                || arguments
                    .iter()
                    .position(|argument| argument == "--")
                    .is_some_and(|dash| arguments.get(dash + 1).is_some_and(|path| path == "."));
            if worktree {
                record(details, "git-reset", arguments.join(" "));
            }
        }
        _ => {}
    }
}

fn inspect_registry(segment: &[String], details: &mut [Option<String>]) {
    let first = lower(&segment[0]);
    if matches!(first.as_str(), "reg" | "reg.exe") {
        let Some(subcommand) = segment.get(1).map(|token| lower(token)) else {
            return;
        };
        if matches!(
            subcommand.as_str(),
            "add" | "delete" | "import" | "copy" | "load" | "restore"
        ) {
            record(details, "registry-write", segment[1..].join(" "));
        }
        return;
    }
    if matches!(first.as_str(), "regedit" | "regedit.exe")
        && segment
            .iter()
            .any(|token| matches!(lower(token).as_str(), "/s" | "/import"))
    {
        record(details, "registry-write", segment[1..].join(" "));
        return;
    }
    if matches!(
        first.as_str(),
        "set-itemproperty" | "new-itemproperty" | "remove-itemproperty" | "new-item"
    ) && segment.iter().any(|token| {
        let token = lower(token);
        token.starts_with("hklm:")
            || token.starts_with("hkcu:")
            || token.starts_with("hkey_")
            || token.contains("registry::")
    }) {
        record(details, "registry-write", segment[1..].join(" "));
    }
}

fn inspect_delete(
    segment: &[String],
    cwd: &Path,
    project_root: &Path,
    details: &mut [Option<String>],
) {
    let verb = lower(&segment[0]);
    let verbs: &[&str] = &["rm", "del", "erase", "rmdir", "rd", "remove-item", "ri"];
    if !verbs.contains(&verb.as_str()) {
        return;
    }

    let arguments = &segment[1..];
    let recursive = arguments.iter().any(|argument| {
        let argument = lower(argument);
        argument == "--recursive"
            || argument == "-recurse"
            || argument == "/s"
            || (argument.starts_with('-')
                && !argument.starts_with("--")
                && argument.contains('r')
                && !argument.starts_with("-re"))
    });
    if recursive {
        record(details, "recursive-remove", segment.join(" "));
    }

    for target in arguments.iter().filter(|argument| !is_flag(argument)) {
        match lexical_path(target, cwd) {
            Some(path) if inside(&path, project_root) => {}
            _ => {
                record(details, "delete-outside-project", target.clone());
            }
        }
    }
}

/// Whether `path` is inside `root`, comparing plain text rather than `Path::starts_with` so a
/// canonical `\\?\` prefix on Windows does not make every absolute in-project path look outside.
fn inside(path: &Path, root: &Path) -> bool {
    let path = comparable(path);
    let root = comparable(root).trim_end_matches('/').to_string();
    if root.is_empty() {
        return path.starts_with('/');
    }
    path == root || path.starts_with(&format!("{root}/"))
}

fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
    if cfg!(windows) {
        text.to_ascii_lowercase()
    } else {
        text
    }
}

/// A flag, not a path. `cmd`'s `/s` and `/q` are single letters; `/tmp` is a path.
fn is_flag(argument: &str) -> bool {
    if argument.starts_with('-') || argument.starts_with("--") {
        return true;
    }
    if let Some(name) = argument.strip_prefix('/') {
        return matches!(
            name.to_ascii_lowercase().as_str(),
            "s" | "q" | "f" | "r" | "a" | "p" | "d" | "t" | "c" | "k"
        );
    }
    false
}

/// Resolve a deletion target without touching the filesystem, and reject anything that cannot be
/// resolved (`$VAR`, `%APPDATA%`, `~`): an unknown path is treated as outside.
fn lexical_path(target: &str, cwd: &Path) -> Option<PathBuf> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    let unquoted = target
        .trim_matches('"')
        .trim_matches('\'')
        .trim_start_matches("./")
        .to_string();
    let lower = unquoted.to_ascii_lowercase();
    if unquoted.starts_with('~')
        || unquoted.contains('$')
        || unquoted.contains('%')
        || lower.starts_with("$env:")
    {
        return None;
    }

    let path = Path::new(&unquoted);
    let joined = if is_absolute_text(&unquoted) {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Some(normalized)
}

/// Absolute on either platform: a leading separator, a UNC path, or a drive letter. A Windows
/// path is still "outside" when the tests run on Unix, which is what the policy means.
fn is_absolute_text(target: &str) -> bool {
    let bytes = target.as_bytes();
    target.starts_with('/')
        || target.starts_with('\\')
        || (bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::{classify, POLICIES};
    use std::path::Path;

    fn ids(command: &str) -> Vec<&'static str> {
        classify(command, Path::new("/work/demo"), Path::new("/work/demo"))
            .into_iter()
            .map(|finding| finding.id)
            .collect()
    }

    #[test]
    fn every_policy_row_has_a_stable_unique_id() {
        let mut seen = std::collections::HashSet::new();
        for policy in POLICIES {
            assert!(seen.insert(policy.id), "duplicate row {}", policy.id);
            assert!(!policy.summary.is_empty());
        }
    }

    #[test]
    fn a_delete_outside_the_project_is_flagged() {
        for command in [
            "rm ../secrets.txt",
            "rm -rf /etc",
            "del C:\\Windows\\System32\\drivers\\etc\\hosts",
            "Remove-Item ~/notes.txt",
            "rm \"$HOME/.ssh/id_rsa\"",
        ] {
            assert!(
                ids(command).contains(&"delete-outside-project"),
                "{command} should be outside"
            );
        }
    }

    #[test]
    fn a_delete_inside_the_project_is_not_flagged() {
        assert!(ids("rm src/tmp.txt").is_empty());
        assert!(ids("rm -f ./target/debug/app").is_empty());
        assert!(ids("del build\\out.txt").is_empty());
    }

    #[test]
    fn a_recursive_remove_is_flagged_anywhere() {
        for command in [
            "rm -rf target",
            "rm -r src/generated",
            "rmdir /s /q build",
            "Remove-Item -Recurse node_modules",
        ] {
            assert!(
                ids(command).contains(&"recursive-remove"),
                "{command} should be recursive"
            );
        }
        assert!(!ids("rm target/app").contains(&"recursive-remove"));
    }

    #[test]
    fn a_force_push_is_flagged() {
        for command in [
            "git push --force origin main",
            "git push -f",
            "git push origin +main:main",
        ] {
            assert!(
                ids(command).contains(&"force-push"),
                "{command} should force-push"
            );
        }
        assert!(!ids("git push origin main").contains(&"force-push"));
        assert!(!ids("git status").contains(&"force-push"));
    }

    #[test]
    fn discarding_uncommitted_work_is_flagged() {
        for command in [
            "git reset --hard HEAD~1",
            "git clean -fdx",
            "git checkout -- .",
            "git restore .",
        ] {
            assert!(
                ids(command).contains(&"git-reset"),
                "{command} should discard work"
            );
        }
        assert!(!ids("git reset HEAD src/lib.rs").contains(&"git-reset"));
        assert!(!ids("git checkout main").contains(&"git-reset"));
    }

    #[test]
    fn a_registry_write_is_flagged() {
        for command in [
            "reg add HKLM\\Software\\Rigel /v X /d 1",
            "reg delete HKCU\\Software\\Rigel /f",
            "Set-ItemProperty -Path HKCU:\\Software\\Rigel -Name X -Value 1",
            "regedit /s policy.reg",
        ] {
            assert!(
                ids(command).contains(&"registry-write"),
                "{command} should write the registry"
            );
        }
        assert!(!ids("reg query HKLM\\Software").contains(&"registry-write"));
    }

    #[test]
    fn ordinary_commands_have_no_findings() {
        for command in [
            "cargo test --workspace",
            "git status --short",
            "git commit -m \"fix the parser\"",
            "rm src/tmp.txt && cargo build",
        ] {
            assert!(ids(command).is_empty(), "{command} should be clean");
        }
    }

    #[test]
    fn a_canonical_windows_root_still_contains_its_children() {
        if !cfg!(windows) {
            return;
        }
        let findings = classify(
            "rm C:\\work\\demo\\src\\a.txt",
            Path::new("C:\\work\\demo"),
            Path::new(r"\\?\C:\work\demo"),
        );
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn a_finding_names_the_token_that_matched() {
        let finding = classify(
            "rm -rf /etc",
            Path::new("/work/demo"),
            Path::new("/work/demo"),
        )
        .into_iter()
        .find(|finding| finding.id == "delete-outside-project")
        .unwrap();
        assert_eq!(finding.detail, "/etc");
    }
}

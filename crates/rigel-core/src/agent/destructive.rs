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
//! | `powershell-content` | `Clear-Content`/`Set-Content`/`Out-File`/`Add-Content` |
//! | `cmd-disk` | `format` and `diskpart` |
//! | `wsl-destroy` | `wsl --unregister` |
//! | `package-remove` | `npm`/`pip`/`cargo`/`apt`/`brew`/... uninstall, remove or purge |
//! | `cloud-destroy` | `aws`/`gcloud`/`az`/`terraform`/`kubectl`/`docker` deletes |
//! | `unknown-shell` | a PowerShell/cmd/WSL body no other row can place |

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

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
    Policy {
        id: "powershell-content",
        summary: "truncates or overwrites a file through PowerShell",
    },
    Policy {
        id: "cmd-disk",
        summary: "formats or repartitions a disk",
    },
    Policy {
        id: "wsl-destroy",
        summary: "unregisters a WSL distribution and its filesystem",
    },
    Policy {
        id: "package-remove",
        summary: "uninstalls a package from a project or the machine",
    },
    Policy {
        id: "cloud-destroy",
        summary: "deletes a cloud resource or infrastructure",
    },
    Policy {
        id: "unknown-shell",
        summary: "runs a shell-wrapped command the classifier cannot place",
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

/// Q239: what an approval card can prove about a destructive command.
///
/// The preview is deliberately separate from the decision: it never grants anything and it
/// never returns a guessed count. A target that cannot be resolved lexically is reported as
/// unknown rather than silently omitted, because the human is about to approve the deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlastRadius {
    /// Exact command segments that matched a destructive policy.
    pub commands: Vec<String>,
    /// Resolved deletion targets, with a bounded listing of what is under each one.
    pub targets: Vec<BlastTarget>,
    /// Policy rows whose effect cannot be enumerated locally (a remote, the registry, a glob).
    pub cannot_determine: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlastTarget {
    /// The target as resolved against the command's cwd.
    pub path: String,
    /// `file`, `directory`, `missing`, or `unknown`.
    pub kind: &'static str,
    /// Project-relative paths under a directory, bounded so the approval card stays readable.
    #[serde(default)]
    pub files: Vec<String>,
    /// The full number of files found, when the target was enumerated completely.
    pub file_count: Option<usize>,
    pub byte_count: Option<u64>,
    /// Why the target is only partially known, if it is.
    pub note: Option<String>,
}

const MAX_PREVIEW_FILES: usize = 40;
const MAX_PREVIEW_ENTRIES: usize = 5_000;

/// Build the Q239 preview from the deterministic classifier. The caller still asks the user;
/// this only makes the question concrete.
pub fn blast_radius(command: &str, cwd: &Path, project_root: &Path) -> BlastRadius {
    let mut commands = Vec::new();
    let mut targets = Vec::new();
    let mut cannot_determine = Vec::new();

    for segment in segments(command) {
        let text = segment.join(" ");
        let findings = classify(&text, cwd, project_root);
        if findings.is_empty() {
            continue;
        }
        commands.push(text.clone());
        let verb = segment
            .first()
            .map(|token| lower(token))
            .unwrap_or_default();
        if matches!(
            verb.as_str(),
            "rm" | "del" | "erase" | "rmdir" | "rd" | "remove-item" | "ri"
        ) {
            for target in segment.iter().skip(1).filter(|arg| !is_flag(arg)) {
                match lexical_path(target, cwd) {
                    Some(path) => targets.push(inspect_target(&path, project_root)),
                    None => cannot_determine.push(format!(
                        "the deletion target `{target}` contains a variable, glob, or home shorthand and cannot be resolved safely"
                    )),
                }
            }
        }
        if findings.iter().any(|finding| finding.id == "git-reset") {
            match changed_paths(project_root) {
                Some(paths) => {
                    let count = paths.len();
                    targets.push(BlastTarget {
                        path: project_root.display().to_string(),
                        kind: "directory",
                        files: paths.into_iter().take(MAX_PREVIEW_FILES).collect(),
                        file_count: Some(count),
                        byte_count: None,
                        note: Some("uncommitted paths reported by git status".to_string()),
                    });
                }
                None => cannot_determine.push(
                    "git could not report the uncommitted paths, so the discard count is unknown"
                        .to_string(),
                ),
            }
        }
        for finding in findings {
            if matches!(finding.id, "force-push" | "registry-write") {
                cannot_determine.push(format!(
                    "{}: {}; the remote or registry is not readable from here",
                    finding.id, finding.summary
                ));
            }
        }
    }

    // Keep one row per target when a command names the same path twice.
    targets.sort_by(|left, right| left.path.cmp(&right.path));
    targets.dedup_by(|left, right| left.path == right.path);
    cannot_determine.sort();
    cannot_determine.dedup();
    BlastRadius {
        commands,
        targets,
        cannot_determine,
    }
}

fn inspect_target(path: &Path, project_root: &Path) -> BlastTarget {
    let shown = path.display().to_string();
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return BlastTarget {
                path: shown,
                kind: "missing",
                files: Vec::new(),
                file_count: Some(0),
                byte_count: Some(0),
                note: Some("the target does not exist".to_string()),
            };
        }
        Err(error) => {
            return BlastTarget {
                path: shown,
                kind: "unknown",
                files: Vec::new(),
                file_count: None,
                byte_count: None,
                note: Some(format!("could not read the target: {error}")),
            };
        }
    };
    if metadata.file_type().is_symlink() {
        return BlastTarget {
            path: shown,
            kind: "unknown",
            files: Vec::new(),
            file_count: None,
            byte_count: None,
            note: Some("a symbolic link is not followed by the preview".to_string()),
        };
    }
    if metadata.is_file() {
        return BlastTarget {
            path: shown,
            kind: "file",
            files: vec![relative_shown(path, project_root)],
            file_count: Some(1),
            byte_count: Some(metadata.len()),
            note: None,
        };
    }
    if !metadata.is_dir() {
        return BlastTarget {
            path: shown,
            kind: "unknown",
            files: Vec::new(),
            file_count: None,
            byte_count: None,
            note: Some("the target is not a regular file or directory".to_string()),
        };
    }

    let mut files = Vec::new();
    let mut bytes = 0u64;
    let mut visited = 0usize;
    let mut complete = true;
    walk(
        path,
        project_root,
        &mut files,
        &mut bytes,
        &mut visited,
        &mut complete,
    );
    BlastTarget {
        path: shown,
        kind: "directory",
        files: files.into_iter().take(MAX_PREVIEW_FILES).collect(),
        file_count: complete.then_some(visited),
        byte_count: complete.then_some(bytes),
        note: (!complete).then(|| {
            format!(
                "the preview stops after {MAX_PREVIEW_ENTRIES} entries; the count is a lower bound"
            )
        }),
    }
}

fn walk(
    dir: &Path,
    project_root: &Path,
    files: &mut Vec<String>,
    bytes: &mut u64,
    visited: &mut usize,
    complete: &mut bool,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        *complete = false;
        return;
    };
    for entry in entries.flatten() {
        if *visited >= MAX_PREVIEW_ENTRIES {
            *complete = false;
            return;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            *complete = false;
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            walk(&path, project_root, files, bytes, visited, complete);
        } else if metadata.is_file() {
            *visited += 1;
            *bytes = bytes.saturating_add(metadata.len());
            files.push(relative_shown(&path, project_root));
        }
    }
}

fn relative_shown(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn changed_paths(project_root: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["status", "--porcelain", "-z"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut paths = Vec::new();
    let mut iter = text.split('\0');
    while let Some(record) = iter.next() {
        if record.len() < 4 {
            continue;
        }
        // `XY path`; rename records carry a second NUL-terminated path.
        let path = record.get(3..).unwrap_or("").trim();
        if !path.is_empty() {
            paths.push(path.to_string());
        }
        if record.starts_with("R") || record.starts_with("C") {
            let _ = iter.next();
        }
    }
    Some(paths)
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
        inspect_families(&segment, &mut details);
        inspect_shell(&segment, cwd, project_root, &mut details);
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

fn is_package_manager(first: &str) -> bool {
    matches!(
        first,
        "npm"
            | "npm.cmd"
            | "pnpm"
            | "pnpm.cmd"
            | "yarn"
            | "yarn.cmd"
            | "bun"
            | "pip"
            | "pip3"
            | "python"
            | "python3"
            | "cargo"
            | "apt"
            | "apt-get"
            | "brew"
            | "winget"
            | "choco"
            | "scoop"
    )
}

fn is_cloud_cli(first: &str) -> bool {
    matches!(
        first,
        "aws" | "gcloud" | "az" | "terraform" | "kubectl" | "docker" | "pulumi" | "helm"
    )
}

fn is_shell_launcher(first: &str) -> bool {
    matches!(
        first,
        "pwsh"
            | "pwsh.exe"
            | "powershell"
            | "powershell.exe"
            | "cmd"
            | "cmd.exe"
            | "wsl"
            | "wsl.exe"
    )
}

/// Families whose destructive verbs are not `rm`/`git`/`reg`. The same check runs on a top-level
/// command and on the body of a shell launcher, so `pwsh -Command "Clear-Content x"` is not
/// hidden by the wrapper.
fn inspect_families(segment: &[String], details: &mut [Option<String>]) {
    let first = lower(&segment[0]);
    let words: Vec<String> = segment.iter().map(|token| lower(token)).collect();

    if matches!(
        first.as_str(),
        "clear-content" | "set-content" | "out-file" | "add-content"
    ) {
        record(details, "powershell-content", segment[1..].join(" "));
    }
    if matches!(
        first.as_str(),
        "format" | "format.com" | "diskpart" | "diskpart.exe"
    ) {
        record(details, "cmd-disk", segment[1..].join(" "));
    }
    if matches!(first.as_str(), "wsl" | "wsl.exe")
        && words.iter().any(|word| word == "--unregister")
    {
        record(details, "wsl-destroy", segment[1..].join(" "));
    }
    if is_package_manager(&first)
        && words
            .iter()
            .any(|word| matches!(word.as_str(), "uninstall" | "remove" | "purge" | "rm"))
    {
        record(details, "package-remove", segment[1..].join(" "));
    }
    if is_cloud_cli(&first)
        && words.iter().any(|word| {
            matches!(
                word.as_str(),
                "delete"
                    | "delete-stack"
                    | "terminate"
                    | "destroy"
                    | "rm"
                    | "rb"
                    | "purge"
                    | "prune"
            )
        })
    {
        record(details, "cloud-destroy", segment[1..].join(" "));
    }
}

/// A body a shell launcher wraps. Recognised families keep their own row; anything else is the
/// most restrictive row rather than silently ordinary.
fn known_body(segment: &[String]) -> bool {
    let first = lower(&segment[0]);
    is_shell_launcher(&first)
        || matches!(
            first.as_str(),
            "git"
                | "git.exe"
                | "reg"
                | "reg.exe"
                | "regedit"
                | "regedit.exe"
                | "rm"
                | "del"
                | "erase"
                | "rmdir"
                | "rd"
                | "remove-item"
                | "ri"
                | "clear-content"
                | "set-content"
                | "out-file"
                | "add-content"
                | "format"
                | "format.com"
                | "diskpart"
                | "diskpart.exe"
                | "wsl"
                | "wsl.exe"
                | "dir"
                | "ls"
                | "cat"
                | "type"
                | "echo"
                | "pwd"
                | "cd"
                | "where"
                | "which"
                | "get-childitem"
                | "get-content"
                | "test-path"
                | "cargo"
                | "node"
                | "python"
                | "python3"
                | "pytest"
                | "go"
                | "rustc"
        )
        || is_package_manager(&first)
        || is_cloud_cli(&first)
}

fn inspect_shell(
    segment: &[String],
    cwd: &Path,
    project_root: &Path,
    details: &mut [Option<String>],
) {
    let first = lower(&segment[0]);
    if !is_shell_launcher(&first) {
        return;
    }
    let Some(start) = segment
        .iter()
        .position(|token| matches!(lower(token).as_str(), "-command" | "-c" | "/c"))
    else {
        return;
    };
    // The outer tokenizer preserves spaces inside quotes so a detail can name a quoted
    // argument. A quoted shell body is still a command, so split it before classifying it.
    let inner: Vec<String> = segment[start + 1..]
        .iter()
        .flat_map(|token| token.split_whitespace().map(str::to_string))
        .collect();
    if inner.is_empty() {
        return;
    }
    inspect_git(&inner, details);
    inspect_registry(&inner, details);
    inspect_delete(&inner, cwd, project_root, details);
    inspect_families(&inner, details);
    if !known_body(&inner) {
        record(details, "unknown-shell", inner.join(" "));
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
    use super::{POLICIES, blast_radius, classify};
    use std::fs;
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
    fn every_shell_and_cloud_family_has_a_policy_row() {
        let cases = [
            ("Clear-Content src/app.ts", "powershell-content"),
            ("Set-Content src/app.ts 0", "powershell-content"),
            ("format D: /q", "cmd-disk"),
            ("diskpart /s layout.txt", "cmd-disk"),
            ("wsl --unregister Ubuntu", "wsl-destroy"),
            ("npm uninstall left-pad", "package-remove"),
            ("pnpm remove left-pad", "package-remove"),
            ("pip3 uninstall requests", "package-remove"),
            ("cargo uninstall cargo-watch", "package-remove"),
            ("apt-get purge nginx", "package-remove"),
            ("brew uninstall --force node", "package-remove"),
            ("aws s3 rb s3://bucket --force", "cloud-destroy"),
            ("gcloud projects delete demo", "cloud-destroy"),
            ("az group delete -n demo", "cloud-destroy"),
            ("terraform destroy -auto-approve", "cloud-destroy"),
            ("kubectl delete namespace demo", "cloud-destroy"),
            ("reg add HKLM\\Software\\Rigel /v X /d 1", "registry-write"),
        ];
        for (command, expected) in cases {
            assert!(
                ids(command).contains(&expected),
                "{command} should match {expected}, got {:?}",
                ids(command)
            );
        }
        // The non-destructive form of each family stays clean.
        for command in [
            "npm install left-pad",
            "wsl --list --verbose",
            "aws s3 ls",
            "kubectl get pods",
            "Get-Content src/app.ts",
        ] {
            assert!(
                ids(command).is_empty(),
                "{command} should be clean: {:?}",
                ids(command)
            );
        }
    }

    #[test]
    fn a_shell_wrapped_command_is_classified_and_unknown_bodies_are_restrictive() {
        assert!(ids("pwsh -Command \"Clear-Content src/app.ts\"").contains(&"powershell-content"));
        assert!(
            ids("cmd /c \"del /f /s /q build\"").contains(&"recursive-remove"),
            "the wrapper must not hide a recursive delete"
        );
        assert!(ids("powershell -Command \"npm uninstall left-pad\"").contains(&"package-remove"));
        assert!(
            ids("pwsh -Command \"frobnicate --all\"").contains(&"unknown-shell"),
            "an unrecognised body falls to the most restrictive row"
        );
        assert!(!ids("pwsh -Command \"git status\"").contains(&"unknown-shell"));
        assert!(!ids("cmd /c \"Get-ChildItem src\"").contains(&"unknown-shell"));
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
    fn blast_radius_names_resolved_files_and_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("tree/sub")).unwrap();
        fs::write(root.join("tree/a.txt"), b"one").unwrap();
        fs::write(root.join("tree/sub/b.txt"), b"two").unwrap();

        let preview = blast_radius("rm -rf tree", root, root);

        assert_eq!(preview.commands, vec!["rm -rf tree"]);
        assert_eq!(preview.targets.len(), 1, "{preview:?}");
        let target = &preview.targets[0];
        assert_eq!(target.kind, "directory");
        assert_eq!(target.file_count, Some(2));
        assert_eq!(target.byte_count, Some(6));
        assert!(target.files.iter().any(|file| file.ends_with("tree/a.txt")));
        assert!(
            target
                .files
                .iter()
                .any(|file| file.ends_with("tree/sub/b.txt"))
        );
    }

    #[test]
    fn blast_radius_never_guesses_an_unresolvable_target() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let preview = blast_radius("rm -rf $BUILD_DIR", root, root);

        assert!(preview.targets.is_empty(), "{preview:?}");
        assert!(
            preview
                .cannot_determine
                .iter()
                .any(|note| note.contains("BUILD_DIR")),
            "{preview:?}"
        );
    }

    #[test]
    fn blast_radius_marks_a_remote_rewrite_as_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        let preview = blast_radius("git push --force origin main", tmp.path(), tmp.path());
        assert!(preview.targets.is_empty());
        assert!(
            preview
                .cannot_determine
                .iter()
                .any(|note| note.contains("force-push")),
            "{preview:?}"
        );
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

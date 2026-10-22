//! Where each harness's binary lives, how it is spawned, and what a probe can say about it.
//!
//! One module rather than the same four-path lookup in each adapter: `--version` parsing, the
//! Windows `.cmd` shim and the install hints are identical for all of them, and a harness added
//! later should not have to rediscover them.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

use crate::{Capabilities, Harness};

/// The name a human sees. Used in errors, so it says "Claude Code", not "Claude".
pub fn display_name(harness: Harness) -> &'static str {
    match harness {
        Harness::Native => "Rigel",
        Harness::Codex => "Codex",
        Harness::Claude => "Claude Code",
        Harness::Cursor => "Cursor",
        Harness::Cline => "Cline",
    }
}

/// The default executable name on this platform. Codex and Claude ship an `.exe` on Windows;
/// Cursor ships `agent.exe`; Cline ships a `cline.cmd` shim, which is why [`command_for`] exists.
pub fn exe_name(harness: Harness) -> &'static str {
    match harness {
        Harness::Native => "rigel",
        Harness::Codex => {
            if cfg!(windows) {
                "codex.exe"
            } else {
                "codex"
            }
        }
        Harness::Claude => {
            if cfg!(windows) {
                "claude.exe"
            } else {
                "claude"
            }
        }
        Harness::Cursor => {
            if cfg!(windows) {
                "agent.exe"
            } else {
                "agent"
            }
        }
        Harness::Cline => {
            if cfg!(windows) {
                "cline.cmd"
            } else {
                "cline"
            }
        }
    }
}

/// Turn a configured executable into the path a probe reports. A bare name is searched on `PATH`;
/// anything with a separator is used as given, because the caller meant that file.
pub fn resolve_exe(_harness: Harness, explicit: &Path) -> PathBuf {
    if explicit.components().count() > 1 || explicit.is_absolute() {
        return explicit.to_path_buf();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return explicit.to_path_buf();
    };
    std::env::split_paths(&path)
        .map(|dir| dir.join(explicit))
        .find(|candidate| candidate.is_file())
        // Not found: report the name as configured rather than the default binary for this
        // harness, so a probe says what it looked for.
        .unwrap_or_else(|| explicit.to_path_buf())
}

/// How to launch a resolved executable. Windows cannot spawn a `.cmd`/`.bat` directly (CreateProcess
/// has no shell), so the npm-installed shims go through `cmd /C`; everything else is spawned
/// as-is.
pub fn command_for(exe: &Path) -> Command {
    let batch = exe
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        });
    if batch && cfg!(windows) {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(exe);
        command
    } else {
        Command::new(exe)
    }
}

/// `--version`, with the version picked out of whatever prose the harness prints
/// (`2.1.251 (Claude Code)`, `codex-cli 0.162.1`, `mock-agent 1.4.2`).
pub async fn version_of(exe: &Path) -> Option<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        command_for(exe).arg("--version").output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_version(&String::from_utf8_lossy(&output.stdout))
}

/// The first token that looks like a dotted version. A `--version` that prints only a name is not
/// evidence of a version, so it yields `None` rather than a guess.
pub fn parse_version(stdout: &str) -> Option<String> {
    stdout
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| !c.is_ascii_digit() && c != '.'))
        .find(|token| {
            token.contains('.')
                && token
                    .split('.')
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        })
        .map(str::to_string)
}

/// What a harness can do, as far as Rigel relies on. Version-gated differences belong here when a
/// real harness changes shape; no harness has needed one yet.
pub fn capabilities(harness: Harness, _version: Option<&str>) -> Capabilities {
    match harness {
        Harness::Native => Capabilities {
            acp: false,
            json_stream: true,
            programmatic_approval: true,
            worktree: true,
            nested_agents: true,
        },
        Harness::Codex => Capabilities {
            acp: false,
            json_stream: true,
            programmatic_approval: true,
            worktree: true,
            // Codex's item stream does not guarantee nested agent events (§4.2).
            nested_agents: false,
        },
        Harness::Claude | Harness::Cursor | Harness::Cline => Capabilities {
            acp: harness != Harness::Claude,
            json_stream: true,
            programmatic_approval: true,
            worktree: true,
            nested_agents: true,
        },
    }
}

/// What to do about a missing harness. Shown verbatim next to the row, so it has to be a command
/// or a link, not "install the CLI".
pub fn install_hint(harness: Harness) -> String {
    match harness {
        Harness::Native => "Rigel's own agent is part of this app".to_string(),
        Harness::Codex => "install Codex: npm install -g @openai/codex".to_string(),
        Harness::Claude => {
            "install Claude Code: npm install -g @anthropic-ai/claude-code".to_string()
        }
        Harness::Cursor => {
            "install the Cursor CLI (agent): https://cursor.com/docs/cli".to_string()
        }
        Harness::Cline => "install the Cline CLI (cline --acp): npm install -g cline".to_string(),
    }
}

/// A vendor login file, if one exists. Its presence means the harness could run on a subscription,
/// which is exactly the state Rigel refuses (D21). This is a signal, not proof: the child's first
/// event is still checked, because the file can exist beside a valid API key.
///
/// Cline has no entry on purpose: `cline auth` writes either a Cline API key or a ClinePass/ChatGPT
/// subscription into the same configuration, and no file distinguishes them. Guessing
/// `~/.cline/settings.json` would label an API-key user as a subscription; the honest answer is
/// "no key injected", which the CLI already refuses with the key instructions.
pub fn vendor_login(harness: Harness) -> Option<PathBuf> {
    let home = home_dir()?;
    let candidate = match harness {
        Harness::Claude => home.join(".claude.json"),
        Harness::Cursor => home.join(".cursor").join("cli-config.json"),
        Harness::Native | Harness::Codex | Harness::Cline => return None,
    };
    candidate.is_file().then_some(candidate)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_picked_out_of_the_prose() {
        assert_eq!(
            parse_version("2.1.251 (Claude Code)").as_deref(),
            Some("2.1.251")
        );
        assert_eq!(
            parse_version("codex-cli 0.162.1\n").as_deref(),
            Some("0.162.1")
        );
        assert_eq!(parse_version("mock-agent 1.4.2").as_deref(), Some("1.4.2"));
        assert_eq!(parse_version("no version here"), None);
        assert_eq!(parse_version("trailing."), None);
    }

    #[test]
    fn every_external_harness_has_a_name_a_hint_and_capabilities() {
        for harness in Harness::external() {
            assert!(!display_name(harness).is_empty());
            assert!(!install_hint(harness).is_empty());
            assert!(capabilities(harness, None).json_stream);
        }
        // Cline and Cursor speak ACP; Claude and Codex do not.
        assert!(capabilities(Harness::Cursor, None).acp);
        assert!(capabilities(Harness::Cline, None).acp);
        assert!(!capabilities(Harness::Claude, None).acp);
        assert!(!capabilities(Harness::Codex, None).acp);
    }

    #[test]
    fn a_path_with_a_separator_is_used_as_given() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(exe_name(Harness::Codex));
        std::fs::write(&exe, "").unwrap();
        assert_eq!(resolve_exe(Harness::Codex, &exe), exe);
    }

    #[test]
    fn an_unknown_bare_name_is_reported_as_configured() {
        // Deliberately not a harness name: the assertion must not depend on what this machine
        // happens to have installed.
        let name = Path::new("rigel-fixture-binary-that-does-not-exist");
        assert_eq!(resolve_exe(Harness::Codex, name), name);
    }

    #[test]
    fn a_batch_shim_is_launched_through_cmd() {
        let command = command_for(Path::new("cline.cmd"));
        if cfg!(windows) {
            assert_eq!(command.as_std().get_program().to_string_lossy(), "cmd");
        } else {
            assert_eq!(
                command.as_std().get_program().to_string_lossy(),
                "cline.cmd"
            );
        }
    }
}

//! Command timeout policy (roadmap 35).
//!
//! A command argument wins, then the project's `.rigel/command-timeout` file, then the documented
//! 200-second default. Every value is clamped to the same ceiling the tool schema advertises, so a
//! hand-edited file cannot buy an unbounded process.

use std::path::Path;

use serde::Serialize;

pub const DEFAULT_TIMEOUT_SECS: u64 = 200;
pub const MAX_TIMEOUT_SECS: u64 = 600;
pub const PROJECT_FILE: &str = ".rigel/command-timeout";

/// Where the effective limit came from. The transcript names this when a kill happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutSource {
    Command,
    Project,
    Default,
}

impl TimeoutSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Project => "project",
            Self::Default => "default",
        }
    }
}

/// Resolve the limit for one command. Invalid project files are ignored rather than silently
/// becoming zero; the command still runs under the documented default.
pub fn resolve(project_root: &Path, requested: Option<u64>) -> (u64, TimeoutSource) {
    if let Some(seconds) = requested {
        return (seconds.clamp(1, MAX_TIMEOUT_SECS), TimeoutSource::Command);
    }
    if let Ok(text) = std::fs::read_to_string(project_root.join(PROJECT_FILE))
        && let Ok(seconds) = text.trim().parse::<u64>()
    {
        return (seconds.clamp(1, MAX_TIMEOUT_SECS), TimeoutSource::Project);
    }
    (DEFAULT_TIMEOUT_SECS, TimeoutSource::Default)
}

/// The sentence stored with a timed-out command. It names both the limit and where it came from;
/// partial stdout/stderr remain in the same result.
pub fn kill_reason(limit_secs: u64, source: TimeoutSource) -> String {
    format!(
        "the command exceeded its {limit_secs}s limit ({} timeout); partial output is kept",
        source.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_TIMEOUT_SECS, MAX_TIMEOUT_SECS, PROJECT_FILE, TimeoutSource, resolve};

    #[test]
    fn a_command_argument_wins_over_the_project_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".rigel")).unwrap();
        std::fs::write(tmp.path().join(PROJECT_FILE), "45\n").unwrap();
        assert_eq!(resolve(tmp.path(), Some(12)), (12, TimeoutSource::Command));
        assert_eq!(resolve(tmp.path(), None), (45, TimeoutSource::Project));
    }

    #[test]
    fn a_missing_or_invalid_project_file_keeps_the_documented_default() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve(tmp.path(), None),
            (DEFAULT_TIMEOUT_SECS, TimeoutSource::Default)
        );
        std::fs::create_dir_all(tmp.path().join(".rigel")).unwrap();
        std::fs::write(tmp.path().join(PROJECT_FILE), "not a number").unwrap();
        assert_eq!(
            resolve(tmp.path(), None),
            (DEFAULT_TIMEOUT_SECS, TimeoutSource::Default)
        );
    }

    #[test]
    fn every_source_is_clamped_to_the_advertised_ceiling() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".rigel")).unwrap();
        std::fs::write(tmp.path().join(PROJECT_FILE), "9999").unwrap();
        assert_eq!(
            resolve(tmp.path(), None),
            (MAX_TIMEOUT_SECS, TimeoutSource::Project)
        );
        assert_eq!(resolve(tmp.path(), Some(0)), (1, TimeoutSource::Command));
    }
}

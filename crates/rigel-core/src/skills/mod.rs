//! The shelf's engine: remote sources, the local index, search, clustering and evidence.
//!
//! Module responsibilities (handoff §14.2): `skills` owns the index, FTS, pinning and the
//! tool wrapper, and must never execute scripts. Nothing in this module knows about harnesses
//! or sessions, so a skill can be indexed without a project open.
//!
//! | file | owns |
//! |---|---|
//! | [`frontmatter`] | reading `---` headers |
//! | [`scan`] | static capability inventory and the grade |
//! | [`blob`] | bodies on disk, addressed by sha256 |
//! | [`http`] | cached, conditional outbound requests |
//! | [`source`] | the `Source` trait and its value types |
//! | [`github`] | repo → skills, one tree call + raw bodies |
//! | [`hub`] | SkillsMP and SkillHub (HTML) connectors |
//! | [`cluster`] | SimHash and near-duplicate clusters |
//! | [`evidence`] | measured vs advertised numbers |
//! | [`skillssh`] | the directory's page, its audits and their dates |
//! | [`search`] | FTS5 query construction and the documented ordering |
//! | [`index`] | the `SkillIndex` service that ties all of the above together |

pub mod blob;
pub mod cluster;
pub mod evidence;
pub mod frontmatter;
pub mod github;
pub mod http;
pub mod hub;
pub mod index;
pub mod scan;
pub mod search;
pub mod skillssh;
pub mod source;

use std::io;
use std::time::Duration;

pub use crate::storage::StorageError;
pub use blob::BlobStore;
pub use cluster::{HAMMING_THRESHOLD, simhash};
pub use evidence::{Evidence, EvidenceSummary};
pub use github::GitHubSource;
pub use http::{Fetched, Http, HttpError};
pub use hub::{SkillHubSource, SkillsMpSource};
pub use index::{
    ImportReport, IndexStats, IngestReport, SkillDetail, SkillIndex, SweepReport, VariantRow,
};
pub use scan::{Capability, EvidenceLine, Severity};
pub use search::{SearchHit, SearchOrder};
pub use skillssh::{AuditVerdict, SkillsShPage};
pub use source::{
    FetchedSkill, RepoSnapshot, SkillRecord, Source, SourceState, SourceStatus, SyncPolicy,
};

/// How long a cached URL is trusted without a request. Ten minutes is long enough that a
/// re-index right after an index is free (P2.11) and short enough that `--force` is a
/// deliberate act, not a reflex.
pub const DEFAULT_TTL: Duration = Duration::from_secs(600);

/// A source is stale after a day without a successful sync, and the shelf says so with a
/// date rather than a spinner.
pub const STALE_AFTER_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, thiserror::Error)]
pub enum SkillsError {
    #[error(transparent)]
    Http(#[from] HttpError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error("blob store error: {0}")]
    Io(#[from] io::Error),

    #[error("{0}")]
    InvalidRepo(String),

    #[error("{0}")]
    NoSkillsFound(String),

    #[error("no skill with id {0} in the index — run `rigel skills index <repo>` first")]
    NotIndexed(String),

    #[error("{0} is not supported by the {1} source")]
    Unsupported(&'static str, &'static str),

    #[error("unknown source {0}")]
    UnknownSource(String),

    #[error("skill {0} has no body in the local index")]
    NoBody(String),

    #[error("{0}")]
    Offline(String),

    #[error("{0}")]
    Other(String),
}

impl SkillsError {
    /// §7.2: `1` is a user error, `2` a failed precondition. The CLI maps these straight
    /// through so a script can tell "you asked for something that does not exist" from
    /// "the index is not ready".
    pub fn exit_code(&self) -> u8 {
        match self {
            SkillsError::NotIndexed(_) | SkillsError::NoBody(_) | SkillsError::InvalidRepo(_) => 1,
            SkillsError::NoSkillsFound(_)
            | SkillsError::Offline(_)
            | SkillsError::UnknownSource(_)
            | SkillsError::Http(_) => 2,
            SkillsError::Unsupported(..) => 1,
            SkillsError::Storage(_)
            | SkillsError::Database(_)
            | SkillsError::Io(_)
            | SkillsError::Other(_) => 2,
        }
    }
}

/// `2026-10-20 14:05` in UTC, without pulling in a date library for one string.
pub fn fmt_time(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let rem = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// Howard Hinnant's `civil_from_days`, for the Gregorian calendar since 1970.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_render_as_utc_without_a_date_crate() {
        assert_eq!(fmt_time(0), "1970-01-01 00:00");
        assert_eq!(fmt_time(1_760_961_600_000), "2025-10-20 12:00");
        assert_eq!(fmt_time(1_792_454_400_000), "2026-10-20 00:00");
    }

    #[test]
    fn error_codes_follow_the_cli_contract() {
        assert_eq!(SkillsError::NotIndexed("x".into()).exit_code(), 1);
        assert_eq!(SkillsError::Offline("no net".into()).exit_code(), 2);
    }
}

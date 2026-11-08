//! Pre-write secret scan (roadmap 60).
//!
//! A proposed write is scanned with gitleaks-style rules before it reaches the disk. A
//! match blocks the write by default. The only way through is a typed override that names
//! the rule that fired and a reason; the override is appended to a JSONL audit log, never
//! to the transcript, and it never contains the secret itself.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::migrate::now_ms;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rule {
    AwsAccessKey,
    GitHubToken,
    GitLabToken,
    SlackToken,
    StripeLiveKey,
    GoogleApiKey,
    ProviderKey,
    PemPrivateKey,
    Jwt,
    BearerToken,
}

impl Rule {
    pub const ALL: [Rule; 10] = [
        Rule::AwsAccessKey,
        Rule::GitHubToken,
        Rule::GitLabToken,
        Rule::SlackToken,
        Rule::StripeLiveKey,
        Rule::GoogleApiKey,
        Rule::ProviderKey,
        Rule::PemPrivateKey,
        Rule::Jwt,
        Rule::BearerToken,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Rule::AwsAccessKey => "aws-access-key",
            Rule::GitHubToken => "github-token",
            Rule::GitLabToken => "gitlab-token",
            Rule::SlackToken => "slack-token",
            Rule::StripeLiveKey => "stripe-live-key",
            Rule::GoogleApiKey => "google-api-key",
            Rule::ProviderKey => "provider-key",
            Rule::PemPrivateKey => "pem-private-key",
            Rule::Jwt => "jwt",
            Rule::BearerToken => "bearer-token",
        }
    }

    fn pattern(self) -> &'static str {
        match self {
            Rule::AwsAccessKey => r"\bAKIA[0-9A-Z]{16}\b",
            Rule::GitHubToken => r"\bghp_[A-Za-z0-9]{36}\b",
            Rule::GitLabToken => r"\bglpat-[A-Za-z0-9_-]{20,}\b",
            Rule::SlackToken => r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b",
            Rule::StripeLiveKey => r"\bsk_live_[A-Za-z0-9]{16,}\b",
            Rule::GoogleApiKey => r"\bAIza[0-9A-Za-z_-]{35}\b",
            Rule::ProviderKey => r"\bsk-(?:ant-|or-)?[A-Za-z0-9_-]{20,}\b",
            Rule::PemPrivateKey => r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----",
            Rule::Jwt => r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b",
            Rule::BearerToken => r"(?i)\bbearer\s+[A-Za-z0-9._-]{20,}\b",
        }
    }

    fn regex(self) -> &'static Regex {
        static CACHE: OnceLock<Vec<(Rule, Regex)>> = OnceLock::new();
        let all = CACHE.get_or_init(|| {
            Rule::ALL
                .iter()
                .map(|rule| {
                    (
                        *rule,
                        Regex::new(rule.pattern())
                            .unwrap_or_else(|error| panic!("rule {} failed: {error}", rule.name())),
                    )
                })
                .collect()
        });
        &all.iter()
            .find(|(rule, _)| *rule == self)
            .expect("rule in cache")
            .1
    }
}

/// One match. It deliberately records the rule, line and width, never the matched text:
/// the finding itself must be safe to paste into an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: Rule,
    pub line: usize,
    pub column: usize,
    pub width: usize,
}

impl Finding {
    pub fn sentence(&self) -> String {
        format!(
            "{} on line {} (column {})",
            self.rule.name(),
            self.line,
            self.column
        )
    }
}

/// Every match in a proposed write, in source order and without duplicates.
pub fn scan(text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for rule in Rule::ALL {
        for found in rule.regex().find_iter(text) {
            let prefix = &text[..found.start()];
            let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
            let column = prefix
                .rsplit_once('\n')
                .map(|(_, tail)| tail.chars().count() + 1)
                .unwrap_or_else(|| prefix.chars().count() + 1);
            let finding = Finding {
                rule,
                line,
                column,
                width: found.as_str().chars().count(),
            };
            if !findings.contains(&finding) {
                findings.push(finding);
            }
        }
    }
    findings.sort_by_key(|finding| (finding.line, finding.column, finding.rule.name()));
    findings
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteScan {
    Clean,
    Blocked(Vec<Finding>),
}

impl WriteScan {
    pub fn is_blocked(&self) -> bool {
        matches!(self, WriteScan::Blocked(_))
    }
}

/// Scan a proposed write. A match blocks; the caller must not touch the file.
pub fn scan_write(_path: &str, content: &str) -> WriteScan {
    let findings = scan(content);
    if findings.is_empty() {
        WriteScan::Clean
    } else {
        WriteScan::Blocked(findings)
    }
}

/// Scan a staged diff. Only added lines are considered, so removing a secret is never
/// blocked by the removal itself.
pub fn scan_staged_diff(diff: &str) -> WriteScan {
    let mut added = String::new();
    for line in diff.lines() {
        if let Some(body) = line.strip_prefix('+')
            && !body.starts_with("+++")
        {
            added.push_str(body);
            added.push('\n');
        }
    }
    scan_write("staged diff", &added)
}

#[derive(Debug, thiserror::Error)]
pub enum OverrideError {
    #[error("the override must type the exact rule name {expected:?}")]
    WrongRule { expected: String },

    #[error("the override needs a reason of at least {minimum} characters")]
    ReasonTooShort { minimum: usize },

    #[error("could not write the audit log: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideRecord {
    pub rule: Rule,
    pub path: String,
    pub reason: String,
    pub typed: String,
    pub at_ms: i64,
}

/// Record a deliberate override. The typed phrase must equal the rule name, so a click
/// cannot approve a secret by accident. The JSONL row names the rule and the path; it
/// never includes the secret.
pub fn record_override(
    log: &Path,
    finding: &Finding,
    path: &str,
    typed: &str,
    reason: &str,
) -> Result<OverrideRecord, OverrideError> {
    if typed != finding.rule.name() {
        return Err(OverrideError::WrongRule {
            expected: finding.rule.name().to_string(),
        });
    }
    let reason = reason.trim();
    if reason.chars().count() < 8 {
        return Err(OverrideError::ReasonTooShort { minimum: 8 });
    }
    if let Some(parent) = log.parent() {
        fs::create_dir_all(parent)?;
    }
    let record = OverrideRecord {
        rule: finding.rule,
        path: path.to_string(),
        reason: reason.to_string(),
        typed: typed.to_string(),
        at_ms: now_ms(),
    };
    let mut file = OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(file, "{}", serde_json::to_string(&record)?)?;
    Ok(record)
}

impl From<serde_json::Error> for OverrideError {
    fn from(error: serde_json::Error) -> Self {
        OverrideError::Io(std::io::Error::other(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_github() -> String {
        format!("ghp_{}", "a".repeat(36))
    }

    #[test]
    fn a_match_blocks_the_write_by_default() {
        let content = format!("token = \"{}\"\n", fake_github());
        let outcome = scan_write("src/config.rs", &content);
        match outcome {
            WriteScan::Blocked(findings) => {
                assert_eq!(findings.len(), 1);
                assert_eq!(findings[0].rule, Rule::GitHubToken);
                assert_eq!(findings[0].line, 1);
            }
            WriteScan::Clean => panic!("a GitHub token must block"),
        }
    }

    #[test]
    fn benign_lookalikes_are_clean() {
        let content = "let label = \"sk-etch\";\nlet word = \"AIza\";\nlet key = \"sk-learn\";";
        assert_eq!(scan(content), Vec::new());
        assert_eq!(scan_write("README.md", content), WriteScan::Clean);
    }

    #[test]
    fn a_staged_diff_scans_only_added_lines() {
        let removed = format!("-token = \"{}\"\n+token = \"safe\"\n", fake_github());
        assert_eq!(scan_staged_diff(&removed), WriteScan::Clean);
        let added = format!("+token = \"{}\"\n", fake_github());
        assert!(scan_staged_diff(&added).is_blocked());
    }

    #[test]
    fn a_typed_override_is_stored_with_the_rule_that_fired() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("audit").join("secret-overrides.jsonl");
        let finding = Finding {
            rule: Rule::StripeLiveKey,
            line: 3,
            column: 4,
            width: 30,
        };
        let record = record_override(
            &log,
            &finding,
            "src/pay.rs",
            "stripe-live-key",
            "test fixture, rotated after the run",
        )
        .unwrap();
        assert_eq!(record.rule, Rule::StripeLiveKey);
        assert_eq!(record.path, "src/pay.rs");
        let written = fs::read_to_string(&log).unwrap();
        assert!(written.contains("stripe-live-key"));
        assert!(written.contains("rotated after the run"));
        assert!(!written.contains("sk_live_"));
        let parsed: OverrideRecord = serde_json::from_str(written.trim()).unwrap();
        assert_eq!(parsed.rule, Rule::StripeLiveKey);
    }

    #[test]
    fn the_override_refuses_a_wrong_typed_phrase_or_a_thin_reason() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("overrides.jsonl");
        let finding = Finding {
            rule: Rule::AwsAccessKey,
            line: 1,
            column: 1,
            width: 20,
        };
        assert!(matches!(
            record_override(&log, &finding, "a", "yes", "a real reason here"),
            Err(OverrideError::WrongRule { .. })
        ));
        assert!(matches!(
            record_override(&log, &finding, "a", "aws-access-key", "short"),
            Err(OverrideError::ReasonTooShort { .. })
        ));
        assert!(!log.exists());
    }

    #[test]
    fn every_rule_has_a_real_shaped_fixture_and_a_name() {
        let fixtures = [
            (Rule::AwsAccessKey, format!("AKIA{}", "A".repeat(16))),
            (Rule::GitHubToken, fake_github()),
            (Rule::GitLabToken, format!("glpat-{}", "b".repeat(20))),
            (Rule::SlackToken, format!("xoxb-{}", "1".repeat(12))),
            (Rule::StripeLiveKey, format!("sk_live_{}", "c".repeat(20))),
            (Rule::GoogleApiKey, format!("AIza{}", "d".repeat(35))),
            (Rule::ProviderKey, format!("sk-ant-{}", "e".repeat(24))),
            (
                Rule::PemPrivateKey,
                "-----BEGIN RSA PRIVATE KEY-----".to_string(),
            ),
            (
                Rule::Jwt,
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.c2lnbmF0dXJlMTIzNDU2".to_string(),
            ),
            (
                Rule::BearerToken,
                format!("Authorization: Bearer {}", "f".repeat(24)),
            ),
        ];
        for (rule, fixture) in fixtures {
            let found = scan(&fixture);
            assert!(
                found.iter().any(|finding| finding.rule == rule),
                "{} was not found in {fixture}",
                rule.name()
            );
            assert!(!rule.name().is_empty());
        }
    }
}

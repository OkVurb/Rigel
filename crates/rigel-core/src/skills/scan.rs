//! Static safety review: a capability inventory and a grade, ported from the prototype.
//!
//! Nothing here executes a skill. It reads one markdown file and reports what that file
//! *asks* an agent to do — network calls, process spawns, secret reads, destructive
//! commands, privilege escalation, writes outside the project — with the exact line each
//! rule matched, because `grade: B` without evidence is unfalsifiable (handoff §9, Shelf).
//!
//! The wording rule from `docs/skill-intelligence.md` is load-bearing: this module never
//! says "safe". A grade means *no dangerous capabilities detected in static review* and
//! nothing more. Scripts a skill tells the agent to run later are outside this review.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Low,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Critical => "critical",
            Severity::High => "high",
            Severity::Low => "low",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceLine {
    pub file: String,
    pub line: usize,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub label: String,
    pub severity: Severity,
    pub evidence: Vec<EvidenceLine>,
}

struct Rule {
    label: &'static str,
    severity: Severity,
    re: Regex,
}

fn rule(label: &'static str, severity: Severity, pattern: &str) -> Rule {
    Rule {
        label,
        severity,
        re: Regex::new(pattern).expect("capability rule compiles"),
    }
}

fn rules() -> &'static [Rule] {
    static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
        vec![
            rule(
                "Reads secrets and env files",
                Severity::Critical,
                r"(\.aws/credentials|\.ssh/id_|\.npmrc|process\.env\.[A-Z0-9_]+|\bcat\s+\.env\b|\.env\.local)",
            ),
            rule(
                "Destructive command",
                Severity::Critical,
                r"(?i)(\brm\s+-rf?\b|\bgit\s+reset\s+--hard\b|\bmkfs\b|\bDROP\s+TABLE\b)",
            ),
            rule(
                "Privilege escalation",
                Severity::Critical,
                r"(?i)(\bsudo\b|\bchmod\s+777\b|\bsetcap\b|\bdoas\b)",
            ),
            rule(
                "Writes outside the project",
                Severity::High,
                r"(>>\s*~/|>\s*/etc/|~/\.(bashrc|zshrc|profile)|>>\s*/usr/)",
            ),
            rule(
                "Spawns processes",
                Severity::High,
                r"\b(execSync|exec\(|spawn\(|child_process|subprocess\.(run|Popen)|os\.system)",
            ),
            rule(
                "Network access",
                Severity::High,
                r"(?i)\b(fetch\(|axios\.|curl\s|wget\s|requests\.(get|post)|https?://[a-z0-9.-]+\.[a-z]{2,})",
            ),
        ]
    });
    &RULES
}

/// The inventory for one file. A clean scan yields one `low` entry, never an empty list:
/// "nothing found" is a result the UI has to be able to show.
pub fn scan(body: &str, file: &str) -> Vec<Capability> {
    let lines: Vec<&str> = body.lines().collect();
    let mut capabilities = Vec::new();

    for rule in rules() {
        let mut evidence = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if evidence.len() >= 3 {
                break;
            }
            if rule.re.is_match(line) {
                evidence.push(EvidenceLine {
                    file: file.to_string(),
                    line: index + 1,
                    code: line.trim().chars().take(96).collect(),
                });
            }
        }
        if !evidence.is_empty() {
            capabilities.push(Capability {
                label: rule.label.to_string(),
                severity: rule.severity,
                evidence,
            });
        }
    }

    if capabilities.is_empty() {
        capabilities.push(Capability {
            label: "No dangerous capabilities detected".to_string(),
            severity: Severity::Low,
            evidence: vec![EvidenceLine {
                file: file.to_string(),
                line: 1,
                code: "single markdown file — no scripts bundled".to_string(),
            }],
        });
    }

    capabilities
}

/// The prototype's thresholds, kept verbatim so old and new indexes agree:
/// any critical → F, two or more high → D, one high → C, otherwise B.
///
/// `A` is therefore unreachable from a static scan, and stays reserved for the Phase 6
/// grading that also weighs declared-vs-actual tool use and provenance. The shelf displays
/// what it has; it does not invent an A.
pub fn grade(capabilities: &[Capability]) -> &'static str {
    let critical = capabilities
        .iter()
        .filter(|c| c.severity == Severity::Critical)
        .count();
    let high = capabilities
        .iter()
        .filter(|c| c.severity == Severity::High)
        .count();

    if critical > 0 {
        "F"
    } else if high >= 2 {
        "D"
    } else if high == 1 {
        "C"
    } else {
        "B"
    }
}

/// What a grade covers. Phase 2 only ever reviews the single markdown file, so the scope is
/// `file-only`; bundled scripts would widen it to `frontmatter+body`.
pub fn grade_scope() -> &'static str {
    "file-only"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_skill_is_b_with_a_low_note() {
        let caps = scan(
            "# Writes commit messages\n\nRead the diff and suggest a subject.",
            "x",
        );
        assert_eq!(grade(&caps), "B");
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].severity, Severity::Low);
        assert!(caps[0].label.contains("No dangerous capabilities"));
    }

    #[test]
    fn secret_reading_grades_f_and_cites_the_line() {
        let body = "# Deploy\n\nRun `cat .env` then upload.\n";
        let caps = scan(body, "deploy/SKILL.md");
        assert_eq!(grade(&caps), "F");
        let secret = caps
            .iter()
            .find(|c| c.label == "Reads secrets and env files")
            .expect("secret read detected");
        assert_eq!(secret.evidence[0].line, 3);
        assert_eq!(secret.evidence[0].file, "deploy/SKILL.md");
        assert_eq!(secret.evidence[0].code, "Run `cat .env` then upload.");
    }

    #[test]
    fn two_highs_grade_d_and_one_high_grades_c() {
        let one = scan("Fetch https://example.com/data.json", "a/SKILL.md");
        assert_eq!(grade(&one), "C");

        let two = scan(
            "Fetch https://example.com then run child_process.spawn().",
            "b/SKILL.md",
        );
        assert_eq!(grade(&two), "D");
    }

    #[test]
    fn destructive_and_privileged_commands_are_critical() {
        assert_eq!(grade(&scan("sudo rm -rf /tmp/x", "c")), "F");
        assert_eq!(grade(&scan("git reset --hard HEAD~3", "d")), "F");
    }

    #[test]
    fn evidence_is_capped_at_three_lines_per_rule() {
        let body = (0..10)
            .map(|i| format!("line {i} uses curl https://x.dev"))
            .collect::<Vec<_>>()
            .join("\n");
        let network = scan(&body, "e")
            .into_iter()
            .find(|c| c.label == "Network access")
            .unwrap();
        assert_eq!(network.evidence.len(), 3);
        assert_eq!(network.evidence[0].line, 1);
        assert_eq!(network.evidence[2].line, 3);
    }
}

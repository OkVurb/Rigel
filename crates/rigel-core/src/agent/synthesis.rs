//! Subagent synthesis (roadmap 43).
//!
//! The parent already receives each child's final answer, spend and files. Synthesis turns those
//! answers into one claim list where every claim carries the child and the files it came from,
//! plus a conflict row when two children make different claims about the same subject.
//!
//! The parser is deliberately simple and deterministic. A line of a child's answer is one claim;
//! a `subject: value` or `subject = value` line gives the subject and value used for comparison,
//! while free prose uses the whole line as both. Identical subject/value pairs from different
//! children agree. A subject with two distinct values from two distinct children is a conflict —
//! the parent may resolve it, but the synthesis never silently picks a winner. The child prompt
//! already asks for the files a child worked on; this module never invents a source.

use serde::Serialize;
use serde_json::Value;

/// A child can return up to 6,000 characters. This keeps the synthesis bounded when a child
/// writes a long report, without hiding the fact that it was truncated.
pub const MAX_CLAIMS_PER_CHILD: usize = 20;

/// One statement made by one child, with the provenance needed to check it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SynthesisClaim {
    /// The statement as the child wrote it, without a list marker.
    pub text: String,
    /// The normalized key used to compare claims across children.
    pub subject: String,
    /// The normalized value; equal values for one subject agree.
    pub value: String,
    pub agent_id: String,
    pub chat_name: String,
    pub role: String,
    /// The files this child was assigned. This is what makes the claim traceable.
    pub files: Vec<String>,
}

/// Two or more children said different things about one subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SynthesisConflict {
    pub subject: String,
    /// One claim per participating child, in the order the children were collected.
    pub claims: Vec<SynthesisClaim>,
    pub agents: Vec<String>,
    pub files: Vec<String>,
}

/// The parent-facing result. `summary` is a compact cited rendering for a model that wants one
/// block of text; `claims` and `conflicts` are the structured form used by tests and callers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Synthesis {
    pub claims: Vec<SynthesisClaim>,
    pub conflicts: Vec<SynthesisConflict>,
    /// Claims over [`MAX_CLAIMS_PER_CHILD`] that the structured result did not carry. The raw
    /// child answer is still in `agents`, and the summary says how many were left out.
    pub truncated: usize,
    pub summary: String,
}

/// Build a synthesis from the `agents` array returned by `subagent_results`. Failed, pending and
/// text-less children contribute no claims; their status and error remain in that array.
pub fn synthesize(children: &[Value]) -> Synthesis {
    let mut claims = Vec::new();
    let mut incomplete = Vec::new();
    let mut truncated = 0usize;

    for child in children {
        let text = child.get("text").and_then(Value::as_str).unwrap_or("");
        if text.trim().is_empty() {
            let name = child
                .get("chat_name")
                .and_then(Value::as_str)
                .or_else(|| child.get("role").and_then(Value::as_str))
                .unwrap_or("a child");
            incomplete.push(name.to_string());
            continue;
        }
        let agent_id = child
            .get("agent_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let chat_name = child
            .get("chat_name")
            .and_then(Value::as_str)
            .unwrap_or(&agent_id)
            .to_string();
        let role = child
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let files: Vec<String> = child
            .get("files")
            .and_then(Value::as_array)
            .map(|files| {
                files
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let lines: Vec<&str> = text
            .lines()
            .map(clean_line)
            .filter(|line| !line.is_empty())
            .collect();
        truncated += lines.len().saturating_sub(MAX_CLAIMS_PER_CHILD);
        for line in lines.into_iter().take(MAX_CLAIMS_PER_CHILD) {
            let text = line.trim().to_string();
            let (subject, value) = split_claim(&text);
            claims.push(SynthesisClaim {
                text,
                subject,
                value,
                agent_id: agent_id.clone(),
                chat_name: chat_name.clone(),
                role: role.clone(),
                files: files.clone(),
            });
        }
    }

    let conflicts = conflicts(&claims);
    let summary = render_summary(&claims, &conflicts, &incomplete, truncated);
    Synthesis {
        claims,
        conflicts,
        truncated,
        summary,
    }
}

fn clean_line(line: &str) -> &str {
    let trimmed = line.trim();
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim();
        }
    }
    // Numbered list markers (`1. `, `12) `) are presentation, not part of the claim.
    let mut chars = trimmed.char_indices();
    let mut saw_digit = false;
    for (index, ch) in chars.by_ref() {
        if ch.is_ascii_digit() {
            saw_digit = true;
        } else if saw_digit && (ch == '.' || ch == ')') {
            let rest = trimmed[index + ch.len_utf8()..].trim_start();
            return rest;
        } else {
            break;
        }
    }
    trimmed
}

/// Split a claim into a comparable subject and value. Only delimiters with a following space are
/// used, so `https://…` and `C:\…` stay whole instead of becoming a fake subject.
fn split_claim(text: &str) -> (String, String) {
    for delimiter in [": ", " = "] {
        if let Some((left, right)) = text.split_once(delimiter) {
            let subject = normalize(left);
            let value = normalize(right);
            if !subject.is_empty() && !value.is_empty() {
                return (subject, value);
            }
        }
    }
    let normalized = normalize(text);
    (normalized.clone(), normalized)
}

fn normalize(text: &str) -> String {
    text.trim()
        .trim_matches(|ch: char| matches!(ch, '*' | '`' | '"' | '\''))
        .trim_end_matches(['.', ':', ';', ','])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn conflicts(claims: &[SynthesisClaim]) -> Vec<SynthesisConflict> {
    let mut out = Vec::new();
    let mut seen_subjects = Vec::new();
    for claim in claims {
        if seen_subjects.contains(&claim.subject) {
            continue;
        }
        seen_subjects.push(claim.subject.clone());

        let mut participants: Vec<&SynthesisClaim> = Vec::new();
        let mut values: Vec<&str> = Vec::new();
        let mut agents: Vec<String> = Vec::new();
        for candidate in claims
            .iter()
            .filter(|candidate| candidate.subject == claim.subject)
        {
            if agents.iter().any(|agent| agent == &candidate.agent_id) {
                continue;
            }
            // Keep one value per child. If one child contradicts itself, that is its own
            // failure, not two children disagreeing; the first claim is the one compared.
            agents.push(candidate.agent_id.clone());
            values.push(candidate.value.as_str());
            participants.push(candidate);
        }
        if agents.len() < 2 || values.iter().all(|value| *value == values[0]) {
            continue;
        }
        let mut files: Vec<String> = participants
            .iter()
            .flat_map(|claim| claim.files.iter().cloned())
            .collect();
        files.sort();
        files.dedup();
        out.push(SynthesisConflict {
            subject: claim.subject.clone(),
            claims: participants.into_iter().cloned().collect(),
            agents,
            files,
        });
    }
    out
}

fn render_summary(
    claims: &[SynthesisClaim],
    conflicts: &[SynthesisConflict],
    incomplete: &[String],
    truncated: usize,
) -> String {
    if claims.is_empty() {
        if incomplete.is_empty() {
            return "No child returned a claim.".to_string();
        }
        return format!("No cited claim came back from {}.", incomplete.join(", "));
    }
    let mut lines = Vec::new();
    for claim in claims {
        let files = if claim.files.is_empty() {
            "no files".to_string()
        } else {
            claim.files.join(", ")
        };
        lines.push(format!("- {} [{}; {}]", claim.text, claim.chat_name, files));
    }
    for conflict in conflicts {
        let sides: Vec<String> = conflict
            .claims
            .iter()
            .map(|claim| format!("{} says {}", claim.chat_name, claim.text))
            .collect();
        lines.push(format!(
            "CONFLICT {}: {}",
            conflict.subject,
            sides.join("; ")
        ));
    }
    if !incomplete.is_empty() {
        lines.push(format!("NO CLAIM: {}.", incomplete.join(", ")));
    }
    if truncated > 0 {
        lines.push(format!(
            "TRUNCATED: {truncated} claim(s) omitted from the cited synthesis."
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn child(id: &str, chat: &str, files: &[&str], text: &str) -> Value {
        json!({
            "ok": true,
            "agent_id": id,
            "chat_name": chat,
            "role": "reader",
            "files": files,
            "text": text,
        })
    }

    #[test]
    fn every_claim_cites_the_child_and_its_files() {
        let synthesis = synthesize(&[
            child(
                "child-a",
                "reader-a",
                &["README.md"],
                "language: Rust\nstatus: ready",
            ),
            child(
                "child-b",
                "reader-b",
                &["src/lib.rs"],
                "- language: Rust\n- status: ready",
            ),
        ]);

        assert_eq!(synthesis.claims.len(), 4);
        for claim in &synthesis.claims {
            assert!(!claim.agent_id.is_empty());
            assert!(!claim.chat_name.is_empty());
            assert!(!claim.files.is_empty());
        }
        assert_eq!(synthesis.claims[0].files, ["README.md"]);
        assert_eq!(synthesis.claims[2].files, ["src/lib.rs"]);
        assert!(
            synthesis.summary.contains("[reader-a; README.md]"),
            "the summary carries the citation: {}",
            synthesis.summary
        );
        assert!(synthesis.conflicts.is_empty(), "equal values agree");
    }

    #[test]
    fn two_children_disagreeing_produce_a_conflict_row() {
        let synthesis = synthesize(&[
            child("child-a", "reader-a", &["README.md"], "language: Rust"),
            child("child-b", "reader-b", &["src/lib.rs"], "language: Python"),
        ]);

        assert_eq!(synthesis.conflicts.len(), 1);
        let conflict = &synthesis.conflicts[0];
        assert_eq!(conflict.subject, "language");
        assert_eq!(conflict.agents, ["child-a", "child-b"]);
        assert_eq!(conflict.files, ["README.md", "src/lib.rs"]);
        assert_eq!(conflict.claims.len(), 2);
        assert_eq!(conflict.claims[0].value, "rust");
        assert_eq!(conflict.claims[1].value, "python");
        assert!(
            synthesis.summary.contains("CONFLICT language:"),
            "the parent sees the disagreement in the compact summary: {}",
            synthesis.summary
        );
        assert!(synthesis.summary.contains("reader-a says language: Rust"));
        assert!(synthesis.summary.contains("reader-b says language: Python"));
    }

    #[test]
    fn one_child_disagreeing_with_itself_is_not_two_children() {
        let synthesis = synthesize(&[child(
            "child-a",
            "reader-a",
            &["README.md"],
            "language: Rust\nlanguage: Python",
        )]);
        assert_eq!(synthesis.claims.len(), 2);
        assert!(
            synthesis.conflicts.is_empty(),
            "a single child's inconsistency is not a two-child conflict"
        );
    }

    #[test]
    fn a_long_answer_is_bounded_and_says_how_many_claims_were_omitted() {
        let text = (0..MAX_CLAIMS_PER_CHILD + 2)
            .map(|index| format!("subject-{index}: value"))
            .collect::<Vec<_>>()
            .join("\n");
        let synthesis = synthesize(&[child("child-a", "reader-a", &["README.md"], &text)]);
        assert_eq!(synthesis.claims.len(), MAX_CLAIMS_PER_CHILD);
        assert_eq!(synthesis.truncated, 2);
        assert!(synthesis.summary.contains("TRUNCATED: 2 claim(s)"));
    }

    #[test]
    fn free_prose_is_cited_and_failed_children_do_not_invent_claims() {
        let synthesis = synthesize(&[
            child("child-a", "reader-a", &["README.md"], "README says Demo."),
            json!({
                "ok": false,
                "agent_id": "child-b",
                "chat_name": "reader-b",
                "role": "reader",
                "files": ["src/lib.rs"],
                "error": "no answer",
            }),
        ]);
        assert_eq!(synthesis.claims.len(), 1);
        assert_eq!(synthesis.claims[0].subject, "readme says demo");
        assert_eq!(synthesis.claims[0].files, ["README.md"]);
        assert!(synthesis.conflicts.is_empty());
        assert!(
            synthesis.summary.contains("reader-b"),
            "the failed child is named in the summary: {}",
            synthesis.summary
        );
    }
}

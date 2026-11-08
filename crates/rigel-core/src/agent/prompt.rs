//! Per-section prompt budget (roadmap 53).
//!
//! The system prompt is assembled from four independently owned sections. Each one is
//! measured in characters and estimated tokens, and an over-budget section is trimmed in
//! the documented order below. The turn carries the resulting [`PromptReport`], so a
//! shortened prompt is never silent.
//!
//! Trim order: **skills, project, tools, system**. Skills are the most droppable context
//! (the model can search again); the system section is last because it carries the safety
//! rules and the output contract. A trim replaces the tail of a section with a visible
//! marker, so the model can see that context was removed rather than guessing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    System,
    Project,
    Skill,
    Tools,
}

impl Section {
    pub const ALL: [Section; 4] = [
        Section::System,
        Section::Project,
        Section::Skill,
        Section::Tools,
    ];

    /// The documented trim order. The system section is last.
    pub const TRIM_ORDER: [Section; 4] = [
        Section::Skill,
        Section::Project,
        Section::Tools,
        Section::System,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Section::System => "system",
            Section::Project => "project",
            Section::Skill => "skill",
            Section::Tools => "tools",
        }
    }
}

/// Roughly four characters per token, the same method the context estimator uses. The
/// report names the method so a future estimator can change it deliberately.
pub const CHARS_PER_TOKEN: usize = 4;
pub const ESTIMATE_METHOD: &str = "chars/4";

#[derive(Debug, Clone, Default)]
pub struct Sections {
    pub system: String,
    pub project: String,
    pub skill: String,
    pub tools: String,
}

impl Sections {
    pub fn get(&self, section: Section) -> &str {
        match section {
            Section::System => &self.system,
            Section::Project => &self.project,
            Section::Skill => &self.skill,
            Section::Tools => &self.tools,
        }
    }

    fn get_mut(&mut self, section: Section) -> &mut String {
        match section {
            Section::System => &mut self.system,
            Section::Project => &mut self.project,
            Section::Skill => &mut self.skill,
            Section::Tools => &mut self.tools,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SectionReport {
    pub section: Section,
    pub chars: usize,
    pub tokens: usize,
    pub budget_chars: usize,
    pub dropped_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrimRecord {
    pub section: Section,
    pub original_chars: usize,
    pub kept_chars: usize,
    pub dropped_chars: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptReport {
    pub sections: Vec<SectionReport>,
    pub trims: Vec<TrimRecord>,
    pub total_chars: usize,
    pub total_tokens: usize,
    pub method: String,
}

impl PromptReport {
    pub fn section(&self, section: Section) -> Option<&SectionReport> {
        self.sections.iter().find(|row| row.section == section)
    }

    pub fn within_budget(&self) -> bool {
        self.trims.is_empty()
    }
}

/// Per-section character budgets. The total is deliberately below the smallest window
/// Rigel supports, leaving room for history and the answer.
pub fn default_budgets() -> BTreeMap<Section, usize> {
    BTreeMap::from([
        (Section::System, 12_000),
        (Section::Project, 8_000),
        (Section::Skill, 6_000),
        (Section::Tools, 10_000),
    ])
}

/// Assemble the prompt with the default budgets.
pub fn assemble(sections: &Sections) -> (String, PromptReport) {
    assemble_with(sections, &default_budgets())
}

/// Assemble the prompt, trimming each over-budget section in [`Section::TRIM_ORDER`].
pub fn assemble_with(
    sections: &Sections,
    budgets: &BTreeMap<Section, usize>,
) -> (String, PromptReport) {
    let mut working = sections.clone();
    let mut trims = Vec::new();

    for section in Section::TRIM_ORDER {
        let budget = budgets.get(&section).copied().unwrap_or(usize::MAX);
        let text = working.get(section);
        let chars = text.chars().count();
        if chars <= budget {
            continue;
        }
        let marker = format!(
            "\n[trimmed: {} chars from the {} section]",
            chars - budget,
            section.as_str()
        );
        let keep = budget.saturating_sub(marker.chars().count());
        let kept = text.chars().take(keep).collect::<String>();
        let kept_chars = kept.chars().count();
        *working.get_mut(section) = format!("{kept}{marker}");
        trims.push(TrimRecord {
            section,
            original_chars: chars,
            kept_chars,
            dropped_chars: chars - kept_chars,
            reason: format!(
                "{} section exceeded its {} character budget; trimmed in documented order",
                section.as_str(),
                budget
            ),
        });
    }

    let mut rows = Vec::new();
    let mut text = String::new();
    for section in Section::ALL {
        let body = working.get(section);
        let chars = body.chars().count();
        let dropped = trims
            .iter()
            .find(|trim| trim.section == section)
            .map(|trim| trim.dropped_chars)
            .unwrap_or(0);
        rows.push(SectionReport {
            section,
            chars,
            tokens: chars.div_ceil(CHARS_PER_TOKEN),
            budget_chars: budgets.get(&section).copied().unwrap_or(usize::MAX),
            dropped_chars: dropped,
        });
        if !body.trim().is_empty() {
            text.push_str(body);
            if !body.ends_with('\n') {
                text.push('\n');
            }
        }
    }

    let total_chars = text.chars().count();
    (
        text,
        PromptReport {
            sections: rows,
            trims,
            total_chars,
            total_tokens: total_chars.div_ceil(CHARS_PER_TOKEN),
            method: ESTIMATE_METHOD.to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(chars: usize) -> String {
        "x".repeat(chars)
    }

    #[test]
    fn each_section_reports_its_own_characters_and_tokens() {
        let sections = Sections {
            system: long(40),
            project: long(80),
            skill: long(12),
            tools: long(4),
        };
        let (text, report) = assemble(&sections);
        assert_eq!(text.chars().count(), 140);
        assert_eq!(report.section(Section::System).unwrap().tokens, 10);
        assert_eq!(report.section(Section::Project).unwrap().tokens, 20);
        assert_eq!(report.section(Section::Tools).unwrap().tokens, 1);
        assert_eq!(report.method, ESTIMATE_METHOD);
        assert!(report.within_budget());
    }

    #[test]
    fn an_over_budget_skill_section_is_trimmed_first_and_the_system_survives() {
        let budgets = BTreeMap::from([
            (Section::System, 100),
            (Section::Project, 100),
            (Section::Skill, 20),
            (Section::Tools, 100),
        ]);
        let sections = Sections {
            system: "SAFETY RULES".to_string(),
            project: "project facts".to_string(),
            skill: long(200),
            tools: "[]".to_string(),
        };
        let (text, report) = assemble_with(&sections, &budgets);
        assert_eq!(report.trims.len(), 1);
        let trim = &report.trims[0];
        assert_eq!(trim.section, Section::Skill);
        assert_eq!(trim.original_chars, 200);
        assert!(trim.dropped_chars > 0);
        assert!(trim.reason.contains("documented order"));
        assert!(text.contains("SAFETY RULES"));
        assert!(text.contains("[trimmed:"));
        assert!(text.chars().count() < 400);
        assert_eq!(report.section(Section::System).unwrap().dropped_chars, 0);
    }

    #[test]
    fn a_section_within_budget_is_byte_identical() {
        let budgets = BTreeMap::from([(Section::Skill, 1_000)]);
        let sections = Sections {
            skill: "keep every byte".to_string(),
            ..Sections::default()
        };
        let (text, report) = assemble_with(&sections, &budgets);
        assert_eq!(text, "keep every byte\n");
        assert!(report.trims.is_empty());
    }

    #[test]
    fn an_over_budget_system_is_trimmed_last_and_still_names_the_trim() {
        let budgets = BTreeMap::from([(Section::System, 50)]);
        let sections = Sections {
            system: long(500),
            ..Sections::default()
        };
        let (_, report) = assemble_with(&sections, &budgets);
        assert_eq!(report.trims.len(), 1);
        assert_eq!(report.trims[0].section, Section::System);
    }
}

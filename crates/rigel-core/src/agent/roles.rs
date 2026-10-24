//! D94: the four role presets, each a saved prompt, model/effort pair and tool allowance.
//!
//! A role is a name on an agent (`agents.role`), so applying a preset from the roster only
//! changes that row. The prompt and tools are looked up again on every turn, which means an
//! edited preset applies to the next turn without rewriting history.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RolePreset {
    pub id: &'static str,
    pub title: &'static str,
    pub brief: &'static str,
    pub model: &'static str,
    pub effort: &'static str,
    /// The only tools this role is offered, by registry name.
    pub tools: &'static [&'static str],
}

pub const PRESETS: &[RolePreset] = &[
    RolePreset {
        id: "implementer",
        title: "Implementer",
        brief: "Change the code, then say exactly what changed and what you verified. Prefer the smallest edit that works; do not refactor beyond the task.",
        model: "openai/gpt-5.4-mini",
        effort: "medium",
        tools: &[
            "read_file",
            "list_files",
            "edit_file",
            "write_file",
            "run_command",
            "skills_search",
            "skill_load",
            "budget",
        ],
    },
    RolePreset {
        id: "reviewer",
        title: "Reviewer",
        brief: "Read for correctness, regressions and missing tests. Report findings with file paths and line numbers; do not rewrite the author's work unless asked.",
        model: "openai/gpt-5.4-mini",
        effort: "high",
        tools: &[
            "read_file",
            "list_files",
            "run_command",
            "skills_search",
            "skill_load",
            "budget",
        ],
    },
    RolePreset {
        id: "tester",
        title: "Tester",
        brief: "Build the smallest test that would fail before the change and pass after it. Run it, and report the exact command and exit status you saw.",
        model: "openai/gpt-5-nano",
        effort: "low",
        tools: &["read_file", "list_files", "run_command", "budget"],
    },
    RolePreset {
        id: "researcher",
        title: "Researcher",
        brief: "Answer from the project first and the web only when the project cannot. Cite the file or URL for every claim, and separate what you read from what you inferred.",
        model: "openai/gpt-5-nano",
        effort: "low",
        tools: &[
            "read_file",
            "list_files",
            "fetch_url",
            "skills_search",
            "skill_load",
            "budget",
        ],
    },
];

pub fn preset(id: Option<&str>) -> Option<&'static RolePreset> {
    let id = id?.trim().to_ascii_lowercase();
    PRESETS.iter().find(|preset| preset.id == id)
}

/// The preset's instruction, appended to the agent's normal system prompt.
pub fn guidance(id: Option<&str>) -> Option<String> {
    let preset = preset(id)?;
    Some(format!(
        "Role preset `{}` (D94): {} Tools for this role: {}.",
        preset.id,
        preset.brief,
        preset.tools.join(", ")
    ))
}

/// Layer a role's instruction over the base prompt without changing the base rules.
pub fn with_guidance(base: &str, id: Option<&str>) -> String {
    match guidance(id) {
        Some(guidance) => format!("{base}\n\n{guidance}"),
        None => base.to_string(),
    }
}

/// The tool allowance for a role, or `None` for a custom role (all normal tools).
pub fn tools(id: Option<&str>) -> Option<Vec<String>> {
    preset(id).map(|preset| preset.tools.iter().map(|tool| tool.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_presets_are_complete_and_distinct() {
        assert_eq!(PRESETS.len(), 4);
        let ids: Vec<_> = PRESETS.iter().map(|preset| preset.id).collect();
        assert_eq!(ids, ["implementer", "reviewer", "tester", "researcher"]);
        for preset in PRESETS {
            assert!(!preset.brief.is_empty());
            assert!(!preset.model.is_empty());
            assert_eq!(
                preset.id == "implementer",
                preset.tools.contains(&"edit_file"),
                "only the implementer may write"
            );
        }
    }

    #[test]
    fn lookup_is_case_insensitive_and_unknown_roles_are_open() {
        assert_eq!(preset(Some(" REVIEWER ")).unwrap().id, "reviewer");
        assert!(preset(Some("custom")).is_none());
        assert!(tools(Some("custom")).is_none());
        assert!(with_guidance("base", Some("tester")).contains("Role preset `tester`"));
    }
}

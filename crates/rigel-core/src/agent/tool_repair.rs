//! One bounded repair for a tool call (roadmap 50).
//!
//! Models make three predictable mistakes: arguments arrive with text around the JSON, the tool
//! name is close but not exact, or an argument uses the provider's favourite spelling. This
//! module makes exactly one deterministic correction and says what it changed. It never invents
//! a missing required argument, and a corrected call that still fails is recorded with its raw
//! name and arguments so the transcript keeps the evidence.

use serde_json::{Map, Value};

use super::model::{ToolCall, ToolSpec};

#[derive(Debug, Clone, PartialEq)]
pub struct Repair {
    pub name: String,
    pub arguments: Value,
    pub reason: String,
    pub raw_name: String,
    pub raw_arguments: String,
}

/// Aliases a model uses often. They are only accepted when the target tool is actually offered.
const NAME_ALIASES: &[(&str, &str)] = &[
    ("read", "read_file"),
    ("readfile", "read_file"),
    ("open_file", "read_file"),
    ("list", "list_files"),
    ("ls", "list_files"),
    ("edit", "edit_file"),
    ("write", "write_file"),
    ("bash", "run_command"),
    ("shell", "run_command"),
    ("exec", "run_command"),
    ("search_skills", "skills_search"),
    ("load_skill", "skill_load"),
    ("send_email", "email_send"),
];

const STRING_FIELDS: &[&str] = &[
    "path",
    "command",
    "content",
    "old_string",
    "new_string",
    "query",
    "skill_id",
    "url",
    "topic",
    "to",
    "subject",
    "text",
];

/// Return the one repair worth attempting, or `None` when the call is already as exact as we
/// can make it. The caller executes at most one returned call.
pub fn repair(call: &ToolCall, specs: &[ToolSpec]) -> Option<Repair> {
    let mut reasons = Vec::new();
    let name = repair_name(&call.name, specs, &mut reasons);
    let mut arguments = parse_loose(&call.arguments, &mut reasons);
    if let Some(object) = arguments.as_object_mut() {
        unwrap_envelope(object, &mut reasons);
        alias_fields(object, &name, &mut reasons);
        stringify_fields(object, &mut reasons);
    }
    if reasons.is_empty() {
        return None;
    }
    Some(Repair {
        name,
        arguments,
        reason: reasons.join("; "),
        raw_name: call.name.clone(),
        raw_arguments: call.arguments.clone(),
    })
}

fn repair_name(name: &str, specs: &[ToolSpec], reasons: &mut Vec<String>) -> String {
    let trimmed = name.trim();
    if specs.iter().any(|spec| spec.name == trimmed) {
        return trimmed.to_string();
    }
    let normalized = normalize(trimmed);
    let aliased = NAME_ALIASES
        .iter()
        .find(|(alias, _)| normalize(alias) == normalized)
        .map(|(_, target)| *target);
    let found = aliased
        .filter(|target| specs.iter().any(|spec| spec.name == *target))
        .or_else(|| {
            specs
                .iter()
                .find(|spec| normalize(&spec.name) == normalized)
                .map(|spec| spec.name.as_str())
        });
    match found {
        Some(target) => {
            reasons.push(format!("unknown tool {name:?} -> {target}"));
            target.to_string()
        }
        None => trimmed.to_string(),
    }
}

fn normalize(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .replace(['-', ' ', '.'], "_")
}

/// Parse JSON that may be wrapped in a code fence or surrounded by prose. This is deliberately
/// narrow: it extracts a first complete object rather than rewriting quotes or inventing keys.
fn parse_loose(raw: &str, reasons: &mut Vec<String>) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        reasons.push("empty arguments -> {}".to_string());
        return serde_json::json!({});
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(text) = value.as_str() {
            // Some providers send the argument object as a JSON string, sometimes with a fence
            // or prose inside it. Recurse once on the decoded text, not on the quoted form.
            let inner = parse_loose(text, reasons);
            if inner.get("raw").is_none() || !inner["raw"].is_string() {
                reasons.push("double-encoded arguments unwrapped".to_string());
                return inner;
            }
        }
        return value;
    }

    let unfenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|text| text.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    if let Ok(value) = serde_json::from_str::<Value>(unfenced) {
        reasons.push("markdown fence removed from arguments".to_string());
        return value;
    }

    if let Some(start) = unfenced.find('{')
        && let Some(end) = unfenced.rfind('}')
        && end > start
        && let Ok(value) = serde_json::from_str::<Value>(&unfenced[start..=end])
    {
        reasons.push("surrounding text removed from arguments".to_string());
        return value;
    }
    serde_json::json!({ "raw": raw })
}

fn unwrap_envelope(object: &mut Map<String, Value>, reasons: &mut Vec<String>) {
    if object.len() != 1 {
        return;
    }
    for key in ["arguments", "input", "params", "parameters"] {
        if let Some(Value::Object(inner)) = object.get(key) {
            let inner = inner.clone();
            *object = inner;
            reasons.push(format!("arguments envelope {key:?} unwrapped"));
            return;
        }
    }
}

fn alias_fields(object: &mut Map<String, Value>, name: &str, reasons: &mut Vec<String>) {
    let aliases: &[(&str, &str)] = match name {
        "read_file" | "list_files" | "edit_file" | "write_file" => &[
            ("file_path", "path"),
            ("filepath", "path"),
            ("filename", "path"),
        ],
        "run_command" => &[("cmd", "command"), ("command_line", "command")],
        "skill_load" => &[("skill", "skill_id"), ("name", "skill_id")],
        _ => &[],
    };
    for (alias, canonical) in aliases {
        if object.contains_key(*canonical) {
            continue;
        }
        if let Some(value) = object.remove(*alias) {
            object.insert((*canonical).to_string(), value);
            reasons.push(format!("argument {alias:?} -> {canonical:?}"));
        }
    }
}

fn stringify_fields(object: &mut Map<String, Value>, reasons: &mut Vec<String>) {
    for field in STRING_FIELDS {
        let Some(value) = object.get_mut(*field) else {
            continue;
        };
        if value.is_number() || value.is_boolean() {
            *value = Value::String(value.to_string());
            reasons.push(format!("argument {field:?} was not a string"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::Tools;

    fn specs() -> Vec<ToolSpec> {
        Tools::specs_all()
    }

    fn call(name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }
    }

    #[test]
    fn a_close_name_and_provider_spelling_are_repaired_once() {
        let repair = repair(&call("Read-File", r#"{"file_path":"README.md"}"#), &specs()).unwrap();
        assert_eq!(repair.name, "read_file");
        assert_eq!(repair.arguments["path"], "README.md");
        assert!(repair.reason.contains("unknown tool"));
        assert!(repair.reason.contains("file_path"));
        assert_eq!(repair.raw_name, "Read-File");
    }

    #[test]
    fn arguments_surrounded_by_prose_or_a_fence_are_recovered() {
        let fenced = repair(
            &call("read_file", "```json\n{\"path\":\"README.md\"}\n```"),
            &specs(),
        )
        .unwrap();
        assert_eq!(fenced.arguments["path"], "README.md");

        let prose = repair(
            &call(
                "read_file",
                "Here is the call: {\"path\":\"README.md\"} thanks",
            ),
            &specs(),
        )
        .unwrap();
        assert_eq!(prose.arguments["path"], "README.md");
    }

    #[test]
    fn an_argument_envelope_and_a_number_are_normalised() {
        let repair = repair(
            &call("read_file", r#"{"arguments":{"file_path":42}}"#),
            &specs(),
        )
        .unwrap();
        assert_eq!(repair.arguments["path"], "42");
        assert!(repair.reason.contains("envelope"));
    }

    #[test]
    fn an_exact_call_is_not_repaired() {
        assert!(repair(&call("read_file", r#"{"path":"README.md"}"#), &specs()).is_none());
    }

    #[test]
    fn an_unrepairable_name_or_body_stays_raw() {
        assert!(repair(&call("frobnicate", "{}"), &specs()).is_none());
        assert!(
            repair(&call("read_file", "{\"path\":"), &specs()).is_none(),
            "unrecoverable JSON is reported, not guessed at"
        );
    }
}

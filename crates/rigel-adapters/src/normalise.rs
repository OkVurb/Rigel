//! P4.8: one vocabulary for the ten item kinds.
//!
//! Every harness speaks its own wire format, but a transcript must not care which one produced a
//! line: a Claude `Bash` call, a Codex `commandExecution` item and an ACP `execute` tool call are
//! the same thing to the reader ("a command ran"), and §5.2 fixes the ten kinds they can be.
//!
//! This module is the single table that answers "which kind is this?" per harness, plus the
//! constructor set that builds item bodies in the shape the native agent already writes. Two rules
//! from §5.2 hold everywhere:
//!
//! - every decodable event becomes one of the ten kinds;
//! - anything that does not map becomes [`ItemKind::Error`] with `body.unmapped = true` and the
//!   raw event under `body.raw`, so a debug pane can show it and nothing is silently dropped.

use rigel_core::ItemKind;
use serde_json::{Map, Value, json};

/// A decoded harness event already sorted into one of the ten kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalItem {
    pub kind: ItemKind,
    pub body: Value,
}

impl NormalItem {
    pub fn new(kind: ItemKind, body: Value) -> Self {
        Self {
            kind,
            body: match body {
                Value::Object(map) => Value::Object(map),
                other => json!({ "value": other }),
            },
        }
    }

    /// Attach fields the adapter knows and the shared vocabulary does not, without replacing what
    /// is already there. Adapter-specific keys lose to nothing: the caller's own fields win only
    /// when the base body is empty.
    pub fn with(mut self, extra: Value) -> Self {
        self.body = merge(self.body, extra);
        self
    }

    pub fn message(text: impl Into<String>) -> Self {
        Self::new(ItemKind::Message, json!({ "text": text.into() }))
    }

    pub fn reasoning(text: impl Into<String>) -> Self {
        Self::new(ItemKind::Reasoning, json!({ "text": text.into() }))
    }

    pub fn command(command: impl Into<String>) -> Self {
        Self::new(ItemKind::Command, json!({ "command": command.into() }))
    }

    /// `operation` is the harness's own word (`read`, `write`, `delete`, `move`), kept verbatim so
    /// the transcript can say what happened to the path without inventing a second vocabulary.
    pub fn file_change(operation: &str, path: Option<&str>) -> Self {
        let mut body = Map::new();
        body.insert("operation".into(), json!(operation));
        if let Some(path) = path {
            body.insert("path".into(), json!(path));
        }
        Self::new(ItemKind::FileChange, Value::Object(body))
    }

    pub fn mcp_call(server: &str, tool: &str, arguments: Value) -> Self {
        Self::new(
            ItemKind::McpCall,
            json!({ "server": server, "tool": tool, "arguments": arguments }),
        )
    }

    pub fn web_search(query: &str) -> Self {
        Self::new(ItemKind::WebSearch, json!({ "query": query }))
    }

    pub fn plan(entries: Vec<String>) -> Self {
        Self::new(ItemKind::Plan, json!({ "entries": entries }))
    }

    pub fn approval(tool_call_id: &str, title: &str) -> Self {
        Self::new(
            ItemKind::Approval,
            json!({ "tool_call_id": tool_call_id, "title": title }),
        )
    }

    pub fn usage(body: Value) -> Self {
        Self::new(ItemKind::Usage, body)
    }

    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self::new(
            ItemKind::Error,
            json!({ "code": code, "message": message.into() }),
        )
    }

    /// The §5.2 fallback: keep the event, label it, and let a debug pane show the wire shape.
    pub fn unmapped(harness: &str, raw: Value, reason: &str) -> Self {
        Self::new(
            ItemKind::Error,
            json!({ "unmapped": true, "harness": harness, "reason": reason, "raw": raw }),
        )
    }

    pub fn is_unmapped(&self) -> bool {
        self.body.get("unmapped").and_then(Value::as_bool) == Some(true)
    }
}

fn merge(base: Value, extra: Value) -> Value {
    match (base, extra) {
        (Value::Object(mut base), Value::Object(extra)) => {
            for (key, value) in extra {
                base.entry(key).or_insert(value);
            }
            Value::Object(base)
        }
        (base, _) => base,
    }
}

/// Claude Code's builtin tools, mapped to a kind and the operation word the item carries.
///
/// Tool names come from the CLI's own catalogue. An MCP tool keeps its `mcp__<server>__<tool>`
/// name, which is exactly what makes it a `mcp_call`; anything else unknown is `None` and the
/// caller records it as unmapped rather than guessing.
pub fn claude_tool(name: &str) -> Option<(ItemKind, &'static str)> {
    let mapping = match name {
        "Bash" | "BashOutput" | "KillShell" => (ItemKind::Command, "execute"),
        "Read" | "NotebookRead" => (ItemKind::FileChange, "read"),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => (ItemKind::FileChange, "write"),
        "WebFetch" => (ItemKind::WebSearch, "fetch"),
        "WebSearch" => (ItemKind::WebSearch, "search"),
        "TodoWrite" => (ItemKind::Plan, "plan"),
        "Task" => (ItemKind::Plan, "subagent"),
        _ if name.starts_with("mcp__") => (ItemKind::McpCall, "mcp"),
        _ => return None,
    };
    Some(mapping)
}

/// Split an MCP tool name (`mcp__server__tool`) into its two halves.
pub fn mcp_parts(name: &str) -> (&str, &str) {
    let rest = name.strip_prefix("mcp__").unwrap_or(name);
    match rest.split_once("__") {
        Some((server, tool)) => (server, tool),
        None => ("mcp", rest),
    }
}

/// Codex app-server item types (`camelCase`, as the protocol JSON names them).
pub fn codex_item(item_type: &str) -> Option<ItemKind> {
    Some(match item_type {
        "agentMessage" => ItemKind::Message,
        "reasoning" => ItemKind::Reasoning,
        "commandExecution" => ItemKind::Command,
        "fileChange" => ItemKind::FileChange,
        "mcpToolCall" => ItemKind::McpCall,
        "webSearch" => ItemKind::WebSearch,
        "plan" | "updatePlan" | "todoList" => ItemKind::Plan,
        "error" => ItemKind::Error,
        _ => return None,
    })
}

/// `codex exec --json` names the same items in `snake_case`; the fallback transport must not lose
/// a command just because it spells the type differently.
pub fn codex_exec_item(item_type: &str) -> Option<ItemKind> {
    let camel = match item_type {
        "agent_message" => "agentMessage",
        "command_execution" => "commandExecution",
        "file_change" => "fileChange",
        "mcp_tool_call" => "mcpToolCall",
        "web_search" => "webSearch",
        "todo_list" => "todoList",
        other => other,
    };
    codex_item(camel)
}

/// ACP's `kind` field on a tool call. Every documented value maps; an absent or unknown kind
/// still describes a tool that ran, so it stays a `command`, which is the ACP client's existing
/// behaviour and the only kind that does not claim more than was seen.
pub fn acp_tool(kind: Option<&str>) -> (ItemKind, &'static str) {
    match kind {
        Some("execute") => (ItemKind::Command, "execute"),
        Some("read") => (ItemKind::FileChange, "read"),
        Some("edit") => (ItemKind::FileChange, "write"),
        Some("delete") => (ItemKind::FileChange, "delete"),
        Some("move") => (ItemKind::FileChange, "move"),
        Some("search") => (ItemKind::WebSearch, "search"),
        Some("fetch") => (ItemKind::WebSearch, "fetch"),
        Some("think") => (ItemKind::Reasoning, "think"),
        _ => (ItemKind::Command, "execute"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_one_of_the_ten_kinds_can_be_built() {
        let items = vec![
            NormalItem::message("hi"),
            NormalItem::reasoning("thinking"),
            NormalItem::command("cargo test"),
            NormalItem::file_change("write", Some("src/lib.rs")),
            NormalItem::mcp_call("files", "read", json!({})),
            NormalItem::web_search("rustfmt"),
            NormalItem::plan(vec!["one".into(), "two".into()]),
            NormalItem::approval("call-1", "Run the tests"),
            NormalItem::usage(json!({ "used": 1, "size": 2 })),
            NormalItem::error("DENIED", "the user said no"),
        ];
        let kinds = items.iter().map(|item| item.kind).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                ItemKind::Message,
                ItemKind::Reasoning,
                ItemKind::Command,
                ItemKind::FileChange,
                ItemKind::McpCall,
                ItemKind::WebSearch,
                ItemKind::Plan,
                ItemKind::Approval,
                ItemKind::Usage,
                ItemKind::Error,
            ]
        );
        // Ten distinct kinds, so the table above is not passing by repeating one.
        let mut unique = kinds.clone();
        unique.dedup();
        assert_eq!(unique.len(), 10);
    }

    #[test]
    fn an_unknown_event_is_kept_as_an_unmapped_error_with_its_raw_payload() {
        let raw = json!({ "type": "brandNewThing", "value": 7 });
        let item = NormalItem::unmapped("claude", raw.clone(), "no kind for `brandNewThing`");
        assert_eq!(item.kind, ItemKind::Error);
        assert!(item.is_unmapped());
        assert_eq!(item.body["raw"], raw);
        assert_eq!(item.body["harness"], "claude");
    }

    #[test]
    fn claude_tools_map_to_the_documented_kinds() {
        assert_eq!(claude_tool("Bash"), Some((ItemKind::Command, "execute")));
        assert_eq!(claude_tool("Edit"), Some((ItemKind::FileChange, "write")));
        assert_eq!(claude_tool("Read"), Some((ItemKind::FileChange, "read")));
        assert_eq!(
            claude_tool("WebFetch"),
            Some((ItemKind::WebSearch, "fetch"))
        );
        assert_eq!(claude_tool("TodoWrite"), Some((ItemKind::Plan, "plan")));
        assert_eq!(
            claude_tool("mcp__files__read"),
            Some((ItemKind::McpCall, "mcp"))
        );
        // An unknown builtin is not guessed into a neighbouring kind.
        assert_eq!(claude_tool("SomeFutureTool"), None);
    }

    #[test]
    fn mcp_names_split_into_server_and_tool() {
        assert_eq!(mcp_parts("mcp__files__read"), ("files", "read"));
        assert_eq!(mcp_parts("mcp__files"), ("mcp", "files"));
        assert_eq!(mcp_parts("Bash"), ("mcp", "Bash"));
    }

    #[test]
    fn codex_spells_the_same_items_two_ways() {
        for (app_server, exec) in [
            ("agentMessage", "agent_message"),
            ("commandExecution", "command_execution"),
            ("fileChange", "file_change"),
            ("mcpToolCall", "mcp_tool_call"),
            ("webSearch", "web_search"),
            ("todoList", "todo_list"),
        ] {
            assert_eq!(
                codex_item(app_server),
                codex_exec_item(exec),
                "{app_server}"
            );
        }
        assert_eq!(codex_item("commandExecution"), Some(ItemKind::Command));
        assert_eq!(codex_item("nonsense"), None);
    }

    #[test]
    fn acp_tool_kinds_cover_the_published_vocabulary() {
        assert_eq!(acp_tool(Some("execute")), (ItemKind::Command, "execute"));
        assert_eq!(acp_tool(Some("edit")), (ItemKind::FileChange, "write"));
        assert_eq!(acp_tool(Some("delete")), (ItemKind::FileChange, "delete"));
        assert_eq!(acp_tool(Some("move")), (ItemKind::FileChange, "move"));
        assert_eq!(acp_tool(Some("search")), (ItemKind::WebSearch, "search"));
        assert_eq!(acp_tool(Some("fetch")), (ItemKind::WebSearch, "fetch"));
        assert_eq!(acp_tool(Some("think")), (ItemKind::Reasoning, "think"));
        // Absent is still a tool that ran; it does not become an error.
        assert_eq!(acp_tool(None), (ItemKind::Command, "execute"));
    }

    #[test]
    fn with_keeps_the_fields_it_already_has() {
        let item = NormalItem::command("cargo test").with(json!({
            "command": "something else",
            "exit_code": 0
        }));
        assert_eq!(item.body["command"], "cargo test");
        assert_eq!(item.body["exit_code"], 0);
    }
}

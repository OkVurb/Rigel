//! Tightening a composer draft before it is sent.
//!
//! One cheap model call reads the draft, the target and — in the room — the last few messages.
//! It returns at most three questions and a proposed brief, or names up to three project files
//! it needs; those are read under the project root and fed back for a second, final call.
//!
//! The refiner has no tools, cannot write, and never sends anything: the window shows its answer
//! as a review card and the send button stays the user's. Every call is metered, and the usage
//! travels back with the refinement so the card can say what asking cost — the saving sought is
//! fewer wrong turns, not a free rewrite.

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::model::{ChatMessage, ChatRequest, Delta, Model, usage_from_raw};
use crate::domain::Usage;

const MAX_DRAFT_CHARS: usize = 4_000;
const MAX_REFINED_CHARS: usize = 6_000;
const MAX_CONTEXT_MESSAGES: usize = 6;
const MAX_CONTEXT_CHARS: usize = 400;
const MAX_ANSWERS: usize = 4;
const MAX_QUESTIONS: usize = 3;
const MAX_READS: usize = 3;
const MAX_READ_BYTES: usize = 8 * 1024;
/// Outermost round is 0; three rounds is the most a draft may be refined.
pub const MAX_ROUNDS: u8 = 3;

const SYSTEM: &str = "You tighten a prompt before it is sent to a coding agent. You have no \
tools and cannot read, run or change anything except the files the caller lists for you.

Return JSON only:
{\"questions\":[\"...\"],\"refined\":\"...\",\"reads\":[\"relative/path\"]}

Rules:
- Ask at most 3 questions, shortest first, and only where the answer changes what the agent \
will do. Never ask what is already clear from the draft, the target or the recent messages. Do \
not ask questions you could answer yourself. If nothing is genuinely ambiguous, return an empty \
list.
- Rewrite the draft as a compact imperative brief for the target agent. Keep the user's intent, \
constraints and named files exactly. Add nothing they did not imply. Prefer short sentences and \
concrete verbs over explanation. Do not pad; if the draft is already precise, keep it short.
- If one to three project files would let you ask a better question, list their \
project-relative paths in reads. Otherwise leave reads empty. Never ask for secrets, .env \
files, .git, node_modules or target.
- The user sees your questions and can answer them; you will be called again with those answers. \
The refined brief is a proposal, not a message you send.";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RefineRequest {
    pub draft: String,
    /// Who will receive the brief: `@claude`, `Claude Code`, `Rigel native`.
    pub target: String,
    pub channel: Option<String>,
    /// Recent room or turn messages, already flattened to `sender: text`.
    pub recent: Vec<String>,
    /// The user's answers to the previous round's questions.
    pub answers: Vec<String>,
    pub round: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct Refinement {
    pub questions: Vec<String>,
    pub refined: String,
    /// Files actually read for this refinement, project-relative.
    pub reads: Vec<String>,
    pub read_notes: Vec<String>,
    /// Anything the caller should know: a format fallback, a refused path, a skipped file.
    pub notes: Vec<String>,
    pub usage: Usage,
    pub model: String,
}

/// Runs the refiner on the cheap model the caller supplies. It owns no keys and no project.
pub struct PromptRefiner {
    model: Arc<dyn Model>,
    model_name: String,
}

impl PromptRefiner {
    pub fn new(model: Arc<dyn Model>, model_name: impl Into<String>) -> Self {
        Self {
            model,
            model_name: model_name.into(),
        }
    }

    /// One refinement round. `round` 0 may read files; later rounds only see the draft, context
    /// and the answers, so a refinement cannot walk the repository.
    pub async fn refine(
        &self,
        request: &RefineRequest,
        project_root: Option<&Path>,
    ) -> Result<Refinement, String> {
        if request.round >= MAX_ROUNDS {
            return Err(format!(
                "this draft has already been refined {MAX_ROUNDS} times; send it or start over"
            ));
        }
        let draft = clip(&request.draft, MAX_DRAFT_CHARS);
        if draft.is_empty() {
            return Err("there is nothing to refine".to_string());
        }
        let request = RefineRequest {
            draft,
            answers: request
                .answers
                .iter()
                .take(MAX_ANSWERS)
                .map(|answer| clip(answer, MAX_CONTEXT_CHARS))
                .filter(|answer| !answer.is_empty())
                .collect(),
            ..request.clone()
        };

        let mut usage = Usage::default();
        let mut notes = Vec::new();

        let (text, call_usage) = self.call(user_message(&request, &[])).await?;
        usage.add(&call_usage);
        let (mut reply, parse_note) = parse_reply(&text);
        if let Some(note) = parse_note {
            notes.push(note);
        }

        let mut read_notes = Vec::new();
        let mut reads = Vec::new();
        if !reply.reads.is_empty() {
            if request.round > 0 {
                notes.push(
                    "it asked for more files, but files are read on the first round only"
                        .to_string(),
                );
            } else if let Some(root) = project_root {
                let (snippets, file_notes) = read_requested(root, &reply.reads);
                read_notes = file_notes;
                if snippets.is_empty() {
                    notes.push(
                        "no requested file could be read; the brief was written without them"
                            .to_string(),
                    );
                } else {
                    let (second_text, second_usage) =
                        self.call(user_message(&request, &snippets)).await?;
                    usage.add(&second_usage);
                    let (second, second_note) = parse_reply(&second_text);
                    if let Some(note) = second_note {
                        notes.push(note);
                    }
                    reply = second;
                    reads = snippets.iter().map(|(path, _)| path.clone()).collect();
                    if !reply.reads.is_empty() {
                        notes.push("only one round of file reads is allowed".to_string());
                    }
                }
            } else {
                notes.push("it asked for files, but no project is open".to_string());
            }
        }

        if reply.refined.trim().is_empty() {
            reply.refined = request.draft.clone();
            notes.push("the model returned no rewrite; the draft is unchanged".to_string());
        }

        Ok(Refinement {
            questions: reply.questions,
            refined: clip(&reply.refined, MAX_REFINED_CHARS),
            reads,
            read_notes,
            notes,
            usage,
            model: self.model_name.clone(),
        })
    }

    async fn call(&self, user: String) -> Result<(String, Usage), String> {
        let request = ChatRequest {
            model: self.model_name.clone(),
            messages: vec![ChatMessage::system(SYSTEM), ChatMessage::user(user)],
            tools: Vec::new(),
            max_tokens: Some(700),
            temperature: 0.2,
            effort: None,
        };
        let cancel = CancellationToken::new();
        let mut text = String::new();
        let mut usage = Usage::default();
        {
            let mut sink = |delta: Delta| match delta {
                Delta::Text(piece) => text.push_str(&piece),
                Delta::Usage(raw) => usage = usage_from_raw(raw, &self.model_name),
                _ => {}
            };
            self.model
                .chat(request, &cancel, &mut sink)
                .await
                .map_err(|error| format!("the refiner could not answer: {error}"))?;
        }
        Ok((text, usage))
    }
}

fn user_message(request: &RefineRequest, reads: &[(String, String)]) -> String {
    let mut out = String::new();
    out.push_str("The draft goes to: ");
    out.push_str(if request.target.trim().is_empty() {
        "a coding agent"
    } else {
        request.target.trim()
    });
    out.push('\n');
    if let Some(channel) = request.channel.as_deref().filter(|name| !name.is_empty()) {
        out.push_str(&format!("Channel: #{channel}\n"));
    }
    out.push_str("\nThe draft:\n");
    out.push_str(&request.draft);
    out.push('\n');

    let recent: Vec<&String> = request
        .recent
        .iter()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(MAX_CONTEXT_MESSAGES)
        .collect();
    if !recent.is_empty() {
        out.push_str("\nRecent messages, oldest first:\n");
        for line in recent.into_iter().rev() {
            out.push_str("  - ");
            out.push_str(&clip(line, MAX_CONTEXT_CHARS));
            out.push('\n');
        }
    }

    if !request.answers.is_empty() {
        out.push_str("\nThe user answered your questions:\n");
        for (index, answer) in request.answers.iter().enumerate() {
            out.push_str(&format!("  {}. {}\n", index + 1, answer));
        }
    }

    if !reads.is_empty() {
        out.push_str("\nFiles you asked for:\n");
        for (path, text) in reads {
            out.push_str(&format!("\n--- {path} ---\n{text}\n"));
        }
    }

    out.push_str(
        "\nReturn the JSON object now. `reads` must be empty on this call when files are present.",
    );
    out
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Reply {
    questions: Vec<String>,
    refined: String,
    reads: Vec<String>,
}

/// Models wrap JSON in prose or fences often enough that a strict parse would throw away a
/// usable answer. One object is accepted anywhere in the reply; plain text is used as the
/// rewrite with a note, never dropped.
fn parse_reply(text: &str) -> (Reply, Option<String>) {
    let trimmed = text.trim();
    let body = if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.split_once('\n').map(|(_, rest)| rest).unwrap_or(rest);
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    };

    if let (Some(start), Some(end)) = (body.find('{'), body.rfind('}')) {
        if end > start {
            if let Ok(reply) = serde_json::from_str::<Reply>(&body[start..=end]) {
                return (capped(reply), None);
            }
        }
    }

    (
        Reply {
            refined: trimmed.to_string(),
            ..Reply::default()
        },
        Some("the model did not return JSON; its text was used as the rewrite".to_string()),
    )
}

fn capped(mut reply: Reply) -> Reply {
    reply.questions = reply
        .questions
        .iter()
        .map(|question| clip(question, 240))
        .filter(|question| !question.is_empty())
        .take(MAX_QUESTIONS)
        .collect();
    reply.refined = reply.refined.trim().to_string();
    reply.reads = reply.reads.iter().take(MAX_READS).cloned().collect();
    reply
}

/// A model-supplied path is untrusted. Plain relative paths only, no traversal, and never a
/// known secret or dependency tree.
fn safe_relative(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim().trim_matches('"').replace('\\', "/");
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(&trimmed);
    if path.is_absolute() {
        return None;
    }
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let name = part.to_string_lossy().to_lowercase();
                if matches!(name.as_str(), "node_modules" | "target" | ".git") {
                    return None;
                }
                if name.starts_with(".env")
                    || name == ".mcp.json"
                    || name.ends_with(".pem")
                    || name.ends_with(".key")
                {
                    return None;
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(path)
}

fn read_requested(root: &Path, requested: &[String]) -> (Vec<(String, String)>, Vec<String>) {
    let Ok(root) = root.canonicalize() else {
        return (
            Vec::new(),
            vec!["the project root could not be resolved, so no file was read".to_string()],
        );
    };
    let mut snippets = Vec::new();
    let mut notes = Vec::new();

    for raw in requested.iter().take(MAX_READS) {
        let Some(relative) = safe_relative(raw) else {
            notes.push(format!(
                "refused to read `{raw}`: only plain project-relative paths are read"
            ));
            continue;
        };
        let Ok(full) = root.join(&relative).canonicalize() else {
            notes.push(format!("`{}` does not exist", relative.display()));
            continue;
        };
        if !full.starts_with(&root) {
            notes.push(format!(
                "refused to read `{raw}`: it is outside the project"
            ));
            continue;
        }
        let Ok(metadata) = std::fs::metadata(&full) else {
            notes.push(format!("`{}` could not be inspected", relative.display()));
            continue;
        };
        if !metadata.is_file() {
            notes.push(format!("`{}` is not a file", relative.display()));
            continue;
        }
        let Ok(mut file) = std::fs::File::open(&full) else {
            notes.push(format!("`{}` could not be opened", relative.display()));
            continue;
        };
        let mut buffer = Vec::new();
        if let Err(error) = file
            .by_ref()
            .take(MAX_READ_BYTES as u64 + 1)
            .read_to_end(&mut buffer)
        {
            notes.push(format!(
                "`{}` could not be read: {error}",
                relative.display()
            ));
            continue;
        }
        let truncated = buffer.len() > MAX_READ_BYTES;
        buffer.truncate(MAX_READ_BYTES);
        let text = String::from_utf8_lossy(&buffer).into_owned();
        if text.contains('\0') {
            notes.push(format!(
                "`{}` looks binary and was skipped",
                relative.display()
            ));
            continue;
        }
        let path = relative.display().to_string().replace('\\', "/");
        notes.push(format!(
            "read `{path}`{}{}",
            if truncated { " (cut at 8 KB)" } else { "" },
            if String::from_utf8_lossy(&buffer).contains('\u{FFFD}') {
                " (some bytes were not UTF-8)"
            } else {
                ""
            }
        ));
        snippets.push((path, text));
    }

    (snippets, notes)
}

fn clip(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let mut cut: String = trimmed.chars().take(limit).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// Replies from a queue and keeps every request, so a test can see what the second call
    /// was given without a network.
    struct CapturingModel {
        replies: Mutex<VecDeque<String>>,
        seen: Mutex<Vec<ChatRequest>>,
    }

    impl CapturingModel {
        fn new(replies: Vec<String>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
                seen: Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<ChatRequest> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Model for CapturingModel {
        fn id(&self) -> String {
            "capture".to_string()
        }

        async fn chat(
            &self,
            request: ChatRequest,
            _cancel: &CancellationToken,
            on_delta: &mut crate::agent::model::DeltaSink<'_>,
        ) -> Result<(), crate::agent::model::ModelError> {
            self.seen.lock().unwrap().push(request);
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(crate::agent::model::ModelError::ScriptExhausted)?;
            on_delta(Delta::Text(reply));
            on_delta(Delta::Usage(crate::agent::model::RawUsage {
                input_tokens: 100,
                cached_input_tokens: 0,
                output_tokens: 20,
                reasoning_tokens: 0,
                cost_usd: Some(0.0001),
            }));
            Ok(())
        }
    }

    fn refiner(replies: Vec<String>) -> (PromptRefiner, Arc<CapturingModel>) {
        let model = Arc::new(CapturingModel::new(replies));
        let refiner = PromptRefiner::new(model.clone(), "test/cheap");
        (refiner, model)
    }

    fn request(draft: &str, round: u8) -> RefineRequest {
        RefineRequest {
            draft: draft.to_string(),
            target: "@claude".to_string(),
            channel: Some("general".to_string()),
            recent: vec!["user: make the test faster".to_string()],
            answers: Vec::new(),
            round,
        }
    }

    #[tokio::test]
    async fn reads_one_round_of_files_and_bills_both_calls() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/lib.rs"), "pub fn hello() {}").unwrap();

        let (refiner, model) = refiner(vec![
            r#"{"questions":["Which test should get faster?"],"refined":"Make the test faster.","reads":["src/lib.rs"]}"#
                .to_string(),
            r#"{"questions":[],"refined":"Speed up `test_parse` in src/lib.rs without changing what it asserts.","reads":[]}"#
                .to_string(),
        ]);

        let outcome = refiner
            .refine(&request("make the test faster", 0), Some(root.path()))
            .await
            .unwrap();

        assert!(outcome.questions.is_empty());
        assert!(outcome.refined.contains("test_parse"));
        assert_eq!(outcome.reads, vec!["src/lib.rs".to_string()]);
        assert!(
            outcome
                .read_notes
                .iter()
                .any(|note| note.contains("src/lib.rs"))
        );
        assert_eq!(outcome.usage.cost_micros, 200, "two calls, both billed");

        let seen = model.seen();
        assert_eq!(seen.len(), 2);
        let second = seen[1].messages[1].content.as_deref().unwrap();
        assert!(
            second.contains("pub fn hello() {}"),
            "the second call must receive the file it asked for: {second}"
        );
    }

    #[tokio::test]
    async fn a_path_outside_the_project_is_refused_without_a_second_call() {
        let root = tempfile::tempdir().unwrap();
        let inner = root.path().join("project");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(root.path().join("secret.txt"), "do not read me").unwrap();

        let (refiner, model) = refiner(vec![
            r#"{"questions":[],"refined":"Do the thing.","reads":["../secret.txt"]}"#.to_string(),
            r#"{"questions":[],"refined":"unused","reads":[]}"#.to_string(),
        ]);

        let outcome = refiner
            .refine(&request("do the thing", 0), Some(&inner))
            .await
            .unwrap();

        assert!(outcome.reads.is_empty());
        assert!(
            outcome
                .read_notes
                .iter()
                .any(|note| note.contains("project-relative")),
            "{:?}",
            outcome.read_notes
        );
        assert_eq!(
            model.seen().len(),
            1,
            "nothing was read, so nothing was re-asked"
        );
    }

    #[tokio::test]
    async fn a_later_round_cannot_read_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.rs"), "fn a() {}").unwrap();
        let (refiner, model) = refiner(vec![
            r#"{"questions":[],"refined":"Answer round.","reads":["a.rs"]}"#.to_string(),
        ]);

        let outcome = refiner
            .refine(&request("do it", 1), Some(root.path()))
            .await
            .unwrap();

        assert!(outcome.reads.is_empty());
        assert!(
            outcome
                .notes
                .iter()
                .any(|note| note.contains("first round only"))
        );
        assert_eq!(model.seen().len(), 1);
    }

    #[tokio::test]
    async fn plain_text_is_still_a_usable_rewrite() {
        let (refiner, _) = refiner(vec!["Rewrite: add a test for the parser.".to_string()]);
        let outcome = refiner
            .refine(&request("add a parser test", 0), None)
            .await
            .unwrap();

        assert_eq!(outcome.refined, "Rewrite: add a test for the parser.");
        assert!(
            outcome
                .notes
                .iter()
                .any(|note| note.contains("did not return JSON"))
        );
        assert!(outcome.questions.is_empty());
    }

    #[tokio::test]
    async fn questions_are_capped_and_fences_are_tolerated() {
        let reply = "```json\n{\"questions\":[\"one\",\"two\",\"three\",\"four\"],\"refined\":\"go\",\"reads\":[]}\n```";
        let (refiner, _) = refiner(vec![reply.to_string()]);
        let outcome = refiner.refine(&request("go", 0), None).await.unwrap();
        assert_eq!(outcome.questions, vec!["one", "two", "three"]);
        assert_eq!(outcome.refined, "go");
    }

    #[tokio::test]
    async fn a_fourth_round_is_refused_before_any_call() {
        let (refiner, model) = refiner(vec![]);
        let error = refiner
            .refine(&request("go", MAX_ROUNDS), None)
            .await
            .unwrap_err();
        assert!(error.contains("already been refined"));
        assert!(model.seen().is_empty());
    }

    #[tokio::test]
    async fn an_empty_draft_is_refused() {
        let (refiner, model) = refiner(vec![]);
        assert!(refiner.refine(&request("   ", 0), None).await.is_err());
        assert!(model.seen().is_empty());
    }

    #[test]
    fn secret_and_dependency_paths_are_not_offerable() {
        for path in [
            ".env",
            ".env.local",
            "deploy/key.pem",
            "node_modules/x.js",
            "../x",
        ] {
            assert!(safe_relative(path).is_none(), "{path} must be refused");
        }
        assert!(safe_relative("src/lib.rs").is_some());
        assert!(safe_relative("./src\\lib.rs").is_some());
    }

    #[test]
    fn a_large_file_is_cut_at_the_read_limit() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("big.txt"), "a".repeat(MAX_READ_BYTES * 2)).unwrap();
        let (snippets, notes) = read_requested(root.path(), &["big.txt".to_string()]);
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].1.len(), MAX_READ_BYTES);
        assert!(notes.iter().any(|note| note.contains("cut at 8 KB")));
    }
}

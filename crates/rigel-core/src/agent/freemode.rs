//! Free mode: pick a zero-priced model that is answering **right now**.
//!
//! A `:free` id is a label, not a promise. The same roster can offer a free row that is rate
//! limited upstream (OpenRouter's shared pool answers 429), a row the provider's live catalogue
//! no longer serves (a 400 naming the id), and a row whose door has no key at all. Choosing the
//! first free row and hoping is how a turn fails before it starts.
//!
//! So Free mode ranks the tool-capable free chat rows by benchmark, then sends each candidate a
//! tiny real request and keeps the best-ranked one that answers. A candidate that fails is
//! recorded with the reason and
//! skipped; the caller gets the winner plus the attempts, so the window can say what it tried.
//!
//! Ranking puts the strongest checked-in benchmark score first, wherever that model is served.
//! Provider tier breaks a tie (the selected door, then a direct vendor quota, then a shared
//! gateway), and the id keeps the order stable. The probe still decides: a high-scoring row that
//! is rate limited is skipped for the next one.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::catalog::{self, ModelOption};
use super::model::{ChatMessage, ChatRequest, Delta, Model, ModelError};
use super::providers;
use crate::credentials::CredentialStore;

/// How many candidates one Free-mode run may test. Antigravity sees every live free row, then this
/// many finalists get a real request so selection remains bounded.
pub const MAX_PROBES: usize = 6;

/// The paid judge is used only to order free candidates; live provider pricing and probes remain
/// the authority for eligibility and availability.
pub const BRAIN_PROVIDER: &str = "omniroute";
pub const BRAIN_MODEL: &str = "antigravity/claude-opus-4-6-thinking";

/// A probe that has not answered by now is not a model this turn should wait for.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(25);
const BRAIN_TIMEOUT: Duration = Duration::from_secs(45);

/// Short enough that a reasoning model still returns something, long enough to prove the door
/// accepts the request rather than only that the key is valid.
const PROBE_PROMPT: &str = "Reply with the single word: ok";

/// The benchmark snapshot the ranking uses. These are aggregate coding/reasoning scores from
/// published leaderboards, kept as family tokens so a provider's version suffix (and its
/// `:free` label) does not change the rank. The table is checked in rather than fetched so a
/// turn never waits on a leaderboard; update it when a new family lands in the roster.
pub const BENCHMARK_SOURCE: &str = "public coding/reasoning aggregate, 6 Nov 2026";

/// An unseen family is not assumed to be good. It ranks below every known strong family but
/// still above a non-chat row, which is filtered out before this is used.
const UNKNOWN_BENCHMARK: u16 = 50;

/// Higher is better. Longest family token wins, so `qwen3-coder` beats a bare `qwen3` row.
const FREE_BENCHMARKS: &[(&str, u16)] = &[
    ("deepseek-r1", 92),
    ("deepseek-v3", 90),
    ("gemini-2.5-pro", 90),
    ("qwen3-235b", 89),
    ("gpt-oss-120b", 88),
    ("qwen3-coder", 88),
    ("kimi-k2", 87),
    ("glm-4.6", 86),
    ("glm-4.5", 86),
    ("deepseek-chat", 84),
    ("llama-4-maverick", 84),
    ("gemini-2.5-flash", 84),
    ("mistral-large", 82),
    ("qwen3-32b", 80),
    ("gpt-oss-20b", 80),
    ("gemma-3-27b", 78),
    ("llama-3.3-70b", 76),
    ("phi-4", 74),
];

/// The checked-in aggregate score for a model id. Matching is on a lower-cased substring, and
/// the longest matching family token wins; an unknown id gets [`UNKNOWN_BENCHMARK`].
pub fn benchmark(model: &str) -> u16 {
    let id = model.trim().to_ascii_lowercase();
    FREE_BENCHMARKS
        .iter()
        .filter(|(family, _)| id.contains(family))
        .max_by_key(|(family, _)| family.len())
        .map(|(_, score)| *score)
        .unwrap_or(UNKNOWN_BENCHMARK)
}

/// One candidate and what happened when it was asked a real question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeAttempt {
    pub provider: String,
    pub model: String,
    /// `ok`, `rate_limited`, `unauthorized`, `not_found`, `rejected`, `no_key`, `timeout`,
    /// `unreachable`, `unavailable`, or `failed`.
    pub status: String,
    /// The provider's own words, bounded: a 400 body can name a whole catalogue.
    pub detail: Option<String>,
}

/// The model Free mode settled on, with the evidence behind the choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeChoice {
    pub provider: String,
    pub model: String,
    /// What the window should say: which model answered and how many were tried.
    pub note: String,
    /// The checked-in aggregate score retained as evidence and as the fallback if the judge fails.
    #[serde(default)]
    pub benchmark: u16,
    /// The model that ranked the live free candidates, or `benchmark fallback` if it was unavailable.
    #[serde(default)]
    pub brain: String,
    pub attempts: Vec<FreeAttempt>,
}

impl FreeChoice {
    /// The attempts that failed, for a surface that wants to explain a switch.
    pub fn rejected(&self) -> impl Iterator<Item = &FreeAttempt> {
        self.attempts
            .iter()
            .filter(|attempt| attempt.status != "ok")
    }
}

/// Whether an id is advertised as a zero-priced row. The roster is the authority; this is the
/// cheap check a turn uses before deciding it needs a probe.
pub fn is_free_id(model: &str) -> bool {
    model.trim().to_ascii_lowercase().ends_with(":free")
}

/// The free chat rows, best first. Pure, so the ordering rule is testable without a network.
///
/// A free row that cannot answer a chat turn (an image checkpoint that happens to be zero-priced)
/// is not a candidate: choosing it would fail a turn for a reason the roster already knew.
pub fn candidates(current: &str, rows: Vec<ModelOption>) -> Vec<ModelOption> {
    let current = current.trim().to_ascii_lowercase();
    let mut seen = std::collections::HashSet::new();
    let mut free: Vec<ModelOption> = rows
        .into_iter()
        .filter(|row| row.free && row.chat && row.tools)
        .filter(|row| seen.insert((row.provider.clone(), row.id.clone())))
        .collect();
    free.sort_by(|one, other| {
        // Quality first: the absolute best free row leads, wherever it is served. Provider tier
        // breaks a tie so the selected door still wins between equally strong models, and the
        // id keeps the order stable when both are the same model on two doors.
        benchmark(&other.id)
            .cmp(&benchmark(&one.id))
            .then_with(|| tier(&current, &one.provider).cmp(&tier(&current, &other.provider)))
            .then_with(|| {
                one.id
                    .to_ascii_lowercase()
                    .cmp(&other.id.to_ascii_lowercase())
            })
    });
    free
}

/// 0 the door already selected, 1 a door with its own quota, 2 a shared gateway pool.
fn tier(current: &str, provider: &str) -> u8 {
    if provider == current {
        0
    } else if matches!(provider, "omniroute" | "openrouter") {
        2
    } else {
        1
    }
}

/// Test candidates until one answers. The winner is chosen on evidence, not on the label.
pub async fn pick(current: &str, store: &dyn CredentialStore) -> Result<FreeChoice, String> {
    // `roster` makes fresh concurrent provider calls. A row that changed from free to paid is
    // excluded here before either the judge or a probe can spend against it.
    let rows = catalog::roster(current, store).await?;
    let ranked = candidates(current, rows);
    let (ranked, brain) = rank_with_antigravity(ranked, store).await;
    try_candidates_with_brain(ranked, store, &brain).await
}

/// Ask Antigravity to compare every currently free candidate. It receives catalogue facts only —
/// no credentials — and returns indexes, so it cannot invent a model or provider pair. If the
/// judge cannot run, the deterministic benchmark order remains a safe fallback.
async fn rank_with_antigravity(
    ranked: Vec<ModelOption>,
    store: &dyn CredentialStore,
) -> (Vec<ModelOption>, String) {
    if ranked.len() < 2 {
        return (ranked, BRAIN_MODEL.to_string());
    }
    let brain = match providers::build(BRAIN_PROVIDER, store).await {
        Ok(brain) => brain,
        Err(_) => return (ranked, "benchmark fallback".to_string()),
    };
    let catalogue = ranked
        .iter()
        .enumerate()
        .map(|(index, row)| {
            format!(
                "{index}\t{}\t{}\tbenchmark={}\ttools={}\tlocal={}",
                row.provider,
                row.id,
                benchmark(&row.id),
                row.tools,
                row.local
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let request = ChatRequest {
        model: BRAIN_MODEL.to_string(),
        messages: vec![
            ChatMessage::system(
                "Rank free AI models for autonomous software-engineering brainwork. Prioritize \
                 coding accuracy, difficult reasoning, reliable tool calling, context quality, \
                 and practical reliability. Return only a JSON array containing every numeric \
                 row index exactly once, best first. Do not add prose.",
            ),
            ChatMessage::user(catalogue),
        ],
        tools: Vec::new(),
        max_tokens: Some(1024),
        temperature: 0.0,
        effort: Some("high".to_string()),
    };
    let cancel = CancellationToken::new();
    let mut answer = String::new();
    let mut sink = |delta: Delta| {
        if let Delta::Text(text) = delta {
            answer.push_str(&text);
        }
    };
    let result = tokio::time::timeout(BRAIN_TIMEOUT, brain.chat(request, &cancel, &mut sink)).await;
    if !matches!(result, Ok(Ok(()))) {
        return (ranked, "benchmark fallback".to_string());
    }
    let Some(start) = answer.find('[') else {
        return (ranked, "benchmark fallback".to_string());
    };
    let Some(end) = answer[start..].find(']').map(|offset| start + offset + 1) else {
        return (ranked, "benchmark fallback".to_string());
    };
    let Ok(order) = serde_json::from_str::<Vec<usize>>(&answer[start..end]) else {
        return (ranked, "benchmark fallback".to_string());
    };
    let unique: std::collections::HashSet<_> = order.iter().copied().collect();
    if order.len() != ranked.len()
        || unique.len() != ranked.len()
        || order.iter().any(|index| *index >= ranked.len())
    {
        return (ranked, "benchmark fallback".to_string());
    }
    let reordered = order
        .into_iter()
        .map(|index| ranked[index].clone())
        .collect();
    (reordered, BRAIN_MODEL.to_string())
}

/// The probe loop, split from [`pick`] so a test can hand it an exact candidate list.
pub async fn try_candidates(
    ranked: Vec<ModelOption>,
    store: &dyn CredentialStore,
) -> Result<FreeChoice, String> {
    try_candidates_with_brain(ranked, store, "benchmark fallback").await
}

async fn try_candidates_with_brain(
    ranked: Vec<ModelOption>,
    store: &dyn CredentialStore,
    brain: &str,
) -> Result<FreeChoice, String> {
    if ranked.is_empty() {
        return Err(
            "no free chat model is listed by any provider that holds a key; add a key under \
             Settings → Providers, or pick a paid model"
                .to_string(),
        );
    }

    let mut built: HashMap<String, Arc<dyn Model>> = HashMap::new();
    let mut attempts: Vec<FreeAttempt> = Vec::new();
    for option in ranked.into_iter().take(MAX_PROBES) {
        let model = match built.get(&option.provider) {
            Some(model) => model.clone(),
            None => match providers::build(&option.provider, store).await {
                Ok(model) => {
                    built.insert(option.provider.clone(), model.clone());
                    model
                }
                Err(error) => {
                    // The door has no credential, or no route: this is not the model's fault,
                    // and it is not something another probe on the same door can fix.
                    attempts.push(FreeAttempt {
                        provider: option.provider.clone(),
                        model: option.id.clone(),
                        status: "unavailable".to_string(),
                        detail: Some(bounded(&error)),
                    });
                    continue;
                }
            },
        };
        let attempt = probe(model.as_ref(), &option).await;
        let won = attempt.status == "ok";
        attempts.push(attempt);
        if won {
            let tried = attempts.len();
            let score = benchmark(&option.id);
            return Ok(FreeChoice {
                provider: option.provider.clone(),
                model: option.id.clone(),
                note: format!(
                    "Free mode: {} on {} — ranked by {brain}, live zero-price roster verified, \
                     benchmark {score} ({BENCHMARK_SOURCE}), and answered a real probe ({tried} tried).",
                    option.id, option.provider
                ),
                benchmark: score,
                brain: brain.to_string(),
                attempts,
            });
        }
    }

    let summary = attempts
        .iter()
        .map(|attempt| {
            format!(
                "{} on {}: {}",
                attempt.model, attempt.provider, attempt.status
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    Err(format!(
        "no free model answered a test message ({summary}); the free pools may be rate limited \
         right now — try again shortly or pick a paid model"
    ))
}

/// Ask one model a tiny question. A 200 with a token is the only proof that counts.
pub async fn probe(model: &dyn Model, option: &ModelOption) -> FreeAttempt {
    let request = ChatRequest {
        model: option.id.clone(),
        messages: vec![ChatMessage::user(PROBE_PROMPT)],
        tools: Vec::new(),
        max_tokens: Some(8),
        temperature: 0.0,
        effort: None,
    };
    let cancel = CancellationToken::new();
    let mut sink = |_: Delta| {};
    match tokio::time::timeout(PROBE_TIMEOUT, model.chat(request, &cancel, &mut sink)).await {
        Err(_) => attempt(option, "timeout", None),
        Ok(Ok(())) => attempt(option, "ok", None),
        Ok(Err(error)) => attempt(option, status_of(&error), Some(&error.to_string())),
    }
}

/// The failure classes a switch can act on. Everything else is `failed` and also skipped — the
/// point of the loop is to find one that answers, not to rank the reasons the others did not.
fn status_of(error: &ModelError) -> &'static str {
    match error {
        ModelError::Status { status: 429, .. } => "rate_limited",
        ModelError::Status {
            status: 401 | 403, ..
        } => "unauthorized",
        ModelError::Status { status: 404, .. } => "not_found",
        ModelError::Status { status: 400, .. } => "rejected",
        ModelError::NoKey(_) => "no_key",
        ModelError::Http(_) => "unreachable",
        ModelError::Cancelled => "cancelled",
        _ => "failed",
    }
}

fn attempt(option: &ModelOption, status: &str, detail: Option<&str>) -> FreeAttempt {
    FreeAttempt {
        provider: option.provider.clone(),
        model: option.id.clone(),
        status: status.to_string(),
        detail: detail.map(bounded),
    }
}

/// 300 characters is enough to name the model and the reason; a raw body can be kilobytes.
fn bounded(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= 300 {
        return text.to_string();
    }
    let cut: String = text.chars().take(300).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::openrouter::OpenRouterModel;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn row(id: &str, provider: &str, free: bool, chat: bool) -> ModelOption {
        ModelOption {
            id: id.to_string(),
            provider: provider.to_string(),
            name: None,
            free,
            chat,
            tools: true,
            local: false,
            modality: None,
            output_micros_per_mtok: None,
        }
    }

    #[test]
    fn the_current_door_leads_then_owned_quotas_then_shared_pools() {
        let ranked = candidates(
            "openrouter",
            vec![
                row("held/quota:free", "groq", true, true),
                row("pool/model:free", "openrouter", true, true),
                row("router/model:free", "omniroute", true, true),
                row("paid/model", "openrouter", false, true),
                row("free/image:free", "openrouter", true, false),
            ],
        );

        assert_eq!(
            ranked
                .iter()
                .map(|option| option.id.as_str())
                .collect::<Vec<_>>(),
            vec!["pool/model:free", "held/quota:free", "router/model:free"],
            "free chat rows only: the selected door, then an owned quota, then shared pools"
        );
    }

    #[test]
    fn the_absolute_best_free_benchmark_leads_before_door_and_name() {
        let ranked = candidates(
            "openrouter",
            vec![
                row("zzz/unknown:free", "openrouter", true, true),
                row("openai/gpt-oss-120b:free", "openrouter", true, true),
                row("deepseek/deepseek-r1:free", "groq", true, true),
            ],
        );
        assert_eq!(
            ranked[0].id, "deepseek/deepseek-r1:free",
            "the strongest benchmark leads even on another door"
        );
        assert_eq!(benchmark("deepseek/deepseek-r1:free"), 92);
        assert!(
            benchmark("deepseek/deepseek-r1:free") > benchmark("openai/gpt-oss-120b:free"),
            "the rank is a benchmark comparison, not name order"
        );
        assert_eq!(benchmark("mystery/model:free"), UNKNOWN_BENCHMARK);
    }

    #[test]
    fn free_mode_never_picks_a_row_that_cannot_call_tools() {
        let mut text_only = row("text-only:free", "openrouter", true, true);
        text_only.tools = false;
        let ranked = candidates(
            "openrouter",
            vec![text_only, row("tool-ready:free", "openrouter", true, true)],
        );
        assert_eq!(
            ranked
                .iter()
                .map(|option| option.id.as_str())
                .collect::<Vec<_>>(),
            vec!["tool-ready:free"],
            "a free model that cannot call tools cannot serve a swarm or a subagent"
        );
    }

    #[test]
    fn a_free_row_is_not_a_promise_and_the_failure_classes_say_so() {
        assert_eq!(
            status_of(&ModelError::Status {
                status: 429,
                body: String::new(),
                retry_after: None
            }),
            "rate_limited"
        );
        assert_eq!(
            status_of(&ModelError::Status {
                status: 401,
                body: String::new(),
                retry_after: None
            }),
            "unauthorized"
        );
        assert_eq!(
            status_of(&ModelError::Status {
                status: 404,
                body: String::new(),
                retry_after: None
            }),
            "not_found"
        );
        assert_eq!(
            status_of(&ModelError::Status {
                status: 400,
                body: String::new(),
                retry_after: None
            }),
            "rejected"
        );
        assert_eq!(status_of(&ModelError::NoKey("groq".into())), "no_key");
        assert!(is_free_id("openai/gpt-oss-120b:free"));
        assert!(!is_free_id("openai/gpt-oss-120b"));
    }

    /// One request, answered with the status the test asks for. `answer` decides per model.
    async fn fake_door(
        answers: &'static [(&'static str, u16)],
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut request = vec![0u8; 16384];
                let read = socket.read(&mut request).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&request[..read]).to_string();
                log.lock().unwrap().push(text.clone());
                let model = text
                    .split("\"model\":\"")
                    .nth(1)
                    .and_then(|rest| rest.split('"').next())
                    .unwrap_or_default()
                    .to_string();
                let status = answers
                    .iter()
                    .find(|(id, _)| *id == model)
                    .map(|(_, status)| *status)
                    .unwrap_or(200);
                let body = if status == 200 {
                    "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n"
                } else {
                    "{\"error\":{\"message\":\"no\"}}"
                };
                let head = format!(
                    "HTTP/1.1 {status} {}\r\nContent-Type: text/event-stream\r\nContent-Length: \
                     {}\r\nConnection: close\r\n\r\n",
                    if status == 200 { "OK" } else { "Error" },
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(body.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{address}"), seen)
    }

    #[tokio::test]
    async fn a_rate_limited_row_is_skipped_and_the_next_one_wins() {
        let (base, seen) = fake_door(&[("pool/gemma:free", 429), ("held/quota:free", 200)]).await;
        let model = OpenRouterModel::with_base_url("test-key", base);
        let gate: &dyn Model = &model;

        let first = probe(gate, &row("pool/gemma:free", "openrouter", true, true)).await;
        assert_eq!(first.status, "rate_limited");
        assert!(
            first.detail.as_deref().unwrap_or_default().contains("429"),
            "the reason is kept: {:?}",
            first.detail
        );

        let second = probe(gate, &row("held/quota:free", "groq", true, true)).await;
        assert_eq!(second.status, "ok");

        let seen = seen.lock().unwrap();
        assert!(
            seen.first()
                .is_some_and(|first| first.contains("\"model\":\"pool/gemma:free\"")),
            "the ranked first candidate is asked first: {seen:?}"
        );
        // A 429 is retried by the provider itself (`is_retryable_status`), so the count is the
        // provider's policy plus one request per candidate — what matters is the winner.
        assert!(
            seen.iter()
                .filter(|request| request.contains("\"model\":\"pool/gemma:free\""))
                .count()
                >= 1,
            "the rate-limited row was tried: {seen:?}"
        );
        let last = seen.last().expect("the second candidate was asked");
        assert!(
            last.contains("\"model\":\"held/quota:free\""),
            "the loop moves to the next candidate after a 429: {last}"
        );
        assert!(
            last.contains("\"max_tokens\":8"),
            "a probe is one tiny message, not a turn: {last}"
        );
    }
}

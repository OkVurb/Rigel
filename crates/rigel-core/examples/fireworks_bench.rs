//! Cost-per-success benchmark for the Fireworks models Rigel is considering.
//!
//! This is a focused model comparison, not the full agent harness: it exercises routing,
//! streaming assembly, pricing arithmetic and FireRouter header knowledge with deterministic
//! answers. Run it with the key already stored in Rigel's keychain:
//!
//! ```text
//! cargo run -p rigel-core --example fireworks_bench
//! ```
//!
//! Or with an environment key, which the same credential store checks first:
//!
//! ```text
//! FIREWORKS_API_KEY=... cargo run -p rigel-core --example fireworks_bench
//! ```
//!
//! `--dry-run` prints the task set and prices without reading a key or making a request. The key
//! is never printed, logged, or written anywhere.

use std::time::Instant;

use rigel_core::agent::pricing;
use rigel_core::credentials::{self, KeychainCredentialStore};
use serde_json::{Value, json};

const BASE_URL: &str = "https://api.fireworks.ai/inference/v1";

struct Candidate {
    id: &'static str,
    label: &'static str,
}

const CANDIDATES: &[Candidate] = &[
    Candidate {
        id: "accounts/fireworks/models/glm-5p3-flash",
        label: "GLM 5.3 Flash",
    },
    Candidate {
        id: "accounts/fireworks/models/deepseek-v4p1-flash",
        label: "DeepSeek V4.1 Flash",
    },
    Candidate {
        id: "accounts/fireworks/models/kimi-k3p1-pro",
        label: "Kimi K3.1 Pro",
    },
];

enum Expected {
    Exact(&'static str),
    Number { value: f64, tolerance: f64 },
    ContainsAll(&'static [&'static str]),
}

struct Task {
    id: &'static str,
    prompt: &'static str,
    expected: Expected,
    max_tokens: u32,
}

const TASKS: &[Task] = &[
    Task {
        id: "routing",
        prompt: "Rigel has provider `groq` selected and a saved model id `gemini-4-argon`. \
                 The id belongs to another provider. Does Rigel silently substitute Groq's \
                 default model, or refuse the turn with an error? Return only JSON: \
                 {\"answer\":\"ALLOW\"} or {\"answer\":\"REFUSE\"}.",
        expected: Expected::Exact("refuse"),
        max_tokens: 80,
    },
    Task {
        id: "fireworks-cost",
        prompt: "A Fireworks model costs $0.15 per million uncached input tokens, $0.03 per \
                 million cached input tokens, and $0.50 per million output tokens. A request \
                 uses 200,000 uncached input tokens, 800,000 cached input tokens, and 200,000 \
                 output tokens. What is the total cost in US dollars? Return only JSON: \
                 {\"answer\":0.0}.",
        expected: Expected::Number {
            value: 0.154,
            tolerance: 0.0005,
        },
        max_tokens: 80,
    },
    Task {
        id: "sse-assembly",
        prompt: "Three SSE tool-call fragments arrive in order: `{\"pa`, `th\":\"a`, `.txt\"}`. \
                 Concatenate them exactly. Return only JSON with the resulting string: \
                 {\"answer\":\"...\"}.",
        expected: Expected::Exact("{\"path\":\"a.txt\"}"),
        max_tokens: 80,
    },
    Task {
        id: "firerouter-headers",
        prompt: "Rigel calls FireRouter with only a Fireworks bearer key plus any stored \
                 Anthropic/OpenAI provider keys. FireRouter needs those provider keys to serve \
                 closed models. Name the two request headers Rigel adds. Return only JSON with a \
                 comma-separated string in `answer`.",
        expected: Expected::ContainsAll(&["x-anthropic-api-key", "x-openai-api-key"]),
        max_tokens: 100,
    },
    Task {
        id: "cheapest-output",
        prompt: "At Fireworks list prices, rank these by output price: GLM 5.3 Flash \
                 ($0.15 input / $0.03 cached / $0.50 output per million), DeepSeek V4.1 Flash \
                 ($0.30 / $0.006 / $1.20), Kimi K3.1 Pro ($2.00 / $0.20 / $10.00). Which id has \
                 the lowest output price? Return only JSON: {\"answer\":\"...\"} with the id \
                 `glm-5p3-flash`, `deepseek-v4p1-flash`, or `kimi-k3p1-pro`.",
        expected: Expected::Exact("glm-5p3-flash"),
        max_tokens: 80,
    },
];

struct CallResult {
    passed: bool,
    cost_micros: i64,
    tokens: u64,
    latency_ms: u128,
}

fn answer_json(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    serde_json::from_str(&trimmed[start..=end]).ok()
}

fn answer_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        other => other.to_string(),
    }
}

fn passed(task: &Task, text: &str) -> bool {
    let Some(value) = answer_json(text) else {
        return false;
    };
    let answer = value.get("answer").unwrap_or(&value);
    match &task.expected {
        Expected::Exact(expected) => answer_text(answer).eq_ignore_ascii_case(expected),
        Expected::Number { value, tolerance } => {
            let parsed = answer
                .as_f64()
                .or_else(|| answer_text(answer).parse::<f64>().ok());
            parsed.is_some_and(|parsed| (parsed - value).abs() <= *tolerance)
        }
        Expected::ContainsAll(needles) => {
            let lower = answer_text(answer).to_ascii_lowercase();
            needles.iter().all(|needle| lower.contains(needle))
        }
    }
}

fn money(micros: i64) -> String {
    format!("${:.6}", micros as f64 / 1_000_000.0)
}

async fn one_call(
    client: &reqwest::Client,
    key: &str,
    candidate: &Candidate,
    task: &Task,
) -> Result<CallResult, String> {
    let start = Instant::now();
    let response = client
        .post(format!("{BASE_URL}/chat/completions"))
        .bearer_auth(key)
        .json(&json!({
            "model": candidate.id,
            "messages": [
                {
                    "role": "system",
                    "content": "Answer the question directly. Follow the requested JSON shape."
                },
                { "role": "user", "content": task.prompt }
            ],
            "temperature": 0,
            "max_tokens": task.max_tokens,
            "stream": false
        }))
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "HTTP {status}: {}",
            body.chars().take(240).collect::<String>()
        ));
    }
    let record: Value =
        serde_json::from_str(&body).map_err(|error| format!("unreadable JSON: {error}"))?;
    let text = record
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let usage = record.get("usage").cloned().unwrap_or(Value::Null);
    if usage.is_null() {
        return Err("response had no usage; cannot price the call".to_string());
    }
    let input = usage
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cached = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = usage
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let reasoning = usage
        .pointer("/completion_tokens_details/reasoning_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cost_micros = pricing::cost_micros(candidate.id, input, cached, output + reasoning);
    Ok(CallResult {
        passed: passed(task, text),
        cost_micros,
        tokens: input + output + reasoning,
        latency_ms: start.elapsed().as_millis(),
    })
}

#[tokio::main]
async fn main() {
    let dry_run = std::env::args().any(|arg| arg == "--dry-run");
    println!("# Fireworks cost-per-success benchmark");
    println!(
        "Focused Rigel routing/streaming/pricing tasks. This is not a full agent harness; it \
         measures model reasoning and bills each call at Rigel's local Fireworks price rows.\n"
    );
    if dry_run {
        for candidate in CANDIDATES {
            let price = pricing::price_for(candidate.id);
            println!(
                "- {} (`{}`): ${:.4}/${:.4}/${:.4} per 1M in/cached/out",
                candidate.label, candidate.id, price.input, price.cached_input, price.output
            );
        }
        println!();
        for task in TASKS {
            println!("- {}: {}", task.id, task.prompt);
        }
        return;
    }

    let store = KeychainCredentialStore;
    let key = match credentials::get_from(&store, credentials::FIREWORKS) {
        Ok(key) => key,
        Err(_) => {
            eprintln!(
                "No Fireworks key. Store one with `rigel keys set fireworks` (or set \
                 FIREWORKS_API_KEY) and rerun. Nothing was sent."
            );
            std::process::exit(2);
        }
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .expect("HTTP client");
    let key = key.expose();

    println!("| model | passed | success | total cost | cost / success | tokens | avg latency |");
    println!("|---|---:|---:|---:|---:|---:|---:|");

    for candidate in CANDIDATES {
        let mut passed = 0usize;
        let mut total_micros = 0i64;
        let mut total_tokens = 0u64;
        let mut total_latency = 0u128;
        let mut calls = 0usize;
        let mut errors = Vec::new();

        for task in TASKS {
            match one_call(&client, key, candidate, task).await {
                Ok(result) => {
                    calls += 1;
                    passed += usize::from(result.passed);
                    total_micros += result.cost_micros;
                    total_tokens += result.tokens;
                    total_latency += result.latency_ms;
                    println!(
                        "<!-- {} / {}: {} cost={} tokens={} latency={}ms -->",
                        candidate.label,
                        task.id,
                        if result.passed { "PASS" } else { "FAIL" },
                        money(result.cost_micros),
                        result.tokens,
                        result.latency_ms
                    );
                }
                Err(error) => {
                    errors.push(format!("{}: {error}", task.id));
                    println!(
                        "<!-- {} / {}: ERROR {} -->",
                        candidate.label, task.id, error
                    );
                }
            }
        }

        let success = format!("{passed}/{}", TASKS.len());
        let per_success = if passed == 0 {
            "—".to_string()
        } else {
            money((total_micros as f64 / passed as f64).round() as i64)
        };
        let avg_latency = if calls == 0 {
            "—".to_string()
        } else {
            format!("{} ms", total_latency / calls as u128)
        };
        println!(
            "| {} | {success} | {:.0}% | {} | {per_success} | {} | {avg_latency} |",
            candidate.label,
            (passed as f64 / TASKS.len() as f64) * 100.0,
            money(total_micros),
            total_tokens
        );
        if !errors.is_empty() {
            eprintln!(
                "{} had {} failed call(s): {}",
                candidate.label,
                errors.len(),
                errors.join("; ")
            );
        }
    }

    println!(
        "\nPrices: {} ({}). Cost per successful run divides all attempt cost for that model by \
         its passed tasks.",
        pricing::SOURCE,
        pricing::FETCHED_ON
    );
}

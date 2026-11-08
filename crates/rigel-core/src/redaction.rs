//! Q241: known credential shapes are redacted from tool output, logs and diagnostics.
//!
//! The adapters already replace the *values* of the keys Rigel injects. This pass covers the
//! other direction: a token a command printed, a file the agent read, or a log line carrying a
//! credential Rigel never resolved. Every pattern is deliberately narrow — a marker, a known
//! prefix and a minimum body length — so ordinary prose and code stay readable. A matched span
//! becomes [`MARK`], the same marker the adapter path writes.
//!
//! The pass is a filter, not a parser: it never rewrites JSON structure or a diff's shape, only
//! the string values inside them.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

/// The marker every redaction path writes, so a reader can tell a removed value from a real one.
pub const MARK: &str = "<redacted>";

/// Whole private-key blocks: the marker line alone would leave the key material behind.
fn private_key_block() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|$)")
            .expect("private-key pattern")
    })
}

/// Tokens with a well-known prefix. The length floors are what keep `sk-learn` or `AKIA` in
/// prose from matching: a real key of each shape is longer than the floor.
fn tokens() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            // OpenAI and Anthropic (`sk-ant-…`).
            r"\bsk-[A-Za-z0-9_-]{20,}\b",
            // Stripe restricted and secret keys.
            r"\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}\b",
            // GitHub tokens and fine-grained PATs.
            r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{30,}\b",
            r"\bgithub_pat_[A-Za-z0-9_]{20,}\b",
            // GitLab.
            r"\bglpat-[A-Za-z0-9_-]{20,}\b",
            // Slack.
            r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b",
            // Groq keys and xAI keys carry their own prefixes.
            r"\bgsk_[A-Za-z0-9]{20,}\b",
            r"\bxai-[A-Za-z0-9]{20,}\b",
            // AWS access key ids and Google API keys.
            r"\bAKIA[0-9A-Z]{16}\b",
            r"\bAIza[0-9A-Za-z_-]{35}\b",
            // A JSON Web Token: three base64url parts.
            r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b",
        ]
        .iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect()
    })
}

/// `Authorization: Bearer …` keeps its scheme and loses the token.
fn bearer() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/-]{20,}=*").expect("bearer pattern")
    })
}

/// `API_KEY=…`, `"token": "…"`, `password = '…'`. The label and separator stay readable; only a
/// long, unbroken value is replaced, so `token = make_token(user)` is left alone.
fn assigned() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)\b(api[_-]?key|access[_-]?token|auth[_-]?token|client[_-]?secret|token|secret|password)(\s*[:=]\s*)(["']?)[A-Za-z0-9_\-./+=]{20,}"#,
        )
        .expect("assignment pattern")
    })
}

/// Redact every known credential shape in `text`. Idempotent: the marker matches no pattern.
pub fn redact_text(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut out = private_key_block().replace_all(text, MARK).into_owned();
    for pattern in tokens() {
        out = pattern.replace_all(&out, MARK).into_owned();
    }
    out = bearer().replace_all(&out, "${1} <redacted>").into_owned();
    assigned()
        .replace_all(&out, "${1}${2}${3}<redacted>")
        .into_owned()
}

/// [`redact_text`] over every string in a JSON value, keeping its shape.
pub fn redact_json(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_text(text)),
        Value::Array(items) => Value::Array(items.iter().map(redact_json).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), redact_json(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_and_forge_tokens_are_replaced() {
        let text = concat!(
            "OPENAI=sk-proj-AbCdEfGhIjKlMnOpQrStUvWx\n",
            "anthropic sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz\n",
            "github ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789\n",
            "gitlab glpat-AbCdEfGhIjKlMnOpQrSt\n",
            "slack xoxb-1234567890-abcdefghij\n",
            "aws AKIAIOSFODNN7EXAMPLE\n",
            "google AIzaSyA1234567890abcdefghijklmnopqrstuv\n",
            "groq gsk_AbCdEfGhIjKlMnOpQrStUvWxYz012345\n",
            "xai xai-AbCdEfGhIjKlMnOpQrStUvWxYz012345\n",
        );
        let out = redact_text(text);
        for needle in [
            "sk-proj-",
            "sk-ant-api03",
            "ghp_",
            "glpat-",
            "xoxb-",
            "AKIA",
            "AIza",
            "gsk_",
            "xai-",
        ] {
            assert!(!out.contains(needle), "`{needle}` survived: {out}");
        }
        assert!(out.contains("OPENAI="), "the label stays readable: {out}");
        assert_eq!(out.matches(MARK).count(), 9, "{out}");
    }

    #[test]
    fn ordinary_words_that_look_like_prefixes_survive() {
        let text = "sk-learn is short; AKIA is prose; ghp_ is a prefix; token = make_token(user)";
        assert_eq!(redact_text(text), text);
    }

    #[test]
    fn jwt_and_bearer_lose_their_credential_not_their_shape() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let out = redact_text(&format!(
            "token {jwt}\nAuthorization: Bearer abcdefghijklmnopqrstuvwxyz"
        ));
        assert!(!out.contains("eyJhbGciOiJIUzI1NiJ9"), "{out}");
        assert!(!out.contains("abcdefghijklmnopqrstuvwxyz"), "{out}");
        assert!(out.contains("Authorization: Bearer <redacted>"), "{out}");
    }

    #[test]
    fn a_private_key_block_is_removed_whole() {
        let text = concat!(
            "before\n",
            "-----BEGIN OPENSSH PRIVATE KEY-----\n",
            "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAAB\n",
            "-----END OPENSSH PRIVATE KEY-----\n",
            "after",
        );
        let out = redact_text(text);
        assert_eq!(out, "before\n<redacted>\nafter");
    }

    #[test]
    fn json_values_are_redacted_and_the_shape_is_kept() {
        let value = json!({
            "ok": true,
            "result": { "content": "key sk-live-AbCdEfGhIjKlMnOp", "lines": 1 },
            "paths": ["ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789"],
        });
        let out = redact_json(&value);
        assert_eq!(out["ok"], json!(true));
        assert_eq!(out["result"]["lines"], json!(1));
        assert_eq!(out["result"]["content"], json!("key <redacted>"));
        assert_eq!(out["paths"][0], json!(MARK));
    }

    #[test]
    fn redaction_is_idempotent() {
        let once =
            redact_text("key sk-live-AbCdEfGhIjKlMnOp and password=\"abcdefghijklmnopqrstuvwx\"");
        assert_eq!(redact_text(&once), once);
    }
}

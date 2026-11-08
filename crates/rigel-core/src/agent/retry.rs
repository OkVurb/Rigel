//! Which failed request may be tried again, and how long the retry waits.
//!
//! Reads (a model roster, a fetch) are idempotent and may be retried freely. A chat post is
//! different: once the provider has emitted a token, a retry would duplicate an answer the user
//! already saw. The provider loops only reach this policy before a stream has emitted anything,
//! but the `emitted` flag is still part of the decision so a future caller cannot lose that rule.
//!
//! Every decision is expressed as a [`RetryEvent`]. The coordinator persists it on the turn, so a
//! retry is visible transcript state rather than a silent pause.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::model::jitter_delay;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryClass {
    /// A GET/read: safe to repeat even when the first response was lost.
    Read,
    /// A chat POST: safe only before the first token reaches the caller.
    Chat,
}

impl RetryClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Chat => "chat",
        }
    }
}

/// One retry that actually happened. Persisted as a transcript row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryEvent {
    pub class: RetryClass,
    /// 1 for the first retry, 2 for the next, independent of the provider's HTTP attempt count.
    pub attempt: u32,
    pub reason: String,
    pub delay_ms: u64,
}

/// The statuses a provider may answer with and still be worth another request. 408 is a timeout,
/// 429 is the shared limiter's signal, and every 5xx is a bad answer from the door.
pub fn retryable_status(status: u16) -> bool {
    status == 408 || status == 429 || status >= 500
}

/// Decide one retry. `attempt` is the attempt that just failed, starting at 1.
///
/// `status` is the HTTP status when there was a response; `transport` is true for a timeout,
/// connect failure or request error. `retry_after` wins over jitter because the provider knows
/// its own window. `None` means stop and return the original failure.
#[allow(clippy::too_many_arguments)]
pub fn decision(
    class: RetryClass,
    attempt: u32,
    max_attempts: u32,
    status: Option<u16>,
    transport: bool,
    emitted: bool,
    retry_after: Option<Duration>,
    base_ms: u64,
) -> Option<RetryEvent> {
    if emitted || attempt >= max_attempts.max(1) {
        return None;
    }
    let reason = match status {
        Some(status) if retryable_status(status) => format!("http {status}"),
        None if transport => "transport".to_string(),
        _ => return None,
    };
    let delay = retry_after.unwrap_or_else(|| jitter_delay(attempt, base_ms));
    Some(RetryEvent {
        class,
        attempt,
        reason,
        delay_ms: delay.as_millis().min(u64::MAX as u128) as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_retries_a_server_failure() {
        let event = decision(RetryClass::Read, 1, 3, Some(503), false, false, None, 0)
            .expect("a read may retry a 503");
        assert_eq!(event.class, RetryClass::Read);
        assert_eq!(event.reason, "http 503");
        assert_eq!(event.attempt, 1);
    }

    #[test]
    fn a_chat_post_stops_after_the_first_token() {
        assert!(
            decision(RetryClass::Chat, 1, 3, Some(503), false, true, None, 0).is_none(),
            "a retry after a token would duplicate the answer"
        );
    }

    #[test]
    fn a_chat_post_reports_its_retry_before_any_token() {
        let event = decision(
            RetryClass::Chat,
            1,
            3,
            Some(429),
            false,
            false,
            Some(Duration::from_secs(2)),
            250,
        )
        .expect("a 429 before the stream is retryable");
        assert_eq!(event.class, RetryClass::Chat);
        assert_eq!(event.delay_ms, 2_000, "Retry-After wins over jitter");
    }

    #[test]
    fn a_terminal_status_is_not_retried() {
        assert!(decision(RetryClass::Read, 1, 3, Some(401), false, false, None, 0).is_none());
        assert!(
            decision(RetryClass::Read, 3, 3, Some(503), false, false, None, 0).is_none(),
            "the attempt budget is exhausted"
        );
    }
}

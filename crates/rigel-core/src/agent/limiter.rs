//! Item 16: one token bucket per provider, shared by every agent in the process.
//!
//! Before this, a 429 was answered by each caller's own retry loop, so three agents on one door
//! produced three retries. This is the single place a provider's budget lives: a caller acquires
//! a permit before a request, and the caller that saw the 429 reports its `Retry-After` so every
//! other caller waits too. For tests the clock is not injected; the rates are, so the accounting
//! is exercised without sleeping for real quota windows.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Tokio's clock, so `start_paused` can test the refill and the penalty without sleeping.
use tokio::time::Instant;

/// Proof that one request may be sent. `waited` is how long the limiter held it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permit {
    pub waited: Duration,
}

struct Bucket {
    tokens: f64,
    updated: Instant,
    blocked_until: Option<Instant>,
}

struct State {
    capacity: f64,
    refill_per_second: f64,
    buckets: HashMap<String, Bucket>,
}

/// A process-wide token bucket per provider.
pub struct RateLimiter {
    state: Mutex<State>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    /// Four requests in hand and one more per second: enough for a normal turn, tight enough
    /// that a retry storm queues instead of multiplying.
    pub fn new() -> Self {
        Self::with_rate(12.0, 6.0)
    }

    pub fn with_rate(capacity: f64, refill_per_second: f64) -> Self {
        Self {
            state: Mutex::new(State {
                capacity: capacity.max(1.0),
                refill_per_second: refill_per_second.max(0.000_001),
                buckets: HashMap::new(),
            }),
        }
    }

    /// The one limiter every agent in this process shares.
    pub fn shared() -> Arc<RateLimiter> {
        static SHARED: OnceLock<Arc<RateLimiter>> = OnceLock::new();
        SHARED.get_or_init(|| Arc::new(RateLimiter::new())).clone()
    }

    /// Wait for a permit. A cancelled caller drops the future; no permit is leaked because the
    /// token is only taken when `try_acquire` succeeds.
    pub async fn acquire(&self, provider: &str) -> Permit {
        let started = Instant::now();
        loop {
            match self.try_acquire(provider, Instant::now()) {
                Ok(_) => {
                    return Permit {
                        waited: started.elapsed(),
                    };
                }
                Err(wait) => tokio::time::sleep(wait).await,
            }
        }
    }

    /// Record a provider's `Retry-After`: every caller waits, not only the one that saw it.
    pub fn penalize(&self, provider: &str, retry_after: Duration) {
        let now = Instant::now();
        let mut state = self.lock();
        let capacity = state.capacity;
        let bucket = state.buckets.entry(provider.to_string()).or_insert(Bucket {
            tokens: capacity,
            updated: now,
            blocked_until: None,
        });
        bucket.tokens = 0.0;
        bucket.updated = now;
        bucket.blocked_until = Some(now + retry_after.max(Duration::from_millis(1)));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn try_acquire(&self, provider: &str, now: Instant) -> Result<Permit, Duration> {
        let mut state = self.lock();
        let capacity = state.capacity;
        let refill = state.refill_per_second;
        let bucket = state.buckets.entry(provider.to_string()).or_insert(Bucket {
            tokens: capacity,
            updated: now,
            blocked_until: None,
        });

        if let Some(until) = bucket.blocked_until {
            if now < until {
                return Err(until - now);
            }
            bucket.blocked_until = None;
        }

        let elapsed = now.saturating_duration_since(bucket.updated);
        bucket.tokens = (bucket.tokens + elapsed.as_secs_f64() * refill).min(capacity);
        bucket.updated = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(Permit {
                waited: Duration::ZERO,
            })
        } else {
            Err(Duration::from_secs_f64((1.0 - bucket.tokens) / refill))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_callers_on_one_token_produce_exactly_one_permit_and_two_waits() {
        let limiter = RateLimiter::with_rate(1.0, 0.000_001);
        let now = Instant::now();
        assert!(limiter.try_acquire("groq", now).is_ok(), "the first sends");
        let second = limiter
            .try_acquire("groq", now)
            .expect_err("the second must wait");
        let third = limiter
            .try_acquire("groq", now)
            .expect_err("the third must wait");
        assert!(second > Duration::ZERO && third > Duration::ZERO);
    }

    #[test]
    fn buckets_are_separate_per_provider() {
        let limiter = RateLimiter::with_rate(1.0, 0.000_001);
        let now = Instant::now();
        assert!(limiter.try_acquire("groq", now).is_ok());
        assert!(
            limiter.try_acquire("anthropic", now).is_ok(),
            "one provider's spend must not block another"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn one_token_hands_out_three_sends_in_order() {
        let limiter = Arc::new(RateLimiter::with_rate(1.0, 1.0));
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut tasks = Vec::new();
        for id in 0..3 {
            let limiter = limiter.clone();
            let order = order.clone();
            tasks.push(tokio::spawn(async move {
                limiter.acquire("groq").await;
                order.lock().unwrap().push(id);
            }));
        }
        tokio::task::yield_now().await;
        assert_eq!(
            *order.lock().unwrap(),
            vec![0],
            "one token, one immediate send"
        );

        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            *order.lock().unwrap(),
            vec![0, 1],
            "the first wait is served at the next refill"
        );

        tokio::time::advance(Duration::from_secs(1)).await;
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(*order.lock().unwrap(), vec![0, 1, 2]);
    }

    #[tokio::test(start_paused = true)]
    async fn retry_after_blocks_every_caller_until_it_expires() {
        let limiter = Arc::new(RateLimiter::with_rate(4.0, 1.0));
        limiter.penalize("groq", Duration::from_secs(2));
        let waiting = {
            let limiter = limiter.clone();
            tokio::spawn(async move { limiter.acquire("groq").await.waited })
        };
        tokio::task::yield_now().await;
        assert!(
            !waiting.is_finished(),
            "a penalised door must not answer immediately"
        );

        tokio::time::advance(Duration::from_millis(1_900)).await;
        tokio::task::yield_now().await;
        assert!(
            !waiting.is_finished(),
            "the Retry-After window is still open"
        );

        tokio::time::advance(Duration::from_millis(100)).await;
        let waited = waiting.await.unwrap();
        assert!(waited >= Duration::from_millis(1_900), "{waited:?}");
    }
}

//! The bill, and the kill switch (§8.3).
//!
//! Accounting is post-hoc per step: no provider offers a hard stop in the middle of a stream,
//! so the guarantee this service makes is **stops within one turn** — the step that crosses
//! the cap is billed, the turn ends before its next step, every running agent in the session
//! is interrupted, and the next turn is refused before it starts.
//!
//! `reserve` is deliberately conservative: it refuses a turn when the *estimate* would cross
//! the cap. The estimate is one cent, not a guess at the answer's length; over-reserving
//! would make small caps unusable, and under-reserving is caught by `account`.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sqlx::SqlitePool;

use crate::agent::pricing::{cost_micros, tokens_for};
use crate::domain::{CostSource, Usage, cents_to_micros, micros_to_cents};
use crate::migrate::now_ms;

/// The soft warning fires here, and the transcript shows an inline *Raise cap / Stop* prompt.
pub const WARN_PERCENT: i64 = 80;

/// What a turn costs before it runs: one cent. Enough to refuse a turn on an exhausted
/// session, small enough not to block a $0.25 cap.
pub const RESERVE_MICROS: i64 = 10_000;

/// D116: $20 a day, across every session in this project, on by default. It sits above the
/// session cap (D10) and the task cap (D92), which is why crossing it interrupts every
/// running agent rather than the one that happened to report last.
pub const DEFAULT_DAILY_CAP_MICROS: i64 = 20_000_000;

/// D113: three writable agents at once, configurable upward. Read-only children are not
/// counted and are not limited by this.
pub const DEFAULT_WRITABLE_AGENTS: i64 = 3;

/// Called when the cap is crossed. The native coordinator's implementation cancels the
/// turn's token; a future adapter implementation sends the harness its own interrupt.
pub trait InterruptSink: Send + Sync {
    fn interrupt(&self, agent_id: &str, reason: &str);
}

pub struct NullInterrupt;

impl InterruptSink for NullInterrupt {
    fn interrupt(&self, _agent_id: &str, _reason: &str) {}
}

#[derive(Debug, thiserror::Error)]
pub enum BudgetError {
    #[error(
        "session budget exhausted: {spend_micros}µ$ spent of {cap_micros}µ$ — raise the cap to continue"
    )]
    Exhausted {
        session_id: String,
        spend_micros: i64,
        cap_micros: i64,
    },

    #[error(
        "task cap exhausted: {spend_micros}µ$ spent of {cap_micros}µ$ — raise the task cap to continue"
    )]
    TaskExhausted {
        agent_id: String,
        spend_micros: i64,
        cap_micros: i64,
    },

    #[error(
        "daily budget exhausted: {spend_micros}µ$ spent of {cap_micros}µ$ on {day} - raise the daily cap to continue"
    )]
    DailyExhausted {
        day: String,
        spend_micros: i64,
        cap_micros: i64,
    },

    #[error("agent {0} does not exist")]
    AgentNotFound(String),

    #[error("session {0} does not exist")]
    SessionNotFound(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetOutcome {
    Ok,
    /// At or above 80% of the cap, below it.
    Warning,
    /// Above the cap. Interrupts have been sent, once.
    Exceeded,
    /// D116: the day's cap, not the session's, was crossed. Every running agent is interrupted.
    DailyExceeded,
}

/// The day's ledger and the settings that bound it (D113, D116).
#[derive(Debug, Clone, Serialize)]
pub struct DailyStatus {
    pub day: String,
    pub spend_micros: i64,
    pub cap_micros: Option<i64>,
}

/// The two safety knobs that live beside the money: how many agents may write at once, and
/// what the day may cost.
#[derive(Debug, Clone, Serialize)]
pub struct SafetySettings {
    pub writable_agents: i64,
    pub daily_cap_micros: Option<i64>,
}

/// A UTC calendar day, so the cap resets at a fixed instant regardless of the machine's
/// time zone. Civil-date arithmetic (Hinnant) keeps a dependency out of a one-line need.
pub fn utc_day(now: i64) -> String {
    let days = now.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = y + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetSnapshot {
    pub session_id: String,
    pub agent_id: String,
    pub session_micros: i64,
    pub agent_micros: i64,
    pub cap_micros: Option<i64>,
    pub remaining_micros: Option<i64>,
    /// D92: a cap on this one agent's task, below the session cap.
    pub task_cap_micros: Option<i64>,
    pub session_cents: i64,
    pub agent_cents: i64,
    pub cap_cents: Option<i64>,
    pub remaining_cents: Option<i64>,
}

/// What the first model call of a turn is expected to cost, and what the session has left.
///
/// Advisory only: [`BudgetService::reserve`] and [`BudgetService::account`] stay the
/// authority. The quote exists so the composer can say "this will cost about this much"
/// before a request is made — it never refuses a turn by itself.
#[derive(Debug, Clone, Serialize)]
pub struct TurnQuote {
    pub model: String,
    /// The fixed request context (system prompt and tool schemas) plus the prompt.
    pub input_tokens: u64,
    /// Assumed completion length, not a forecast.
    pub output_tokens: u64,
    /// The first step only: a turn may make several model calls.
    pub first_step_micros: i64,
    pub spend_micros: i64,
    pub cap_micros: Option<i64>,
    pub remaining_micros: Option<i64>,
    /// The estimate alone is more than the cap leaves. The step still runs if `reserve`
    /// accepts it; the cap then stops the turn after that step.
    pub over_remaining: bool,
    /// What `reserve` will require before the turn starts, so the UI can say whether an
    /// over-budget turn is refused outright or stopped after its first step.
    pub reserve_micros: i64,
}

/// Enough to look like a real answer for a short prompt, small enough not to overstate a
/// long one. The cap, not this number, is what protects the session.
const OUTPUT_FLOOR: u64 = 256;
const OUTPUT_CEILING: u64 = 2_048;

/// Price the first step of a turn without sending anything.
///
/// `context_chars` is the system prompt plus the tool schemas — the part of the request the
/// user cannot see in the composer. Tokens are approximated at four characters each, so the
/// result is deliberately an estimate; the provider's usage replaces it as steps land.
pub fn quote_turn(
    model: &str,
    context_chars: usize,
    prompt: &str,
    spend_micros: i64,
    cap_micros: Option<i64>,
) -> TurnQuote {
    let input_tokens = tokens_for(context_chars + prompt.chars().count());
    let output_tokens = (input_tokens / 2).clamp(OUTPUT_FLOOR, OUTPUT_CEILING);
    let first_step_micros = cost_micros(model, input_tokens, 0, output_tokens);
    let remaining_micros = cap_micros.map(|cap| cap - spend_micros);
    TurnQuote {
        model: model.to_string(),
        input_tokens,
        output_tokens,
        first_step_micros,
        spend_micros,
        cap_micros,
        remaining_micros,
        over_remaining: remaining_micros
            .map(|remaining| first_step_micros > remaining)
            .unwrap_or(false),
        reserve_micros: RESERVE_MICROS,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileLine {
    pub agent_id: String,
    pub recorded_micros: i64,
    pub summed_micros: i64,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileReport {
    pub session_id: String,
    pub session_recorded_micros: i64,
    pub session_summed_micros: i64,
    pub ok: bool,
    pub agents: Vec<ReconcileLine>,
}

#[derive(Default)]
struct BudgetState {
    /// Sessions whose 80% prompt has already fired.
    warned: HashSet<String>,
    /// Sessions whose cap has already been enforced. Interrupts are sent once per session.
    killed: HashSet<String>,
    /// The last day whose daily 80% warning fired (D116).
    daily_warned: Option<String>,
}

/// Owns every write to `spend_*`, `turns.usage_json` and the warning/kill bookkeeping.
///
/// There is one instance per process, shared behind an `Arc`: two services would each fire
/// their own interrupt.
pub struct BudgetService {
    pool: SqlitePool,
    interrupt: Arc<dyn InterruptSink>,
    state: Mutex<BudgetState>,
    /// One writer at a time. Concurrent subagents each report usage; SQLite serialises the
    /// writes but two deferred transactions can read the same session total and then fight
    /// for the upgrade, so the read-modify-write in [`BudgetService::account`] is serialised
    /// here instead. The work under it is a single short transaction.
    writer: tokio::sync::Mutex<()>,
}

impl BudgetService {
    pub fn new(pool: SqlitePool, interrupt: Arc<dyn InterruptSink>) -> Self {
        Self {
            pool,
            interrupt,
            state: Mutex::new(BudgetState::default()),
            writer: tokio::sync::Mutex::new(()),
        }
    }

    pub fn with_null_interrupt(pool: SqlitePool) -> Self {
        Self::new(pool, Arc::new(NullInterrupt))
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// D113/D116: the two safety knobs, from the one row that holds their defaults.
    pub async fn safety_settings(&self) -> Result<SafetySettings, BudgetError> {
        let row: Option<(i64, Option<i64>)> = sqlx::query_as(
            "SELECT writable_agents, daily_cap_micros FROM safety_settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(match row {
            Some((writable_agents, daily_cap_micros)) => SafetySettings {
                writable_agents,
                daily_cap_micros,
            },
            None => SafetySettings {
                writable_agents: DEFAULT_WRITABLE_AGENTS,
                daily_cap_micros: Some(DEFAULT_DAILY_CAP_MICROS),
            },
        })
    }

    pub async fn set_writable_agents(&self, count: i64) -> Result<(), BudgetError> {
        let count = count.clamp(1, 16);
        sqlx::query("UPDATE safety_settings SET writable_agents = ? WHERE id = 1")
            .bind(count)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The cap for the day. `None` turns it off; the value is in micro-dollars.
    pub async fn set_daily_cap(
        &self,
        session_id: &str,
        cap_micros: Option<i64>,
    ) -> Result<(), BudgetError> {
        sqlx::query("UPDATE safety_settings SET daily_cap_micros = ? WHERE id = 1")
            .bind(cap_micros)
            .execute(&self.pool)
            .await?;
        {
            let mut state = self.state.lock().expect("budget state poisoned");
            state.daily_warned = None;
        }
        sqlx::query(
            "INSERT INTO interventions (id, session_id, kind, detail_json, at)
             VALUES (?, ?, 'daily_cap_changed', ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(serde_json::json!({ "cap_micros": cap_micros }).to_string())
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn daily_spent(&self, day: &str) -> Result<i64, BudgetError> {
        let row: Option<(i64,)> = sqlx::query_as("SELECT micros FROM daily_spend WHERE day = ?")
            .bind(day)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(micros,)| micros).unwrap_or(0))
    }

    pub async fn daily_status(&self) -> Result<DailyStatus, BudgetError> {
        let day = utc_day(now_ms());
        let spend_micros = self.daily_spent(&day).await?;
        let cap_micros = self.safety_settings().await?.daily_cap_micros;
        Ok(DailyStatus {
            day,
            spend_micros,
            cap_micros,
        })
    }

    /// The 80% point of the daily cap, once per day. The caller emits the event; the flag is
    /// claimed here so two concurrent agents cannot both announce it.
    pub async fn daily_warning_due(&self) -> Result<Option<DailyStatus>, BudgetError> {
        let status = self.daily_status().await?;
        let Some(cap) = status.cap_micros else {
            return Ok(None);
        };
        if cap <= 0 || status.spend_micros * 100 < cap * WARN_PERCENT {
            return Ok(None);
        }
        let mut state = self.state.lock().expect("budget state poisoned");
        if state.daily_warned.as_deref() == Some(status.day.as_str()) {
            return Ok(None);
        }
        state.daily_warned = Some(status.day.clone());
        Ok(Some(status))
    }

    async fn session_of(&self, agent_id: &str) -> Result<(String, i64, Option<i64>), BudgetError> {
        let row: Option<(String, i64, Option<i64>)> = sqlx::query_as(
            "SELECT s.id, s.spend_micros, s.budget_cents
             FROM sessions s JOIN agents a ON a.session_id = s.id
             WHERE a.id = ?",
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await?;

        row.ok_or_else(|| BudgetError::AgentNotFound(agent_id.to_string()))
    }

    /// Refuse a turn that would cross the cap before it makes a request.
    pub async fn reserve(&self, agent_id: &str) -> Result<(), BudgetError> {
        self.reserve_with(agent_id, RESERVE_MICROS).await
    }

    /// Estimate the first step of a turn against this agent's session. Reads only.
    pub async fn quote(
        &self,
        agent_id: &str,
        model: &str,
        context_chars: usize,
        prompt: &str,
    ) -> Result<TurnQuote, BudgetError> {
        let (session_id, _spend, _cap) = self.session_of(agent_id).await?;
        self.quote_session(&session_id, model, context_chars, prompt)
            .await
    }

    /// The same estimate for a session that has no agent yet — the composer quotes the very
    /// first turn, before anything exists to charge it to.
    pub async fn quote_session(
        &self,
        session_id: &str,
        model: &str,
        context_chars: usize,
        prompt: &str,
    ) -> Result<TurnQuote, BudgetError> {
        let row: Option<(i64, Option<i64>)> =
            sqlx::query_as("SELECT spend_micros, budget_cents FROM sessions WHERE id = ?")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?;
        let (spend, cap_cents) =
            row.ok_or_else(|| BudgetError::SessionNotFound(session_id.to_string()))?;
        Ok(quote_turn(
            model,
            context_chars,
            prompt,
            spend,
            cap_cents.map(cents_to_micros),
        ))
    }

    pub async fn reserve_with(&self, agent_id: &str, estimate: i64) -> Result<(), BudgetError> {
        let (session_id, spend, cap_cents) = self.session_of(agent_id).await?;
        let Some(cap_cents) = cap_cents else {
            return Ok(());
        };
        let cap = cents_to_micros(cap_cents);
        if spend + estimate > cap {
            return Err(BudgetError::Exhausted {
                session_id,
                spend_micros: spend,
                cap_micros: cap,
            });
        }
        if let Some(task_cap) = self.task_cap(agent_id).await? {
            let (agent_spend,): (i64,) =
                sqlx::query_as("SELECT spend_micros FROM agents WHERE id = ?")
                    .bind(agent_id)
                    .fetch_one(&self.pool)
                    .await?;
            if agent_spend + estimate > task_cap {
                return Err(BudgetError::TaskExhausted {
                    agent_id: agent_id.to_string(),
                    spend_micros: agent_spend,
                    cap_micros: task_cap,
                });
            }
        }
        // D116: the day is the outermost cap, so it is checked last and refuses the turn
        // outright rather than letting one more step through.
        if let Some(cap) = self.safety_settings().await?.daily_cap_micros {
            let day = utc_day(now_ms());
            let spent = self.daily_spent(&day).await?;
            if spent + estimate > cap {
                return Err(BudgetError::DailyExhausted {
                    day,
                    spend_micros: spent,
                    cap_micros: cap,
                });
            }
        }
        Ok(())
    }

    /// The task cap for an agent, if one was set (D92).
    pub async fn task_cap(&self, agent_id: &str) -> Result<Option<i64>, BudgetError> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT cap_micros FROM task_caps WHERE agent_id = ?")
                .bind(agent_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(cap,)| cap))
    }

    /// Set or clear a task cap. `None` clears it; the session cap still applies.
    pub async fn set_task_cap(
        &self,
        session_id: &str,
        agent_id: &str,
        cap_micros: Option<i64>,
    ) -> Result<(), BudgetError> {
        if let Some(cap) = cap_micros {
            sqlx::query(
                "INSERT INTO task_caps (session_id, agent_id, cap_micros, set_at)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(session_id, agent_id)
                 DO UPDATE SET cap_micros = excluded.cap_micros, set_at = excluded.set_at",
            )
            .bind(session_id)
            .bind(agent_id)
            .bind(cap)
            .bind(now_ms())
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query("DELETE FROM task_caps WHERE agent_id = ?")
                .bind(agent_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// Charge a turn and decide whether the session must stop.
    ///
    /// The turn's `usage_json` is written here, in the same transaction as the agent and
    /// session totals, so I6's sum check has one writer and no window where they disagree.
    pub async fn account(
        &self,
        agent_id: &str,
        turn_id: &str,
        usage: &Usage,
    ) -> Result<BudgetOutcome, BudgetError> {
        let _writer = self.writer.lock().await;
        let (session_id, _spend_before, cap_cents) = self.session_of(agent_id).await?;

        // `BEGIN IMMEDIATE`, not the deferred default: this transaction reads `turns`
        // before it writes, and in WAL mode a deferred transaction that upgrades after a
        // read fails with SQLITE_BUSY_SNAPSHOT (517) if the log moved underneath it —
        // immediately, without consulting `busy_timeout`, so retrying on a lock error never
        // sees it. Taking the write lock first turns that into an ordinary wait.
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        // A native turn makes several model calls, so `account` runs several times per turn.
        // Merge with the existing usage row: reconcile sums `usage_json` per turn and would
        // otherwise count only the last call.
        let existing: Option<(Option<String>,)> =
            sqlx::query_as("SELECT usage_json FROM turns WHERE id = ?")
                .bind(turn_id)
                .fetch_optional(&mut *tx)
                .await?;
        let mut merged = existing
            .and_then(|(json,)| json)
            .and_then(|json| serde_json::from_str::<Usage>(&json).ok())
            .unwrap_or_default();
        merged.add(usage);
        let usage_json = serde_json::to_string(&merged).unwrap_or_else(|_| "{}".to_string());

        sqlx::query(
            "UPDATE turns SET usage_json = ?, stop_reason = COALESCE(stop_reason, ?) WHERE id = ?",
        )
        .bind(&usage_json)
        .bind("usage_recorded")
        .bind(turn_id)
        .execute(&mut *tx)
        .await?;

        // `spend_micros` on the right-hand side is the pre-update value: cents are rounded
        // from the new total, not from each turn.
        sqlx::query(
            "UPDATE agents SET
                 spend_cents  = (spend_micros + ? + 5000) / 10000,
                 spend_micros = spend_micros + ?,
                 tokens_in    = tokens_in + ?,
                 tokens_out   = tokens_out + ?
             WHERE id = ?",
        )
        .bind(usage.cost_micros)
        .bind(usage.cost_micros)
        .bind((usage.input_tokens + usage.cached_input_tokens) as i64)
        .bind((usage.output_tokens + usage.reasoning_tokens) as i64)
        .bind(agent_id)
        .execute(&mut *tx)
        .await?;

        let (spend,): (i64,) = sqlx::query_as("SELECT spend_micros FROM sessions WHERE id = ?")
            .bind(&session_id)
            .fetch_one(&mut *tx)
            .await?;
        let new_spend = spend + usage.cost_micros;
        sqlx::query("UPDATE sessions SET spend_micros = ?, spend_cents = ? WHERE id = ?")
            .bind(new_spend)
            .bind(micros_to_cents(new_spend))
            .bind(&session_id)
            .execute(&mut *tx)
            .await?;

        // D116: the day's total moves in the same transaction as the session's, so the two
        // can never disagree after a crash.
        let day = utc_day(now_ms());
        sqlx::query(
            "INSERT INTO daily_spend (day, micros) VALUES (?, ?)
             ON CONFLICT(day) DO UPDATE SET micros = micros + excluded.micros",
        )
        .bind(&day)
        .bind(usage.cost_micros)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // The daily cap is checked before the session one: it is the wider promise, and the
        // transcript should say which wall was hit.
        if let Some(cap) = self.safety_settings().await?.daily_cap_micros {
            if usage.cost_micros > 0 {
                let spent = self.daily_spent(&day).await?;
                if spent > cap {
                    let running: Vec<(String,)> = sqlx::query_as(
                        "SELECT id FROM agents WHERE session_id = ? AND status = 'running'",
                    )
                    .bind(&session_id)
                    .fetch_all(&self.pool)
                    .await?;
                    for (id,) in running {
                        self.interrupt.interrupt(&id, "daily_budget_exceeded");
                    }
                    return Ok(BudgetOutcome::DailyExceeded);
                }
            }
        }

        let Some(cap_cents) = cap_cents else {
            return Ok(BudgetOutcome::Ok);
        };
        let cap = cents_to_micros(cap_cents);

        if new_spend > cap {
            let first = {
                let mut state = self.state.lock().expect("budget state poisoned");
                state.killed.insert(session_id.clone())
            };
            if first {
                let running: Vec<(String,)> = sqlx::query_as(
                    "SELECT id FROM agents WHERE session_id = ? AND status = 'running'",
                )
                .bind(&session_id)
                .fetch_all(&self.pool)
                .await?;
                for (id,) in running {
                    self.interrupt.interrupt(&id, "budget_exceeded");
                }
            }
            return Ok(BudgetOutcome::Exceeded);
        }

        if cap > 0 && new_spend * 100 >= cap * WARN_PERCENT {
            let first = {
                let mut state = self.state.lock().expect("budget state poisoned");
                state.warned.insert(session_id.clone())
            };
            if first {
                return Ok(BudgetOutcome::Warning);
            }
        }

        Ok(BudgetOutcome::Ok)
    }

    /// True when the provider reported a cost the local table cannot explain, or the sums
    /// disagree. Called at boot and by `rigel doctor` (I6).
    pub async fn reconcile(&self, session_id: &str) -> Result<ReconcileReport, BudgetError> {
        let (session_recorded,): (i64,) =
            sqlx::query_as("SELECT spend_micros FROM sessions WHERE id = ?")
                .bind(session_id)
                .fetch_one(&self.pool)
                .await?;

        let agents: Vec<(String, i64)> =
            sqlx::query_as("SELECT id, spend_micros FROM agents WHERE session_id = ?")
                .bind(session_id)
                .fetch_all(&self.pool)
                .await?;

        let mut lines = Vec::new();
        let mut session_summed = 0;
        for (agent_id, recorded) in agents {
            let turns: Vec<(Option<String>,)> =
                sqlx::query_as("SELECT usage_json FROM turns WHERE agent_id = ?")
                    .bind(&agent_id)
                    .fetch_all(&self.pool)
                    .await?;
            let summed: i64 = turns
                .iter()
                .filter_map(|(json,)| json.as_deref())
                .filter_map(|json| serde_json::from_str::<Usage>(json).ok())
                .map(|usage| usage.cost_micros)
                .sum();
            session_summed += summed;
            lines.push(ReconcileLine {
                agent_id,
                recorded_micros: recorded,
                summed_micros: summed,
                ok: recorded == summed,
            });
        }

        Ok(ReconcileReport {
            session_id: session_id.to_string(),
            session_recorded_micros: session_recorded,
            session_summed_micros: session_summed,
            ok: session_recorded == session_summed && lines.iter().all(|line| line.ok),
            agents: lines,
        })
    }

    pub async fn snapshot(&self, agent_id: &str) -> Result<BudgetSnapshot, BudgetError> {
        let (session_id, session_micros, cap_cents) = self.session_of(agent_id).await?;
        let (agent_micros,): (i64,) =
            sqlx::query_as("SELECT spend_micros FROM agents WHERE id = ?")
                .bind(agent_id)
                .fetch_one(&self.pool)
                .await?;
        let cap_micros = cap_cents.map(cents_to_micros);
        let task_cap_micros = self.task_cap(agent_id).await?;
        Ok(BudgetSnapshot {
            session_id,
            agent_id: agent_id.to_string(),
            session_micros,
            agent_micros,
            cap_micros,
            remaining_micros: cap_micros.map(|cap| cap - session_micros),
            task_cap_micros,
            session_cents: micros_to_cents(session_micros),
            agent_cents: micros_to_cents(agent_micros),
            cap_cents,
            remaining_cents: cap_cents.map(|cap| cap - micros_to_cents(session_micros)),
        })
    }

    /// Change the cap. A raise resets the kill and warning flags so the next turn is
    /// allowed and the next threshold can fire again; a lower cap is enforced on the next
    /// `account`, not retroactively.
    pub async fn set_cap(
        &self,
        session_id: &str,
        cap_cents: Option<i64>,
    ) -> Result<(), BudgetError> {
        sqlx::query("UPDATE sessions SET budget_cents = ? WHERE id = ?")
            .bind(cap_cents)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        {
            let mut state = self.state.lock().expect("budget state poisoned");
            state.killed.remove(session_id);
            state.warned.remove(session_id);
        }
        Ok(())
    }

    /// Record a user decision on the 80% prompt. Raising the cap is an intervention (§8.8).
    pub async fn raise_cap(&self, session_id: &str, new_cap_cents: i64) -> Result<(), BudgetError> {
        self.set_cap(session_id, Some(new_cap_cents)).await?;
        sqlx::query(
            "INSERT INTO interventions (id, session_id, kind, detail_json, at)
             VALUES (?, ?, 'budget_raised', ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(serde_json::json!({ "cap_cents": new_cap_cents }).to_string())
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        sqlx::query("UPDATE sessions SET interventions = interventions + 1 WHERE id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// `usage.source` is meaningless for a zero-cost turn; this keeps the transcript honest
/// without inventing a provider figure.
pub fn zero_usage() -> Usage {
    Usage {
        source: CostSource::Estimated,
        ..Usage::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SONNET: &str = "anthropic/claude-sonnet-4.5";

    #[test]
    fn a_quote_prices_the_fixed_context_and_the_prompt() {
        // 2,000 + 4,000 chars at 4 chars/token = 1,500 input; output is half of that.
        // Sonnet: 1,500 × $3/M + 750 × $15/M = 4,500 + 11,250 µ$.
        let quote = quote_turn(SONNET, 2_000, &"x".repeat(4_000), 0, Some(250_000));
        assert_eq!(quote.input_tokens, 1_500);
        assert_eq!(quote.output_tokens, 750);
        assert_eq!(quote.first_step_micros, 15_750);
        assert_eq!(quote.remaining_micros, Some(250_000));
        assert!(!quote.over_remaining);
        assert_eq!(quote.reserve_micros, RESERVE_MICROS);
    }

    #[test]
    fn the_quote_warns_when_only_the_reserve_would_fit() {
        let quote = quote_turn(SONNET, 2_000, &"x".repeat(4_000), 249_000, Some(250_000));
        assert_eq!(quote.remaining_micros, Some(1_000));
        assert!(quote.over_remaining, "$0.0158 does not fit in $0.001");
        assert!(quote.first_step_micros > quote.reserve_micros);
    }

    #[test]
    fn the_quote_is_the_same_shape_without_a_cap() {
        let quote = quote_turn(SONNET, 0, "hi", 0, None);
        assert_eq!(quote.cap_micros, None);
        assert_eq!(quote.remaining_micros, None);
        assert!(!quote.over_remaining);
        assert!(quote.first_step_micros > 0, "a tiny prompt is still billed");
    }

    #[test]
    fn the_assumed_completion_hits_a_floor_and_a_ceiling() {
        let tiny = quote_turn(SONNET, 0, "hi", 0, None);
        assert_eq!(tiny.input_tokens, 1);
        assert_eq!(tiny.output_tokens, OUTPUT_FLOOR);

        let huge = quote_turn(SONNET, 1_000_000, "", 0, None);
        assert_eq!(huge.output_tokens, OUTPUT_CEILING);
    }
}

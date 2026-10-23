//! Spend over time (D92). A read-only query over the turns already recorded; it never writes.

use std::collections::BTreeMap;

use serde::Serialize;
use sqlx::SqlitePool;

use crate::domain::Usage;

#[derive(Debug, Clone, Serialize)]
pub struct SpendRow {
    pub key: String,
    pub label: String,
    pub micros: i64,
    pub turns: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpendReport {
    pub total_micros: i64,
    pub turns: i64,
    pub by_session: Vec<SpendRow>,
    pub by_agent: Vec<SpendRow>,
    pub by_day: Vec<SpendRow>,
}

/// `YYYY-MM-DD` from Unix days, without pulling in a date-time crate for one label.
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Group every turn's authoritative cost. `label` is human-facing; `key` is stable.
pub async fn report(pool: &SqlitePool) -> Result<SpendReport, sqlx::Error> {
    // agent, session, title, role, usage JSON, started_at
    type TurnRow = (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
    );
    let rows: Vec<TurnRow> = sqlx::query_as(
        "SELECT t.agent_id, a.session_id, s.title, a.role, t.usage_json, t.started_at
             FROM turns t
             JOIN agents a ON a.id = t.agent_id
             JOIN sessions s ON s.id = a.session_id
             WHERE t.usage_json IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    let mut by_session: BTreeMap<String, SpendRow> = BTreeMap::new();
    let mut by_agent: BTreeMap<String, SpendRow> = BTreeMap::new();
    let mut by_day: BTreeMap<String, SpendRow> = BTreeMap::new();
    let mut total = 0i64;
    for (agent_id, session_id, title, role, usage, started_at) in rows {
        let Some(usage) = usage.and_then(|json| serde_json::from_str::<Usage>(&json).ok()) else {
            continue;
        };
        let cost = usage.cost_micros;
        total += cost;
        let session_label = title.unwrap_or_else(|| session_id.chars().take(8).collect());
        let session = by_session.entry(session_id.clone()).or_insert(SpendRow {
            key: session_id,
            label: session_label,
            micros: 0,
            turns: 0,
        });
        session.micros += cost;
        session.turns += 1;
        let agent_label = format!(
            "{} · {}",
            role.unwrap_or_else(|| "agent".to_string()),
            agent_id.chars().take(8).collect::<String>()
        );
        let agent = by_agent.entry(agent_id.clone()).or_insert(SpendRow {
            key: agent_id,
            label: agent_label,
            micros: 0,
            turns: 0,
        });
        agent.micros += cost;
        agent.turns += 1;
        let day = civil_date(started_at.div_euclid(86_400_000));
        let entry = by_day.entry(day.clone()).or_insert(SpendRow {
            key: day.clone(),
            label: day,
            micros: 0,
            turns: 0,
        });
        entry.micros += cost;
        entry.turns += 1;
    }

    fn sorted(map: BTreeMap<String, SpendRow>) -> Vec<SpendRow> {
        let mut rows: Vec<SpendRow> = map.into_values().collect();
        rows.sort_by(|left, right| {
            right
                .micros
                .cmp(&left.micros)
                .then(left.key.cmp(&right.key))
        });
        rows
    }
    let turns = by_session.values().map(|row| row.turns).sum();
    Ok(SpendReport {
        total_micros: total,
        turns,
        by_session: sorted(by_session),
        by_agent: sorted(by_agent),
        by_day: sorted(by_day),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_cross_a_leap_year_without_a_date_crate() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(19_782), "2024-02-29");
        assert_eq!(civil_date(-1), "1969-12-31");
    }
}

//! Measured versus advertised.
//!
//! This is the feature `docs/phase-0-teardown.md` says nobody ships: a skill that claims
//! "60–90% token reduction" shows that number next to whether anyone measured it. The row
//! keeps both values, where each came from, and when it was observed; the label is derived,
//! never stored, so `unmeasured` cannot drift away from the data.
//!
//! Phase 2 records **advertised** claims from the description and leaves `measured` NULL.
//! Measuring is Phase 6 work (it needs a harness and a task) and inventing a "measured"
//! column now would be exactly the dishonesty this table exists to prevent.

use std::sync::LazyLock;

use regex::Regex;
use sqlx::SqlitePool;

use crate::migrate::now_ms;

#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub skill_id: String,
    pub metric: String,
    pub measured: Option<f64>,
    pub advertised: Option<f64>,
    pub source: String,
    pub source_url: Option<String>,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EvidenceSummary {
    pub metric: String,
    pub measured: Option<f64>,
    pub advertised: Option<f64>,
    pub source: String,
    pub source_url: Option<String>,
    pub observed_at: i64,
    /// `measured`, `unmeasured`, or `audited` for a dated third-party verdict. The shelf
    /// prints this verbatim next to the numbers.
    pub label: &'static str,
}

impl From<Evidence> for EvidenceSummary {
    fn from(evidence: Evidence) -> Self {
        let label = if evidence.metric.starts_with("audit:") {
            // A directory's verdict is dated evidence, not a number Rigel measured. It never
            // moves the grade (D93); the label says so next to the row.
            "audited"
        } else if evidence.measured.is_some() {
            "measured"
        } else {
            "unmeasured"
        };
        Self {
            metric: evidence.metric,
            measured: evidence.measured,
            advertised: evidence.advertised,
            source: evidence.source,
            source_url: evidence.source_url,
            observed_at: evidence.observed_at,
            label,
        }
    }
}

impl Evidence {
    /// Record one observation. `(skill_id, metric, source)` is the primary key, so a second
    /// observation from the same source replaces the first rather than piling up.
    pub async fn record(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO evidence
               (skill_id, metric, measured, advertised, source, source_url, observed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(skill_id, metric, source) DO UPDATE SET
               measured = excluded.measured,
               advertised = excluded.advertised,
               source_url = excluded.source_url,
               observed_at = excluded.observed_at",
        )
        .bind(&self.skill_id)
        .bind(&self.metric)
        .bind(self.measured)
        .bind(self.advertised)
        .bind(&self.source)
        .bind(&self.source_url)
        .bind(self.observed_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn for_skill(
        pool: &SqlitePool,
        skill_id: &str,
    ) -> Result<Vec<EvidenceSummary>, sqlx::Error> {
        // metric, measured, advertised, source, source_url, observed_at
        type Row = (
            String,
            Option<f64>,
            Option<f64>,
            Option<String>,
            Option<String>,
            Option<i64>,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT metric, measured, advertised, source, source_url, observed_at
               FROM evidence WHERE skill_id = ? ORDER BY metric",
        )
        .bind(skill_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(metric, measured, advertised, source, source_url, observed_at)| {
                    EvidenceSummary::from(Evidence {
                        skill_id: skill_id.to_string(),
                        metric,
                        measured,
                        advertised,
                        source: source.unwrap_or_default(),
                        source_url,
                        observed_at: observed_at.unwrap_or(0),
                    })
                },
            )
            .collect())
    }
}

/// A percentage written as a claim, with the metric it appears to be about.
pub struct Claim {
    pub metric: &'static str,
    pub value: f64,
}

/// The first percentage-like claim in a description, if any.
///
/// A range ("60–90%") records its **upper** bound: that is the number the marketing copy
/// leads with, and showing it next to `measured: null` is the point. A single percentage in
/// text about latency, cost or tokens is attributed to that metric; anything else is a
/// generic `percent-claim`.
pub fn advertised_claims(description: &str) -> Vec<Claim> {
    static RANGE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)\s*[–—-]\s*(\d+(?:\.\d+)?)\s*%").unwrap());
    static SINGLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)\s*%").unwrap());

    let lower = description.to_ascii_lowercase();
    let metric = if lower.contains("token") {
        "token-reduction"
    } else if lower.contains("cost") || lower.contains("spend") {
        "cost-reduction"
    } else if lower.contains("faster") || lower.contains("latency") || lower.contains("speed") {
        "latency-reduction"
    } else {
        "percent-claim"
    };

    if let Some(captures) = RANGE.captures(description) {
        if let Ok(high) = captures[2].parse::<f64>() {
            return vec![Claim {
                metric,
                value: high,
            }];
        }
    }

    match SINGLE.captures(description) {
        Some(captures) => match captures[1].parse::<f64>() {
            Ok(value) => vec![Claim { metric, value }],
            Err(_) => Vec::new(),
        },
        None => Vec::new(),
    }
}

/// Every advertised claim in a description, as evidence rows. `source` is the skill's own
/// text, which is the weakest possible provenance and is labelled that way in the UI.
pub fn claims_from_description(
    skill_id: &str,
    description: &str,
    url: Option<&str>,
) -> Vec<Evidence> {
    advertised_claims(description)
        .into_iter()
        .map(|claim| Evidence {
            skill_id: skill_id.to_string(),
            metric: claim.metric.to_string(),
            measured: None,
            advertised: Some(claim.value),
            source: "description".to_string(),
            source_url: url.map(ToOwned::to_owned),
            observed_at: now_ms(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_records_its_upper_bound() {
        let claims = advertised_claims("Claims 60–90% token reduction.");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].metric, "token-reduction");
        assert_eq!(claims[0].value, 90.0);
    }

    #[test]
    fn a_single_claim_is_attributed_by_keyword() {
        assert_eq!(
            advertised_claims("Cuts cost by 40%").remove(0).metric,
            "cost-reduction"
        );
        assert_eq!(
            advertised_claims("3x faster, 15% less latency")
                .remove(0)
                .metric,
            "latency-reduction"
        );
        assert_eq!(
            advertised_claims("Works 99% of the time").remove(0).metric,
            "percent-claim"
        );
    }

    #[test]
    fn no_percentage_means_no_evidence_row() {
        assert!(advertised_claims("Writes commit messages from the diff.").is_empty());
    }

    #[test]
    fn evidence_without_a_measurement_is_labelled_unmeasured() {
        let evidence =
            claims_from_description("github:acme/rtk:SKILL.md", "12% token reduction", None)
                .remove(0);
        let summary = EvidenceSummary::from(evidence);
        assert_eq!(summary.label, "unmeasured");
        assert_eq!(summary.measured, None);
        assert_eq!(summary.advertised, Some(12.0));
        assert_eq!(summary.source, "description");
    }

    #[test]
    fn a_measured_row_is_labelled_measured() {
        let summary = EvidenceSummary::from(Evidence {
            skill_id: "x".into(),
            metric: "token-reduction".into(),
            measured: Some(18.0),
            advertised: Some(90.0),
            source: "rigel bench 2026-10".into(),
            source_url: None,
            observed_at: 0,
        });
        assert_eq!(summary.label, "measured");
    }
}

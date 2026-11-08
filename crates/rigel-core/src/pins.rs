//! D141: a moved skill pin is a decision, not a silent re-pin.
//!
//! The session owns the pinned hash; the index owns the bodies. This module joins the two
//! without copying untrusted prose into the project database. When the index has moved, the
//! pinned body is read back from its content-addressed blob so the window can show a real diff.

use serde::Serialize;
use serde_json::json;

use crate::session::{SessionError, SessionService, SkillPin};
use crate::skills::{SkillIndex, SkillsError};

#[derive(Debug, thiserror::Error)]
pub enum PinError {
    #[error(transparent)]
    Session(#[from] SessionError),

    #[error(transparent)]
    Skills(#[from] SkillsError),

    #[error("skill {0} is not pinned to this session")]
    NotPinned(String),

    #[error("skill {0} is not in the local index")]
    NotIndexed(String),
}

/// The session's pin plus enough index state for the composer's changed badge. Bodies are kept
/// out of the list; [`review`] reads them only when the user opens the diff.
#[derive(Debug, Clone, Serialize)]
pub struct SkillPinView {
    pub skill_id: String,
    pub body_sha: String,
    pub pinned_at: i64,
    pub current_body_sha: Option<String>,
    pub changed: bool,
    pub pinned_body_available: bool,
    pub current_body_available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillPinReview {
    pub pin: SkillPinView,
    pub pinned_body: Option<String>,
    pub current_body: Option<String>,
}

/// Every pin on the session, with its current index status. A missing index row is represented
/// by `current_body_sha: None`; it is not an error, because an offline shelf must still list pins.
pub async fn list(
    service: &SessionService,
    index: &SkillIndex,
    session_id: &str,
) -> Result<Vec<SkillPinView>, PinError> {
    let pins = service.skill_pins(session_id).await?;
    let mut out = Vec::with_capacity(pins.len());
    for pin in pins {
        out.push(view(index, &pin).await?);
    }
    Ok(out)
}

pub async fn one(
    service: &SessionService,
    index: &SkillIndex,
    session_id: &str,
    skill_id: &str,
) -> Result<SkillPinView, PinError> {
    let pin = pinned(service, session_id, skill_id).await?;
    view(index, &pin).await
}

/// The full review: the pinned body and the current indexed body, when each is still on disk.
/// A changed pin whose old blob was collected still reports the change; the UI says the body is
/// unavailable rather than inventing a diff.
pub async fn review(
    service: &SessionService,
    index: &SkillIndex,
    session_id: &str,
    skill_id: &str,
) -> Result<SkillPinReview, PinError> {
    let pin = pinned(service, session_id, skill_id).await?;
    let current_body_sha = index.body_sha(skill_id).await?;
    let pinned_body = index.body_at(&pin.body_sha)?;
    let current_body = match current_body_sha.as_deref() {
        Some(sha) => index.body_at(sha)?,
        None => None,
    };
    Ok(SkillPinReview {
        pin: view(index, &pin).await?,
        pinned_body,
        current_body,
    })
}

/// Keep the old body. This is a deliberate intervention when the shelf moved, so the decision
/// and its reason are recorded on the session; an unchanged pin has nothing to decide.
pub async fn keep(
    service: &SessionService,
    index: &SkillIndex,
    session_id: &str,
    skill_id: &str,
    reason: &str,
) -> Result<SkillPinView, PinError> {
    let pin = pinned(service, session_id, skill_id).await?;
    let current_body_sha = index.body_sha(skill_id).await?;
    if current_body_sha.as_deref() != Some(pin.body_sha.as_str()) {
        service
            .record_intervention(
                session_id,
                "skill_pin",
                json!({
                    "action": "keep",
                    "skill_id": skill_id,
                    "body_sha": pin.body_sha,
                    "current_body_sha": current_body_sha,
                    "reason": clean_reason(reason),
                }),
            )
            .await?;
    }
    view(index, &pin).await
}

/// Keep every changed pin in one decision. The pinned hashes do not move; the reason is recorded
/// once for the batch with each pin's pinned and current hash, rather than once per skill. A
/// pin whose index row is gone is not counted as changed by the UI, so it is left out here too.
pub async fn keep_all(
    service: &SessionService,
    index: &SkillIndex,
    session_id: &str,
    reason: &str,
) -> Result<Vec<SkillPinView>, PinError> {
    let pins = service.skill_pins(session_id).await?;
    let mut keeps: Vec<(String, String, String)> = Vec::new();
    for pin in &pins {
        let Some(current_body_sha) = index.body_sha(&pin.skill_id).await? else {
            continue;
        };
        if current_body_sha == pin.body_sha {
            continue;
        }
        keeps.push((pin.skill_id.clone(), pin.body_sha.clone(), current_body_sha));
    }

    if !keeps.is_empty() {
        let rows: Vec<_> = keeps
            .iter()
            .map(|(skill_id, body_sha, current_body_sha)| {
                json!({
                    "skill_id": skill_id,
                    "body_sha": body_sha,
                    "current_body_sha": current_body_sha,
                })
            })
            .collect();
        service
            .record_intervention(
                session_id,
                "skill_pin",
                json!({
                    "action": "keep_all",
                    "keeps": rows,
                    "reason": clean_reason(reason),
                }),
            )
            .await?;
    }

    list(service, index, session_id).await
}

/// Take the current indexed body. The hash is resolved here, never supplied by the renderer.
pub async fn update(
    service: &SessionService,
    index: &SkillIndex,
    session_id: &str,
    skill_id: &str,
    reason: &str,
) -> Result<SkillPinView, PinError> {
    let pin = pinned(service, session_id, skill_id).await?;
    let current_body_sha = index
        .body_sha(skill_id)
        .await?
        .ok_or_else(|| PinError::NotIndexed(skill_id.to_string()))?;

    if current_body_sha != pin.body_sha {
        service
            .pin_skill(session_id, skill_id, &current_body_sha, true)
            .await?;
        service
            .record_intervention(
                session_id,
                "skill_pin",
                json!({
                    "action": "update",
                    "skill_id": skill_id,
                    "from_body_sha": pin.body_sha,
                    "to_body_sha": current_body_sha,
                    "reason": clean_reason(reason),
                }),
            )
            .await?;
    }

    one(service, index, session_id, skill_id).await
}

/// Update every changed pin in one decision. The reason is recorded once for the batch, with
/// each old/new pair, rather than once per skill. A changed hash whose current body is missing
/// is left pinned: moving to a body the index cannot read would only hide the problem.
pub async fn update_all(
    service: &SessionService,
    index: &SkillIndex,
    session_id: &str,
    reason: &str,
) -> Result<Vec<SkillPinView>, PinError> {
    let pins = service.skill_pins(session_id).await?;
    let mut updates: Vec<(String, String, String)> = Vec::new();
    for pin in &pins {
        let Some(current_body_sha) = index.body_sha(&pin.skill_id).await? else {
            continue;
        };
        if current_body_sha == pin.body_sha || index.body_at(&current_body_sha)?.is_none() {
            continue;
        }
        service
            .pin_skill(session_id, &pin.skill_id, &current_body_sha, true)
            .await?;
        updates.push((pin.skill_id.clone(), pin.body_sha.clone(), current_body_sha));
    }

    if !updates.is_empty() {
        let rows: Vec<_> = updates
            .iter()
            .map(|(skill_id, from_body_sha, to_body_sha)| {
                json!({
                    "skill_id": skill_id,
                    "from_body_sha": from_body_sha,
                    "to_body_sha": to_body_sha,
                })
            })
            .collect();
        service
            .record_intervention(
                session_id,
                "skill_pin",
                json!({
                    "action": "update_all",
                    "updates": rows,
                    "reason": clean_reason(reason),
                }),
            )
            .await?;
    }

    list(service, index, session_id).await
}

async fn pinned(
    service: &SessionService,
    session_id: &str,
    skill_id: &str,
) -> Result<SkillPin, PinError> {
    service
        .skill_pins(session_id)
        .await?
        .into_iter()
        .find(|pin| pin.skill_id == skill_id)
        .ok_or_else(|| PinError::NotPinned(skill_id.to_string()))
}

async fn view(index: &SkillIndex, pin: &SkillPin) -> Result<SkillPinView, PinError> {
    let current_body_sha = index.body_sha(&pin.skill_id).await?;
    let pinned_body_available = index.body_at(&pin.body_sha)?.is_some();
    let current_body_available = match current_body_sha.as_deref() {
        Some(sha) => index.body_at(sha)?.is_some(),
        None => false,
    };
    Ok(SkillPinView {
        skill_id: pin.skill_id.clone(),
        body_sha: pin.body_sha.clone(),
        pinned_at: pin.pinned_at,
        changed: current_body_sha
            .as_deref()
            .is_some_and(|sha| sha != pin.body_sha),
        current_body_sha,
        pinned_body_available,
        current_body_available,
    })
}

fn clean_reason(reason: &str) -> String {
    reason
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(280)
        .collect()
}

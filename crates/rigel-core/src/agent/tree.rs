//! Nested subagent depth and the budget a tree inherits (roadmap 44).
//!
//! A top-level turn opens a [`TreeBudget`] from what remains of its session and task
//! allowances. Every child on every level shares that one budget, so a deep tree or one
//! greedy child cannot spend past what the root was allowed. A reservation is taken before a
//! model call and replaced by the actual charge afterwards; the reservation keeps two sibling
//! children from both starting a step with the same last micro-dollar.

use std::sync::{Arc, Mutex};

use serde::Serialize;

/// The amount held back before one model step. It mirrors the session reserve, but a tree can
/// be much smaller than a session, so the guard is deliberately modest.
pub const STEP_RESERVE_MICROS: i64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeBudgetSnapshot {
    /// `None` means the root had no session or task cap to inherit.
    pub allowance_micros: Option<i64>,
    pub spent_micros: i64,
    pub reserved_micros: i64,
    pub remaining_micros: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeBudgetError {
    #[error(
        "tree budget exhausted: {remaining_micros}µ$ left of {allowance_micros}µ$ \
         (spent {spent_micros}µ$, reserved {reserved_micros}µ$); agent {agent_id} at depth \
         {depth} requested {requested_micros}µ$"
    )]
    Exhausted {
        agent_id: String,
        depth: u8,
        allowance_micros: i64,
        spent_micros: i64,
        reserved_micros: i64,
        remaining_micros: i64,
        requested_micros: i64,
    },
}

#[derive(Debug, Default)]
struct TreeState {
    allowance_micros: Option<i64>,
    spent_micros: i64,
    reserved_micros: i64,
}

/// A shared allowance. `Tools::subagent` clones the `Arc`, not the numbers, so every level
/// charges the same account.
#[derive(Debug)]
pub struct TreeBudget {
    state: Mutex<TreeState>,
}

impl Default for TreeBudget {
    fn default() -> Self {
        Self::unbounded()
    }
}

impl TreeBudget {
    /// No session or task cap: spend is still measured and reported, but a step is never
    /// refused for the tree alone.
    pub fn unbounded() -> Self {
        Self {
            state: Mutex::new(TreeState::default()),
        }
    }

    /// Open a fresh tree for a root turn. A resumed or continued root re-opens from what is
    /// actually left, rather than replaying spend the session already recorded.
    pub fn restart(&self, allowance_micros: Option<i64>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.allowance_micros = allowance_micros;
        state.spent_micros = 0;
        state.reserved_micros = 0;
    }

    pub fn snapshot(&self) -> TreeBudgetSnapshot {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        TreeBudgetSnapshot {
            allowance_micros: state.allowance_micros,
            spent_micros: state.spent_micros,
            reserved_micros: state.reserved_micros,
            remaining_micros: remaining(&state),
        }
    }

    /// Hold back `estimate_micros` for a step. The returned reservation releases itself if
    /// the step fails before usage arrives; [`Self::charge`] replaces it with the real bill.
    pub fn reserve(
        self: &Arc<Self>,
        agent_id: &str,
        depth: u8,
        estimate_micros: i64,
    ) -> Result<TreeReservation, TreeBudgetError> {
        let requested = estimate_micros.max(0);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(remaining) = remaining(&state)
            && remaining < requested
        {
            return Err(TreeBudgetError::Exhausted {
                agent_id: agent_id.to_string(),
                depth,
                allowance_micros: state.allowance_micros.unwrap_or_default(),
                spent_micros: state.spent_micros,
                reserved_micros: state.reserved_micros,
                remaining_micros: remaining,
                requested_micros: requested,
            });
        }
        state.reserved_micros = state.reserved_micros.saturating_add(requested);
        Ok(TreeReservation {
            budget: self.clone(),
            amount_micros: requested,
            released: false,
        })
    }

    /// Replace a reservation with the step's actual cost and report the new tree balance.
    /// A charge without a reservation is used when usage arrived after the guard was gone.
    pub fn charge(
        &self,
        reservation: Option<TreeReservation>,
        spent_micros: i64,
    ) -> TreeBudgetSnapshot {
        if let Some(reservation) = reservation {
            let amount = reservation.release();
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.reserved_micros = state.reserved_micros.saturating_sub(amount);
            state.spent_micros = state.spent_micros.saturating_add(spent_micros.max(0));
        } else {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.spent_micros = state.spent_micros.saturating_add(spent_micros.max(0));
        }
        self.snapshot()
    }

    fn release(&self, amount_micros: i64) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.reserved_micros = state.reserved_micros.saturating_sub(amount_micros);
    }
}

fn remaining(state: &TreeState) -> Option<i64> {
    state.allowance_micros.map(|allowance| {
        allowance
            .saturating_sub(state.spent_micros)
            .saturating_sub(state.reserved_micros)
    })
}

/// A held step estimate. Dropping it without a charge returns the reservation.
#[derive(Debug)]
pub struct TreeReservation {
    budget: Arc<TreeBudget>,
    amount_micros: i64,
    released: bool,
}

impl TreeReservation {
    fn release(mut self) -> i64 {
        self.released = true;
        self.amount_micros
    }
}

impl Drop for TreeReservation {
    fn drop(&mut self) {
        if !self.released {
            self.budget.release(self.amount_micros);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_charge_comes_out_of_the_inherited_allowance_and_reservations_bound_it() {
        let tree = Arc::new(TreeBudget::unbounded());
        tree.restart(Some(1_000));

        let first = tree.reserve("child-a", 1, 400).unwrap();
        assert_eq!(tree.snapshot().remaining_micros, Some(600));
        let after_charge = tree.charge(Some(first), 700);
        assert_eq!(after_charge.spent_micros, 700);
        assert_eq!(after_charge.reserved_micros, 0);
        assert_eq!(after_charge.remaining_micros, Some(300));

        let error = tree.reserve("child-b", 2, 500).unwrap_err();
        match error {
            TreeBudgetError::Exhausted {
                remaining_micros,
                requested_micros,
                allowance_micros,
                ..
            } => {
                assert_eq!(remaining_micros, 300);
                assert_eq!(requested_micros, 500);
                assert_eq!(allowance_micros, 1_000);
            }
        }

        // A reservation that never became a billed step is returned in full.
        let unused = tree.reserve("child-c", 2, 200).unwrap();
        assert_eq!(tree.snapshot().remaining_micros, Some(100));
        drop(unused);
        assert_eq!(tree.snapshot().remaining_micros, Some(300));
    }

    #[test]
    fn an_unbounded_tree_still_reports_what_the_children_spent() {
        let tree = Arc::new(TreeBudget::unbounded());
        tree.restart(None);
        let reservation = tree.reserve("child", 1, 50).unwrap();
        let snapshot = tree.charge(Some(reservation), 125);
        assert_eq!(snapshot.allowance_micros, None);
        assert_eq!(snapshot.spent_micros, 125);
        assert_eq!(snapshot.remaining_micros, None);
    }
}

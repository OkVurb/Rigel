//! D92: spend is attributable across sessions, and a task cap is narrower than a session cap.

use rigel_core::{spend, BudgetError, BudgetService, SessionService, Usage};

fn usage(micros: i64) -> Usage {
    Usage {
        input_tokens: 10,
        output_tokens: 5,
        cost_micros: micros,
        ..Usage::default()
    }
}

#[tokio::test]
async fn spend_sums_every_session_and_groups_by_day() {
    let tmp = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let first = service.start(Some("first"), None).await.unwrap();
    let second = service.start(Some("second"), None).await.unwrap();

    let a = service
        .register_agent_with(&first.id, "native", Some("implementer"), None)
        .await
        .unwrap();
    let b = service
        .register_agent_with(&second.id, "native", Some("reviewer"), None)
        .await
        .unwrap();
    let budget = BudgetService::with_null_interrupt(service.pool().clone());

    for (agent, micros) in [(&a.id, 20_000), (&a.id, 30_000), (&b.id, 60_000)] {
        let turn = service.append_turn(agent, Some("go")).await.unwrap();
        budget
            .account(agent, &turn.id, &usage(micros))
            .await
            .unwrap();
    }

    let report = spend::report(service.pool()).await.unwrap();
    assert_eq!(report.total_micros, 110_000);
    assert_eq!(report.turns, 3);
    assert_eq!(report.by_session.len(), 2);
    assert_eq!(report.by_session[0].label, "second", "largest first");
    assert_eq!(
        report.by_day.iter().map(|row| row.micros).sum::<i64>(),
        110_000
    );
    assert_eq!(
        report.by_agent.iter().map(|row| row.micros).sum::<i64>(),
        110_000
    );
}

#[tokio::test]
async fn a_task_cap_refuses_the_next_turn_but_the_session_can_continue() {
    let tmp = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service
        .start(Some("capped task"), Some(10_000))
        .await
        .unwrap();
    let agent = service
        .register_agent_with(&session.id, "native", Some("implementer"), None)
        .await
        .unwrap();
    let budget = BudgetService::with_null_interrupt(service.pool().clone());

    budget
        .set_task_cap(&session.id, &agent.id, Some(15_000))
        .await
        .unwrap();
    let snapshot = budget.snapshot(&agent.id).await.unwrap();
    assert_eq!(snapshot.task_cap_micros, Some(15_000));

    let turn = service.append_turn(&agent.id, Some("go")).await.unwrap();
    budget
        .account(&agent.id, &turn.id, &usage(14_000))
        .await
        .unwrap();
    // The one-cent reserve would cross the task cap, while the $1.00 session cap is far away.
    match budget.reserve(&agent.id).await {
        Err(BudgetError::TaskExhausted { cap_micros, .. }) => assert_eq!(cap_micros, 15_000),
        other => panic!("expected a task-cap refusal, got {other:?}"),
    }

    budget
        .set_task_cap(&session.id, &agent.id, None)
        .await
        .unwrap();
    budget
        .reserve(&agent.id)
        .await
        .expect("clearing the cap frees the task");
}

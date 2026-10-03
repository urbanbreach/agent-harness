use super::*;
use harness_core::event::{LiveEventEnvelope, LiveEventV1, SubagentProgressEvent};

fn publish(app: &mut AppState, generation: u64, attempt: &str, count: u32) {
    app.ingest_runtime_event(RuntimeEvent::Live(Box::new(LiveEventEnvelope {
        event_id: format!("progress-{generation}-{count}"),
        run_id: "run_app_tests".into(),
        mono_ms: 8,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("child".into())),
        correlation_id: Some(attempt.into()),
        causation_id: None,
        stream_key: None,
        payload: LiveEventV1::SubagentProgress(SubagentProgressEvent {
            child_id: "child".into(),
            attempt_id: attempt.into(),
            generation,
            duration_ms: 2500,
            turn_count: 1,
            tool_call_count: count,
            tokens_used: Some(9000),
            context_window_tokens: Some(10_000),
            context_usage_pct: Some(90),
            tools_used: vec!["read".into()],
            error_count: 0,
        }),
    })));
}

pub(super) fn while_child_open(app: &mut AppState) {
    assert_eq!(app.current_session_id(), Some("child"));
    let durable_count = app.events().count();
    publish(app, 1, "req_child", 7);
    publish(app, 0, "old_attempt", 99);
    publish(app, 1, "other_attempt", 99);
    assert_count(app, 7);
    assert_eq!(app.events().count(), durable_count, "progress is transient");
}

pub(super) fn assert_count(app: &AppState, count: usize) {
    assert_eq!(
        app.projection
            .native_subagent_task("spawn")
            .map(|task| task.child_tool_call_count),
        Some(count)
    );
}

pub(super) fn assert_native_completion(app: &mut AppState, mut finished: SubagentTransitionV1) {
    finished.transition = SubagentTransitionKind::Finished;
    finished.outcome = Some(SubagentTerminalOutcome::Completed);
    finished.accounting = Some(SubagentTerminalAccounting {
        tool_calls: 8,
        turns: 1,
        duration_ms: 1234,
        tokens_used: None,
        output_tokens_used: None,
        total_tokens_used: None,
        output_usage_incomplete: true,
    });
    for seq in [9, 10] {
        app.ingest_event(envelope(
            seq,
            "req_child",
            EventV1::SubagentTransition(Box::new(finished.clone())),
        ));
    }
    publish(app, 1, "req_child", 99);
    assert_count(app, 8);
    app.set_tool_group_outputs_expanded(&["spawn".into()], true);
    let root = render_text(&app, 120, 40);
    assert_eq!(
        root.matches("Subagent completed in 1.2s").count(),
        1,
        "{root}"
    );
    assert_eq!(root.matches("Subagent started:").count(), 1, "{root}");
    assert!(app.task_pane_rows().is_empty());
    app.tasks_pane.show_done = true;
    assert_eq!(app.task_pane_rows().len(), 2);
}

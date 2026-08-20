use super::super::*;

fn schedule_task(
    app: &mut AppState,
    seq: u64,
    request_id: &str,
    task_id: &str,
    queue_key: &str,
    parent_tool_call_id: &str,
    child_request_id: Option<&str>,
    actor: EventActor,
) {
    app.ingest_event(envelope_with_actor(
        seq,
        request_id,
        actor,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: task_id.into(),
            state: TaskScheduleState::Started,
            queue_key: Some(queue_key.to_string()),
            metadata: Some(TaskScheduleMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some(parent_tool_call_id.to_string()),
                    child_request_id: child_request_id.map(str::to_string),
                    ..TaskLineageMetadata::default()
                }),
            }),
        }),
    ));
}

fn add_correlated_subagent_rows(
    app: &mut AppState,
    seq: u64,
    suffix: &str,
    parent_tool_call_id: &str,
) {
    let request_id = format!("req_{suffix}");
    let tool_task_id = format!("task_tool_{suffix}");
    let turn_task_id = format!("task_turn_{suffix}");
    let actor = EventActor::new(ActorKind::Worker, Some(format!("child_{suffix}")));
    app.ingest_event(envelope(
        seq,
        "req_parent",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: parent_tool_call_id.into(),
            tool_id: "task".to_string(),
            args_summary: r#"{"run_in_background":false}"#.to_string(),
            args_digest: format!("digest-{parent_tool_call_id}"),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        seq + 1,
        "req_parent",
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: parent_tool_call_id.into(),
        }),
    ));
    schedule_task(
        app,
        seq + 2,
        "req_parent",
        &tool_task_id,
        "tool:task",
        parent_tool_call_id,
        None,
        EventActor::new(ActorKind::Worker, Some("parent".to_string())),
    );
    schedule_task(
        app,
        seq + 3,
        &request_id,
        &turn_task_id,
        "provider_model:default:model-1",
        parent_tool_call_id,
        Some(&request_id),
        actor,
    );
}

#[test]
fn watcher_count_deduplicates_tool_and_worker_rows_for_one_child() {
    // arrange
    // Given: one delegated child is represented by both tool and provider task rows.
    let mut app = AppState::new_live(None, false, None);
    add_correlated_subagent_rows(&mut app, 1, "alpha", "tool_alpha");

    // When: watcher counts are projected.
    let watchers = app.live_turn_watchers();

    // act
    // Then: the logical child appears exactly once.
    // assert
    assert_eq!(watchers.subagents, 1);
}

#[test]
fn watcher_count_preserves_distinct_child_lineages() {
    // arrange
    // Given: two separate children each have overlapping tool/provider rows.
    let mut app = AppState::new_live(None, false, None);
    add_correlated_subagent_rows(&mut app, 1, "alpha", "tool_alpha");
    add_correlated_subagent_rows(&mut app, 10, "beta", "tool_beta");

    // When: watcher counts are projected.
    let watchers = app.live_turn_watchers();

    // act
    // Then: deduplication preserves both stable child identities.
    // assert
    assert_eq!(watchers.subagents, 2);
}

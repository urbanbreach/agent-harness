use super::super::*;

fn schedule_task(
    app: &mut AppState,
    seq: u64,
    request_id: &str,
    task_id: &str,
    queue_key: &str,
    lineage: Option<(&str, &str)>,
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
            metadata: lineage.map(
                |(parent_tool_call_id, child_request_id)| TaskScheduleMetadata {
                    lineage: Some(TaskLineageMetadata {
                        parent_tool_call_id: Some(parent_tool_call_id.to_string()),
                        child_request_id: Some(child_request_id.to_string()),
                        ..TaskLineageMetadata::default()
                    }),
                },
            ),
        }),
    ));
}

fn start_tool(app: &mut AppState, seq: u64, request_id: &str, id: &str, tool_id: &str) {
    let args_summary = match tool_id {
        "background_output" => r#"{"task_id":"bg_1","block":true}"#,
        _ => r#"{"description":"delegated child"}"#,
    };
    app.ingest_event(envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: id.into(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: format!("digest-{id}"),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        seq + 1,
        request_id,
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: id.into(),
        }),
    ));
}

#[test]
fn demotion_selects_only_exact_parent_tool_lineage() {
    // arrange
    // Given: a visible task wait with one exact child and a newer unrelated child.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(provider_started(1, "req_parent", "default", "model-1"));
    start_tool(&mut app, 2, "req_parent", "tool_exact", "task");
    schedule_task(
        &mut app,
        4,
        "req_exact",
        "task_exact",
        "provider_model:default:model-1",
        Some(("tool_exact", "req_exact")),
        EventActor::new(ActorKind::Worker, Some("child_exact".to_string())),
    );
    app.ingest_event(child_agent_spawned(
        5,
        "child_unrelated",
        "worker",
        "parent",
    ));
    schedule_task(
        &mut app,
        6,
        "req_unrelated",
        "task_unrelated",
        "provider_model:default:model-1",
        Some(("tool_unrelated", "req_unrelated")),
        EventActor::new(ActorKind::Worker, Some("child_unrelated".to_string())),
    );

    // act
    // When/Then: recency and child ownership cannot override exact tool-call lineage.
    // assert
    assert_eq!(
        app.live_turn_demote_handle_id(),
        Some("req_exact".to_string())
    );
}

#[test]
fn parked_status_has_no_invisible_control_hit_rectangles() {
    // arrange
    // Given: a parked parent whose history also contains a running foreground child wait.
    let mut app = AppState::new_live(None, false, None);
    schedule_task(
        &mut app,
        1,
        "req_parent",
        "task_parent",
        "provider_model:default:model-1",
        None,
        EventActor::new(ActorKind::System, Some("parent".to_string())),
    );
    app.ingest_event(provider_started(2, "req_parent", "default", "model-1"));
    start_tool(&mut app, 3, "req_parent", "tool_child", "task");
    schedule_task(
        &mut app,
        5,
        "req_child",
        "task_child",
        "provider_model:default:model-1",
        Some(("tool_child", "req_child")),
        EventActor::new(ActorKind::Worker, Some("child".to_string())),
    );
    start_tool(
        &mut app,
        6,
        "req_parent",
        "tool_background",
        "background_output",
    );
    app.queued_prompt_count = 1;

    // When: geometry is derived for the parked presentation.
    let controls = (
        ui::live_turn_stop_rect(&app, TEST_FRAME_AREA),
        ui::live_turn_background_rect(&app, TEST_FRAME_AREA),
    );

    // act
    // Then: neither non-rendered control owns clickable terminal cells.
    // assert
    assert_eq!(controls, (None, None));
}

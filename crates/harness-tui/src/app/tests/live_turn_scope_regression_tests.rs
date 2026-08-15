use super::super::*;

fn schedule_turn(app: &mut AppState, seq: u64, request_id: &str, task_id: &str, actor: EventActor) {
    app.ingest_event(envelope_with_actor(
        seq,
        request_id,
        actor,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: task_id.into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
            metadata: None,
        }),
    ));
}

fn start_background_wait(app: &mut AppState, seq: u64, request_id: &str) {
    app.ingest_event(envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: format!("tool_wait_{seq}").into(),
            tool_id: "background_output".to_string(),
            args_summary: r#"{"task_id":"bg_1","block":true}"#.to_string(),
            args_digest: format!("digest-wait-{seq}"),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        seq + 1,
        request_id,
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: format!("tool_wait_{seq}").into(),
        }),
    ));
}

#[test]
fn hidden_child_turn_does_not_expose_parent_stop_control() {
    // Given: only a delegated child turn is active while the parent session is visible.
    let mut app = AppState::new_live(None, false, None);
    app.session_path = Some(PathBuf::from("/tmp/parent"));
    app.ingest_event(child_agent_spawned(1, "child", "worker", "parent"));
    let child = EventActor::new(ActorKind::Worker, Some("child".to_string()));
    schedule_turn(&mut app, 2, "req_child", "task_child", child.clone());
    let mut started = provider_started(3, "req_child", "default", "model-1");
    started.actor = child;
    app.ingest_event(started);

    // When: parent-view foreground authority is queried.
    let stop_rect = ui::live_turn_stop_rect(&app, TEST_FRAME_AREA);

    // Then: the hidden child cannot appear as the parent's stoppable foreground turn.
    assert!(!app.live_turn_stop_available());
    assert_eq!(stop_rect, None);
}

#[test]
fn hidden_child_wait_does_not_enable_parent_send_now() {
    // Given: the visible parent is responding while a hidden child enters a sendable wait.
    let mut app = AppState::new_live(None, false, None);
    app.session_path = Some(PathBuf::from("/tmp/parent"));
    schedule_turn(
        &mut app,
        1,
        "req_parent",
        "task_parent",
        EventActor::new(ActorKind::System, Some("parent".to_string())),
    );
    app.ingest_event(provider_started(2, "req_parent", "default", "model-1"));
    app.ingest_event(child_agent_spawned(3, "child", "worker", "parent"));
    let child = EventActor::new(ActorKind::Worker, Some("child".to_string()));
    schedule_turn(&mut app, 4, "req_child", "task_child", child.clone());
    let mut child_started = provider_started(5, "req_child", "default", "model-1");
    child_started.actor = child;
    app.ingest_event(child_started);
    start_background_wait(&mut app, 6, "req_child");
    app.queued_prompt_count = 1;

    // When/Then: hidden child activity cannot make send-now actionable in the parent view.
    assert!(!app.queued_prompt_send_now_available());
}

#[test]
fn send_now_interrupts_only_the_visible_parent_turn() {
    // Given: a parked visible parent and an independently active hidden child turn.
    let intents = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&intents);
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/parent")),
        false,
        Some(Arc::new(move |intent| {
            sink.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(intent);
        })),
    );
    schedule_turn(
        &mut app,
        1,
        "req_parent",
        "task_parent",
        EventActor::new(ActorKind::System, Some("parent".to_string())),
    );
    app.ingest_event(provider_started(2, "req_parent", "default", "model-1"));
    start_background_wait(&mut app, 3, "req_parent");
    app.ingest_event(child_agent_spawned(5, "child", "worker", "parent"));
    schedule_turn(
        &mut app,
        6,
        "req_child",
        "task_child",
        EventActor::new(ActorKind::Worker, Some("child".to_string())),
    );
    app.queued_prompt_count = 1;

    // When: the queued prompt is promoted from the parent view.
    assert!(app.send_queued_prompt_now());

    // Then: only the task matching the visible parked parent is interrupted.
    assert_eq!(
        intents
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_slice(),
        &[UiIntent::InterruptSession {
            task_ids: vec!["task_parent".to_string()],
            reason: InterruptReason::SendNow,
        }]
    );
}

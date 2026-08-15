use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ExecutionTimingMetadata,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    TaskCancelledEvent, TaskCompletedEvent, TaskCompletionMetadata, TaskScheduleState,
    TaskScheduledEvent, TaskTerminalScope, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::AppState;
use harness_tui::layout::FrameLayoutPlan;
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

const WIDTH: u16 = 100;
const HEIGHT: u16 = 30;
const REQUEST_ID: &str = "req_settled_metadata";

fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-settled-metadata-{seq:04}"),
        seq,
        run_id: "run_settled_metadata".into(),
        mono_ms: seq.saturating_mul(1_000),
        ts: Some("2026-08-14T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("settled-metadata-test".to_string())),
        correlation_id: Some(REQUEST_ID.to_string()),
        causation_id: None,
        stream_key: Some("run:run_settled_metadata".to_string()),
        payload,
    }
}

fn streaming_app() -> AppState {
    streaming_app_with_task_queue_key("provider_model:mock:model-settle")
}

fn streaming_app_with_task_queue_key(queue_key: &str) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    for event in [
        envelope(
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: REQUEST_ID.into(),
                text: "Settle metadata in place".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_settled_metadata".into(),
                state: TaskScheduleState::Started,
                queue_key: Some(queue_key.to_string()),
                metadata: None,
            }),
        ),
        envelope(
            3,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: REQUEST_ID.into(),
                provider_id: "mock".to_string(),
                model_id: "model-settle".to_string(),
                prompt_summary: "Settle metadata in place".to_string(),
                request_digest: "digest-settled-metadata".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: REQUEST_ID.into(),
                delta: "Stable answer row".to_string(),
            }),
        ),
    ] {
        app.ingest_event(event);
    }
    app
}

fn finish(app: &mut AppState) {
    app.ingest_event(envelope(
        5,
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: REQUEST_ID.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-settled-provider".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        6,
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_settled_metadata".into(),
            result_summary: "Stable answer row".to_string(),
            result_digest: "digest-settled-task".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: None,
                task_scope: Some(TaskTerminalScope::AgentTurn),
                timing: Some(ExecutionTimingMetadata {
                    started_mono_ms: Some(1_000),
                    finished_mono_ms: Some(6_000),
                    elapsed_ms: Some(5_000),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, WIDTH, HEIGHT), |app, frame, _area| {
        ui::render_app(frame, app);
    })
}

#[test]
fn baseline_complete_settles_with_the_reference_worked_for_marker() {
    // Given: a streamed assistant answer with deterministic event timing.
    let mut app = streaming_app();

    // When: both provider and turn-task completion barriers arrive.
    finish(&mut app);
    let screen = render(&app);

    // Then: completion releases the status row and settles the reference duration marker.
    assert!(
        FrameLayoutPlan::for_app(&app, Rect::new(0, 0, WIDTH, HEIGHT))
            .status
            .is_none()
    );
    assert!(screen.contains("Stable answer row"), "screen: {screen}");
    assert!(screen.contains("Worked for 5.0s"), "screen: {screen}");
}

#[test]
fn active_to_idle_status_transition_preserves_the_assistant_row_anchor() {
    // Given: a visible assistant row while the one-row status surface is active.
    let mut app = streaming_app();
    let active_screen = render(&app);
    let active_row = active_screen
        .lines()
        .position(|line| line.contains("Stable answer row"))
        .expect("active assistant row");

    // When: completion changes the status allocation from one row to zero rows.
    finish(&mut app);
    let settled_screen = render(&app);
    let settled_row = settled_screen
        .lines()
        .position(|line| line.contains("Stable answer row"))
        .expect("settled assistant row");

    // Then: the semantic transcript row remains anchored at the same terminal row.
    assert_eq!(active_row, settled_row);
}

#[test]
fn user_cancellation_settles_with_the_reference_duration_marker() {
    let mut app = streaming_app();
    app.ingest_event(envelope(
        5,
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_settled_metadata".into(),
            reason: "interrupted".to_string(),
            task_scope: Some(TaskTerminalScope::AgentTurn),
        }),
    ));

    let screen = render(&app);

    assert!(
        screen.contains("Turn cancelled by user in 4.0s."),
        "screen: {screen}"
    );
    assert!(!screen.contains("Worked for"), "screen: {screen}");
}

#[test]
fn send_now_cancellation_is_silent_and_releases_the_live_row() {
    let mut app = streaming_app();
    app.ingest_event(envelope(
        5,
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_settled_metadata".into(),
            reason: "send_now".to_string(),
            task_scope: Some(TaskTerminalScope::AgentTurn),
        }),
    ));

    let screen = render(&app);

    assert!(
        !screen.contains("Turn cancelled by user"),
        "screen: {screen}"
    );
    assert!(!screen.contains("Worked for"), "screen: {screen}");
    assert!(
        FrameLayoutPlan::for_app(&app, Rect::new(0, 0, WIDTH, HEIGHT))
            .status
            .is_none()
    );
}

#[test]
fn explicit_agent_turn_completion_scope_overrides_scheduler_row_inference() {
    // Given: stale scheduler metadata classifies the outer turn task like a child task.
    let mut app = streaming_app_with_task_queue_key("agent:child");

    // When: the authoritative terminal event explicitly identifies the task as an agent turn.
    finish(&mut app);
    let screen = render(&app);

    // Then: the turn settles with the authoritative completion marker.
    assert!(screen.contains("Worked for 5.0s"), "screen: {screen}");
}

#[test]
fn explicit_agent_turn_cancellation_scope_overrides_scheduler_row_inference() {
    // Given: stale scheduler metadata classifies the outer turn task like a child task.
    let mut app = streaming_app_with_task_queue_key("agent:child");

    // When: the authoritative cancellation explicitly identifies the task as an agent turn.
    app.ingest_event(envelope(
        5,
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_settled_metadata".into(),
            reason: "interrupted".to_string(),
            task_scope: Some(TaskTerminalScope::AgentTurn),
        }),
    ));
    let screen = render(&app);

    // Then: cancellation settles with the authoritative user-cancellation marker.
    assert!(
        screen.contains("Turn cancelled by user in 4.0s."),
        "screen: {screen}"
    );
}

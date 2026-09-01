use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RuntimeEvent, TaskScheduleState,
    TaskScheduledEvent, ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStatus,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::UnwrapOrAbort;
use harness_tui::{
    live_update_channel, run_tui_with_options, LiveUpdate, TuiMode, TuiOptions, UiIntent,
};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub(crate) const SCENARIO_ENV: &str = "HARNESS_TUI_P0_02_SCENARIO";
pub(crate) const APPEND_STATUS: &str = "P0-02 append applied";
pub(crate) const APPENDED_STREAM_TEXT: &str =
    "P0-02 detached streaming append must remain below the viewport.";
pub(crate) const HELPER_CONTRACT: &str = "P0-02 helper command: HARNESS_TUI_P0_02_SCENARIO=1 HARNESS_DETERMINISTIC=1 HARNESS_DISABLE_ANIMATIONS=1 HARNESS_SEED=42 cargo test -p harness-tui --test p0_02_pty_recorded -- --exact p0_02_pty_helper --nocapture; detach, then input Ctrl+C; exit with the command palette.";

pub(crate) fn run_helper() {
    if std::env::var(SCENARIO_ENV).as_deref() != Ok("1") {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let events = scenario_events();
    let append_seq = u64::try_from(events.len())
        .unwrap_or_abort()
        .saturating_add(1);
    let (update_tx, update_rx) = live_update_channel();
    let append_tx = update_tx.clone();
    let appended = Arc::new(AtomicBool::new(false));
    let on_ui_intent: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(move |intent| {
        if !matches!(intent, UiIntent::InterruptSession { .. })
            || appended.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let request_id = "req_p0_02_append";
        let appended_events = [
            provider_started(request_id, "P0-02 detached append"),
            provider_delta(request_id, APPENDED_STREAM_TEXT),
        ];
        for (offset, mut event) in appended_events.into_iter().enumerate() {
            let seq = append_seq.saturating_add(u64::try_from(offset).unwrap_or_abort());
            resequence(&mut event, seq);
            append_tx
                .send(LiveUpdate::Event(Box::new(RuntimeEvent::Durable(
                    Box::new(event),
                ))))
                .unwrap_or_abort();
        }
        append_tx
            .send(LiveUpdate::Status(APPEND_STATUS.to_string()))
            .unwrap_or_abort();
        let mut stdout = std::io::stdout().lock();
        write!(stdout, "\x1b]2;{APPEND_STATUS}\x07").unwrap_or_abort();
        stdout.flush().unwrap_or_abort();
    });

    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: events,
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: Some(on_ui_intent),
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
    drop(update_tx);
    println!("{HELPER_CONTRACT}");
}

fn scenario_events() -> Vec<EventEnvelopeV1> {
    let mut events = Vec::new();
    for turn in 1..=3 {
        let request_id = format!("req_p0_02_final_{turn}");
        events.extend(completed_turn(&request_id, turn));
    }
    events.extend(active_turn());
    for (index, event) in events.iter_mut().enumerate() {
        let seq = u64::try_from(index).unwrap_or_abort().saturating_add(1);
        resequence(event, seq);
    }
    events
}

fn completed_turn(request_id: &str, turn: usize) -> Vec<EventEnvelopeV1> {
    let response = match turn {
        1 => "P0-02 completed response one carries a deliberately long reflow sentence across terminal widths while preserving stable response identity and command disclosure state.",
        2 => "P0-02 completed response two confirms deterministic response navigation.",
        _ => "P0-02 completed response three confirms final-response clamping.",
    };
    let prompt = format!("P0-02 deterministic prompt {turn}");
    let mut events = vec![
        envelope(
            0,
            request_id,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: prompt.clone(),
            }),
        ),
        provider_started(request_id, &prompt),
        provider_delta(request_id, response),
    ];
    if turn == 1 {
        for command in 1..=14 {
            let tool_call_id = format!("tool_p0_02_command_{command:02}");
            events.push(envelope(
                0,
                request_id,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: tool_call_id.clone().into(),
                    tool_id: "bash".to_string(),
                    args_summary: format!(r#"{{"command":"printf command-{command:02}"}}"#),
                    args_digest: format!("digest-p0-02-command-{command:02}"),
                    metadata: None,
                }),
            ));
            events.push(envelope(
                0,
                request_id,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: tool_call_id.into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some(format!("command-{command:02} complete")),
                    output_digest: Some(format!("digest-p0-02-output-{command:02}")),
                    output_json: None,
                    metadata: None,
                }),
            ));
        }
    }
    events.push(provider_finished(request_id, "stop"));
    events
}

fn active_turn() -> Vec<EventEnvelopeV1> {
    let request_id = "req_p0_02_active";
    vec![
        envelope(
            0,
            request_id,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "P0-02 active streaming prompt".to_string(),
            }),
        ),
        envelope(
            0,
            request_id,
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_p0_02_active".into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:p0-02-model".to_string()),
                metadata: None,
            }),
        ),
        provider_started(request_id, "P0-02 active streaming prompt"),
        provider_delta(request_id, "P0-02 active streaming block remains open."),
        envelope(
            0,
            request_id,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool_p0_02_failed_active".into(),
                tool_id: "bash".to_string(),
                args_summary: r#"{"command":"false"}"#.to_string(),
                args_digest: "digest-p0-02-failed-active".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            0,
            request_id,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool_p0_02_failed_active".into(),
                status: ToolCallStatus::Failed,
                output_summary: Some(
                    "exit code: 1\nstderr: deterministic active failure".to_string(),
                ),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ),
        provider_finished(request_id, "error"),
    ]
}

fn provider_started(request_id: &str, prompt: &str) -> EventEnvelopeV1 {
    envelope(
        0,
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "p0-02-model".to_string(),
            prompt_summary: prompt.to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    )
}

fn provider_delta(request_id: &str, delta: &str) -> EventEnvelopeV1 {
    envelope(
        0,
        request_id,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: delta.to_string(),
        }),
    )
}

fn provider_finished(request_id: &str, reason: &str) -> EventEnvelopeV1 {
    envelope(
        0,
        request_id,
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: reason.to_string(),
            output_digest: Some(format!("digest-{request_id}-output")),
            usage: None,
            metadata: None,
        }),
    )
}

fn resequence(event: &mut EventEnvelopeV1, seq: u64) {
    event.seq = seq;
    event.mono_ms = seq.saturating_mul(100);
    event.event_id = format!("evt-p0-02-{seq:04}");
}

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-p0-02-{seq:04}"),
        seq,
        run_id: "run_p0_02".into(),
        mono_ms: seq.saturating_mul(100),
        ts: Some("2026-08-31T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("p0-02-pty".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some("run:run_p0_02".to_string()),
        payload,
    }
}

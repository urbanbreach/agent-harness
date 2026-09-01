use std::time::{Duration, Instant};

use crossbeam_channel::bounded;
use crossterm::event::Event;
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    ProviderStreamDeltaEvent, TaskScheduleState, TaskScheduledEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::app::{AppState, RuntimeStateKind};
use harness_tui::event::TuiEvent;
use harness_tui::input::{TerminalEnvelope, TerminalReaderStatus, TerminalSequence};
use harness_tui::runtime_wait_set::{RuntimeWaitSet, RuntimeWake};
use harness_tui::{live_update_channel, run_tui_with_options, LiveUpdate, TuiMode, TuiOptions};

#[test]
fn terminal_and_live_sources_wake_parked_wait() {
    // arrange
    // act
    let (_frame_tx, frame_rx) = bounded(1);
    let (_status_tx, status_rx) = bounded::<TerminalReaderStatus>(1);
    let (terminal_tx, terminal_rx) = bounded(1);
    let (live_tx, live_rx) = live_update_channel();
    terminal_tx
        .send(TerminalEnvelope::new(
            TerminalSequence::new(1),
            Instant::now(),
            TuiEvent::FocusGained,
        ))
        .expect("terminal send");
    let wait = RuntimeWaitSet {
        frame: &frame_rx,
        reader: &status_rx,
        terminal: &terminal_rx,
        live: Some(live_rx.receiver()),
    };
    // assert
    assert!(matches!(wait.wait(None), RuntimeWake::Terminal(_)));
    live_tx
        .send(LiveUpdate::Status("ready".to_string()))
        .expect("live send");
    assert!(matches!(wait.wait(None), RuntimeWake::Live(_)));
}

#[test]
fn deadline_wakes_without_periodic_polling() {
    // arrange
    // act
    let (_frame_tx, frame_rx) = bounded(1);
    let (_status_tx, status_rx) = bounded::<TerminalReaderStatus>(1);
    let (_terminal_tx, terminal_rx) = bounded(1);
    let (_live_tx, live_rx) = live_update_channel();
    let wait = RuntimeWaitSet {
        frame: &frame_rx,
        reader: &status_rx,
        terminal: &terminal_rx,
        live: Some(live_rx.receiver()),
    };
    // assert
    assert!(matches!(
        wait.wait(Some(Instant::now() + Duration::from_millis(1))),
        RuntimeWake::Deadline
    ));
}

#[test]
fn empty_live_wake_uses_terminal_disconnect_transition_once() {
    // Given: a live shell parked on an empty update source that has closed.
    let (_frame_tx, frame_rx) = bounded(1);
    let (_status_tx, status_rx) = bounded::<TerminalReaderStatus>(1);
    let (_terminal_tx, terminal_rx) = bounded(1);
    let (live_tx, live_rx) = live_update_channel();
    drop(live_tx);
    let mut app = AppState::new_live(None, false, None);
    let mut live_updates = Some(live_rx);
    let wait = RuntimeWaitSet {
        frame: &frame_rx,
        reader: &status_rx,
        terminal: &terminal_rx,
        live: live_updates.as_ref().map(|receiver| receiver.receiver()),
    };

    // When: the parked wait reports closure and the runtime clears its receiver.
    let wake = wait.wait(None);
    assert!(matches!(wake, RuntimeWake::LiveDisconnected));
    if matches!(wake, RuntimeWake::LiveDisconnected) {
        assert!(app.apply_runtime_event_stream_closed());
        live_updates = None;
    }

    // Then: clearing the receiver first applies the idempotent terminal transition exactly once.
    assert!(live_updates.is_none());
    assert!(!app.apply_runtime_event_stream_closed());
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Disconnected);
    assert_eq!(
        app.status_banner.as_deref(),
        Some("live event stream disconnected")
    );
}

#[test]
fn p0_05_disconnect_pty_helper() {
    // Given: direct invocation opts into a real closed live-update source.
    let Ok(mode) = std::env::var("HARNESS_TUI_P0_05_SCENARIO") else {
        return;
    };
    assert!(matches!(mode.as_str(), "truth" | "duplicate"));
    let run_dir = tempfile::tempdir().expect("disconnect helper run dir");
    harness_tui::app::set_pending_live_prompt_draft(Some("disconnect draft preserved".to_string()));
    let (live_tx, live_rx) = live_update_channel();
    if mode == "duplicate" {
        live_tx
            .send(LiveUpdate::Status(
                "live event stream disconnected".to_string(),
            ))
            .expect("duplicate disconnect status");
    }
    drop(live_tx);

    // When: the shipped TUI consumes the source through its native runtime wait set.
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: disconnect_fixture_events(),
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx: live_rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })
    .expect("disconnect helper TUI");
    // Then: the browser driver owns visible-state assertions and clean exit.
}

fn disconnect_fixture_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_p0_05_disconnect";
    [
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "P0-05 preserve this transcript".to_string(),
        }),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_p0_05_disconnect".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:mock:p0-05-model".to_string()),
            metadata: None,
        }),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "p0-05-model".to_string(),
            prompt_summary: "P0-05 preserve this transcript".to_string(),
            request_digest: "digest-p0-05-disconnect".to_string(),
            metadata: None,
        }),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: "P0-05 transcript remains visible".to_string(),
        }),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, payload)| {
        let seq = u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1);
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-p0-05-{seq:04}"),
            seq,
            run_id: "run_p0_05_disconnect".into(),
            mono_ms: seq.saturating_mul(1_000),
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("p0-05-helper".to_string())),
            correlation_id: Some(request_id.to_string()),
            causation_id: None,
            stream_key: Some("run:run_p0_05_disconnect".to_string()),
            payload,
        }
    })
    .collect()
}

#[test]
fn live_disconnect_is_reported_once() {
    // arrange
    let (_frame_tx, frame_rx) = bounded(1);
    let (_status_tx, status_rx) = bounded::<TerminalReaderStatus>(1);
    let (terminal_tx, terminal_rx) = bounded(1);
    let (live_tx, live_rx) = live_update_channel();
    drop(live_tx);
    let with_live = RuntimeWaitSet {
        frame: &frame_rx,
        reader: &status_rx,
        terminal: &terminal_rx,
        live: Some(live_rx.receiver()),
    };
    assert!(matches!(
        with_live.wait(None),
        RuntimeWake::LiveDisconnected
    ));

    // act
    terminal_tx
        .send(TerminalEnvelope::new(
            TerminalSequence::new(1),
            Instant::now(),
            TuiEvent::FocusLost,
        ))
        .expect("terminal send");
    let without_live = RuntimeWaitSet {
        frame: &frame_rx,
        reader: &status_rx,
        terminal: &terminal_rx,
        live: None,
    };
    // assert
    assert!(matches!(without_live.wait(None), RuntimeWake::Terminal(_)));
}

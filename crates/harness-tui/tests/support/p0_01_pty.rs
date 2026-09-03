use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::UnwrapOrAbort;
use harness_tui::{run_tui_with_options, TuiMode, TuiOptions};

pub(crate) const SCENARIO_ENV: &str = "HARNESS_TUI_P0_01_SCENARIO";
pub(crate) const TAIL_MARKER: &str = "P0-01-TRANSCRIPT-TAIL";
pub(crate) const MIDDLE_MARKER: &str = "P0-01-MIDDLE-ROW-30";
pub(crate) const HELPER_CONTRACT: &str = "P0-01 helper command: HARNESS_TUI_P0_01_SCENARIO=1 HARNESS_DETERMINISTIC=1 HARNESS_DISABLE_ANIMATIONS=1 HARNESS_SEED=42 cargo test -p harness-tui --test p0_01_pty_recorded -- --exact p0_01_pty_helper --nocapture; wait for the tail marker, scroll up to detach, open the dashboard with Ctrl+x s, resize, close with Escape, and assert the detached anchor returns.";

const BLOCK_COUNT: usize = 48;

pub(crate) fn run_helper() {
    if std::env::var(SCENARIO_ENV).as_deref() != Ok("1") {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = harness_tui::live_update_channel();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: initial_events(),
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
    drop(update_tx);
    println!("{HELPER_CONTRACT}");
}

fn initial_events() -> Vec<EventEnvelopeV1> {
    let mut events = Vec::new();
    let mut seq = 1u64;
    let mut mono = 100u64;

    for index in 0..BLOCK_COUNT {
        let request_id = format!("req_p0_01_{index:02}");
        let text = if index == 30 {
            MIDDLE_MARKER.to_string()
        } else if index == BLOCK_COUNT - 1 {
            format!("{TAIL_MARKER} {index}")
        } else {
            format!("p0-01 filler row {index:02} Latin/CJK 東京 line for the transcript")
        };

        events.push(envelope(
            &mut seq,
            &mut mono,
            &request_id,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.clone().into(),
                text: "go".to_string(),
            }),
        ));
        events.push(envelope(
            &mut seq,
            &mut mono,
            &request_id,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.clone().into(),
                provider_id: "mock".to_string(),
                model_id: "model-p0-01".to_string(),
                prompt_summary: text.chars().take(24).collect(),
                request_digest: format!("digest-p0-01-{index:02}"),
                metadata: None,
            }),
        ));
        events.push(envelope(
            &mut seq,
            &mut mono,
            &request_id,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.clone().into(),
                delta: text,
            }),
        ));
        events.push(envelope(
            &mut seq,
            &mut mono,
            &request_id,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.clone().into(),
                finish_reason: "stop".to_string(),
                output_digest: None,
                usage: None,
                metadata: None,
            }),
        ));
    }
    events
}

fn envelope(
    seq: &mut u64,
    mono_ms: &mut u64,
    request_id: &str,
    payload: EventV1,
) -> EventEnvelopeV1 {
    let current_seq = *seq;
    *seq += 1;
    *mono_ms += 10;
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-p0-01-{current_seq:04}"),
        seq: current_seq,
        run_id: "run_p0_01".into(),
        mono_ms: *mono_ms,
        ts: Some("2026-09-03T09:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("p0-01-pty".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some("run:run_p0_01".to_string()),
        payload,
    }
}

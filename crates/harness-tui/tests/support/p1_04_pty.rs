use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::{run_tui_with_options, TuiMode, TuiOptions, UnwrapOrAbort};

pub(crate) const SCENARIO_ENV: &str = "HARNESS_TUI_P1_04_SCENARIO";
pub(crate) const READY_MARKER: &str = "P1-04 responsive ready 中文 emoji 🧭";
pub(crate) const HELPER_TEST: &str = "p1_04_pty_helper";

pub(crate) fn run_helper() {
    if std::env::var(SCENARIO_ENV).as_deref() != Ok("1") {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = harness_tui::live_update_channel();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: fixture_events(),
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
}

fn fixture_events() -> Vec<EventEnvelopeV1> {
    let mut events = Vec::new();
    for turn in 1_u64..=12 {
        let request_id = format!("req_p1_04_{turn:02}");
        let prompt = format!("Harness responsive prompt {turn:02}");
        events.push(envelope(
            &request_id,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.clone().into(),
                text: prompt.clone(),
            }),
        ));
        events.push(envelope(
            &request_id,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.clone().into(),
                provider_id: "mock".to_string(),
                model_id: "harness-responsive".to_string(),
                prompt_summary: prompt,
                request_digest: format!("digest-p1-04-{turn:02}"),
                metadata: None,
            }),
        ));
        events.push(envelope(
            &request_id,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.clone().into(),
                delta: format!(
                    "Harness response {turn:02}: stable resize anchor with Unicode 中文 and terminal-safe status feedback."
                ),
            }),
        ));
        events.push(envelope(
            &request_id,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.clone().into(),
                finish_reason: "stop".to_string(),
                output_digest: Some(format!("digest-p1-04-output-{turn:02}")),
                usage: None,
                metadata: None,
            }),
        ));
    }

    let request_id = "req_p1_04_active";
    events.push(envelope(
        request_id,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "Harness active responsive prompt".to_string(),
        }),
    ));
    events.push(envelope(
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "harness-responsive".to_string(),
            prompt_summary: "Harness active responsive prompt".to_string(),
            request_digest: "digest-p1-04-active".to_string(),
            metadata: None,
        }),
    ));
    events.push(envelope(
        request_id,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: READY_MARKER.to_string(),
        }),
    ));

    for (index, event) in events.iter_mut().enumerate() {
        let seq = u64::try_from(index).unwrap_or_abort().saturating_add(1);
        event.seq = seq;
        event.mono_ms = seq.saturating_mul(100);
        event.event_id = format!("evt-p1-04-{seq:04}");
    }
    events
}

fn envelope(request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: String::new(),
        seq: 0,
        run_id: "run_p1_04".into(),
        mono_ms: 0,
        ts: Some("2026-09-02T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("p1-04-native-pty".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some("run:run_p1_04".to_string()),
        payload,
    }
}

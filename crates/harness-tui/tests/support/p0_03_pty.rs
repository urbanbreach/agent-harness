use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RuntimeEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::UnwrapOrAbort;
use harness_tui::{
    live_update_channel, run_tui_with_options, LiveUpdate, TuiMode, TuiOptions, UiIntent,
};
use std::io::Write;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

pub(crate) const SCENARIO_ENV: &str = "HARNESS_TUI_P0_03_SCENARIO";
pub(crate) const READY_MARKER: &str = "QA-P0-03-READY";
pub(crate) const FENCE_CHUNK_1_MARKER: &str = "QA-P0-03-FENCE-CHUNK-1";
pub(crate) const FENCE_CHUNK_2_MARKER: &str = "QA-P0-03-FENCE-CHUNK-2";
pub(crate) const SETTLED_MARKER: &str = "QA-P0-03-SETTLED";
pub(crate) const CHUNK_1_COMMAND: &str = "p0-03 fence chunk 1";
pub(crate) const CHUNK_2_COMMAND: &str = "p0-03 fence chunk 2";
pub(crate) const HELPER_CONTRACT: &str = "P0-03 helper command: HARNESS_TUI_P0_03_SCENARIO=1 HARNESS_DETERMINISTIC=1 HARNESS_DISABLE_ANIMATIONS=1 HARNESS_SEED=42 cargo test -p harness-tui --test p0_03_pty_recorded -- --exact p0_03_pty_helper --nocapture; wait QA-P0-03-READY; type p0-03 fence chunk 1 and press Enter; wait QA-P0-03-FENCE-CHUNK-1; type p0-03 fence chunk 2 and press Enter; wait QA-P0-03-FENCE-CHUNK-2 and QA-P0-03-SETTLED; open Commands with Ctrl+P, choose Exit the app, and press Enter.";

const INITIAL_FIXTURE_TEXT: &str = "P0-03 boxed markdown fixture\n\n| Signal | Payload |\n| --- | --- |\n| **nested _emphasis_** | 東京 👩‍💻 |\n| [valid](https://example.com/p0-03) | [unsafe](javascript:alert(1)) |\n\nCJK: 東京 · ZWJ: 👩‍💻\nQA-P0-03-READY\n```rust\nfn p0_03_fixture() {\n";
const FENCE_CHUNK_1: &str =
    "    let stage = \"chunk one\";\n    let city = \"東京\";\nQA-P0-03-FENCE-CHUNK-1\n";
const FENCE_CHUNK_2: &str = "    let operator = \"👩‍💻\";\n    println!(\"{stage} {city} {operator}\");\n}\n```\nQA-P0-03-FENCE-CHUNK-2\nQA-P0-03-SETTLED";

pub(crate) fn assert_fixture_contract() {
    assert!(INITIAL_FIXTURE_TEXT.contains("| Signal | Payload |"));
    assert!(INITIAL_FIXTURE_TEXT.contains("**nested _emphasis_**"));
    assert!(INITIAL_FIXTURE_TEXT.contains("東京"));
    assert!(INITIAL_FIXTURE_TEXT.contains("👩‍💻"));
    assert!(INITIAL_FIXTURE_TEXT.contains("CJK: 東京 · ZWJ: 👩‍💻"));
    assert_eq!(
        INITIAL_FIXTURE_TEXT
            .matches("https://example.com/p0-03")
            .count(),
        1
    );
    assert_eq!(INITIAL_FIXTURE_TEXT.matches("javascript:").count(), 1);
    assert!(INITIAL_FIXTURE_TEXT.contains(READY_MARKER));
    assert!(!FENCE_CHUNK_1.contains("```"));
    assert!(FENCE_CHUNK_1.contains(FENCE_CHUNK_1_MARKER));
    assert!(FENCE_CHUNK_2.contains("```"));
    assert!(FENCE_CHUNK_2.contains(FENCE_CHUNK_2_MARKER));
    assert!(FENCE_CHUNK_2.contains(SETTLED_MARKER));
}

pub(crate) fn run_helper() {
    if std::env::var(SCENARIO_ENV).as_deref() != Ok("1") {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let events = initial_events();
    let next_seq = Arc::new(AtomicU64::new(
        u64::try_from(events.len())
            .unwrap_or_abort()
            .saturating_add(1),
    ));
    let (update_tx, update_rx) = live_update_channel();
    let append_tx = update_tx.clone();
    let stage = Arc::new(AtomicU8::new(0));
    let on_ui_intent: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let stage = Arc::clone(&stage);
        let next_seq = Arc::clone(&next_seq);
        Arc::new(move |intent| {
            let UiIntent::SubmitPrompt { text, .. } = intent else {
                return;
            };
            match (stage.load(Ordering::Acquire), text.as_str()) {
                (0, CHUNK_1_COMMAND) => {
                    if stage
                        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        send_delta(&append_tx, &next_seq, FENCE_CHUNK_1);
                    }
                }
                (1, CHUNK_2_COMMAND) => {
                    if stage
                        .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        send_delta(&append_tx, &next_seq, FENCE_CHUNK_2);
                        send_finished(&append_tx, &next_seq);
                        append_tx
                            .send(LiveUpdate::Status(SETTLED_MARKER.to_string()))
                            .unwrap_or_abort();
                    }
                }
                _ => {}
            }
        })
    };

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

fn initial_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_p0_03_fixture";
    let mut events = vec![
        envelope(
            1,
            request_id,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "P0-03 deterministic recorded fixture".to_string(),
            }),
        ),
        envelope(
            2,
            request_id,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "p0-03-model".to_string(),
                prompt_summary: "P0-03 deterministic recorded fixture".to_string(),
                request_digest: "digest-req-p0-03-fixture".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            request_id,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: INITIAL_FIXTURE_TEXT.to_string(),
            }),
        ),
    ];
    for (index, event) in events.iter_mut().enumerate() {
        resequence(
            event,
            u64::try_from(index).unwrap_or_abort().saturating_add(1),
        );
    }
    events
}

fn send_delta(tx: &harness_tui::LiveUpdateSender, next_seq: &AtomicU64, delta: &str) {
    let mut event = provider_delta("req_p0_03_fixture", delta);
    resequence(&mut event, next_seq.fetch_add(1, Ordering::Relaxed));
    tx.send(LiveUpdate::Event(Box::new(RuntimeEvent::Durable(
        Box::new(event),
    ))))
    .unwrap_or_abort();
}

fn send_finished(tx: &harness_tui::LiveUpdateSender, next_seq: &AtomicU64) {
    let mut event = provider_finished("req_p0_03_fixture");
    resequence(&mut event, next_seq.fetch_add(1, Ordering::Relaxed));
    tx.send(LiveUpdate::Event(Box::new(RuntimeEvent::Durable(
        Box::new(event),
    ))))
    .unwrap_or_abort();
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

fn provider_finished(request_id: &str) -> EventEnvelopeV1 {
    envelope(
        0,
        request_id,
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-req-p0-03-fixture-output".to_string()),
            usage: None,
            metadata: None,
        }),
    )
}

fn resequence(event: &mut EventEnvelopeV1, seq: u64) {
    event.seq = seq;
    event.mono_ms = seq.saturating_mul(100);
    event.event_id = format!("evt-p0-03-{seq:04}");
}

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-p0-03-{seq:04}"),
        seq,
        run_id: "run_p0_03".into(),
        mono_ms: seq.saturating_mul(100),
        ts: Some("2026-08-31T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("p0-03-pty".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some("run:run_p0_03".to_string()),
        payload,
    }
}

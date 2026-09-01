use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    ProviderStreamDeltaEvent, TaskScheduleState, TaskScheduledEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::UnwrapOrAbort;
use harness_tui::{run_tui_with_options, TuiMode, TuiOptions, UiIntent};
use std::io::Write;
use std::sync::Arc;

pub(crate) const SCENARIO_ENV: &str = "HARNESS_TUI_P0_04_SCENARIO";
pub(crate) const READY_MARKER: &str = "P0-04 active streaming";
pub(crate) const SUBMITTED_MARKER: &str = "P0-04 submitted";
pub(crate) const QUEUED_MARKER: &str = "P0-04 submitted queued";
pub(crate) const INTERJECT_MARKER: &str = "P0-04 interject submitted queued";
pub(crate) const REPLACE_INTERRUPT_MARKER: &str = "P0-04 replacement interrupted";
pub(crate) const REPLACE_MARKER: &str = "P0-04 replacement submitted queued";
pub(crate) const EMPTY_MARKER: &str = "P0-04 empty submitted";
pub(crate) const PHANTOM_MARKER: &str = "P0-04 phantom";
pub(crate) const HELPER_CONTRACT: &str = "P0-04 helper command: HARNESS_TUI_P0_04_SCENARIO=1 HARNESS_DETERMINISTIC=1 HARNESS_DISABLE_ANIMATIONS=1 HARNESS_SEED=42 cargo test -p harness-tui --test p0_04_pty_recorded -- --exact p0_04_pty_helper --nocapture; toggle multiline with Alt+M; type first, Enter, second, then send with Alt+S; interject with Alt+I; cancel and replace with Alt+R; enhanced terminals may also use the modified Enter bindings; exit with the command palette.";

const FIRST_DRAFT: &str = "first\nsecond";
const INTERJECT_DRAFT: &str = "interject draft";
const REPLACEMENT_DRAFT: &str = "replacement draft";

pub(crate) fn run_helper() {
    if std::env::var(SCENARIO_ENV).as_deref() != Ok("1") {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = harness_tui::live_update_channel();
    let on_ui_intent: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(move |intent| {
        let marker = match intent {
            UiIntent::InterruptSession { .. } => Some(REPLACE_INTERRUPT_MARKER),
            UiIntent::SubmitPrompt { ref text, .. } => match text.as_str() {
                FIRST_DRAFT => Some(QUEUED_MARKER),
                INTERJECT_DRAFT => Some(INTERJECT_MARKER),
                REPLACEMENT_DRAFT => Some(REPLACE_MARKER),
                _ => Some(SUBMITTED_MARKER),
            },
            _ => None,
        };
        if let Some(marker) = marker {
            let mut stdout = std::io::stdout().lock();
            write!(stdout, "\x1b]2;{marker}\x07").unwrap_or_abort();
            stdout.flush().unwrap_or_abort();
        }
    });

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
    let request_id = "req_p0_04_streaming";
    let mut events = vec![
        envelope(
            1,
            request_id,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "P0-04 deterministic active turn".to_string(),
            }),
        ),
        envelope(
            2,
            request_id,
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_p0_04_streaming".into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:p0-04-model".to_string()),
                metadata: None,
            }),
        ),
        envelope(
            3,
            request_id,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "p0-04-model".to_string(),
                prompt_summary: "P0-04 deterministic active turn".to_string(),
                request_digest: "digest-req-p0-04-streaming".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            request_id,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: format!("P0-04 active streaming response\n{READY_MARKER}"),
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

fn resequence(event: &mut EventEnvelopeV1, seq: u64) {
    event.seq = seq;
    event.mono_ms = seq.saturating_mul(100);
    event.event_id = format!("evt-p0-04-{seq:04}");
}

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-p0-04-{seq:04}"),
        seq,
        run_id: "run_p0_04".into(),
        mono_ms: seq.saturating_mul(100),
        ts: Some("2026-08-31T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("p0-04-pty".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some("run:run_p0_04".to_string()),
        payload,
    }
}

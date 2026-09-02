use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::UnwrapOrAbort;
use harness_tui::{run_tui_with_options, TuiMode, TuiOptions, UiIntent};
use std::io::Write;
use std::sync::Arc;

pub(crate) const SCENARIO_ENV: &str = "HARNESS_TUI_P1_01_SCENARIO";
pub(crate) const READY_MARKER: &str = "P1-01 slash ready";
pub(crate) const HELPER_CONTRACT: &str = "P1-01 helper command: HARNESS_TUI_P1_01_SCENARIO=1 HARNESS_DETERMINISTIC=1 HARNESS_DISABLE_ANIMATIONS=1 HARNESS_SEED=42 cargo test -p harness-tui --test p1_01_pty_recorded -- --exact p1_01_pty_helper --nocapture; type a slash token, use Tab to accept text, supply required arguments, use Enter to execute, and exit with Ctrl+Q.";

pub(crate) fn run_helper() {
    if std::env::var(SCENARIO_ENV).as_deref() != Ok("1") {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = harness_tui::live_update_channel();
    let on_ui_intent: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(move |intent| {
        if let UiIntent::UpdateSessionTitle { title } = intent {
            let mut stdout = std::io::stdout().lock();
            write!(stdout, "\x1b]2;P1-01 renamed:{title}\x07").unwrap_or_abort();
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
    vec![EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: "evt-p1-01-0001".to_string(),
        seq: 1,
        run_id: "run_p1_01".into(),
        mono_ms: 100,
        ts: Some("2026-09-01T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::User, Some("p1-01-pty".to_string())),
        correlation_id: Some("req_p1_01".to_string()),
        causation_id: None,
        stream_key: Some("run:run_p1_01".to_string()),
        payload: EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_p1_01".into(),
            text: READY_MARKER.to_string(),
        }),
    }]
}

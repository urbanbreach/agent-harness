use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;

use harness_core::event::EventEnvelopeV1;
use harness_tui::app::{SessionHistoryEntry, TogglesConfig};
use harness_tui::{LiveUpdate, TuiMode, TuiOptions};

use super::workflow::UiIntentSink;

#[expect(
    clippy::too_many_arguments,
    reason = "continue live TUI options mirror the live handoff state explicitly"
)]
pub(super) fn continue_live_tui_options(
    run_dir: PathBuf,
    historical_events: Vec<EventEnvelopeV1>,
    session_history_entries: Vec<SessionHistoryEntry>,
    update_rx: std_mpsc::Receiver<LiveUpdate>,
    exit_on_finish: bool,
    ui_intent_sender: UiIntentSink,
    compact_session_supported: bool,
    prompt_history_path: Option<PathBuf>,
    toggles: Option<TogglesConfig>,
) -> TuiOptions {
    TuiOptions {
        mode: TuiMode::Live {
            run_dir,
            historical_events,
            session_history_entries,
            prompt_history_path,
            update_rx,
            compact_session_supported,
        },
        exit_on_finish,
        on_ui_intent: Some(ui_intent_sender),
        keybindings: None,
        toggles,
        preserve_terminal_on_exit: true,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "new live TUI options mirror the live handoff state explicitly"
)]
pub(super) fn new_live_tui_options(
    run_dir: PathBuf,
    session_history_entries: Vec<SessionHistoryEntry>,
    update_rx: std_mpsc::Receiver<LiveUpdate>,
    exit_on_finish: bool,
    ui_intent_sender: UiIntentSink,
    compact_session_supported: bool,
    prompt_history_path: Option<PathBuf>,
    toggles: Option<TogglesConfig>,
) -> TuiOptions {
    TuiOptions {
        mode: TuiMode::Live {
            run_dir,
            historical_events: Vec::new(),
            session_history_entries,
            prompt_history_path,
            update_rx,
            compact_session_supported,
        },
        exit_on_finish,
        on_ui_intent: Some(ui_intent_sender),
        keybindings: None,
        toggles,
        preserve_terminal_on_exit: true,
    }
}

use harness_tui::{run_tui_with_options, TuiMode, TuiOptions, UnwrapOrAbort};

pub(crate) const SCENARIO_ENV: &str = "HARNESS_TUI_P1_03_SCENARIO";
pub(crate) const REDUCED_SCENARIO_ENV: &str = "HARNESS_TUI_P1_03_REDUCED";
pub(crate) const HELPER_TEST: &str = "p1_03_pty_helper";

pub(crate) fn run_helper() {
    if std::env::var(SCENARIO_ENV).as_deref() != Ok("1") {
        return;
    }

    let (_update_tx, update_rx) = harness_tui::live_update_channel();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Startup {
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
}

#[allow(dead_code)]
pub(crate) const STARTUP_HOME_WORDMARK_MARKER: &str = "Harness";
#[allow(dead_code)]
pub(crate) const STARTUP_HOME_ASCII_WORDMARK_MARKER: &str = "Harness";
pub(crate) const STARTUP_HOME_SHORTCUT_MARKER: &str = "Ctrl+p open";
pub(crate) const STARTUP_HOME_VALUE_PROP_MARKER: &str = "open a fresh session in this directory";
pub(crate) const STARTUP_HOME_COMPOSER_HINT_MARKER: &str = "Ask anything... \"inspect src/ui.rs\"";
pub(crate) const STARTUP_HOME_DENSE_VALUE_PROP_MARKER: &str =
    "open a fresh session in this directory";
pub(crate) const STARTUP_LAUNCHER_READY_MARKER: &str = STARTUP_HOME_SHORTCUT_MARKER;
pub(crate) const STARTUP_COMMAND_PALETTE_MARKER: &str = "Commands";
pub(crate) const STARTUP_CONTINUE_HISTORY_MARKER: &str = "Continue session";
pub(crate) const STARTUP_REPLAY_HISTORY_MARKER: &str = "Replay session";
pub(crate) const STARTUP_CONTINUE_HISTORY_READY_MARKER: &str = "continue ready";
pub(crate) const REPLAY_READY_MARKER: &str = "q quit";
pub(crate) const REPLAY_DENSE_READY_MARKER: &str = "Replay · read-only";
pub(crate) const LIVE_OPERATOR_EMPTY_MARKER: &str = "Context";
pub(crate) const LIVE_SUCCESS_COMPOSER_MARKER: &str = "Worker · model-1";
pub(crate) const LIVE_READY_NEXT_TURN_MARKER: &str = "Ctrl+p commands";
pub(crate) const OPERATOR_FILES_MARKER: &str = "Modified Files";
pub(crate) const RUN_FINISHED_SHELL_MARKERS: &[&str] = &[LIVE_READY_NEXT_TURN_MARKER];

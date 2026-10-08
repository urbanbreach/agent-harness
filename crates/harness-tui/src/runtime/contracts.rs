//! Public CLI/runtime messages. These contracts survive the implementation replacement.
use crate::{
    app::{LaunchMetadata, SessionHistoryEntry, TogglesConfig, UiIntent},
    runtime_live_updates::LiveUpdateReceiver,
};
use anyhow::Result;
use harness_core::event::{EventEnvelopeV1, RuntimeEvent};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

static REPLAY_METADATA: Mutex<Option<LaunchMetadata>> = Mutex::new(None);
pub fn set_pending_replay_launch_metadata(metadata: Option<LaunchMetadata>) {
    *REPLAY_METADATA.lock().unwrap_or_else(|e| e.into_inner()) = metadata;
}
pub(super) fn take_replay_metadata() -> Option<LaunchMetadata> {
    REPLAY_METADATA
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

pub enum LiveUpdate {
    CommandOutput(Box<harness_core::coord::CommandSnapshot>),
    RewindPoints {
        generation: u64,
        result: Result<Vec<harness_core::conversation_rewind::RewindPoint>, String>,
    },
    RewindComplete {
        generation: u64,
        result: Result<harness_core::conversation_rewind::RewindPoint, String>,
    },
    Event(Box<RuntimeEvent>),
    YoloModeChanged {
        enabled: bool,
    },
    YoloModeChangeFailed,
    Status(String),
    ModelPromptNotice(String),
    SessionHistory(Vec<SessionHistoryEntry>),
    ContinueSession {
        run_id: String,
        run_dir: PathBuf,
        prompt_draft: String,
    },
    OperatorNotice {
        message: String,
        level: OperatorNoticeLevel,
    },
    AuthBackendResult {
        success: bool,
        message: String,
    },
    AuthProviderCatalogRefreshed {
        launch_metadata: Box<LaunchMetadata>,
    },
    /// Input the user queued for a turn they interrupted, returned to the editor.
    RestoreQueuedInput(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperatorNoticeLevel {
    Info,
    Error,
}

pub enum TuiMode {
    Startup {
        session_history_entries: Vec<SessionHistoryEntry>,
        prompt_history_path: Option<PathBuf>,
        update_rx: LiveUpdateReceiver,
    },
    Replay {
        run_dir: PathBuf,
        events: Vec<EventEnvelopeV1>,
    },
    Live {
        run_dir: PathBuf,
        historical_events: Vec<EventEnvelopeV1>,
        session_history_entries: Vec<SessionHistoryEntry>,
        prompt_history_path: Option<PathBuf>,
        update_rx: LiveUpdateReceiver,
        compact_session_supported: bool,
    },
}

pub struct TuiOptions {
    pub storage_data_dir: Option<PathBuf>,
    pub mode: TuiMode,
    pub exit_on_finish: bool,
    pub on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    pub keybindings: Option<BTreeMap<String, String>>,
    pub toggles: Option<TogglesConfig>,
    pub preserve_terminal_on_exit: bool,
    pub skip_alternate_screen: bool,
}

use harness_compaction::{CompactionMethod, ManualMode};
use harness_providers::{NativeCompactionError, ProviderErrorCategory};
use std::collections::BTreeMap;

/// What a manual compaction, handoff or idle pass did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualCompactionOutcome {
    NoOp {
        reason: String,
    },
    Compacted {
        tokens_before: u32,
        tokens_after: u32,
        summary_preview: String,
        method: CompactionMethod,
        /// Where a handoff document was saved, when the pass wrote one.
        saved_path: Option<String>,
    },
}

/// Options for a manual `/compact`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManualCompactionOptions {
    pub through_request_id: Option<String>,
    pub reason: String,
    /// Public focus the user typed; hooks see it.
    pub instructions: Option<String>,
    /// Host guidance the summarizer follows but hooks never see. It overrides the focus.
    pub internal_guidance: Option<String>,
    pub mode: Option<ManualMode>,
    /// Do not resume a turn this pass interrupts; the caller owns the next turn.
    pub suppress_continuation: bool,
}

/// Which stage of a compaction pass failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionErrorKind {
    Summary,
    ShortSummary,
    TurnPrefix,
    Handoff,
    Branch,
    Native,
    Endpoint,
    Hook,
}

/// A compaction failure that keeps the provider's classification, status and response
/// headers so retry and fallback decisions can tell it from an ordinary summary failure.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct CompactionError {
    pub kind: CompactionErrorKind,
    pub message: String,
    pub status: Option<u16>,
    pub category: Option<ProviderErrorCategory>,
    pub auth: bool,
    pub headers: BTreeMap<String, String>,
    pub cause: Option<String>,
}

impl CompactionError {
    pub fn new(kind: CompactionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status: None,
            category: None,
            auth: false,
            headers: BTreeMap::new(),
            cause: None,
        }
    }
}

impl From<NativeCompactionError> for CompactionError {
    fn from(error: NativeCompactionError) -> Self {
        Self {
            kind: CompactionErrorKind::Native,
            message: error.message,
            status: error.status,
            category: error.category,
            auth: error.auth,
            headers: error.headers,
            cause: error.cause,
        }
    }
}

/// Why a compaction pass was cancelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelReason {
    Manual,
    Superseded,
    Shutdown,
    Timeout,
    Other(String),
}

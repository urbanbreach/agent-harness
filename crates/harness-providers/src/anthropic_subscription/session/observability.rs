//! Per-turn continuity observability.
//!
//! A decision is staged
//! per attempt and emitted only when the attempt is retained, so every completed turn yields
//! exactly one observation; a turn where every attempt failed yields one `failed`.
use std::{
    collections::VecDeque,
    sync::{LazyLock, Mutex},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuityObservation {
    pub kind: &'static str,
    pub reason: &'static str,
    pub delta_messages: Option<usize>,
    pub payload_bytes: Option<usize>,
    pub collapsed_directives: Option<usize>,
}

const REASONS: [&str; 37] = [
    "prefix_matched",
    "registry_miss",
    "idle_ttl",
    "capacity",
    "model_selected",
    "thinking_level_selected",
    "bound_account_token_expiring",
    "account_changed",
    "credential_refreshed",
    "model_changed",
    "toolset_changed",
    "system_prompt_changed",
    "assistant_stream_diverged",
    "options_changed",
    "history_rolled_back",
    "assistant_rewritten",
    "transcript_missing",
    "cross_root_unsupported",
    "sent_stream_diverged",
    "branch_diverged",
    "branch_boundary_unavailable",
    "branch_resume",
    "tainted_compaction",
    "tainted_fork",
    "tainted_abort",
    "tainted_assistant_provenance_unverified",
    "resume_initialization_failed",
    "resume_initialization_aborted",
    "resume_mode_off",
    "query_failed",
    "turn_attribution_failed",
    "session_unconfirmed",
    "abort_timeout",
    "extensions_removed",
    "session_shutdown",
    "timeout_retry",
    "other",
];

fn member(text: &str) -> Option<&'static str> {
    REASONS.iter().copied().find(|reason| *reason == text)
}

/// Buckets arbitrary close/pump failures into the fixed close-cause vocabulary.
pub fn sanitize_close_cause(text: &str) -> &'static str {
    if let Some(reason) = member(text) {
        return reason;
    }
    let lower = text.to_lowercase();
    if lower.contains("did not terminate") || lower.contains("interrupt failed") {
        return "abort_timeout";
    }
    if lower.contains("user_message_uuid did not match")
        || lower.contains("result arrived before replay claim")
        || lower.contains("pre-replay buffer overflow")
    {
        return "turn_attribution_failed";
    }
    if lower.contains("query ended before")
        || lower.contains("claude sdk oauth query")
        || lower.contains("anthropic subscription query")
        || lower.contains("claude code")
    {
        return "query_failed";
    }
    "other"
}

/// Maps a vocabulary member through, and anything else to a bucketed cause.
pub fn sanitize_reason(text: &str) -> &'static str {
    if let Some(reason) = member(text) {
        return reason;
    }
    if let Some(tainted) = text.strip_prefix("tainted:")
        && let Some(reason) = member(&format!("tainted_{tainted}"))
    {
        return reason;
    }
    sanitize_close_cause(text)
}

/// The failure that ended a turn after every attempt failed; raw text never escapes.
pub fn sanitize_terminal_failure(text: &str) -> &'static str {
    match sanitize_close_cause(text) {
        "other" => "query_failed",
        cause => cause,
    }
}

const PENDING_CLOSE_CAUSE_LIMIT: usize = 256;
static PENDING_CLOSE_CAUSES: LazyLock<Mutex<VecDeque<(String, &'static str)>>> =
    LazyLock::new(Mutex::default);

pub fn record_pending_close_cause(session_id: &str, reason: &str) {
    let cause = sanitize_reason(reason);
    if let Ok(mut causes) = PENDING_CLOSE_CAUSES.lock() {
        causes.retain(|(id, _)| id != session_id);
        if causes.len() >= PENDING_CLOSE_CAUSE_LIMIT {
            causes.pop_front();
        }
        causes.push_back((session_id.into(), cause));
    }
    tracing::debug!(reason = cause, session_id, "claude_sdk_oauth_session_close");
}

pub fn peek_pending_close_cause(session_id: &str) -> Option<&'static str> {
    PENDING_CLOSE_CAUSES
        .lock()
        .ok()?
        .iter()
        .find(|(id, _)| id == session_id)
        .map(|(_, cause)| *cause)
}

pub fn consume_pending_close_cause(session_id: &str) -> Option<&'static str> {
    let mut causes = PENDING_CLOSE_CAUSES.lock().ok()?;
    let position = causes.iter().position(|(id, _)| id == session_id)?;
    causes.remove(position).map(|(_, cause)| cause)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedKind {
    Incremental,
    Resume,
    ColdSeed,
}

pub struct SyncDecision<'a> {
    pub kind: ObservedKind,
    pub reason: Option<&'a str>,
    pub delta_messages: usize,
    pub first_turn: bool,
    pub session_id: &'a str,
    pub payload_bytes: Option<usize>,
    pub collapsed_directives: Option<usize>,
}

/// incremental -> delta, resume -> fork, cold-seed -> flatten (or bootstrap on a first turn).
pub fn observe_session_sync_decision(input: &SyncDecision<'_>) -> ContinuityObservation {
    match input.kind {
        ObservedKind::Incremental => ContinuityObservation {
            kind: "delta",
            reason: "prefix_matched",
            delta_messages: Some(input.delta_messages),
            payload_bytes: None,
            collapsed_directives: None,
        },
        ObservedKind::Resume => {
            let retained = (input.reason == Some("registry_miss"))
                .then(|| consume_pending_close_cause(input.session_id))
                .flatten();
            ContinuityObservation {
                kind: "fork",
                reason: retained
                    .unwrap_or_else(|| input.reason.map_or("branch_resume", sanitize_reason)),
                delta_messages: Some(input.delta_messages),
                payload_bytes: None,
                collapsed_directives: None,
            }
        }
        ObservedKind::ColdSeed => {
            let retained = (input.reason == Some("registry_miss"))
                .then(|| peek_pending_close_cause(input.session_id))
                .flatten();
            let reason = retained.unwrap_or_else(|| sanitize_reason(input.reason.unwrap_or("")));
            ContinuityObservation {
                kind: if input.first_turn && reason == "registry_miss" {
                    "bootstrap"
                } else {
                    "flatten"
                },
                reason,
                delta_messages: Some(input.delta_messages),
                payload_bytes: input.payload_bytes,
                collapsed_directives: input.collapsed_directives,
            }
        }
    }
}

pub fn emit_continuity_observation(observation: &ContinuityObservation, session_id: Option<&str>) {
    tracing::info!(
        kind = observation.kind,
        reason = observation.reason,
        count = observation.delta_messages,
        payload_bytes = observation.payload_bytes,
        session_id,
        "claude_sdk_oauth_session_continuity"
    );
}

/// An attempt's staged decision; `emit` fires once, and only when the attempt is retained.
pub struct StagedDecision {
    pub observation: ContinuityObservation,
    session_id: String,
    emitted: bool,
}
impl StagedDecision {
    pub fn new(observation: ContinuityObservation, session_id: &str) -> Self {
        Self {
            observation,
            session_id: session_id.into(),
            emitted: false,
        }
    }
    pub fn emit(&mut self) {
        if self.emitted {
            return;
        }
        self.emitted = true;
        consume_pending_close_cause(&self.session_id);
        emit_continuity_observation(&self.observation, Some(&self.session_id));
    }
}

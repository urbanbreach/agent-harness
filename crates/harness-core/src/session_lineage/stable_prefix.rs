// allow: SIZE_OK — session management (lineage + projection + inspection)
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event::{EventEnvelopeV1, EventV1, SCHEMA_VERSION};
use crate::proj::RunStatus;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionLineageError {
    #[error("event log must start at seq=1 and remain contiguous: expected seq {expected}, got {actual}")]
    NonContiguousSeq { expected: u64, actual: u64 },
    #[error("event at seq {seq} uses schema_version {actual}; expected schema_version {expected}")]
    UnsupportedSchemaVersion {
        seq: u64,
        expected: u16,
        actual: u16,
    },
    #[error("events contain multiple run ids: expected `{expected}`, got `{actual}` at seq {seq}")]
    RunIdMismatch {
        expected: String,
        actual: String,
        seq: u64,
    },
    #[error("stable prefix cutoff seq {cutoff_seq} is outside event log range 0..={max_seq}")]
    CutoffOutOfRange { cutoff_seq: u64, max_seq: u64 },
    #[error("prefix ending at seq {cutoff_seq} is unstable: {reason}")]
    UnstablePrefix { cutoff_seq: u64, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableSessionPrefix {
    pub cutoff_seq: u64,
    pub event_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<RunStatus>,
}

/// Validate the selected stable prefix used by fork operations.
pub fn validate_fork_stable_prefix(
    events: &[EventEnvelopeV1],
    cutoff_seq: u64,
) -> Result<StableSessionPrefix, SessionLineageError> {
    validate_stable_prefix(events, cutoff_seq)
}

/// Validate an in-memory live prefix used by TUI fork operations.
///
/// Disk-backed forks require a terminal run lifecycle. TUI `/fork` mirrors the reference
/// message-selector behavior: selecting a user message copies the live transcript before that
/// message and restores the selected prompt for editing. Low-level harness task/tool/edit state may
/// still be open in the source prefix, so live materialization terminalizes the copied snapshot
/// before the child is resumed.
pub fn validate_tui_fork_stable_prefix(
    events: &[EventEnvelopeV1],
    cutoff_seq: u64,
) -> Result<StableSessionPrefix, SessionLineageError> {
    validate_stable_prefix_with_options(events, cutoff_seq, true)
}

/// Select the latest stable prefix used by clone operations.
pub fn latest_clone_stable_prefix(
    events: &[EventEnvelopeV1],
) -> Result<StableSessionPrefix, SessionLineageError> {
    let max_seq = validate_event_log(events)?;

    for cutoff_seq in (1..=max_seq).rev() {
        let state = project_prefix_state(events, cutoff_seq);
        if state.unstable_reason(false).is_none() {
            return Ok(stable_prefix_from_state(cutoff_seq, &state));
        }
    }

    if events.is_empty() {
        Ok(StableSessionPrefix {
            cutoff_seq: 0,
            event_count: 0,
            run_id: None,
            status: None,
        })
    } else {
        Err(SessionLineageError::UnstablePrefix {
            cutoff_seq: max_seq,
            reason: "no stable completed prefix exists in the source event log".to_string(),
        })
    }
}

/// Validate an explicit stable prefix cutoff.
pub fn validate_stable_prefix(
    events: &[EventEnvelopeV1],
    cutoff_seq: u64,
) -> Result<StableSessionPrefix, SessionLineageError> {
    validate_stable_prefix_with_options(events, cutoff_seq, false)
}

fn validate_stable_prefix_with_options(
    events: &[EventEnvelopeV1],
    cutoff_seq: u64,
    allow_active_lifecycle: bool,
) -> Result<StableSessionPrefix, SessionLineageError> {
    let max_seq = validate_event_log(events)?;
    if cutoff_seq > max_seq {
        return Err(SessionLineageError::CutoffOutOfRange {
            cutoff_seq,
            max_seq,
        });
    }

    let state = project_prefix_state(events, cutoff_seq);
    if let Some(reason) = state.unstable_reason(allow_active_lifecycle) {
        return Err(SessionLineageError::UnstablePrefix { cutoff_seq, reason });
    }

    Ok(stable_prefix_from_state(cutoff_seq, &state))
}

fn validate_event_log(events: &[EventEnvelopeV1]) -> Result<u64, SessionLineageError> {
    let mut run_id: Option<&str> = None;

    for (expected_seq, event) in (1_u64..).zip(events.iter()) {
        if event.schema_version != SCHEMA_VERSION {
            return Err(SessionLineageError::UnsupportedSchemaVersion {
                seq: event.seq,
                expected: SCHEMA_VERSION,
                actual: event.schema_version,
            });
        }
        if event.seq != expected_seq {
            return Err(SessionLineageError::NonContiguousSeq {
                expected: expected_seq,
                actual: event.seq,
            });
        }

        match run_id {
            None => run_id = Some(event.run_id.as_str()),
            Some(existing) if existing == event.run_id.as_str() => {}
            Some(existing) => {
                return Err(SessionLineageError::RunIdMismatch {
                    expected: existing.to_string(),
                    actual: event.run_id.to_string(),
                    seq: event.seq,
                })
            }
        }
    }

    Ok(events.last().map(|event| event.seq).unwrap_or(0))
}

fn stable_prefix_from_state(cutoff_seq: u64, state: &PrefixState) -> StableSessionPrefix {
    StableSessionPrefix {
        cutoff_seq,
        event_count: usize::try_from(cutoff_seq).unwrap_or(usize::MAX),
        run_id: state.run_id.clone(),
        status: state.lifecycle.status(),
    }
}

fn project_prefix_state(events: &[EventEnvelopeV1], cutoff_seq: u64) -> PrefixState {
    let mut state = PrefixState::default();
    for event in events
        .iter()
        .take(usize::try_from(cutoff_seq).unwrap_or(usize::MAX))
    {
        state.apply(event);
    }
    state
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PrefixLifecycle {
    #[default]
    Empty,
    Active,
    Finished,
    Failed,
}

impl PrefixLifecycle {
    fn status(self) -> Option<RunStatus> {
        match self {
            Self::Empty | Self::Active => None,
            Self::Finished => Some(RunStatus::Finished),
            Self::Failed => Some(RunStatus::Failed),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PrefixState {
    run_id: Option<String>,
    lifecycle: PrefixLifecycle,
    tasks_in_flight: BTreeSet<String>,
    tool_calls_in_flight: BTreeSet<String>,
    provider_requests_in_flight: BTreeSet<String>,
    user_requests_awaiting_provider: BTreeSet<String>,
    pending_permissions: BTreeSet<String>,
    compactions_in_flight: BTreeSet<String>,
    edits_in_flight: BTreeMap<String, String>,
}

impl PrefixState {
    fn apply(&mut self, event: &EventEnvelopeV1) {
        self.run_id.get_or_insert_with(|| event.run_id.to_string());

        match &event.payload {
            EventV1::RunStarted(_) => {
                self.lifecycle = PrefixLifecycle::Active;
                self.tasks_in_flight.clear();
                self.tool_calls_in_flight.clear();
                self.provider_requests_in_flight.clear();
                self.user_requests_awaiting_provider.clear();
                self.pending_permissions.clear();
                self.compactions_in_flight.clear();
                self.edits_in_flight.clear();
            }
            EventV1::RunFinished(_) => self.lifecycle = PrefixLifecycle::Finished,
            EventV1::RunFailed(_) => self.lifecycle = PrefixLifecycle::Failed,
            EventV1::TaskScheduled(payload) => {
                self.tasks_in_flight.insert(payload.task_id.to_string());
            }
            EventV1::TaskCancelled(payload) => {
                self.tasks_in_flight.remove(payload.task_id.as_str());
            }
            EventV1::TaskCompleted(payload) => {
                self.tasks_in_flight.remove(payload.task_id.as_str());
            }
            EventV1::TaskResultLate(payload) => {
                self.tasks_in_flight.remove(payload.task_id.as_str());
            }
            EventV1::ProviderRequestStarted(payload) => {
                if let Some(turn_id) = provider_started_turn_id(payload) {
                    self.user_requests_awaiting_provider.remove(turn_id);
                }
                self.user_requests_awaiting_provider
                    .remove(payload.request_id.as_str());
                self.provider_requests_in_flight
                    .insert(payload.request_id.to_string());
            }
            EventV1::ProviderRequestFinished(payload) => {
                self.provider_requests_in_flight
                    .remove(payload.request_id.as_str());
            }
            EventV1::ToolCallRequested(payload) => {
                self.tool_calls_in_flight
                    .insert(payload.tool_call_id.to_string());
            }
            EventV1::ToolCallStarted(payload) => {
                self.tool_calls_in_flight
                    .insert(payload.tool_call_id.to_string());
            }
            EventV1::ToolCallFinished(payload) => {
                self.tool_calls_in_flight
                    .remove(payload.tool_call_id.as_str());
            }
            EventV1::PermissionRequested(payload) => {
                self.pending_permissions
                    .insert(payload.permission_id.clone());
            }
            EventV1::PermissionResolved(payload) => {
                self.pending_permissions.remove(&payload.permission_id);
            }
            EventV1::CompactionRequested(payload) => {
                self.compactions_in_flight
                    .insert(payload.checkpoint_id.clone());
            }
            EventV1::CompactionWritten(payload) => {
                self.compactions_in_flight.remove(&payload.checkpoint_id);
            }
            EventV1::CompactionApplied(payload) => {
                self.compactions_in_flight.remove(&payload.checkpoint_id);
            }
            EventV1::CompactionFailed(payload) => {
                if let Some(checkpoint_id) = payload.checkpoint_id.as_ref() {
                    self.compactions_in_flight.remove(checkpoint_id);
                }
            }
            EventV1::EditProposed(payload) => {
                self.edits_in_flight
                    .insert(payload.edit_id.clone(), payload.path.clone());
            }
            EventV1::EditApplied(payload) => {
                self.complete_edit(&payload.edit_id, &payload.path);
            }
            EventV1::EditRejected(payload) => {
                self.complete_edit(&payload.edit_id, &payload.path);
            }
            EventV1::UserMessageSubmitted(payload) => {
                if !is_background_task_wakeup_message(&payload.text) {
                    self.user_requests_awaiting_provider
                        .insert(payload.request_id.to_string());
                }
            }
            EventV1::AgentSpawned(_)
            | EventV1::SessionTitleUpdated(_)
            | EventV1::AgentStopped(_)
            | EventV1::BackgroundTaskNotification(_)
            | EventV1::StaleDetected(_)
            | EventV1::ProviderStreamDelta(_)
            | EventV1::ProviderReasoningDelta(_)
            | EventV1::AssistantMessageFinished(_)
            | EventV1::PermissionGrantRecorded(_)
            | EventV1::ArtifactWritten(_)
            | EventV1::PolicyViolationDetected(_)
            | EventV1::UiIntentReceived(_)
            | EventV1::WorkspaceSnapshot(_)
            | EventV1::WorkspaceReverted(_) => {}
        }
    }

    fn unstable_reason(&self, allow_active_lifecycle: bool) -> Option<String> {
        if !allow_active_lifecycle {
            if let Some(reason) =
                first_non_empty_reason("tasks are still in flight", &self.tasks_in_flight)
            {
                return Some(reason);
            }
            if let Some(reason) =
                first_non_empty_reason("tool calls are still in flight", &self.tool_calls_in_flight)
            {
                return Some(reason);
            }
            if let Some(reason) = first_non_empty_reason(
                "provider requests are still in flight",
                &self.provider_requests_in_flight,
            ) {
                return Some(reason);
            }
            if let Some(reason) = first_non_empty_reason(
                "user messages are awaiting a provider turn",
                &self.user_requests_awaiting_provider,
            ) {
                return Some(reason);
            }
            if let Some(reason) = first_non_empty_reason(
                "pending permissions must be resolved",
                &self.pending_permissions,
            ) {
                return Some(reason);
            }
            if let Some(reason) = first_non_empty_reason(
                "compactions are still in flight",
                &self.compactions_in_flight,
            ) {
                return Some(reason);
            }
            if let Some(reason) =
                first_non_empty_map_key_reason("edits are still in flight", &self.edits_in_flight)
            {
                return Some(reason);
            }
        }

        match self.lifecycle {
            PrefixLifecycle::Empty => None,
            PrefixLifecycle::Active if allow_active_lifecycle => None,
            PrefixLifecycle::Active => Some(
                "run is still active; include run_finished or run_failed before cutting"
                    .to_string(),
            ),
            PrefixLifecycle::Finished | PrefixLifecycle::Failed => None,
        }
    }

    fn complete_edit(&mut self, edit_id: &str, path: &str) {
        if self.edits_in_flight.remove(edit_id).is_some() {
            return;
        }

        // Older native-edit events could propose the coordinator fallback id but apply the
        // caller-provided editId. Treat a terminal event for the same path as closing that
        // proposal so historical sessions regain stable fork points after completed edits.
        let Some(matching_id) =
            self.edits_in_flight
                .iter()
                .find_map(|(candidate_id, candidate_path)| {
                    (candidate_path == path).then(|| candidate_id.clone())
                })
        else {
            return;
        };
        self.edits_in_flight.remove(&matching_id);
    }
}

fn provider_started_turn_id(payload: &crate::event::ProviderRequestStartedEvent) -> Option<&str> {
    payload
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.turn_id.as_deref())
}

fn is_background_task_wakeup_message(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("<system-reminder>\n[BACKGROUND TASK ")
        || trimmed.starts_with("<system-reminder>\r\n[BACKGROUND TASK ")
}

fn first_non_empty_reason(label: &str, values: &BTreeSet<String>) -> Option<String> {
    values.iter().next().map(|id| format!("{label}: {id}"))
}

fn first_non_empty_map_key_reason(
    label: &str,
    values: &BTreeMap<String, String>,
) -> Option<String> {
    values.keys().next().map(|id| format!("{label}: {id}"))
}

//! Pure helpers for session lineage operations.
//!
//! Contract:
//! - `fork = selected stable prefix`
//! - `clone = latest stable prefix`
//! - `tree = read-only lineage browser`

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event::{EventEnvelopeV1, EventV1, SCHEMA_VERSION};
use crate::proj::{RunStatus, SessionCatalogEntry};

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

/// Select the latest stable prefix used by clone operations.
pub fn latest_clone_stable_prefix(
    events: &[EventEnvelopeV1],
) -> Result<StableSessionPrefix, SessionLineageError> {
    let max_seq = validate_event_log(events)?;

    for cutoff_seq in (1..=max_seq).rev() {
        let state = project_prefix_state(events, cutoff_seq);
        if state.unstable_reason().is_none() {
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
    let max_seq = validate_event_log(events)?;
    if cutoff_seq > max_seq {
        return Err(SessionLineageError::CutoffOutOfRange {
            cutoff_seq,
            max_seq,
        });
    }

    let state = project_prefix_state(events, cutoff_seq);
    if let Some(reason) = state.unstable_reason() {
        return Err(SessionLineageError::UnstablePrefix { cutoff_seq, reason });
    }

    Ok(stable_prefix_from_state(cutoff_seq, &state))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionLineageTree {
    pub roots: Vec<SessionLineageNode>,
}

impl SessionLineageTree {
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn len(&self) -> usize {
        self.roots.iter().map(SessionLineageNode::subtree_len).sum()
    }

    pub fn flatten(&self) -> Vec<SessionLineageRow<'_>> {
        let mut rows = Vec::new();
        for root in &self.roots {
            root.flatten_into(0, &mut rows);
        }
        rows
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLineageNode {
    pub entry: SessionCatalogEntry,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SessionLineageNode>,
}

impl SessionLineageNode {
    fn subtree_len(&self) -> usize {
        1 + self.children.iter().map(Self::subtree_len).sum::<usize>()
    }

    fn flatten_into<'a>(&'a self, depth: usize, rows: &mut Vec<SessionLineageRow<'a>>) {
        rows.push(SessionLineageRow {
            depth,
            entry: &self.entry,
        });
        for child in &self.children {
            child.flatten_into(depth + 1, rows);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLineageRow<'a> {
    pub depth: usize,
    pub entry: &'a SessionCatalogEntry,
}

/// Project a deterministic, read-only lineage tree over session catalog entries.
///
/// Entries whose `parent_session_id` is missing, blank, unknown, self-referential, or cyclic are
/// treated as roots so legacy or partially migrated catalogs remain browseable.
pub fn project_lineage_tree(
    entries: impl IntoIterator<Item = SessionCatalogEntry>,
) -> SessionLineageTree {
    let entries = entries
        .into_iter()
        .map(|entry| (entry.run_id.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let parent_by_id = entries
        .iter()
        .map(|(run_id, entry)| {
            (
                run_id.clone(),
                normalized_parent_id(entry.parent_session_id.as_deref()),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut children_by_parent = BTreeMap::<String, Vec<String>>::new();
    let mut roots = Vec::<String>::new();

    for run_id in sorted_ids(&entries) {
        let parent_id = parent_by_id
            .get(&run_id)
            .and_then(|parent_id| parent_id.as_deref());

        match parent_id {
            Some(parent_id)
                if entries.contains_key(parent_id)
                    && parent_id != run_id
                    && !parent_chain_reaches(&parent_by_id, parent_id, &run_id) =>
            {
                children_by_parent
                    .entry(parent_id.to_string())
                    .or_default()
                    .push(run_id);
            }
            _ => roots.push(run_id),
        }
    }

    for children in children_by_parent.values_mut() {
        children.sort_by(|left, right| compare_entries(&entries[left], &entries[right]));
    }
    roots.sort_by(|left, right| compare_entries(&entries[left], &entries[right]));

    SessionLineageTree {
        roots: roots
            .into_iter()
            .map(|run_id| build_node(&entries, &children_by_parent, &run_id))
            .collect(),
    }
}

fn validate_event_log(events: &[EventEnvelopeV1]) -> Result<u64, SessionLineageError> {
    let mut expected_seq = 1_u64;
    let mut run_id: Option<&str> = None;

    for event in events {
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
        expected_seq += 1;

        match run_id {
            None => run_id = Some(event.run_id.as_str()),
            Some(existing) if existing == event.run_id => {}
            Some(existing) => {
                return Err(SessionLineageError::RunIdMismatch {
                    expected: existing.to_string(),
                    actual: event.run_id.clone(),
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
        event_count: cutoff_seq as usize,
        run_id: state.run_id.clone(),
        status: state.lifecycle.status(),
    }
}

fn project_prefix_state(events: &[EventEnvelopeV1], cutoff_seq: u64) -> PrefixState {
    let mut state = PrefixState::default();
    for event in events.iter().take(cutoff_seq as usize) {
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
    pending_permissions: BTreeSet<String>,
    compactions_in_flight: BTreeSet<String>,
    edits_in_flight: BTreeSet<String>,
}

impl PrefixState {
    fn apply(&mut self, event: &EventEnvelopeV1) {
        self.run_id.get_or_insert_with(|| event.run_id.clone());

        match &event.payload {
            EventV1::RunStarted(_) => {
                self.lifecycle = PrefixLifecycle::Active;
                self.tasks_in_flight.clear();
                self.tool_calls_in_flight.clear();
                self.provider_requests_in_flight.clear();
                self.pending_permissions.clear();
                self.compactions_in_flight.clear();
                self.edits_in_flight.clear();
            }
            EventV1::RunFinished(_) => self.lifecycle = PrefixLifecycle::Finished,
            EventV1::RunFailed(_) => self.lifecycle = PrefixLifecycle::Failed,
            EventV1::TaskScheduled(payload) => {
                self.tasks_in_flight.insert(payload.task_id.clone());
            }
            EventV1::TaskCancelled(payload) => {
                self.tasks_in_flight.remove(&payload.task_id);
            }
            EventV1::TaskCompleted(payload) => {
                self.tasks_in_flight.remove(&payload.task_id);
            }
            EventV1::TaskResultLate(payload) => {
                self.tasks_in_flight.remove(&payload.task_id);
            }
            EventV1::ProviderRequestStarted(payload) => {
                self.provider_requests_in_flight
                    .insert(payload.request_id.clone());
            }
            EventV1::ProviderRequestFinished(payload) => {
                self.provider_requests_in_flight.remove(&payload.request_id);
            }
            EventV1::ToolCallRequested(payload) => {
                self.tool_calls_in_flight
                    .insert(payload.tool_call_id.clone());
            }
            EventV1::ToolCallStarted(payload) => {
                self.tool_calls_in_flight
                    .insert(payload.tool_call_id.clone());
            }
            EventV1::ToolCallFinished(payload) => {
                self.tool_calls_in_flight.remove(&payload.tool_call_id);
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
                self.edits_in_flight.insert(payload.edit_id.clone());
            }
            EventV1::EditApplied(payload) => {
                self.edits_in_flight.remove(&payload.edit_id);
            }
            EventV1::EditRejected(payload) => {
                self.edits_in_flight.remove(&payload.edit_id);
            }
            EventV1::AgentSpawned(_)
            | EventV1::AgentStopped(_)
            | EventV1::StaleDetected(_)
            | EventV1::UserMessageSubmitted(_)
            | EventV1::ProviderStreamDelta(_)
            | EventV1::ProviderReasoningDelta(_)
            | EventV1::AssistantMessageFinished(_)
            | EventV1::PermissionGrantRecorded(_)
            | EventV1::ArtifactWritten(_)
            | EventV1::PolicyViolationDetected(_)
            | EventV1::UiIntentReceived(_) => {}
        }
    }

    fn unstable_reason(&self) -> Option<String> {
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
            first_non_empty_reason("edits are still in flight", &self.edits_in_flight)
        {
            return Some(reason);
        }

        match self.lifecycle {
            PrefixLifecycle::Empty => None,
            PrefixLifecycle::Active => Some(
                "run is still active; include run_finished or run_failed before cutting"
                    .to_string(),
            ),
            PrefixLifecycle::Finished | PrefixLifecycle::Failed => None,
        }
    }
}

fn first_non_empty_reason(label: &str, values: &BTreeSet<String>) -> Option<String> {
    values.iter().next().map(|id| format!("{label}: {id}"))
}

fn normalized_parent_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parent_chain_reaches(
    parent_by_id: &BTreeMap<String, Option<String>>,
    start_parent_id: &str,
    target_id: &str,
) -> bool {
    let mut seen = BTreeSet::new();
    let mut current = Some(start_parent_id);

    while let Some(run_id) = current {
        if run_id == target_id {
            return true;
        }
        if !seen.insert(run_id.to_string()) {
            return false;
        }
        current = parent_by_id
            .get(run_id)
            .and_then(|parent| parent.as_deref());
    }

    false
}

fn sorted_ids(entries: &BTreeMap<String, SessionCatalogEntry>) -> Vec<String> {
    let mut ids = entries.keys().cloned().collect::<Vec<_>>();
    ids.sort_by(|left, right| compare_entries(&entries[left], &entries[right]));
    ids
}

fn compare_entries(left: &SessionCatalogEntry, right: &SessionCatalogEntry) -> Ordering {
    match (&left.last_updated_at, &right.last_updated_at) {
        (Some(left_updated), Some(right_updated)) => right_updated
            .cmp(left_updated)
            .then_with(|| left.run_id.cmp(&right.run_id)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.run_id.cmp(&right.run_id),
    }
}

fn build_node(
    entries: &BTreeMap<String, SessionCatalogEntry>,
    children_by_parent: &BTreeMap<String, Vec<String>>,
    run_id: &str,
) -> SessionLineageNode {
    SessionLineageNode {
        entry: entries[run_id].clone(),
        children: children_by_parent
            .get(run_id)
            .into_iter()
            .flatten()
            .map(|child_id| build_node(entries, children_by_parent, child_id))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        latest_clone_stable_prefix, project_lineage_tree, validate_fork_stable_prefix,
        validate_stable_prefix, SessionLineageError,
    };
    use crate::event::{
        ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1, PermissionDecision,
        PermissionRequestedEvent, PermissionResolvedEvent, ProviderRequestFinishedEvent,
        ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent, TaskScheduleState,
        TaskScheduledEvent, ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStatus,
        SCHEMA_VERSION,
    };
    use crate::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};

    #[test]
    fn session_lineage_projects_tree_root_child_sibling_deep_ordering() {
        let tree = project_lineage_tree(vec![
            entry("child-old", Some("root"), "2026-05-03T00:01:00Z"),
            entry("grandchild", Some("child-new"), "2026-05-03T00:03:00Z"),
            entry("root", None, "2026-05-03T00:00:00Z"),
            entry("child-new", Some("root"), "2026-05-03T00:02:00Z"),
        ]);

        let flattened = tree
            .flatten()
            .into_iter()
            .map(|row| (row.depth, row.entry.run_id.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(tree.len(), 4);
        assert_eq!(
            flattened,
            vec![
                (0, "root"),
                (1, "child-new"),
                (2, "grandchild"),
                (1, "child-old"),
            ]
        );
    }

    #[test]
    fn session_lineage_handles_empty_sessions() {
        let selected = validate_stable_prefix(&[], 0).expect("empty prefix is stable");
        let latest = latest_clone_stable_prefix(&[]).expect("empty clone prefix is stable");
        let tree = project_lineage_tree(Vec::new());

        assert_eq!(selected.cutoff_seq, 0);
        assert_eq!(selected.event_count, 0);
        assert_eq!(latest, selected);
        assert!(tree.is_empty());
    }

    #[test]
    fn session_lineage_accepts_stable_prefix() {
        let events = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "prompt".to_string(),
                    request_digest: "digest-req".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000001".to_string(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some("digest-out".to_string()),
                    usage: None,
                    metadata: None,
                }),
            ),
            envelope(
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
            envelope(
                6,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "resumed".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
        ];

        let fork = validate_fork_stable_prefix(&events, 5).expect("selected prefix is stable");
        let latest = latest_clone_stable_prefix(&events).expect("latest stable prefix exists");

        assert_eq!(fork.cutoff_seq, 5);
        assert_eq!(fork.event_count, 5);
        assert_eq!(fork.run_id.as_deref(), Some("run_session_lineage"));
        assert_eq!(fork.status, Some(RunStatus::Finished));
        assert_eq!(latest.cutoff_seq, 5);
    }

    #[test]
    fn session_lineage_rejects_in_flight_prefix() {
        let events = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:default:gpt-5".to_string()),
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "terminal but task remained open".to_string(),
                }),
            ),
        ];

        let err = validate_fork_stable_prefix(&events, 3).expect_err("task remains in flight");

        assert!(matches!(
            err,
            SessionLineageError::UnstablePrefix {
                cutoff_seq: 3,
                ref reason
            } if reason.contains("tasks are still in flight")
                && reason.contains("task_000001")
        ));
    }

    #[test]
    fn session_lineage_rejects_corrupt_non_contiguous_logs() {
        let non_contiguous = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ];
        assert!(matches!(
            validate_stable_prefix(&non_contiguous, 1),
            Err(SessionLineageError::NonContiguousSeq {
                expected: 2,
                actual: 3
            })
        ));

        let mut wrong_schema = vec![envelope(
            1,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        )];
        wrong_schema[0].schema_version = SCHEMA_VERSION + 1;
        assert!(matches!(
            validate_stable_prefix(&wrong_schema, 1),
            Err(SessionLineageError::UnsupportedSchemaVersion { seq: 1, .. })
        ));

        let mut run_mismatch = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ];
        run_mismatch[1].run_id = "run_other".to_string();
        assert!(matches!(
            validate_stable_prefix(&run_mismatch, 2),
            Err(SessionLineageError::RunIdMismatch { seq: 2, .. })
        ));
    }

    #[test]
    fn session_lineage_rejects_unstable_prefixes() {
        let active = vec![envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/workspace".to_string(),
            }),
        )];
        assert!(matches!(
            validate_stable_prefix(&active, 1),
            Err(SessionLineageError::UnstablePrefix { reason, .. })
                if reason.contains("run is still active")
        ));

        let pending_permission = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000001".to_string(),
                    kind: "bash".to_string(),
                    tool_call_id: None,
                    summary: "run command".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 1000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ];
        assert!(matches!(
            validate_stable_prefix(&pending_permission, 3),
            Err(SessionLineageError::UnstablePrefix { reason, .. })
                if reason.contains("pending permissions") && reason.contains("perm_000001")
        ));
    }

    #[test]
    fn session_lineage_clone_rejects_running_source_without_stable_prefix() {
        let events = vec![envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/workspace".to_string(),
            }),
        )];

        assert!(matches!(
            latest_clone_stable_prefix(&events),
            Err(SessionLineageError::UnstablePrefix { cutoff_seq: 1, reason })
                if reason.contains("no stable completed prefix")
        ));
    }

    #[test]
    fn session_lineage_handles_first_last_and_out_of_range_cutoffs() {
        let events = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000001".to_string(),
                    kind: "bash".to_string(),
                    tool_call_id: None,
                    summary: "run command".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 1000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                3,
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_000001".to_string(),
                    decision: PermissionDecision::Allow,
                    reason: Some("approved".to_string()),
                }),
            ),
            envelope(
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ];

        assert!(matches!(
            validate_stable_prefix(&events, 1),
            Err(SessionLineageError::UnstablePrefix { .. })
        ));
        assert_eq!(
            validate_stable_prefix(&events, 4)
                .expect("last cutoff is stable")
                .cutoff_seq,
            4
        );
        assert!(matches!(
            validate_stable_prefix(&events, 5),
            Err(SessionLineageError::CutoffOutOfRange {
                cutoff_seq: 5,
                max_seq: 4
            })
        ));
    }

    #[test]
    fn session_lineage_treats_legacy_entries_without_parent_metadata_as_roots() {
        let tree = project_lineage_tree(vec![
            entry("legacy-b", None, "2026-05-03T00:02:00Z"),
            entry("legacy-a", None, "2026-05-03T00:01:00Z"),
            entry("orphan", Some("missing-parent"), "2026-05-03T00:03:00Z"),
        ]);

        let flattened = tree
            .flatten()
            .into_iter()
            .map(|row| (row.depth, row.entry.run_id.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            flattened,
            vec![(0, "orphan"), (0, "legacy-b"), (0, "legacy-a")]
        );
    }

    #[test]
    fn session_lineage_tracks_tool_call_in_flight_cutoffs() {
        let events = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    tool_id: "bash".to_string(),
                    args_summary: "{}".to_string(),
                    args_digest: "digest-tool".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished before tool result".to_string(),
                }),
            ),
            envelope(
                4,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("ok".to_string()),
                    output_digest: Some("digest-output".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
        ];

        assert!(matches!(
            validate_stable_prefix(&events, 3),
            Err(SessionLineageError::UnstablePrefix { reason, .. })
                if reason.contains("tool calls are still in flight")
        ));
        assert_eq!(
            validate_stable_prefix(&events, 4)
                .expect("tool completion closes prefix")
                .cutoff_seq,
            4
        );
    }

    fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: "run_session_lineage".to_string(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_session_lineage".to_string()),
            payload,
        }
    }

    fn entry(
        run_id: &str,
        parent_session_id: Option<&str>,
        last_updated_at: &str,
    ) -> SessionCatalogEntry {
        SessionCatalogEntry {
            run_id: run_id.to_string(),
            run_name: Some(run_id.to_string()),
            status: Some(RunStatus::Finished),
            last_updated_at: Some(last_updated_at.to_string()),
            workspace_root: Some("/workspace".to_string()),
            profile_preset: Some("default".to_string()),
            provider_model: Some("default/gpt-5".to_string()),
            mode_source: SessionModeSource::InteractiveLive,
            is_resumable: true,
            resume_disabled_reason: None,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: parent_session_id.map(str::to_string),
        }
    }
}

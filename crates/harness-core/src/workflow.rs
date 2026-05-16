use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::event::{
    ContinuationLimitReachedEvent, ContinuationReminderQueuedEvent, ContinuationStartedEvent,
    ContinuationStoppedEvent, EventV1, WorkflowCompletedEvent, WorkflowEvidenceRecordedEvent,
    WorkflowOperatorDecisionRecordedEvent, WorkflowStartedEvent, WorkflowTransitionDeniedEvent,
    WorkflowTransitionRecordedEvent,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowRunProjection {
    pub workflow_id: String,
    pub mode: String,
    pub owner: String,
    pub status: String,
    pub terminal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub evidence_categories: BTreeSet<String>,
    #[serde(default)]
    pub operator_decisions: Vec<String>,
    #[serde(default)]
    pub denied_transition_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_snapshot: Option<WorkflowContextSnapshotRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowContextSnapshotRef {
    pub snapshot_id: String,
    pub slug: String,
    pub artifact_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambiguity_score: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowContinuationProjection {
    pub continuation_id: String,
    pub workflow_id: String,
    pub status: String,
    pub iteration: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowProjection {
    pub workflows: BTreeMap<String, WorkflowRunProjection>,
    pub idempotency_index: BTreeMap<String, String>,
    pub evidence: BTreeMap<String, Vec<WorkflowEvidenceRecordedEvent>>,
    pub denied_transitions: Vec<WorkflowTransitionDeniedEvent>,
    pub continuations: BTreeMap<String, WorkflowContinuationProjection>,
    pub context_snapshots: BTreeMap<String, WorkflowContextSnapshotRef>,
}

impl WorkflowProjection {
    pub fn apply_event(&mut self, event: &EventV1) {
        match event {
            EventV1::WorkflowStarted(payload) => self.apply_started(payload),
            EventV1::WorkflowTransitionRecorded(payload) => self.apply_transition(payload),
            EventV1::WorkflowTransitionDenied(payload) => self.apply_denied(payload),
            EventV1::WorkflowEvidenceRecorded(payload) => self.apply_evidence(payload),
            EventV1::WorkflowOperatorDecisionRecorded(payload) => {
                self.apply_operator_decision(payload)
            }
            EventV1::WorkflowCompleted(payload) => self.apply_completed(payload),
            EventV1::ContinuationStarted(payload) => self.apply_continuation_started(payload),
            EventV1::ContinuationReminderQueued(payload) => {
                self.apply_continuation_reminder(payload)
            }
            EventV1::ContinuationStopped(payload) => self.apply_continuation_stopped(payload),
            EventV1::ContinuationLimitReached(payload) => self.apply_continuation_limit(payload),
            _ => {}
        }
    }

    fn apply_started(&mut self, payload: &WorkflowStartedEvent) {
        if !self.workflows.contains_key(&payload.workflow_id) {
            self.workflows.insert(
                payload.workflow_id.clone(),
                WorkflowRunProjection {
                    workflow_id: payload.workflow_id.clone(),
                    mode: payload.mode.clone(),
                    owner: payload.owner.clone(),
                    status: "active".to_string(),
                    terminal: false,
                    lane: payload.lane.clone(),
                    title: payload.title.clone(),
                    idempotency_key: payload.idempotency_key.clone(),
                    evidence_categories: BTreeSet::new(),
                    operator_decisions: Vec::new(),
                    denied_transition_count: 0,
                    context_snapshot: None,
                },
            );
        }
        if let Some(key) = payload.idempotency_key.as_ref() {
            self.idempotency_index
                .entry(key.clone())
                .or_insert_with(|| payload.workflow_id.clone());
        }
    }

    fn apply_transition(&mut self, payload: &WorkflowTransitionRecordedEvent) {
        let Some(run) = self.workflows.get_mut(&payload.workflow_id) else {
            return;
        };
        if run.terminal {
            return;
        }
        run.status = payload.to_status.clone();
        run.owner = payload.owner.clone();
    }

    fn apply_denied(&mut self, payload: &WorkflowTransitionDeniedEvent) {
        if let Some(run) = self.workflows.get_mut(&payload.workflow_id) {
            run.denied_transition_count = run.denied_transition_count.saturating_add(1);
        }
        self.denied_transitions.push(payload.clone());
    }

    fn apply_evidence(&mut self, payload: &WorkflowEvidenceRecordedEvent) {
        let snapshot_ref = (payload.category
            == crate::context_snapshot::CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY)
            .then(|| context_snapshot_ref_from_evidence(payload))
            .flatten();
        if let Some(snapshot_ref) = snapshot_ref.as_ref() {
            self.context_snapshots
                .insert(snapshot_ref.snapshot_id.clone(), snapshot_ref.clone());
        }
        if let Some(run) = self.workflows.get_mut(&payload.workflow_id) {
            run.evidence_categories.insert(payload.category.clone());
            if let Some(snapshot_ref) = snapshot_ref {
                run.context_snapshot = Some(snapshot_ref);
            }
        }
        self.evidence
            .entry(payload.workflow_id.clone())
            .or_default()
            .push(payload.clone());
    }

    fn apply_operator_decision(&mut self, payload: &WorkflowOperatorDecisionRecordedEvent) {
        if let Some(run) = self.workflows.get_mut(&payload.workflow_id) {
            run.operator_decisions.push(payload.decision.clone());
        }
    }

    fn apply_completed(&mut self, payload: &WorkflowCompletedEvent) {
        if let Some(run) = self.workflows.get_mut(&payload.workflow_id) {
            run.status = payload.outcome.clone();
            run.owner = payload.owner.clone();
            run.terminal = true;
        }
    }

    fn apply_continuation_started(&mut self, payload: &ContinuationStartedEvent) {
        let Some(metadata) = payload.workflow.as_ref() else {
            return;
        };
        let Some(workflow_id) = metadata.workflow_id.as_ref() else {
            return;
        };
        self.continuations.insert(
            payload.continuation_id.clone(),
            WorkflowContinuationProjection {
                continuation_id: payload.continuation_id.clone(),
                workflow_id: workflow_id.clone(),
                status: "active".to_string(),
                iteration: metadata.iteration.unwrap_or(0),
                lane: metadata.lane.clone(),
                stop_reason: None,
            },
        );
    }

    fn apply_continuation_reminder(&mut self, payload: &ContinuationReminderQueuedEvent) {
        if let Some(continuation) = self.continuations.get_mut(&payload.continuation_id) {
            continuation.status = "reminder_queued".to_string();
            continuation.iteration = payload.iteration;
        }
    }

    fn apply_continuation_stopped(&mut self, payload: &ContinuationStoppedEvent) {
        if let Some(continuation) = self.continuations.get_mut(&payload.continuation_id) {
            continuation.status = "stopped".to_string();
            continuation.stop_reason = payload
                .workflow
                .as_ref()
                .and_then(|metadata| metadata.stop_reason.clone())
                .or_else(|| Some(payload.reason.clone()));
        }
    }

    fn apply_continuation_limit(&mut self, payload: &ContinuationLimitReachedEvent) {
        if let Some(continuation) = self.continuations.get_mut(&payload.continuation_id) {
            continuation.status = "limit_reached".to_string();
            continuation.iteration = payload.iteration;
            continuation.stop_reason = Some(payload.limit.clone());
        }
    }
}

fn context_snapshot_ref_from_evidence(
    payload: &WorkflowEvidenceRecordedEvent,
) -> Option<WorkflowContextSnapshotRef> {
    let artifact_path = payload.artifact_path.clone()?;
    let snapshot_id = payload
        .metadata
        .get("snapshot_id")
        .cloned()
        .or_else(|| payload.acceptance_ref.clone())?;
    Some(WorkflowContextSnapshotRef {
        snapshot_id,
        slug: payload
            .metadata
            .get("snapshot_slug")
            .cloned()
            .unwrap_or_else(|| "context-snapshot".to_string()),
        artifact_path,
        artifact_digest: payload.artifact_digest.clone(),
        ambiguity_score: payload.metadata.get("ambiguity_score").cloned(),
    })
}

pub fn project_workflows<'a>(events: impl IntoIterator<Item = &'a EventV1>) -> WorkflowProjection {
    let mut projection = WorkflowProjection::default();
    for event in events {
        projection.apply_event(event);
    }
    projection
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStartRequest {
    pub workflow_id: String,
    pub mode: String,
    pub owner: String,
    pub lane: Option<String>,
    pub title: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowStartDecision {
    Start(WorkflowStartedEvent),
    Existing { workflow_id: String },
    Denied(WorkflowTransitionDeniedEvent),
}

pub struct WorkflowTransitionPolicy;

impl WorkflowTransitionPolicy {
    pub fn decide_start(
        projection: &WorkflowProjection,
        request: WorkflowStartRequest,
    ) -> WorkflowStartDecision {
        if let Some(idempotency_key) = request.idempotency_key.as_ref() {
            if let Some(existing_workflow_id) = projection.idempotency_index.get(idempotency_key) {
                return WorkflowStartDecision::Existing {
                    workflow_id: existing_workflow_id.clone(),
                };
            }
        }

        if let Some(existing) = projection.workflows.get(&request.workflow_id) {
            if existing.owner == request.owner {
                return WorkflowStartDecision::Existing {
                    workflow_id: existing.workflow_id.clone(),
                };
            }
            return WorkflowStartDecision::Denied(WorkflowTransitionDeniedEvent {
                workflow_id: request.workflow_id,
                requested_status: "active".to_string(),
                reason: "conflicting workflow owner".to_string(),
                owner: request.owner,
                current_owner: Some(existing.owner.clone()),
                current_status: Some(existing.status.clone()),
                policy_id: "transition.owner_conflict_denied".to_string(),
            });
        }

        WorkflowStartDecision::Start(WorkflowStartedEvent {
            workflow_id: request.workflow_id,
            mode: request.mode,
            owner: request.owner,
            lane: request.lane,
            title: request.title,
            idempotency_key: request.idempotency_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        project_workflows, WorkflowProjection, WorkflowStartDecision, WorkflowStartRequest,
        WorkflowTransitionPolicy,
    };
    use crate::event::{
        ContinuationReminderQueuedEvent, ContinuationStartedEvent, ContinuationStoppedEvent,
        EventV1, WorkflowCompletedEvent, WorkflowEventMetadata, WorkflowEvidenceRecordedEvent,
        WorkflowStartedEvent, WorkflowTransitionRecordedEvent,
    };

    fn start_event() -> EventV1 {
        EventV1::WorkflowStarted(WorkflowStartedEvent {
            workflow_id: "wf_1".to_string(),
            mode: "workflow.run".to_string(),
            owner: "leader".to_string(),
            lane: Some("lane.leader".to_string()),
            title: Some("demo".to_string()),
            idempotency_key: Some("idem-1".to_string()),
        })
    }

    #[test]
    fn old_event_logs_project_empty_workflow_state() {
        let projection = project_workflows(std::iter::empty());
        assert!(projection.workflows.is_empty());
        assert!(projection.denied_transitions.is_empty());
    }

    #[test]
    fn duplicate_start_with_idempotency_returns_existing_projection() {
        let projection = project_workflows([start_event()].iter());
        let decision = WorkflowTransitionPolicy::decide_start(
            &projection,
            WorkflowStartRequest {
                workflow_id: "wf_1".to_string(),
                mode: "workflow.run".to_string(),
                owner: "leader".to_string(),
                lane: Some("lane.leader".to_string()),
                title: None,
                idempotency_key: Some("idem-1".to_string()),
            },
        );
        assert_eq!(
            decision,
            WorkflowStartDecision::Existing {
                workflow_id: "wf_1".to_string()
            }
        );
    }

    #[test]
    fn conflicting_owner_start_is_denied_without_mutating_projection() {
        let projection = project_workflows([start_event()].iter());
        let decision = WorkflowTransitionPolicy::decide_start(
            &projection,
            WorkflowStartRequest {
                workflow_id: "wf_1".to_string(),
                mode: "workflow.run".to_string(),
                owner: "other".to_string(),
                lane: None,
                title: None,
                idempotency_key: None,
            },
        );
        let WorkflowStartDecision::Denied(denied) = decision else {
            panic!("expected denied start")
        };
        assert_eq!(denied.policy_id, "transition.owner_conflict_denied");
        assert_eq!(projection.workflows["wf_1"].owner, "leader");
    }

    #[test]
    fn terminal_late_transition_does_not_mutate_terminal_status() {
        let events = [
            start_event(),
            EventV1::WorkflowCompleted(WorkflowCompletedEvent {
                workflow_id: "wf_1".to_string(),
                outcome: "outcome.finished".to_string(),
                reason: "done".to_string(),
                owner: "leader".to_string(),
            }),
            EventV1::WorkflowTransitionRecorded(WorkflowTransitionRecordedEvent {
                workflow_id: "wf_1".to_string(),
                from_status: Some("outcome.finished".to_string()),
                to_status: "active".to_string(),
                reason: "late result".to_string(),
                owner: "late-worker".to_string(),
                policy_id: Some("transition.terminal_late_result".to_string()),
                idempotency_key: None,
            }),
        ];
        let projection = project_workflows(events.iter());
        let run = &projection.workflows["wf_1"];
        assert_eq!(run.status, "outcome.finished");
        assert!(run.terminal);
        assert_eq!(run.owner, "leader");
    }

    #[test]
    fn continuation_metadata_derives_workflow_schedule_and_stop_state() {
        let events = [
            start_event(),
            EventV1::ContinuationStarted(ContinuationStartedEvent {
                continuation_id: "cont_1".to_string(),
                mode: "ralph".to_string(),
                command: "/ralph-loop".to_string(),
                max_iterations: 4,
                max_wall_clock_ms: 100,
                max_provider_calls: 8,
                max_tool_calls: 16,
                workflow: Some(WorkflowEventMetadata {
                    workflow_id: Some("wf_1".to_string()),
                    lane: Some("lane.delivery".to_string()),
                    iteration: Some(0),
                    stop_reason: None,
                    evidence_category: None,
                    owner: Some("leader".to_string()),
                }),
            }),
            EventV1::ContinuationReminderQueued(ContinuationReminderQueuedEvent {
                continuation_id: "cont_1".to_string(),
                iteration: 2,
                reminder: "continue".to_string(),
                reason: "idle".to_string(),
                workflow: None,
            }),
            EventV1::ContinuationStopped(ContinuationStoppedEvent {
                continuation_id: "cont_1".to_string(),
                reason: "done_marker".to_string(),
                workflow: Some(WorkflowEventMetadata {
                    workflow_id: Some("wf_1".to_string()),
                    lane: Some("lane.delivery".to_string()),
                    iteration: Some(2),
                    stop_reason: Some("acceptance_met".to_string()),
                    evidence_category: Some("evidence.verification".to_string()),
                    owner: Some("leader".to_string()),
                }),
            }),
        ];
        let projection = project_workflows(events.iter());
        let continuation = &projection.continuations["cont_1"];
        assert_eq!(continuation.workflow_id, "wf_1");
        assert_eq!(continuation.lane.as_deref(), Some("lane.delivery"));
        assert_eq!(continuation.iteration, 2);
        assert_eq!(continuation.status, "stopped");
        assert_eq!(continuation.stop_reason.as_deref(), Some("acceptance_met"));
    }

    #[test]
    fn context_snapshot_refs_survive_replay_without_workspace_reads() {
        let events = [
            start_event(),
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: "wf_1".to_string(),
                category: crate::context_snapshot::CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY.to_string(),
                summary: "context snapshot captured".to_string(),
                artifact_path: Some("artifacts/context_snapshots/ctx_123.json".to_string()),
                artifact_digest: Some("digest123".to_string()),
                acceptance_ref: Some("ctx_123".to_string()),
                metadata: std::collections::BTreeMap::from([
                    ("snapshot_id".to_string(), "ctx_123".to_string()),
                    ("snapshot_slug".to_string(), "ship-workflow".to_string()),
                    ("ambiguity_score".to_string(), "0.420".to_string()),
                ]),
            }),
        ];

        let projection = project_workflows(events.iter());
        let snapshot = projection.workflows["wf_1"]
            .context_snapshot
            .as_ref()
            .expect("snapshot ref projected from events");
        assert_eq!(snapshot.snapshot_id, "ctx_123");
        assert_eq!(snapshot.slug, "ship-workflow");
        assert_eq!(
            snapshot.artifact_path,
            "artifacts/context_snapshots/ctx_123.json"
        );
        assert_eq!(snapshot.ambiguity_score.as_deref(), Some("0.420"));
    }

    #[test]
    fn projection_only_reads_are_repeatable() {
        let events = [start_event()];
        let first: WorkflowProjection = project_workflows(events.iter());
        let second: WorkflowProjection = project_workflows(events.iter());
        assert_eq!(first, second);
    }
}

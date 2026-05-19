use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::event::{
    ContinuationLimitReachedEvent, ContinuationReminderQueuedEvent, ContinuationStartedEvent,
    ContinuationStoppedEvent, EventV1, TeamCreatedEvent, TeamDeletedEvent, TeamMemberSpawnedEvent,
    TeamMessageSentEvent, TeamShutdownApprovedEvent, TeamShutdownRejectedEvent,
    TeamShutdownRequestedEvent, TeamTaskCreatedEvent, TeamTaskUpdatedEvent, WorkflowCompletedEvent,
    WorkflowEvidenceRecordedEvent, WorkflowOperatorDecisionRecordedEvent, WorkflowStartedEvent,
    WorkflowTransitionDeniedEvent, WorkflowTransitionRecordedEvent,
};
use crate::event::{PersistentTaskStatus, WorkflowEventMetadata};
use crate::goal_ledger::GoalLedgerProjection;
use crate::persistent_task::PersistentTaskProjection;
use crate::plan_consensus::PlanConsensusProjection;
use crate::research_mission::ResearchMissionProjection;
use crate::workflow_review::{code_review_verdict_from_evidence, CodeReviewVerdict};

pub const SIMULATED_TOOL_EVIDENCE_CATEGORY: &str = "evidence.simulated_tool_result";
pub const SIGNOFF_WAIVER_DECISION: &str = "waive-missing-evidence";
pub const PENDING_TASK_WAIVER_DECISION: &str = "waive-pending-workflow-tasks";
pub const WORKFLOW_TASK_METADATA_KEY: &str = "workflow_id";
pub const WORKFLOW_QUESTION_EVIDENCE_CATEGORY: &str = "evidence.question";
pub const WORKFLOW_QUESTION_STATUS_ASKED: &str = "asked";
pub const WORKFLOW_QUESTION_STATUS_ANSWERED: &str = "answered";
pub const WORKFLOW_QUESTION_STATUS_CLOSED: &str = "closed";
pub const WORKFLOW_QUESTION_STATUS_TIMED_OUT: &str = "timed_out";
pub const WORKFLOW_QUESTION_STATUS_ERROR: &str = "error";

pub const WORKFLOW_QUESTION_METADATA_ID: &str = "question_id";
pub const WORKFLOW_QUESTION_METADATA_STATUS: &str = "question_status";
pub const WORKFLOW_QUESTION_METADATA_REASON_CODE: &str = "reason_code";
pub const WORKFLOW_QUESTION_METADATA_PROMPT_REF: &str = "prompt_ref";
pub const WORKFLOW_QUESTION_METADATA_ANSWER_REF: &str = "answer_ref";
const TEAM_METADATA_WORKFLOW_ID: &str = "workflow_id";
const TEAM_METADATA_EVIDENCE_REF: &str = "evidence_ref";
const TEAM_METADATA_VERIFICATION_EVIDENCE_REF: &str = "verification_evidence_ref";
const TEAM_METADATA_SYNTHESIS_REF: &str = "synthesis_ref";
const TEAM_METADATA_ABORT_REASON: &str = "abort_reason";
const TEAM_METADATA_BLOCKER_REF: &str = "blocker_ref";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowRunProjection {
    pub workflow_id: String,
    pub mode: String,
    pub owner: String,
    pub status: String,
    pub terminal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_verdict: Option<CodeReviewVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_to_ralplan_reason: Option<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operator_decision_records: Vec<WorkflowOperatorDecisionRecordedEvent>,
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
    pub mode: String,
    pub command: String,
    pub status: String,
    pub iteration: u32,
    pub max_iterations: u32,
    pub max_wall_clock_ms: u64,
    pub max_provider_calls: u32,
    pub max_tool_calls: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reminder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_schedule_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowQuestionProjection {
    pub question_id: String,
    pub workflow_id: String,
    pub status: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_ref: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowTeamCloseoutProjection {
    pub team_run_id: String,
    pub workflow_id: String,
    pub status: String,
    #[serde(default)]
    pub task_statuses: BTreeMap<String, String>,
    #[serde(default)]
    pub verification_evidence_refs: Vec<String>,
    #[serde(default)]
    pub synthesis_refs: Vec<String>,
    #[serde(default)]
    pub blocker_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowProjection {
    pub workflows: BTreeMap<String, WorkflowRunProjection>,
    pub idempotency_index: BTreeMap<String, String>,
    pub evidence: BTreeMap<String, Vec<WorkflowEvidenceRecordedEvent>>,
    pub denied_transitions: Vec<WorkflowTransitionDeniedEvent>,
    pub continuations: BTreeMap<String, WorkflowContinuationProjection>,
    pub context_snapshots: BTreeMap<String, WorkflowContextSnapshotRef>,
    #[serde(default)]
    pub questions: BTreeMap<String, WorkflowQuestionProjection>,
    #[serde(default)]
    pub teams: BTreeMap<String, WorkflowTeamCloseoutProjection>,
    #[serde(default)]
    pub plan_consensus: BTreeMap<String, PlanConsensusProjection>,
    #[serde(default)]
    pub goal_ledger: GoalLedgerProjection,
    #[serde(default)]
    pub research_missions: ResearchMissionProjection,
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
            EventV1::TeamCreated(payload) => self.apply_team_created(payload),
            EventV1::TeamMemberSpawned(payload) => self.apply_team_member_spawned(payload),
            EventV1::TeamMessageSent(payload) => self.apply_team_message(payload),
            EventV1::TeamTaskCreated(payload) => self.apply_team_task_created(payload),
            EventV1::TeamTaskUpdated(payload) => self.apply_team_task_updated(payload),
            EventV1::TeamShutdownRequested(payload) => self.apply_team_shutdown_requested(payload),
            EventV1::TeamShutdownApproved(payload) => self.apply_team_shutdown_approved(payload),
            EventV1::TeamShutdownRejected(payload) => self.apply_team_shutdown_rejected(payload),
            EventV1::TeamDeleted(payload) => self.apply_team_deleted(payload),
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
                    phase: default_phase_for_mode(&payload.mode),
                    review_verdict: None,
                    return_to_ralplan_reason: None,
                    lane: payload.lane.clone(),
                    title: payload.title.clone(),
                    idempotency_key: payload.idempotency_key.clone(),
                    evidence_categories: BTreeSet::new(),
                    operator_decisions: Vec::new(),
                    operator_decision_records: Vec::new(),
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
        run.phase = Some(phase_from_status(&payload.to_status));
        run.owner = payload.owner.clone();
    }

    fn apply_denied(&mut self, payload: &WorkflowTransitionDeniedEvent) {
        if let Some(run) = self.workflows.get_mut(&payload.workflow_id) {
            run.denied_transition_count = run.denied_transition_count.saturating_add(1);
        }
        self.denied_transitions.push(payload.clone());
    }

    fn apply_evidence(&mut self, payload: &WorkflowEvidenceRecordedEvent) {
        crate::plan_consensus::apply_plan_consensus_evidence(&mut self.plan_consensus, payload);
        crate::goal_ledger::apply_goal_ledger_evidence(&mut self.goal_ledger, payload);
        crate::research_mission::apply_research_mission_evidence(
            &mut self.research_missions,
            payload,
        );
        self.apply_question_evidence(payload);
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
            if let Some(phase) = phase_from_evidence_metadata(&payload.metadata) {
                run.phase = Some(phase);
            }
            if payload.category == crate::workflow_registry::REVIEW_EVIDENCE_CATEGORY {
                if let Some(verdict) =
                    code_review_verdict_from_evidence(&payload.summary, &payload.metadata)
                {
                    run.review_verdict = Some(verdict);
                }
                if let Some(reason) = payload
                    .metadata
                    .get(crate::workflow_review::RETURN_TO_RALPLAN_REASON_METADATA_KEY)
                    .map(String::as_str)
                    .map(str::trim)
                    .filter(|reason| !reason.is_empty())
                {
                    run.return_to_ralplan_reason = Some(reason.to_string());
                }
            }
            if let Some(snapshot_ref) = snapshot_ref {
                run.context_snapshot = Some(snapshot_ref);
            }
        }
        self.evidence
            .entry(payload.workflow_id.clone())
            .or_default()
            .push(payload.clone());
    }

    fn apply_question_evidence(&mut self, payload: &WorkflowEvidenceRecordedEvent) {
        if payload.category != WORKFLOW_QUESTION_EVIDENCE_CATEGORY {
            return;
        }
        let question_id = payload
            .metadata
            .get(WORKFLOW_QUESTION_METADATA_ID)
            .cloned()
            .or_else(|| payload.acceptance_ref.clone())
            .unwrap_or_else(|| payload.workflow_id.clone());
        self.questions.insert(
            question_id.clone(),
            WorkflowQuestionProjection {
                question_id,
                workflow_id: payload.workflow_id.clone(),
                status: payload
                    .metadata
                    .get(WORKFLOW_QUESTION_METADATA_STATUS)
                    .cloned()
                    .unwrap_or_else(|| WORKFLOW_QUESTION_STATUS_ASKED.to_string()),
                summary: payload.summary.clone(),
                reason_code: payload
                    .metadata
                    .get(WORKFLOW_QUESTION_METADATA_REASON_CODE)
                    .cloned(),
                prompt_ref: payload
                    .metadata
                    .get(WORKFLOW_QUESTION_METADATA_PROMPT_REF)
                    .cloned(),
                answer_ref: payload
                    .metadata
                    .get(WORKFLOW_QUESTION_METADATA_ANSWER_REF)
                    .cloned(),
            },
        );
    }

    fn apply_operator_decision(&mut self, payload: &WorkflowOperatorDecisionRecordedEvent) {
        if let Some(run) = self.workflows.get_mut(&payload.workflow_id) {
            run.operator_decisions.push(payload.decision.clone());
            run.operator_decision_records.push(payload.clone());
        }
    }

    fn apply_completed(&mut self, payload: &WorkflowCompletedEvent) {
        if let Some(run) = self.workflows.get_mut(&payload.workflow_id) {
            if run.terminal {
                return;
            }
            run.status = payload.outcome.clone();
            run.phase = Some(phase_from_status(&payload.outcome));
            run.owner = payload.owner.clone();
            run.terminal = true;
        }
    }

    fn apply_team_created(&mut self, payload: &TeamCreatedEvent) {
        let Some(workflow_id) =
            workflow_id_from_metadata(payload.workflow.as_ref()).or_else(|| {
                payload
                    .spec
                    .metadata
                    .get(TEAM_METADATA_WORKFLOW_ID)
                    .cloned()
            })
        else {
            return;
        };
        let team = self
            .teams
            .entry(payload.team_run_id.clone())
            .or_insert_with(|| WorkflowTeamCloseoutProjection {
                team_run_id: payload.team_run_id.clone(),
                workflow_id: workflow_id.clone(),
                status: "active".to_string(),
                ..WorkflowTeamCloseoutProjection::default()
            });
        team.workflow_id = workflow_id;
        team.status = "active".to_string();
        merge_team_metadata(team, &payload.spec.metadata);
    }

    fn apply_team_member_spawned(&mut self, payload: &TeamMemberSpawnedEvent) {
        if let Some(team) = self.team_from_optional_metadata_mut(
            &payload.team_run_id,
            payload.workflow.as_ref(),
            None,
        ) {
            team.status = "active".to_string();
        }
    }

    fn apply_team_message(&mut self, payload: &TeamMessageSentEvent) {
        if let Some(team) = self.team_from_optional_metadata_mut(
            &payload.team_run_id,
            payload.workflow.as_ref(),
            None,
        ) {
            for reference in &payload.message.references {
                merge_unique(
                    &mut team.synthesis_refs,
                    std::iter::once(reference.path.clone()),
                );
            }
        }
    }

    fn apply_team_task_created(&mut self, payload: &TeamTaskCreatedEvent) {
        if let Some(team) = self.team_from_optional_metadata_mut(
            &payload.team_run_id,
            payload.workflow.as_ref(),
            Some(&payload.task.metadata),
        ) {
            team.task_statuses.insert(
                payload.task.task_id.clone(),
                payload.task.status.as_str().to_string(),
            );
            merge_team_metadata(team, &payload.task.metadata);
            merge_unique(&mut team.blocker_refs, payload.task.blocked_by.clone());
        }
    }

    fn apply_team_task_updated(&mut self, payload: &TeamTaskUpdatedEvent) {
        if let Some(team) = self.team_from_optional_metadata_mut(
            &payload.team_run_id,
            payload.workflow.as_ref(),
            Some(&payload.metadata),
        ) {
            team.task_statuses
                .insert(payload.task_id.clone(), payload.status.as_str().to_string());
            merge_team_metadata(team, &payload.metadata);
        }
    }

    fn apply_team_shutdown_requested(&mut self, payload: &TeamShutdownRequestedEvent) {
        if let Some(team) = self.team_from_optional_metadata_mut(
            &payload.team_run_id,
            payload.workflow.as_ref(),
            None,
        ) {
            team.status = "shutdown_requested".to_string();
        }
    }

    fn apply_team_shutdown_approved(&mut self, payload: &TeamShutdownApprovedEvent) {
        if let Some(team) = self.team_from_optional_metadata_mut(
            &payload.team_run_id,
            payload.workflow.as_ref(),
            Some(&payload.metadata),
        ) {
            team.status = "shutdown_approved".to_string();
            merge_team_metadata(team, &payload.metadata);
        }
    }

    fn apply_team_shutdown_rejected(&mut self, payload: &TeamShutdownRejectedEvent) {
        if let Some(team) = self.team_from_optional_metadata_mut(
            &payload.team_run_id,
            payload.workflow.as_ref(),
            None,
        ) {
            team.status = "shutdown_rejected".to_string();
            merge_unique(
                &mut team.blocker_refs,
                std::iter::once(format!("shutdown_rejected:{}", payload.member_name)),
            );
        }
    }

    fn apply_team_deleted(&mut self, payload: &TeamDeletedEvent) {
        if let Some(team) = self.team_from_optional_metadata_mut(
            &payload.team_run_id,
            payload.workflow.as_ref(),
            Some(&payload.metadata),
        ) {
            team.status = "deleted".to_string();
            merge_team_metadata(team, &payload.metadata);
        }
    }

    fn team_from_optional_metadata_mut(
        &mut self,
        team_run_id: &str,
        workflow: Option<&WorkflowEventMetadata>,
        metadata: Option<&BTreeMap<String, String>>,
    ) -> Option<&mut WorkflowTeamCloseoutProjection> {
        let workflow_id = workflow_id_from_metadata(workflow)
            .or_else(|| {
                metadata.and_then(|metadata| metadata.get(TEAM_METADATA_WORKFLOW_ID).cloned())
            })
            .or_else(|| {
                self.teams
                    .get(team_run_id)
                    .map(|team| team.workflow_id.clone())
            })?;
        let team = self
            .teams
            .entry(team_run_id.to_string())
            .or_insert_with(|| WorkflowTeamCloseoutProjection {
                team_run_id: team_run_id.to_string(),
                workflow_id: workflow_id.clone(),
                status: "active".to_string(),
                ..WorkflowTeamCloseoutProjection::default()
            });
        team.workflow_id = workflow_id;
        Some(team)
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
                mode: payload.mode.clone(),
                command: payload.command.clone(),
                status: "active".to_string(),
                iteration: metadata.iteration.unwrap_or(0),
                max_iterations: payload.max_iterations,
                max_wall_clock_ms: payload.max_wall_clock_ms,
                max_provider_calls: payload.max_provider_calls,
                max_tool_calls: payload.max_tool_calls,
                lane: metadata.lane.clone(),
                owner: metadata.owner.clone(),
                evidence_category: metadata.evidence_category.clone(),
                last_reminder: None,
                last_schedule_reason: None,
                limit: None,
                stop_reason: None,
            },
        );
    }

    fn apply_continuation_reminder(&mut self, payload: &ContinuationReminderQueuedEvent) {
        if let Some(continuation) = self.continuations.get_mut(&payload.continuation_id) {
            continuation.status = "reminder_queued".to_string();
            continuation.iteration = payload.iteration;
            continuation.last_reminder = Some(payload.reminder.clone());
            continuation.last_schedule_reason = Some(payload.reason.clone());
            merge_workflow_metadata(continuation, payload.workflow.as_ref());
        }
    }

    fn apply_continuation_stopped(&mut self, payload: &ContinuationStoppedEvent) {
        if let Some(continuation) = self.continuations.get_mut(&payload.continuation_id) {
            continuation.status = "stopped".to_string();
            merge_workflow_metadata(continuation, payload.workflow.as_ref());
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
            continuation.limit = Some(payload.limit.clone());
            merge_workflow_metadata(continuation, payload.workflow.as_ref());
            continuation.stop_reason = Some(payload.limit.clone());
        }
    }
}

fn merge_workflow_metadata(
    continuation: &mut WorkflowContinuationProjection,
    metadata: Option<&WorkflowEventMetadata>,
) {
    let Some(metadata) = metadata else {
        return;
    };
    if let Some(lane) = metadata.lane.as_ref() {
        continuation.lane = Some(lane.clone());
    }
    if let Some(owner) = metadata.owner.as_ref() {
        continuation.owner = Some(owner.clone());
    }
    if let Some(evidence_category) = metadata.evidence_category.as_ref() {
        continuation.evidence_category = Some(evidence_category.clone());
    }
}

fn workflow_id_from_metadata(metadata: Option<&WorkflowEventMetadata>) -> Option<String> {
    metadata
        .and_then(|metadata| metadata.workflow_id.as_ref())
        .map(|workflow_id| workflow_id.trim())
        .filter(|workflow_id| !workflow_id.is_empty())
        .map(str::to_string)
}

fn default_phase_for_mode(mode: &str) -> Option<String> {
    let phase = match crate::workflow_transitions::normalize_workflow_mode(mode) {
        Some("deep-interview") => "interviewing",
        Some("ralplan") => "planning",
        Some("autopilot") => "planning",
        Some("autoresearch") => "researching",
        Some("team") => "coordinating",
        Some("ralph") => "executing",
        Some("ultrawork") => "executing",
        Some("ultraqa") => "testing",
        _ if mode.trim().is_empty() => return None,
        _ => "active",
    };
    Some(phase.to_string())
}

fn phase_from_status(status: &str) -> String {
    let phase = status
        .trim()
        .strip_prefix("outcome.")
        .unwrap_or(status.trim());
    if phase.is_empty() {
        "unknown".to_string()
    } else {
        phase.replace('_', "-")
    }
}

fn phase_from_evidence_metadata(metadata: &BTreeMap<String, String>) -> Option<String> {
    ["phase", "current_phase", "workflow_phase", "status"]
        .iter()
        .find_map(|key| non_empty_phase(metadata.get(*key)))
        .or_else(|| {
            metadata
                .iter()
                .filter(|(key, _)| {
                    key.ends_with("_status")
                        && key.as_str() != WORKFLOW_QUESTION_METADATA_STATUS
                        && key.as_str() != "question_status"
                })
                .find_map(|(_, value)| non_empty_phase(Some(value)))
        })
}

fn non_empty_phase(value: Option<&String>) -> Option<String> {
    value
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.strip_prefix("outcome.").unwrap_or(value))
        .map(|value| value.replace('_', "-"))
}

fn merge_team_metadata(
    team: &mut WorkflowTeamCloseoutProjection,
    metadata: &BTreeMap<String, String>,
) {
    merge_unique(
        &mut team.verification_evidence_refs,
        metadata_refs(
            metadata,
            &[
                TEAM_METADATA_EVIDENCE_REF,
                TEAM_METADATA_VERIFICATION_EVIDENCE_REF,
            ],
        ),
    );
    merge_unique(
        &mut team.synthesis_refs,
        metadata_refs(
            metadata,
            &[TEAM_METADATA_SYNTHESIS_REF, "lead_synthesis_ref"],
        ),
    );
    merge_unique(
        &mut team.blocker_refs,
        metadata_refs(metadata, &[TEAM_METADATA_BLOCKER_REF]),
    );
    if team.abort_reason.is_none() {
        team.abort_reason = metadata.get(TEAM_METADATA_ABORT_REASON).cloned();
    }
}

fn metadata_refs(metadata: &BTreeMap<String, String>, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter_map(|key| metadata.get(*key))
        .flat_map(|value| value.split([',', '\n']))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn merge_unique(values: &mut Vec<String>, refs: impl IntoIterator<Item = String>) {
    for value in refs {
        if !values.contains(&value) {
            values.push(value);
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
pub struct WorkflowTransitionRequest {
    pub workflow_id: String,
    pub to_status: String,
    pub reason: String,
    pub owner: String,
    pub policy_id: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowEvidenceRequest {
    pub workflow_id: String,
    pub category: String,
    pub summary: String,
    pub artifact_path: Option<String>,
    pub artifact_digest: Option<String>,
    pub acceptance_ref: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowStartDecision {
    Start(WorkflowStartedEvent),
    Existing { workflow_id: String },
    Denied(WorkflowTransitionDeniedEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WorkflowStartResult {
    Started {
        workflow_id: String,
    },
    Existing {
        workflow_id: String,
    },
    Denied {
        workflow_id: String,
        reason: String,
        policy_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowSignoffReadiness {
    pub workflow_id: String,
    pub allowed: bool,
    pub waived: bool,
    pub required_evidence_categories: Vec<String>,
    pub present_evidence_categories: Vec<String>,
    pub missing_evidence_categories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowTaskReadiness {
    pub workflow_id: String,
    pub allowed: bool,
    pub waived: bool,
    pub pending_task_ids: Vec<String>,
    pub claimed_task_ids: Vec<String>,
    pub in_progress_task_ids: Vec<String>,
}

impl WorkflowTaskReadiness {
    pub fn incomplete_task_ids(&self) -> Vec<String> {
        self.pending_task_ids
            .iter()
            .chain(self.claimed_task_ids.iter())
            .chain(self.in_progress_task_ids.iter())
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowCompletionReadiness {
    pub workflow_id: String,
    pub allowed: bool,
    pub signoff: WorkflowSignoffReadiness,
    pub tasks: WorkflowTaskReadiness,
    pub active_continuation_ids: Vec<String>,
    pub missing_quality_gates: Vec<String>,
    pub recovery_hints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowSignoffPolicy {
    required_evidence_categories: Vec<String>,
}

impl WorkflowSignoffPolicy {
    pub fn new(required_evidence_categories: impl IntoIterator<Item = String>) -> Self {
        Self {
            required_evidence_categories: required_evidence_categories.into_iter().collect(),
        }
    }

    pub fn simulator_default() -> Self {
        Self::new([
            crate::context_snapshot::CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY.to_string(),
            SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string(),
        ])
    }

    pub fn required_evidence_categories(&self) -> &[String] {
        &self.required_evidence_categories
    }

    pub fn evaluate(
        &self,
        projection: &WorkflowProjection,
        workflow_id: impl Into<String>,
    ) -> WorkflowSignoffReadiness {
        projection.signoff_readiness(workflow_id, &self.required_evidence_categories)
    }
}

impl WorkflowProjection {
    pub fn signoff_readiness(
        &self,
        workflow_id: impl Into<String>,
        required_evidence_categories: &[String],
    ) -> WorkflowSignoffReadiness {
        let workflow_id = workflow_id.into();
        let present = self
            .workflows
            .get(&workflow_id)
            .map(|run| run.evidence_categories.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let present_set = present.iter().cloned().collect::<BTreeSet<_>>();
        let missing = required_evidence_categories
            .iter()
            .filter(|category| !present_set.contains(*category))
            .cloned()
            .collect::<Vec<_>>();
        let waived = self.workflows.get(&workflow_id).is_some_and(|run| {
            run.operator_decisions
                .iter()
                .any(|decision| decision == SIGNOFF_WAIVER_DECISION)
        });

        WorkflowSignoffReadiness {
            workflow_id,
            allowed: missing.is_empty() || waived,
            waived,
            required_evidence_categories: required_evidence_categories.to_vec(),
            present_evidence_categories: present,
            missing_evidence_categories: missing,
        }
    }

    pub fn task_readiness(
        &self,
        workflow_id: impl Into<String>,
        persistent_tasks: &PersistentTaskProjection,
    ) -> WorkflowTaskReadiness {
        let workflow_id = workflow_id.into();
        let waived = self.workflows.get(&workflow_id).is_some_and(|run| {
            run.operator_decisions
                .iter()
                .any(|decision| decision == PENDING_TASK_WAIVER_DECISION)
        });
        let mut pending_task_ids = Vec::new();
        let mut claimed_task_ids = Vec::new();
        let mut in_progress_task_ids = Vec::new();
        for task in persistent_tasks.tasks.values() {
            if task
                .metadata
                .get(WORKFLOW_TASK_METADATA_KEY)
                .is_none_or(|candidate| candidate != &workflow_id)
            {
                continue;
            }
            match task.status {
                PersistentTaskStatus::Pending => pending_task_ids.push(task.task_id.clone()),
                PersistentTaskStatus::Claimed => claimed_task_ids.push(task.task_id.clone()),
                PersistentTaskStatus::InProgress => in_progress_task_ids.push(task.task_id.clone()),
                PersistentTaskStatus::Completed | PersistentTaskStatus::Cancelled => {}
            }
        }
        WorkflowTaskReadiness {
            workflow_id,
            allowed: waived
                || (pending_task_ids.is_empty()
                    && claimed_task_ids.is_empty()
                    && in_progress_task_ids.is_empty()),
            waived,
            pending_task_ids,
            claimed_task_ids,
            in_progress_task_ids,
        }
    }

    pub fn completion_readiness(
        &self,
        workflow_id: impl Into<String>,
        persistent_tasks: &PersistentTaskProjection,
        signoff_policy: &WorkflowSignoffPolicy,
    ) -> WorkflowCompletionReadiness {
        let workflow_id = workflow_id.into();
        let signoff = signoff_policy.evaluate(self, workflow_id.clone());
        let tasks = self.task_readiness(workflow_id.clone(), persistent_tasks);
        let active_continuation_ids = self
            .continuations
            .values()
            .filter(|continuation| continuation.workflow_id == workflow_id)
            .filter(|continuation| {
                continuation.status == "active" || continuation.status == "reminder_queued"
            })
            .map(|continuation| continuation.continuation_id.clone())
            .collect::<Vec<_>>();
        let mut missing_quality_gates = Vec::new();
        let mut recovery_hints = Vec::new();
        if !signoff.allowed {
            missing_quality_gates.push("signoff_evidence".to_string());
            recovery_hints.push(format!(
                "record evidence for: {} or append `{SIGNOFF_WAIVER_DECISION}`",
                signoff.missing_evidence_categories.join(", ")
            ));
        }
        if !tasks.allowed {
            missing_quality_gates.push("workflow_tasks_complete".to_string());
            recovery_hints.push(format!(
                "complete/cancel workflow-owned tasks: {} or append `{PENDING_TASK_WAIVER_DECISION}`",
                tasks.incomplete_task_ids().join(", ")
            ));
        }
        if !active_continuation_ids.is_empty() {
            missing_quality_gates.push("continuation_stopped".to_string());
            recovery_hints.push(format!(
                "stop or resolve active workflow continuations: {}",
                active_continuation_ids.join(", ")
            ));
        }

        WorkflowCompletionReadiness {
            workflow_id,
            allowed: signoff.allowed && tasks.allowed && active_continuation_ids.is_empty(),
            signoff,
            tasks,
            active_continuation_ids,
            missing_quality_gates,
            recovery_hints,
        }
    }

    pub fn closeout_readiness(
        &self,
        workflow_id: impl Into<String>,
        persistent_tasks: &PersistentTaskProjection,
        signoff_policy: &WorkflowSignoffPolicy,
        closeout_policy: &crate::workflow_closeout::WorkflowCloseoutPolicy,
    ) -> crate::workflow_closeout::WorkflowCloseoutReadiness {
        closeout_policy.evaluate(self, workflow_id, persistent_tasks, signoff_policy)
    }
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
        project_workflows, WorkflowProjection, WorkflowSignoffPolicy, WorkflowStartDecision,
        WorkflowStartRequest, WorkflowTransitionPolicy, PENDING_TASK_WAIVER_DECISION,
        SIGNOFF_WAIVER_DECISION, SIMULATED_TOOL_EVIDENCE_CATEGORY, WORKFLOW_TASK_METADATA_KEY,
    };
    use crate::event::{
        ContinuationReminderQueuedEvent, ContinuationStartedEvent, ContinuationStoppedEvent,
        EventV1, PersistentTask, PersistentTaskStatus, WorkflowCompletedEvent,
        WorkflowEventMetadata, WorkflowEvidenceRecordedEvent,
        WorkflowOperatorDecisionRecordedEvent, WorkflowStartedEvent,
        WorkflowTransitionRecordedEvent,
    };
    use crate::persistent_task::PersistentTaskProjection;

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
    fn terminal_late_completion_does_not_mutate_terminal_outcome() {
        let events = [
            start_event(),
            EventV1::WorkflowCompleted(WorkflowCompletedEvent {
                workflow_id: "wf_1".to_string(),
                outcome: "outcome.finished".to_string(),
                reason: "done".to_string(),
                owner: "leader".to_string(),
            }),
            EventV1::WorkflowCompleted(WorkflowCompletedEvent {
                workflow_id: "wf_1".to_string(),
                outcome: "outcome.failed".to_string(),
                reason: "late failure".to_string(),
                owner: "late-worker".to_string(),
            }),
        ];
        let projection = project_workflows(events.iter());
        let run = &projection.workflows["wf_1"];
        assert_eq!(run.status, "outcome.finished");
        assert!(run.terminal);
        assert_eq!(run.owner, "leader");
    }

    #[test]
    fn workflow_projection_tracks_phase_from_mode_evidence_and_completion() {
        let events = [
            EventV1::WorkflowStarted(WorkflowStartedEvent {
                workflow_id: "wf_phase".to_string(),
                mode: "ralph".to_string(),
                owner: "leader".to_string(),
                lane: None,
                title: None,
                idempotency_key: None,
            }),
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: "wf_phase".to_string(),
                category: "evidence.review".to_string(),
                summary: "architect review running".to_string(),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: None,
                metadata: std::collections::BTreeMap::from([(
                    "current_phase".to_string(),
                    "architect_verifying".to_string(),
                )]),
            }),
        ];

        let projection = project_workflows(events.iter());
        let run = &projection.workflows["wf_phase"];
        assert_eq!(run.phase.as_deref(), Some("architect-verifying"));

        let completed = EventV1::WorkflowCompleted(WorkflowCompletedEvent {
            workflow_id: "wf_phase".to_string(),
            outcome: "outcome.finished".to_string(),
            reason: "done".to_string(),
            owner: "leader".to_string(),
        });
        let projection = project_workflows(events.iter().chain([completed].iter()));
        let run = &projection.workflows["wf_phase"];
        assert_eq!(run.phase.as_deref(), Some("finished"));
    }

    #[test]
    fn workflow_projection_materializes_review_verdict_and_loopback_reason() {
        let events = [
            EventV1::WorkflowStarted(WorkflowStartedEvent {
                workflow_id: "wf_autopilot".to_string(),
                mode: "workflow.autopilot".to_string(),
                owner: "leader".to_string(),
                lane: None,
                title: None,
                idempotency_key: None,
            }),
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: "wf_autopilot".to_string(),
                category: crate::workflow_registry::REVIEW_EVIDENCE_CATEGORY.to_string(),
                summary: r#"{"recommendation":"REQUEST CHANGES","architectural_status":"WATCH","findings":["fix tests"]}"#.to_string(),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: Some("review".to_string()),
                metadata: std::collections::BTreeMap::from([
                    ("phase".to_string(), "ralplan".to_string()),
                    (
                        crate::workflow_review::RETURN_TO_RALPLAN_REASON_METADATA_KEY.to_string(),
                        "tests failed".to_string(),
                    ),
                ]),
            }),
        ];

        let projection = project_workflows(events.iter());
        let run = &projection.workflows["wf_autopilot"];
        let verdict = run.review_verdict.as_ref().expect("review verdict");
        assert_eq!(verdict.recommendation, "REQUEST_CHANGES");
        assert_eq!(verdict.architectural_status, "WATCH");
        assert_eq!(run.phase.as_deref(), Some("ralplan"));
        assert_eq!(
            run.return_to_ralplan_reason.as_deref(),
            Some("tests failed")
        );
    }

    #[test]
    fn workflow_projection_sets_default_phase_for_started_mode() {
        let projection = project_workflows(
            [EventV1::WorkflowStarted(WorkflowStartedEvent {
                workflow_id: "wf_ralph".to_string(),
                mode: "ralph".to_string(),
                owner: "leader".to_string(),
                lane: None,
                title: None,
                idempotency_key: None,
            })]
            .iter(),
        );

        assert_eq!(
            projection.workflows["wf_ralph"].phase.as_deref(),
            Some("executing")
        );
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
        assert_eq!(continuation.mode, "ralph");
        assert_eq!(continuation.command, "/ralph-loop");
        assert_eq!(continuation.max_iterations, 4);
        assert_eq!(continuation.max_provider_calls, 8);
        assert_eq!(continuation.lane.as_deref(), Some("lane.delivery"));
        assert_eq!(continuation.iteration, 2);
        assert_eq!(continuation.status, "stopped");
        assert_eq!(continuation.last_reminder.as_deref(), Some("continue"));
        assert_eq!(continuation.last_schedule_reason.as_deref(), Some("idle"));
        assert_eq!(
            continuation.evidence_category.as_deref(),
            Some("evidence.verification")
        );
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
    fn signoff_policy_requires_mapped_evidence_or_waiver() {
        let events = [
            start_event(),
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: "wf_1".to_string(),
                category: crate::context_snapshot::CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY.to_string(),
                summary: "context snapshot captured".to_string(),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: Some("ctx_1".to_string()),
                metadata: std::collections::BTreeMap::new(),
            }),
        ];
        let projection = project_workflows(events.iter());
        let policy = WorkflowSignoffPolicy::simulator_default();

        let readiness = policy.evaluate(&projection, "wf_1");
        assert!(!readiness.allowed);
        assert_eq!(
            readiness.missing_evidence_categories,
            vec![SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string()]
        );

        let waiver =
            EventV1::WorkflowOperatorDecisionRecorded(WorkflowOperatorDecisionRecordedEvent {
                workflow_id: "wf_1".to_string(),
                decision: SIGNOFF_WAIVER_DECISION.to_string(),
                operator: "operator".to_string(),
                reason: Some("accepted missing simulator evidence".to_string()),
                correlation_id: None,
            });
        let waived_projection = project_workflows(events.iter().chain([waiver].iter()));
        let waived = policy.evaluate(&waived_projection, "wf_1");
        assert!(waived.allowed);
        assert!(waived.waived);
    }

    #[test]
    fn completion_readiness_blocks_pending_workflow_tasks_until_waived() {
        let events = [
            start_event(),
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: "wf_1".to_string(),
                category: crate::context_snapshot::CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY.to_string(),
                summary: "context snapshot captured".to_string(),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: Some("ctx_1".to_string()),
                metadata: std::collections::BTreeMap::new(),
            }),
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: "wf_1".to_string(),
                category: SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string(),
                summary: "simulated tool completed".to_string(),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: Some("acceptance.noop-tool".to_string()),
                metadata: std::collections::BTreeMap::new(),
            }),
        ];
        let projection = project_workflows(events.iter());
        let mut tasks = PersistentTaskProjection::default();
        tasks.tasks.insert(
            "pt_pending".to_string(),
            PersistentTask {
                version: 1,
                task_id: "pt_pending".to_string(),
                run_id: None,
                thread_id: None,
                subject: "verify workflow".to_string(),
                description: "pending workflow-owned task".to_string(),
                status: PersistentTaskStatus::Pending,
                active_form: None,
                owner: Some("leader".to_string()),
                blocks: Vec::new(),
                blocked_by: Vec::new(),
                metadata: std::collections::BTreeMap::from([(
                    WORKFLOW_TASK_METADATA_KEY.to_string(),
                    "wf_1".to_string(),
                )]),
            },
        );

        let readiness = projection.completion_readiness(
            "wf_1",
            &tasks,
            &WorkflowSignoffPolicy::simulator_default(),
        );
        assert!(!readiness.allowed);
        assert!(readiness.signoff.allowed);
        assert_eq!(readiness.tasks.pending_task_ids, vec!["pt_pending"]);
        assert_eq!(
            readiness.missing_quality_gates,
            vec!["workflow_tasks_complete"]
        );

        let waiver =
            EventV1::WorkflowOperatorDecisionRecorded(WorkflowOperatorDecisionRecordedEvent {
                workflow_id: "wf_1".to_string(),
                decision: PENDING_TASK_WAIVER_DECISION.to_string(),
                operator: "operator".to_string(),
                reason: Some("accept pending task risk".to_string()),
                correlation_id: None,
            });
        let waived_projection = project_workflows(events.iter().chain([waiver].iter()));
        let waived = waived_projection.completion_readiness(
            "wf_1",
            &tasks,
            &WorkflowSignoffPolicy::simulator_default(),
        );
        assert!(waived.allowed);
        assert!(waived.tasks.waived);
    }

    #[test]
    fn projection_only_reads_are_repeatable() {
        let events = [start_event()];
        let first: WorkflowProjection = project_workflows(events.iter());
        let second: WorkflowProjection = project_workflows(events.iter());
        assert_eq!(first, second);
    }
}

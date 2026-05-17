use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::event::WorkflowTransitionDeniedEvent;
use crate::persistent_task::PersistentTaskProjection;
use crate::workflow::{
    WorkflowCompletionReadiness, WorkflowContinuationProjection, WorkflowProjection,
    WorkflowSignoffPolicy, WorkflowSignoffReadiness,
};
use crate::workflow_closeout::{WorkflowCloseoutPolicy, WorkflowDossierCloseoutSection};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunDossier {
    pub workflows: Vec<WorkflowDossierEntry>,
    pub denied_transitions: Vec<WorkflowTransitionDeniedEvent>,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowDossierEntry {
    pub workflow_id: String,
    pub mode: String,
    pub owner: String,
    pub status: String,
    pub terminal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub evidence: Vec<WorkflowDossierEvidence>,
    pub operator_decisions: Vec<String>,
    pub signoff: WorkflowSignoffReadiness,
    pub completion: WorkflowCompletionReadiness,
    pub closeout: WorkflowDossierCloseoutSection,
    pub quality_gate: WorkflowDossierQualityGate,
    pub continuations: Vec<WorkflowDossierContinuation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowDossierEvidence {
    pub category: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowDossierQualityGate {
    pub passed: bool,
    pub prompt_to_artifact_complete: bool,
    pub missing: Vec<String>,
    pub recovery_hints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowDossierContinuation {
    pub continuation_id: String,
    pub mode: String,
    pub command: String,
    pub status: String,
    pub iteration: u32,
    pub max_iterations: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_schedule_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
}

pub fn build_run_dossier(
    projection: &WorkflowProjection,
    signoff_policy: &WorkflowSignoffPolicy,
) -> RunDossier {
    build_run_dossier_with_tasks(
        projection,
        &PersistentTaskProjection::default(),
        signoff_policy,
    )
}

pub fn build_run_dossier_with_tasks(
    projection: &WorkflowProjection,
    persistent_tasks: &PersistentTaskProjection,
    signoff_policy: &WorkflowSignoffPolicy,
) -> RunDossier {
    build_run_dossier_with_tasks_and_closeout_policy(
        projection,
        persistent_tasks,
        signoff_policy,
        &WorkflowCloseoutPolicy::default_policy(),
    )
}

pub fn build_run_dossier_with_tasks_and_closeout_policy(
    projection: &WorkflowProjection,
    persistent_tasks: &PersistentTaskProjection,
    signoff_policy: &WorkflowSignoffPolicy,
    closeout_policy: &WorkflowCloseoutPolicy,
) -> RunDossier {
    let workflows = projection
        .workflows
        .values()
        .map(|workflow| {
            let evidence = projection
                .evidence
                .get(&workflow.workflow_id)
                .into_iter()
                .flat_map(|events| events.iter())
                .map(|event| WorkflowDossierEvidence {
                    category: event.category.clone(),
                    summary: event.summary.clone(),
                    acceptance_ref: event.acceptance_ref.clone(),
                    artifact_path: event.artifact_path.clone(),
                    artifact_digest: event.artifact_digest.clone(),
                })
                .collect::<Vec<_>>();
            let completion = projection.completion_readiness(
                &workflow.workflow_id,
                persistent_tasks,
                signoff_policy,
            );
            let closeout_readiness = projection.closeout_readiness(
                &workflow.workflow_id,
                persistent_tasks,
                signoff_policy,
                closeout_policy,
            );
            let continuations = workflow_continuations(projection, &workflow.workflow_id);
            let prompt_to_artifact_complete = workflow.context_snapshot.is_some()
                && evidence
                    .iter()
                    .any(|evidence| evidence.acceptance_ref.is_some());
            let mut quality_missing = completion.missing_quality_gates.clone();
            let mut recovery_hints = completion.recovery_hints.clone();
            if !prompt_to_artifact_complete {
                quality_missing.push("prompt_to_artifact_audit".to_string());
                recovery_hints.push(
                    "record a context snapshot artifact and acceptance evidence refs before final signoff"
                        .to_string(),
                );
            }
            WorkflowDossierEntry {
                workflow_id: workflow.workflow_id.clone(),
                mode: workflow.mode.clone(),
                owner: workflow.owner.clone(),
                status: workflow.status.clone(),
                terminal: workflow.terminal,
                lane: workflow.lane.clone(),
                title: workflow.title.clone(),
                evidence,
                operator_decisions: workflow.operator_decisions.clone(),
                signoff: signoff_policy.evaluate(projection, workflow.workflow_id.clone()),
                closeout: WorkflowDossierCloseoutSection {
                    policy_id: closeout_readiness.policy_id,
                    policy_version: closeout_readiness.policy_version,
                    schema_version: closeout_readiness.schema_version,
                    matrix: closeout_readiness.dimensions,
                    legal_next_actions: closeout_readiness.legal_next_actions,
                    stale_export: closeout_readiness.stale_export,
                    require_export_artifact: closeout_policy.require_export_artifact,
                    overall_allowed: closeout_readiness.overall_allowed,
                },
                quality_gate: WorkflowDossierQualityGate {
                    passed: completion.allowed && prompt_to_artifact_complete,
                    prompt_to_artifact_complete,
                    missing: quality_missing,
                    recovery_hints,
                },
                completion,
                continuations,
            }
        })
        .collect::<Vec<_>>();
    let evidence_count = workflows
        .iter()
        .map(|workflow| workflow.evidence.len())
        .sum();

    RunDossier {
        workflows,
        denied_transitions: projection.denied_transitions.clone(),
        evidence_count,
    }
}

fn workflow_continuations(
    projection: &WorkflowProjection,
    workflow_id: &str,
) -> Vec<WorkflowDossierContinuation> {
    projection
        .continuations
        .values()
        .filter(|continuation| continuation.workflow_id == workflow_id)
        .map(WorkflowDossierContinuation::from)
        .collect()
}

impl From<&WorkflowContinuationProjection> for WorkflowDossierContinuation {
    fn from(continuation: &WorkflowContinuationProjection) -> Self {
        Self {
            continuation_id: continuation.continuation_id.clone(),
            mode: continuation.mode.clone(),
            command: continuation.command.clone(),
            status: continuation.status.clone(),
            iteration: continuation.iteration,
            max_iterations: continuation.max_iterations,
            lane: continuation.lane.clone(),
            stop_reason: continuation.stop_reason.clone(),
            last_schedule_reason: continuation.last_schedule_reason.clone(),
            limit: continuation.limit.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::context_snapshot::CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY;
    use crate::event::{EventV1, WorkflowEvidenceRecordedEvent, WorkflowStartedEvent};
    use crate::workflow::{
        project_workflows, WorkflowSignoffPolicy, SIMULATED_TOOL_EVIDENCE_CATEGORY,
    };

    use super::build_run_dossier;

    #[test]
    fn dossier_reports_missing_and_present_signoff_evidence() {
        let events = [
            EventV1::WorkflowStarted(WorkflowStartedEvent {
                workflow_id: "wf_demo".to_string(),
                mode: "workflow.run".to_string(),
                owner: "sim".to_string(),
                lane: Some("simulated".to_string()),
                title: None,
                idempotency_key: None,
            }),
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: "wf_demo".to_string(),
                category: CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY.to_string(),
                summary: "snapshot captured".to_string(),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: Some("ctx_1".to_string()),
                metadata: BTreeMap::new(),
            }),
        ];
        let projection = project_workflows(events.iter());
        let dossier = build_run_dossier(&projection, &WorkflowSignoffPolicy::simulator_default());

        let workflow = &dossier.workflows[0];
        assert_eq!(workflow.evidence.len(), 1);
        assert!(!workflow.signoff.allowed);
        assert_eq!(
            workflow.closeout.policy_id.0,
            "workflow.closeout.default".to_string()
        );
        assert!(!workflow.closeout.overall_allowed);
        assert!(workflow
            .closeout
            .matrix
            .iter()
            .any(|dimension| dimension.id == "evidence"));
        assert_eq!(
            workflow.signoff.missing_evidence_categories,
            vec![SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string()]
        );
    }
}

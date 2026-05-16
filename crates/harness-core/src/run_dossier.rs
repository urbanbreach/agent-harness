use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::event::WorkflowTransitionDeniedEvent;
use crate::workflow::{WorkflowProjection, WorkflowSignoffPolicy, WorkflowSignoffReadiness};

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

pub fn build_run_dossier(
    projection: &WorkflowProjection,
    signoff_policy: &WorkflowSignoffPolicy,
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
            workflow.signoff.missing_evidence_categories,
            vec![SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string()]
        );
    }
}

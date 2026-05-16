use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::event::{EventV1, WorkflowEvidenceRecordedEvent};

pub const RESEARCH_MISSION_MODE: &str = "workflow.research_mission";
pub const RESEARCH_MISSION_SCHEMA_VERSION: u32 = 1;
pub const RESEARCH_MISSION_ARTIFACT_KIND: &str = "workflow_research_mission";
pub const RESEARCH_RESULT_ARTIFACT_KIND: &str = "workflow_research_result";
pub const RESEARCH_MISSION_ARTIFACT_DIR: &str = "workflows/research_mission";
pub const RESEARCH_MISSION_EVIDENCE_CATEGORY: &str = "evidence.research_mission";

const METADATA_ARTIFACT_KIND: &str = "artifact_kind";
const METADATA_MISSION_ID: &str = "mission_id";
const METADATA_MISSION_STATUS: &str = "mission_status";
const METADATA_OBJECTIVE: &str = "objective";
const METADATA_VALIDATOR_MODE: &str = "validator_mode";
const METADATA_VALIDATOR_STATUS: &str = "validator_status";
const METADATA_VALIDATOR_REF: &str = "validator_ref";
const METADATA_REVIEW_REF: &str = "review_ref";
const METADATA_RESULT_REF: &str = "result_ref";
const METADATA_ITERATION: &str = "iteration";
const METADATA_SUMMARY: &str = "summary";
const METADATA_EVIDENCE_REFS: &str = "evidence_refs";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResearchValidatorMode {
    MissionValidatorScript,
    PromptArchitectArtifact,
}

impl ResearchValidatorMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissionValidatorScript => "mission_validator_script",
            Self::PromptArchitectArtifact => "prompt_architect_artifact",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "mission_validator_script" => Some(Self::MissionValidatorScript),
            "prompt_architect_artifact" => Some(Self::PromptArchitectArtifact),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResearchMissionArtifact {
    pub schema_version: u32,
    pub workflow_id: String,
    pub mission_id: String,
    pub objective: String,
    pub question: String,
    pub validator_mode: ResearchValidatorMode,
    pub sandbox: ResearchSandboxArtifact,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResearchSandboxArtifact {
    pub summary: String,
    #[serde(default)]
    pub allowed_commands: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResearchResultArtifact {
    pub schema_version: u32,
    pub workflow_id: String,
    pub mission_id: String,
    pub iteration: u32,
    pub status: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_ref: Option<String>,
    pub validator: ResearchValidatorArtifact,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResearchValidatorArtifact {
    pub mode: ResearchValidatorMode,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<ResearchValidatorCommand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResearchValidatorCommand {
    pub command: String,
    pub permission_kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResearchMissionProjection {
    pub missions: BTreeMap<String, ResearchMissionStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResearchMissionStatus {
    pub workflow_id: String,
    pub mission_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    pub status: String,
    pub validator_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    #[serde(default)]
    pub iterations: Vec<ResearchIterationStatus>,
    pub ready_for_completion: bool,
    #[serde(default)]
    pub missing_completion_requirements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResearchIterationStatus {
    pub iteration: u32,
    pub status: String,
    pub summary: String,
    pub validator_mode: String,
    pub validator_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator_ref: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
}

pub fn validate_research_mission_artifact(
    artifact: &ResearchMissionArtifact,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if artifact.schema_version != RESEARCH_MISSION_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version must be {RESEARCH_MISSION_SCHEMA_VERSION}"
        ));
    }
    if artifact.workflow_id.trim().is_empty() {
        errors.push("workflow_id is required".to_string());
    }
    if artifact.mission_id.trim().is_empty() {
        errors.push("mission_id is required".to_string());
    }
    if artifact.objective.trim().is_empty() {
        errors.push("objective is required".to_string());
    }
    if artifact.question.trim().is_empty() {
        errors.push("question is required".to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn validate_research_result_artifact(
    artifact: &ResearchResultArtifact,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if artifact.schema_version != RESEARCH_MISSION_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version must be {RESEARCH_MISSION_SCHEMA_VERSION}"
        ));
    }
    if artifact.workflow_id.trim().is_empty() {
        errors.push("workflow_id is required".to_string());
    }
    if artifact.mission_id.trim().is_empty() {
        errors.push("mission_id is required".to_string());
    }
    if !matches!(artifact.status.as_str(), "complete" | "blocked" | "failed") {
        errors.push(format!("unsupported research status `{}`", artifact.status));
    }
    if artifact.status == "complete" {
        if artifact.evidence_refs.is_empty() {
            errors.push("complete research results require evidence refs".to_string());
        }
        validate_validator(&artifact.validator, &mut errors);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_validator(validator: &ResearchValidatorArtifact, errors: &mut Vec<String>) {
    if validator.status != "passed" {
        errors.push("research completion requires a passing validator".to_string());
    }
    match validator.mode {
        ResearchValidatorMode::MissionValidatorScript => {
            if validator
                .result_ref
                .as_deref()
                .unwrap_or_default()
                .is_empty()
            {
                errors.push(
                    "mission_validator_script requires a structured validator result ref"
                        .to_string(),
                );
            }
            if let Some(command) = validator.command.as_ref() {
                if command.permission_kind != "bash" {
                    errors.push("validator commands must declare bash permission".to_string());
                }
            }
        }
        ResearchValidatorMode::PromptArchitectArtifact => {
            if validator
                .review_ref
                .as_deref()
                .unwrap_or_default()
                .is_empty()
            {
                errors.push("prompt_architect_artifact requires a review artifact ref".to_string());
            }
        }
    }
}

pub fn research_mission_artifact_name(mission_id: &str) -> String {
    format!(
        "{RESEARCH_MISSION_ARTIFACT_DIR}/{}.json",
        artifact_safe_id(mission_id)
    )
}

pub fn research_result_artifact_name(mission_id: &str, iteration: u32) -> String {
    format!(
        "{RESEARCH_MISSION_ARTIFACT_DIR}/results/{}-{}.json",
        artifact_safe_id(mission_id),
        iteration
    )
}

pub fn research_validator_artifact_name(mission_id: &str, iteration: u32) -> String {
    format!(
        "{RESEARCH_MISSION_ARTIFACT_DIR}/validators/{}-{}.json",
        artifact_safe_id(mission_id),
        iteration
    )
}

pub fn research_mission_metadata(artifact: &ResearchMissionArtifact) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            METADATA_ARTIFACT_KIND.to_string(),
            RESEARCH_MISSION_ARTIFACT_KIND.to_string(),
        ),
        (METADATA_MISSION_ID.to_string(), artifact.mission_id.clone()),
        (METADATA_MISSION_STATUS.to_string(), "active".to_string()),
        (METADATA_OBJECTIVE.to_string(), artifact.objective.clone()),
        (
            METADATA_VALIDATOR_MODE.to_string(),
            artifact.validator_mode.as_str().to_string(),
        ),
        (
            METADATA_EVIDENCE_REFS.to_string(),
            json_string_array(&artifact.evidence_refs),
        ),
    ])
}

pub fn research_result_metadata(artifact: &ResearchResultArtifact) -> BTreeMap<String, String> {
    let validator_ref = artifact
        .validator
        .result_ref
        .clone()
        .or_else(|| artifact.validator.review_ref.clone())
        .unwrap_or_default();
    let mut metadata = BTreeMap::from([
        (
            METADATA_ARTIFACT_KIND.to_string(),
            RESEARCH_RESULT_ARTIFACT_KIND.to_string(),
        ),
        (METADATA_MISSION_ID.to_string(), artifact.mission_id.clone()),
        (METADATA_MISSION_STATUS.to_string(), artifact.status.clone()),
        (
            METADATA_ITERATION.to_string(),
            artifact.iteration.to_string(),
        ),
        (METADATA_SUMMARY.to_string(), artifact.summary.clone()),
        (
            METADATA_VALIDATOR_MODE.to_string(),
            artifact.validator.mode.as_str().to_string(),
        ),
        (
            METADATA_VALIDATOR_STATUS.to_string(),
            artifact.validator.status.clone(),
        ),
        (METADATA_VALIDATOR_REF.to_string(), validator_ref),
        (
            METADATA_EVIDENCE_REFS.to_string(),
            json_string_array(&artifact.evidence_refs),
        ),
    ]);
    if let Some(result_ref) = artifact.validator.result_ref.as_ref() {
        metadata.insert(METADATA_RESULT_REF.to_string(), result_ref.clone());
    }
    if let Some(review_ref) = artifact.validator.review_ref.as_ref() {
        metadata.insert(METADATA_REVIEW_REF.to_string(), review_ref.clone());
    }
    metadata
}

pub fn apply_research_mission_evidence(
    projection: &mut ResearchMissionProjection,
    payload: &WorkflowEvidenceRecordedEvent,
) {
    if payload.category != RESEARCH_MISSION_EVIDENCE_CATEGORY {
        return;
    }
    let mission_id = payload
        .metadata
        .get(METADATA_MISSION_ID)
        .cloned()
        .or_else(|| payload.acceptance_ref.clone())
        .unwrap_or_else(|| payload.workflow_id.clone());
    let artifact_kind = payload
        .metadata
        .get(METADATA_ARTIFACT_KIND)
        .map(String::as_str)
        .unwrap_or(RESEARCH_MISSION_ARTIFACT_KIND);
    let status = projection
        .missions
        .entry(mission_id.clone())
        .or_insert_with(|| empty_mission(payload.workflow_id.clone(), mission_id));
    status.workflow_id = payload.workflow_id.clone();
    status.artifact_path = payload.artifact_path.clone();
    status.artifact_digest = payload.artifact_digest.clone();
    if artifact_kind == RESEARCH_MISSION_ARTIFACT_KIND {
        status.objective = payload.metadata.get(METADATA_OBJECTIVE).cloned();
        status.status = payload
            .metadata
            .get(METADATA_MISSION_STATUS)
            .cloned()
            .unwrap_or_else(|| "active".to_string());
        status.validator_mode = payload
            .metadata
            .get(METADATA_VALIDATOR_MODE)
            .cloned()
            .unwrap_or_else(|| "unspecified".to_string());
    } else {
        apply_result(status, payload);
    }
    recompute_mission_status(status);
}

pub fn project_research_missions<'a>(
    events: impl IntoIterator<Item = &'a EventV1>,
) -> ResearchMissionProjection {
    let mut projection = ResearchMissionProjection::default();
    for event in events {
        if let EventV1::WorkflowEvidenceRecorded(payload) = event {
            apply_research_mission_evidence(&mut projection, payload);
        }
    }
    projection
}

fn apply_result(status: &mut ResearchMissionStatus, payload: &WorkflowEvidenceRecordedEvent) {
    let iteration = payload
        .metadata
        .get(METADATA_ITERATION)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    let validator_status = payload
        .metadata
        .get(METADATA_VALIDATOR_STATUS)
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let result_status = payload
        .metadata
        .get(METADATA_MISSION_STATUS)
        .cloned()
        .unwrap_or_else(|| "blocked".to_string());
    status.iterations.push(ResearchIterationStatus {
        iteration,
        status: result_status,
        summary: payload
            .metadata
            .get(METADATA_SUMMARY)
            .cloned()
            .unwrap_or_else(|| payload.summary.clone()),
        validator_mode: payload
            .metadata
            .get(METADATA_VALIDATOR_MODE)
            .cloned()
            .unwrap_or_else(|| status.validator_mode.clone()),
        validator_status,
        validator_ref: payload.metadata.get(METADATA_VALIDATOR_REF).cloned(),
        evidence_refs: parse_json_string_array(payload.metadata.get(METADATA_EVIDENCE_REFS)),
        artifact_path: payload.artifact_path.clone(),
        artifact_digest: payload.artifact_digest.clone(),
    });
}

fn recompute_mission_status(status: &mut ResearchMissionStatus) {
    status.ready_for_completion = false;
    status.missing_completion_requirements.clear();
    let Some(latest) = status.iterations.last() else {
        if status.status.is_empty() {
            status.status = "active".to_string();
        }
        status
            .missing_completion_requirements
            .push("validator_result".to_string());
        return;
    };
    if latest.status == "complete" && latest.validator_status == "passed" {
        if latest
            .validator_ref
            .as_deref()
            .unwrap_or_default()
            .is_empty()
        {
            status.status = "blocked".to_string();
            status
                .missing_completion_requirements
                .push("validator_artifact".to_string());
        } else {
            status.status = "complete".to_string();
            status.ready_for_completion = true;
        }
    } else {
        status.status = latest.status.clone();
        status
            .missing_completion_requirements
            .push("passing_validator".to_string());
    }
}

fn empty_mission(workflow_id: String, mission_id: String) -> ResearchMissionStatus {
    ResearchMissionStatus {
        workflow_id,
        mission_id,
        objective: None,
        status: "active".to_string(),
        validator_mode: "unspecified".to_string(),
        artifact_path: None,
        artifact_digest: None,
        iterations: Vec::new(),
        ready_for_completion: false,
        missing_completion_requirements: Vec::new(),
    }
}

fn json_string_array(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string())
}

fn parse_json_string_array(value: Option<&String>) -> Vec<String> {
    value
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default()
}

fn artifact_safe_id(id: &str) -> String {
    let safe = id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if safe.is_empty() {
        "mission".to_string()
    } else {
        safe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn research_completion_requires_validator_artifact() {
        let artifact = ResearchResultArtifact {
            schema_version: RESEARCH_MISSION_SCHEMA_VERSION,
            workflow_id: "wf_research".to_string(),
            mission_id: "mission_1".to_string(),
            iteration: 1,
            status: "complete".to_string(),
            summary: "candidate accepted".to_string(),
            candidate_ref: Some("candidate.json".to_string()),
            validator: ResearchValidatorArtifact {
                mode: ResearchValidatorMode::PromptArchitectArtifact,
                status: "passed".to_string(),
                command: None,
                result_ref: None,
                review_ref: None,
            },
            evidence_refs: vec!["candidate.json".to_string()],
        };
        assert!(validate_research_result_artifact(&artifact)
            .unwrap_err()
            .iter()
            .any(|err| err.contains("review artifact")));
    }

    #[test]
    fn research_projection_never_reruns_validator() {
        let mission = ResearchMissionArtifact {
            schema_version: RESEARCH_MISSION_SCHEMA_VERSION,
            workflow_id: "wf_research".to_string(),
            mission_id: "mission_1".to_string(),
            objective: "Compare workflow options".to_string(),
            question: "Which option is safer?".to_string(),
            validator_mode: ResearchValidatorMode::MissionValidatorScript,
            sandbox: ResearchSandboxArtifact {
                summary: "No network".to_string(),
                allowed_commands: vec!["cargo test".to_string()],
                constraints: vec!["deterministic".to_string()],
            },
            evidence_refs: Vec::new(),
        };
        let result = ResearchResultArtifact {
            schema_version: RESEARCH_MISSION_SCHEMA_VERSION,
            workflow_id: "wf_research".to_string(),
            mission_id: "mission_1".to_string(),
            iteration: 1,
            status: "complete".to_string(),
            summary: "validator passed".to_string(),
            candidate_ref: Some("candidate.json".to_string()),
            validator: ResearchValidatorArtifact {
                mode: ResearchValidatorMode::MissionValidatorScript,
                status: "passed".to_string(),
                command: Some(ResearchValidatorCommand {
                    command: "cargo test".to_string(),
                    permission_kind: "bash".to_string(),
                }),
                result_ref: Some("validator-result.json".to_string()),
                review_ref: None,
            },
            evidence_refs: vec!["validator-result.json".to_string()],
        };
        let events = [
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: mission.workflow_id.clone(),
                category: RESEARCH_MISSION_EVIDENCE_CATEGORY.to_string(),
                summary: "mission created".to_string(),
                artifact_path: Some("artifacts/workflows/research_mission/mission_1.json".into()),
                artifact_digest: Some("digest".to_string()),
                acceptance_ref: Some(mission.mission_id.clone()),
                metadata: research_mission_metadata(&mission),
            }),
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: result.workflow_id.clone(),
                category: RESEARCH_MISSION_EVIDENCE_CATEGORY.to_string(),
                summary: "result accepted".to_string(),
                artifact_path: Some(
                    "artifacts/workflows/research_mission/results/mission_1-1.json".into(),
                ),
                artifact_digest: Some("digest".to_string()),
                acceptance_ref: Some(result.mission_id.clone()),
                metadata: research_result_metadata(&result),
            }),
        ];
        let projection = project_research_missions(events.iter());
        let mission = &projection.missions["mission_1"];
        assert_eq!(mission.status, "complete");
        assert!(mission.ready_for_completion);
        assert_eq!(
            mission.iterations[0].validator_ref.as_deref(),
            Some("validator-result.json")
        );
    }
}

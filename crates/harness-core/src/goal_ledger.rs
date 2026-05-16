use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::event::{EventV1, WorkflowEvidenceRecordedEvent};

pub const GOAL_LEDGER_MODE: &str = "workflow.goal_ledger";
pub const GOAL_LEDGER_SCHEMA_VERSION: u32 = 1;
pub const GOAL_LEDGER_ARTIFACT_KIND: &str = "workflow_goal_ledger";
pub const GOAL_CHECKPOINT_ARTIFACT_KIND: &str = "workflow_goal_checkpoint";
pub const GOAL_LEDGER_ARTIFACT_DIR: &str = "workflows/goal_ledger";
pub const GOAL_LEDGER_EVIDENCE_CATEGORY: &str = "evidence.goal_ledger";

const METADATA_ARTIFACT_KIND: &str = "artifact_kind";
const METADATA_GOAL_ID: &str = "goal_id";
const METADATA_GOAL_STATUS: &str = "goal_status";
const METADATA_OBJECTIVE: &str = "objective";
const METADATA_STORY_COUNT: &str = "story_count";
const METADATA_STORY_PREFIX: &str = "story.";
const METADATA_STORY_ID: &str = "story_id";
const METADATA_STORY_STATUS: &str = "story_status";
const METADATA_CHECKPOINT_STATUS: &str = "checkpoint_status";
const METADATA_SUMMARY: &str = "summary";
const METADATA_EVIDENCE_REFS: &str = "evidence_refs";
const METADATA_FINAL_CHECKPOINT: &str = "final_checkpoint";
const METADATA_QUALITY_GATE_STATUS: &str = "quality_gate_status";
const METADATA_QUALITY_GATE_EVIDENCE_REFS: &str = "quality_gate_evidence_refs";
const METADATA_QUALITY_GATE_VERIFICATION_REFS: &str = "quality_gate_verification_refs";
const METADATA_QUALITY_GATE_REVIEW_REFS: &str = "quality_gate_review_refs";
const METADATA_QUALITY_GATE_CLEANUP_REFS: &str = "quality_gate_cleanup_refs";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalLedgerArtifact {
    pub schema_version: u32,
    pub workflow_id: String,
    pub goal_id: String,
    pub objective: String,
    pub status: String,
    #[serde(default)]
    pub stories: Vec<GoalStoryArtifact>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_gate: Option<GoalQualityGate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalStoryArtifact {
    pub story_id: String,
    pub objective: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_workflow_id: Option<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalCheckpointArtifact {
    pub schema_version: u32,
    pub workflow_id: String,
    pub goal_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub story_id: Option<String>,
    pub status: String,
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub final_checkpoint: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_gate: Option<GoalQualityGate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalQualityGate {
    pub status: String,
    #[serde(default)]
    pub verification_refs: Vec<String>,
    #[serde(default)]
    pub review_refs: Vec<String>,
    #[serde(default)]
    pub cleanup_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

impl GoalQualityGate {
    pub fn passed(&self) -> bool {
        self.missing_requirements().is_empty()
    }

    pub fn missing_requirements(&self) -> Vec<String> {
        let mut missing = Vec::new();
        if self.status != "passed" {
            missing.push("quality_gate_passed".to_string());
        }
        if self.verification_refs.is_empty() {
            missing.push("verification_evidence".to_string());
        }
        if self.review_refs.is_empty() {
            missing.push("review_evidence".to_string());
        }
        missing
    }

    fn merged_evidence_refs(&self) -> Vec<String> {
        let mut refs = self.evidence_refs.clone();
        refs.extend(self.verification_refs.clone());
        refs.extend(self.review_refs.clone());
        refs.extend(self.cleanup_refs.clone());
        refs.sort();
        refs.dedup();
        refs
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalLedgerProjection {
    pub goals: BTreeMap<String, GoalProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalProjection {
    pub workflow_id: String,
    pub goal_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    #[serde(default)]
    pub stories: BTreeMap<String, GoalStoryProjection>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub checkpoints: Vec<GoalCheckpointProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_quality_gate: Option<GoalQualityGateProjection>,
    pub ready_for_completion: bool,
    #[serde(default)]
    pub missing_completion_requirements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalStoryProjection {
    pub story_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_workflow_id: Option<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalCheckpointProjection {
    pub goal_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub story_id: Option<String>,
    pub status: String,
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub final_checkpoint: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_gate: Option<GoalQualityGateProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalQualityGateProjection {
    pub status: String,
    #[serde(default)]
    pub verification_refs: Vec<String>,
    #[serde(default)]
    pub review_refs: Vec<String>,
    #[serde(default)]
    pub cleanup_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub passed: bool,
    #[serde(default)]
    pub missing_requirements: Vec<String>,
}

pub fn validate_goal_ledger_artifact(
    artifact: &GoalLedgerArtifact,
    require_final_quality_gate: bool,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if artifact.schema_version != GOAL_LEDGER_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version must be {GOAL_LEDGER_SCHEMA_VERSION}"
        ));
    }
    if artifact.workflow_id.trim().is_empty() {
        errors.push("workflow_id is required".to_string());
    }
    if artifact.goal_id.trim().is_empty() {
        errors.push("goal_id is required".to_string());
    }
    if artifact.objective.trim().is_empty() {
        errors.push("objective is required".to_string());
    }
    if !is_goal_status(&artifact.status) {
        errors.push(format!("unsupported goal status `{}`", artifact.status));
    }
    if artifact.stories.is_empty() {
        errors.push("at least one story is required".to_string());
    }
    for story in &artifact.stories {
        validate_story(story, &mut errors);
    }
    if artifact.status == "complete" {
        if artifact
            .stories
            .iter()
            .any(|story| story.status != "complete")
        {
            errors.push("complete goals require every story to be complete".to_string());
        }
        validate_final_quality_gate(
            artifact.quality_gate.as_ref(),
            require_final_quality_gate,
            &mut errors,
        );
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn validate_goal_checkpoint_artifact(
    checkpoint: &GoalCheckpointArtifact,
    require_final_quality_gate: bool,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if checkpoint.schema_version != GOAL_LEDGER_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version must be {GOAL_LEDGER_SCHEMA_VERSION}"
        ));
    }
    if checkpoint.workflow_id.trim().is_empty() {
        errors.push("workflow_id is required".to_string());
    }
    if checkpoint.goal_id.trim().is_empty() {
        errors.push("goal_id is required".to_string());
    }
    if !is_story_status(&checkpoint.status) {
        errors.push(format!(
            "unsupported checkpoint status `{}`",
            checkpoint.status
        ));
    }
    if checkpoint.status == "complete" && checkpoint.evidence_refs.is_empty() {
        errors.push("complete story checkpoints require evidence refs".to_string());
    }
    if checkpoint.final_checkpoint && checkpoint.status == "complete" {
        validate_final_quality_gate(
            checkpoint.quality_gate.as_ref(),
            require_final_quality_gate,
            &mut errors,
        );
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_story(story: &GoalStoryArtifact, errors: &mut Vec<String>) {
    if story.story_id.trim().is_empty() {
        errors.push("story_id is required".to_string());
    }
    if story.objective.trim().is_empty() {
        errors.push(format!("story `{}` objective is required", story.story_id));
    }
    if !is_story_status(&story.status) {
        errors.push(format!(
            "story `{}` has unsupported status `{}`",
            story.story_id, story.status
        ));
    }
}

fn validate_final_quality_gate(
    quality_gate: Option<&GoalQualityGate>,
    require_final_quality_gate: bool,
    errors: &mut Vec<String>,
) {
    if !require_final_quality_gate {
        return;
    }
    let Some(quality_gate) = quality_gate else {
        errors.push("final goal completion requires quality gate evidence".to_string());
        return;
    };
    for missing in quality_gate.missing_requirements() {
        errors.push(format!("final quality gate missing {missing}"));
    }
}

fn is_goal_status(status: &str) -> bool {
    matches!(
        status,
        "pending" | "active" | "complete" | "blocked" | "failed" | "pending_final_quality_gate"
    )
}

fn is_story_status(status: &str) -> bool {
    matches!(
        status,
        "pending" | "active" | "complete" | "blocked" | "failed"
    )
}

pub fn goal_ledger_artifact_name(goal_id: &str) -> String {
    format!(
        "{GOAL_LEDGER_ARTIFACT_DIR}/{}.json",
        artifact_safe_id(goal_id)
    )
}

pub fn goal_checkpoint_artifact_name(goal_id: &str, story_id: Option<&str>) -> String {
    let story = story_id
        .map(artifact_safe_id)
        .unwrap_or_else(|| "goal".to_string());
    format!(
        "{GOAL_LEDGER_ARTIFACT_DIR}/checkpoints/{}-{}.json",
        artifact_safe_id(goal_id),
        story
    )
}

pub fn goal_ledger_metadata(artifact: &GoalLedgerArtifact) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::from([
        (
            METADATA_ARTIFACT_KIND.to_string(),
            GOAL_LEDGER_ARTIFACT_KIND.to_string(),
        ),
        (METADATA_GOAL_ID.to_string(), artifact.goal_id.clone()),
        (METADATA_GOAL_STATUS.to_string(), artifact.status.clone()),
        (METADATA_OBJECTIVE.to_string(), artifact.objective.clone()),
        (
            METADATA_STORY_COUNT.to_string(),
            artifact.stories.len().to_string(),
        ),
        (
            METADATA_EVIDENCE_REFS.to_string(),
            json_string_array(&artifact.evidence_refs),
        ),
    ]);
    for (index, story) in artifact.stories.iter().enumerate() {
        let prefix = format!("{METADATA_STORY_PREFIX}{index}.");
        metadata.insert(format!("{prefix}id"), story.story_id.clone());
        metadata.insert(format!("{prefix}objective"), story.objective.clone());
        metadata.insert(format!("{prefix}status"), story.status.clone());
        metadata.insert(
            format!("{prefix}acceptance"),
            json_string_array(&story.acceptance),
        );
        metadata.insert(
            format!("{prefix}evidence_refs"),
            json_string_array(&story.evidence_refs),
        );
        if let Some(owner) = story.owner_workflow_id.as_ref() {
            metadata.insert(format!("{prefix}owner_workflow_id"), owner.clone());
        }
    }
    if let Some(quality_gate) = artifact.quality_gate.as_ref() {
        insert_quality_gate_metadata(&mut metadata, quality_gate);
    }
    metadata
}

pub fn goal_checkpoint_metadata(checkpoint: &GoalCheckpointArtifact) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::from([
        (
            METADATA_ARTIFACT_KIND.to_string(),
            GOAL_CHECKPOINT_ARTIFACT_KIND.to_string(),
        ),
        (METADATA_GOAL_ID.to_string(), checkpoint.goal_id.clone()),
        (
            METADATA_CHECKPOINT_STATUS.to_string(),
            checkpoint.status.clone(),
        ),
        (METADATA_STORY_STATUS.to_string(), checkpoint.status.clone()),
        (METADATA_SUMMARY.to_string(), checkpoint.summary.clone()),
        (
            METADATA_EVIDENCE_REFS.to_string(),
            json_string_array(&checkpoint.evidence_refs),
        ),
        (
            METADATA_FINAL_CHECKPOINT.to_string(),
            checkpoint.final_checkpoint.to_string(),
        ),
    ]);
    if let Some(story_id) = checkpoint.story_id.as_ref() {
        metadata.insert(METADATA_STORY_ID.to_string(), story_id.clone());
    }
    if let Some(quality_gate) = checkpoint.quality_gate.as_ref() {
        insert_quality_gate_metadata(&mut metadata, quality_gate);
    }
    metadata
}

fn insert_quality_gate_metadata(
    metadata: &mut BTreeMap<String, String>,
    quality_gate: &GoalQualityGate,
) {
    metadata.insert(
        METADATA_QUALITY_GATE_STATUS.to_string(),
        quality_gate.status.clone(),
    );
    metadata.insert(
        METADATA_QUALITY_GATE_EVIDENCE_REFS.to_string(),
        json_string_array(&quality_gate.merged_evidence_refs()),
    );
    metadata.insert(
        METADATA_QUALITY_GATE_VERIFICATION_REFS.to_string(),
        json_string_array(&quality_gate.verification_refs),
    );
    metadata.insert(
        METADATA_QUALITY_GATE_REVIEW_REFS.to_string(),
        json_string_array(&quality_gate.review_refs),
    );
    metadata.insert(
        METADATA_QUALITY_GATE_CLEANUP_REFS.to_string(),
        json_string_array(&quality_gate.cleanup_refs),
    );
}

pub fn apply_goal_ledger_evidence(
    projection: &mut GoalLedgerProjection,
    payload: &WorkflowEvidenceRecordedEvent,
) {
    if payload.category != GOAL_LEDGER_EVIDENCE_CATEGORY {
        return;
    }
    let artifact_kind = payload
        .metadata
        .get(METADATA_ARTIFACT_KIND)
        .map(String::as_str)
        .unwrap_or(GOAL_LEDGER_ARTIFACT_KIND);
    match artifact_kind {
        GOAL_CHECKPOINT_ARTIFACT_KIND => apply_checkpoint_evidence(projection, payload),
        _ => apply_ledger_evidence(projection, payload),
    }
}

fn apply_ledger_evidence(
    projection: &mut GoalLedgerProjection,
    payload: &WorkflowEvidenceRecordedEvent,
) {
    let goal_id = payload
        .metadata
        .get(METADATA_GOAL_ID)
        .cloned()
        .or_else(|| payload.acceptance_ref.clone())
        .unwrap_or_else(|| payload.workflow_id.clone());
    let status = payload
        .metadata
        .get(METADATA_GOAL_STATUS)
        .cloned()
        .unwrap_or_else(|| "active".to_string());
    let goal = projection
        .goals
        .entry(goal_id.clone())
        .or_insert_with(|| empty_goal(payload.workflow_id.clone(), goal_id.clone()));
    goal.workflow_id = payload.workflow_id.clone();
    goal.goal_id = goal_id;
    goal.objective = payload.metadata.get(METADATA_OBJECTIVE).cloned();
    goal.status = status;
    goal.artifact_path = payload.artifact_path.clone();
    goal.artifact_digest = payload.artifact_digest.clone();
    merge_refs(
        &mut goal.evidence_refs,
        parse_json_string_array(payload.metadata.get(METADATA_EVIDENCE_REFS)),
    );

    let story_count = payload
        .metadata
        .get(METADATA_STORY_COUNT)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    for index in 0..story_count {
        let prefix = format!("{METADATA_STORY_PREFIX}{index}.");
        let Some(story_id) = payload.metadata.get(&format!("{prefix}id")).cloned() else {
            continue;
        };
        let story = goal
            .stories
            .entry(story_id.clone())
            .or_insert_with(|| empty_story(story_id.clone()));
        story.objective = payload.metadata.get(&format!("{prefix}objective")).cloned();
        story.status = payload
            .metadata
            .get(&format!("{prefix}status"))
            .cloned()
            .unwrap_or_else(|| "pending".to_string());
        story.owner_workflow_id = payload
            .metadata
            .get(&format!("{prefix}owner_workflow_id"))
            .cloned();
        story.acceptance =
            parse_json_string_array(payload.metadata.get(&format!("{prefix}acceptance")));
        merge_refs(
            &mut story.evidence_refs,
            parse_json_string_array(payload.metadata.get(&format!("{prefix}evidence_refs"))),
        );
        merge_refs(&mut goal.evidence_refs, story.evidence_refs.clone());
    }
    if let Some(quality_gate) = quality_gate_from_metadata(&payload.metadata) {
        goal.final_quality_gate = Some(quality_gate);
    }
    recompute_goal_status(goal);
}

fn apply_checkpoint_evidence(
    projection: &mut GoalLedgerProjection,
    payload: &WorkflowEvidenceRecordedEvent,
) {
    let goal_id = payload
        .metadata
        .get(METADATA_GOAL_ID)
        .cloned()
        .or_else(|| payload.acceptance_ref.clone())
        .unwrap_or_else(|| payload.workflow_id.clone());
    let status = payload
        .metadata
        .get(METADATA_STORY_STATUS)
        .or_else(|| payload.metadata.get(METADATA_CHECKPOINT_STATUS))
        .cloned()
        .unwrap_or_else(|| "active".to_string());
    let evidence_refs = parse_json_string_array(payload.metadata.get(METADATA_EVIDENCE_REFS));
    let final_checkpoint = payload
        .metadata
        .get(METADATA_FINAL_CHECKPOINT)
        .is_some_and(|value| value == "true");
    let quality_gate = quality_gate_from_metadata(&payload.metadata);
    let story_id = payload.metadata.get(METADATA_STORY_ID).cloned();
    let checkpoint = GoalCheckpointProjection {
        goal_id: goal_id.clone(),
        story_id: story_id.clone(),
        status: status.clone(),
        summary: payload
            .metadata
            .get(METADATA_SUMMARY)
            .cloned()
            .unwrap_or_else(|| payload.summary.clone()),
        evidence_refs: evidence_refs.clone(),
        final_checkpoint,
        artifact_path: payload.artifact_path.clone(),
        artifact_digest: payload.artifact_digest.clone(),
        quality_gate: quality_gate.clone(),
    };

    let goal = projection
        .goals
        .entry(goal_id.clone())
        .or_insert_with(|| empty_goal(payload.workflow_id.clone(), goal_id));
    goal.workflow_id = payload.workflow_id.clone();
    goal.checkpoints.push(checkpoint);
    merge_refs(&mut goal.evidence_refs, evidence_refs.clone());
    if let Some(story_id) = story_id {
        let story = goal
            .stories
            .entry(story_id.clone())
            .or_insert_with(|| empty_story(story_id));
        story.status = status;
        merge_refs(&mut story.evidence_refs, evidence_refs);
    }
    if final_checkpoint {
        goal.final_quality_gate = quality_gate;
    }
    recompute_goal_status(goal);
}

pub fn project_goal_ledger<'a>(
    events: impl IntoIterator<Item = &'a EventV1>,
) -> GoalLedgerProjection {
    let mut projection = GoalLedgerProjection::default();
    for event in events {
        if let EventV1::WorkflowEvidenceRecorded(payload) = event {
            apply_goal_ledger_evidence(&mut projection, payload);
        }
    }
    projection
}

fn recompute_goal_status(goal: &mut GoalProjection) {
    goal.missing_completion_requirements.clear();
    if goal.stories.values().any(|story| story.status == "failed") {
        goal.status = "failed".to_string();
        goal.ready_for_completion = false;
        return;
    }
    if goal.stories.values().any(|story| story.status == "blocked") {
        goal.status = "blocked".to_string();
        goal.ready_for_completion = false;
        return;
    }
    let all_stories_complete = !goal.stories.is_empty()
        && goal
            .stories
            .values()
            .all(|story| story.status == "complete");
    if all_stories_complete {
        match goal.final_quality_gate.as_ref() {
            Some(quality_gate) if quality_gate.passed => {
                goal.status = "complete".to_string();
                goal.ready_for_completion = true;
            }
            Some(quality_gate) => {
                goal.status = "pending_final_quality_gate".to_string();
                goal.ready_for_completion = false;
                goal.missing_completion_requirements = quality_gate.missing_requirements.clone();
            }
            None => {
                goal.status = "pending_final_quality_gate".to_string();
                goal.ready_for_completion = false;
                goal.missing_completion_requirements = vec!["final_quality_gate".to_string()];
            }
        }
        return;
    }
    if goal
        .stories
        .values()
        .any(|story| story.status == "active" || story.status == "complete")
    {
        goal.status = "active".to_string();
    } else if goal.status.trim().is_empty() {
        goal.status = "pending".to_string();
    }
    goal.ready_for_completion = false;
}

fn quality_gate_from_metadata(
    metadata: &BTreeMap<String, String>,
) -> Option<GoalQualityGateProjection> {
    let status = metadata.get(METADATA_QUALITY_GATE_STATUS)?.clone();
    let verification_refs =
        parse_json_string_array(metadata.get(METADATA_QUALITY_GATE_VERIFICATION_REFS));
    let review_refs = parse_json_string_array(metadata.get(METADATA_QUALITY_GATE_REVIEW_REFS));
    let cleanup_refs = parse_json_string_array(metadata.get(METADATA_QUALITY_GATE_CLEANUP_REFS));
    let evidence_refs = parse_json_string_array(metadata.get(METADATA_QUALITY_GATE_EVIDENCE_REFS));
    let quality_gate = GoalQualityGate {
        status: status.clone(),
        verification_refs: verification_refs.clone(),
        review_refs: review_refs.clone(),
        cleanup_refs: cleanup_refs.clone(),
        evidence_refs: evidence_refs.clone(),
    };
    let missing_requirements = quality_gate.missing_requirements();
    Some(GoalQualityGateProjection {
        status,
        verification_refs,
        review_refs,
        cleanup_refs,
        evidence_refs,
        passed: missing_requirements.is_empty(),
        missing_requirements,
    })
}

fn empty_goal(workflow_id: String, goal_id: String) -> GoalProjection {
    GoalProjection {
        workflow_id,
        goal_id,
        objective: None,
        status: "pending".to_string(),
        artifact_path: None,
        artifact_digest: None,
        stories: BTreeMap::new(),
        evidence_refs: Vec::new(),
        checkpoints: Vec::new(),
        final_quality_gate: None,
        ready_for_completion: false,
        missing_completion_requirements: Vec::new(),
    }
}

fn empty_story(story_id: String) -> GoalStoryProjection {
    GoalStoryProjection {
        story_id,
        objective: None,
        status: "pending".to_string(),
        owner_workflow_id: None,
        acceptance: Vec::new(),
        evidence_refs: Vec::new(),
    }
}

fn merge_refs(target: &mut Vec<String>, refs: Vec<String>) {
    target.extend(refs.into_iter().filter(|reference| !reference.is_empty()));
    target.sort();
    target.dedup();
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
        "goal".to_string()
    } else {
        safe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_artifact() -> GoalLedgerArtifact {
        GoalLedgerArtifact {
            schema_version: GOAL_LEDGER_SCHEMA_VERSION,
            workflow_id: "wf_goal".to_string(),
            goal_id: "goal_1".to_string(),
            objective: "Ship durable goal projection".to_string(),
            status: "active".to_string(),
            stories: vec![
                GoalStoryArtifact {
                    story_id: "G001".to_string(),
                    objective: "Create schema".to_string(),
                    status: "pending".to_string(),
                    owner_workflow_id: Some("wf_goal".to_string()),
                    acceptance: vec!["schema exists".to_string()],
                    evidence_refs: Vec::new(),
                },
                GoalStoryArtifact {
                    story_id: "G002".to_string(),
                    objective: "Checkpoint gate".to_string(),
                    status: "pending".to_string(),
                    owner_workflow_id: Some("wf_goal".to_string()),
                    acceptance: vec!["gate blocks missing review".to_string()],
                    evidence_refs: Vec::new(),
                },
            ],
            evidence_refs: vec!["plan_1".to_string()],
            quality_gate: None,
        }
    }

    fn checkpoint(story_id: &str, final_checkpoint: bool) -> GoalCheckpointArtifact {
        GoalCheckpointArtifact {
            schema_version: GOAL_LEDGER_SCHEMA_VERSION,
            workflow_id: "wf_goal".to_string(),
            goal_id: "goal_1".to_string(),
            story_id: Some(story_id.to_string()),
            status: "complete".to_string(),
            summary: format!("{story_id} complete"),
            evidence_refs: vec![format!("evidence-{story_id}")],
            final_checkpoint,
            quality_gate: final_checkpoint.then(|| GoalQualityGate {
                status: "passed".to_string(),
                verification_refs: vec!["cargo test".to_string()],
                review_refs: vec!["code review approve".to_string()],
                cleanup_refs: Vec::new(),
                evidence_refs: Vec::new(),
            }),
        }
    }

    #[test]
    fn goal_checkpoint_validation_requires_evidence_and_final_quality_gate() {
        let mut missing_evidence = checkpoint("G001", false);
        missing_evidence.evidence_refs.clear();
        assert!(validate_goal_checkpoint_artifact(&missing_evidence, true)
            .unwrap_err()
            .iter()
            .any(|err| err.contains("evidence refs")));

        let mut missing_gate = checkpoint("G002", true);
        missing_gate.quality_gate = None;
        assert!(validate_goal_checkpoint_artifact(&missing_gate, true)
            .unwrap_err()
            .iter()
            .any(|err| err.contains("quality gate")));
    }

    #[test]
    fn goal_status_is_replay_derived_from_checkpoint_evidence() {
        let create = create_artifact();
        let create_event = EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
            workflow_id: create.workflow_id.clone(),
            category: GOAL_LEDGER_EVIDENCE_CATEGORY.to_string(),
            summary: "goal created".to_string(),
            artifact_path: Some("artifacts/workflows/goal_ledger/goal_1.json".to_string()),
            artifact_digest: Some("digest".to_string()),
            acceptance_ref: Some(create.goal_id.clone()),
            metadata: goal_ledger_metadata(&create),
        });
        let first = checkpoint("G001", false);
        let first_event = EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
            workflow_id: first.workflow_id.clone(),
            category: GOAL_LEDGER_EVIDENCE_CATEGORY.to_string(),
            summary: first.summary.clone(),
            artifact_path: None,
            artifact_digest: None,
            acceptance_ref: Some(first.goal_id.clone()),
            metadata: goal_checkpoint_metadata(&first),
        });
        let second = checkpoint("G002", false);
        let second_event = EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
            workflow_id: second.workflow_id.clone(),
            category: GOAL_LEDGER_EVIDENCE_CATEGORY.to_string(),
            summary: second.summary.clone(),
            artifact_path: None,
            artifact_digest: None,
            acceptance_ref: Some(second.goal_id.clone()),
            metadata: goal_checkpoint_metadata(&second),
        });

        let events = [
            create_event.clone(),
            first_event.clone(),
            second_event.clone(),
        ];
        let projection = project_goal_ledger(events.iter());
        let goal = &projection.goals["goal_1"];
        assert_eq!(goal.status, "pending_final_quality_gate");
        assert!(!goal.ready_for_completion);
        assert_eq!(goal.stories["G001"].status, "complete");

        let final_checkpoint = checkpoint("G002", true);
        let final_event = EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
            workflow_id: final_checkpoint.workflow_id.clone(),
            category: GOAL_LEDGER_EVIDENCE_CATEGORY.to_string(),
            summary: final_checkpoint.summary.clone(),
            artifact_path: None,
            artifact_digest: None,
            acceptance_ref: Some(final_checkpoint.goal_id.clone()),
            metadata: goal_checkpoint_metadata(&final_checkpoint),
        });
        let events = [create_event, first_event, second_event, final_event];
        let projection = project_goal_ledger(events.iter());
        let goal = &projection.goals["goal_1"];
        assert_eq!(goal.status, "complete");
        assert!(goal.ready_for_completion);
        assert!(goal
            .final_quality_gate
            .as_ref()
            .is_some_and(|gate| gate.passed));
    }
}

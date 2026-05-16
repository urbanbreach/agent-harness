use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::agent_catalog::{AgentCatalog, AgentCatalogEntry};
use crate::event::{EventV1, WorkflowEvidenceRecordedEvent};

pub const PLAN_CONSENSUS_MODE: &str = "workflow.plan_consensus";
pub const PLAN_CONSENSUS_SCHEMA_VERSION: u32 = 1;
pub const PLAN_CONSENSUS_ARTIFACT_KIND: &str = "workflow_plan_consensus";
pub const PLAN_CONSENSUS_ARTIFACT_DIR: &str = "workflows/plan_consensus";
pub const PLAN_CONSENSUS_EVIDENCE_CATEGORY: &str = "evidence.plan_consensus";

const METADATA_ARTIFACT_KIND: &str = "artifact_kind";
const METADATA_PLAN_ID: &str = "plan_id";
const METADATA_PLAN_STATUS: &str = "plan_status";
const METADATA_CRITIC_VERDICT: &str = "critic_verdict";
const METADATA_CRITIC_ITERATIONS: &str = "critic_iterations";
const METADATA_MAX_ITERATIONS: &str = "max_iterations";
const METADATA_LANE_COUNT: &str = "lane_count";
const METADATA_LANE_PREFIX: &str = "lane.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanConsensusArtifact {
    pub schema_version: u32,
    pub workflow_id: String,
    pub plan_id: String,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_ref: Option<String>,
    #[serde(default)]
    pub lanes: Vec<PlanConsensusLane>,
    pub max_iterations: u32,
    pub critic_iterations: u32,
    pub critic_verdict: String,
    #[serde(default)]
    pub principles: Vec<String>,
    #[serde(default)]
    pub decision_drivers: Vec<String>,
    #[serde(default)]
    pub options: Vec<PlanConsensusOption>,
    pub chosen_option: String,
    #[serde(default)]
    pub rejected_alternatives: Vec<String>,
    pub adr: String,
    #[serde(default)]
    pub work_breakdown: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub test_plan: Vec<String>,
    #[serde(default)]
    pub manual_qa_plan: Vec<String>,
    #[serde(default)]
    pub staffing: Vec<String>,
    #[serde(default)]
    pub handoff_options: Vec<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanConsensusOption {
    pub id: String,
    pub summary: String,
    #[serde(default)]
    pub pros: Vec<String>,
    #[serde(default)]
    pub cons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanConsensusLane {
    pub role: String,
    pub profile: String,
    pub agent_catalog_role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_binding: Option<String>,
    pub description: String,
    pub required: bool,
    pub review_order: u32,
    pub can_redelegate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanConsensusProjection {
    pub workflow_id: String,
    pub plan_id: String,
    pub status: String,
    pub critic_verdict: String,
    pub critic_iterations: u32,
    pub max_iterations: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<String>,
    #[serde(default)]
    pub lanes: Vec<PlanConsensusLane>,
}

pub fn resolve_plan_consensus_lanes(catalog: Option<&AgentCatalog>) -> Vec<PlanConsensusLane> {
    [
        (
            "planner",
            &["plan", "prometheus", "metis", "deep"][..],
            "Drafts the recommended plan, ADR, work breakdown, and test strategy.",
        ),
        (
            "architect",
            &["oracle", "prometheus", "ultrabrain", "metis"][..],
            "Challenges architecture boundaries, invariants, and long-horizon risks.",
        ),
        (
            "critic",
            &["momus", "metis", "oracle", "prometheus"][..],
            "Reviews the plan for gaps, hidden assumptions, and verification weakness.",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, candidates, description))| {
        let entry = catalog.and_then(|catalog| find_catalog_entry(catalog, candidates));
        lane_from_catalog_entry(role, description, (index + 1) as u32, entry, candidates[0])
    })
    .collect()
}

fn find_catalog_entry<'a>(
    catalog: &'a AgentCatalog,
    candidates: &[&str],
) -> Option<&'a AgentCatalogEntry> {
    candidates.iter().find_map(|candidate| {
        catalog
            .entries
            .iter()
            .find(|entry| entry.name == *candidate)
    })
}

fn lane_from_catalog_entry(
    role: &str,
    description: &str,
    review_order: u32,
    entry: Option<&AgentCatalogEntry>,
    fallback_profile: &str,
) -> PlanConsensusLane {
    PlanConsensusLane {
        role: role.to_string(),
        profile: entry
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| fallback_profile.to_string()),
        agent_catalog_role: entry
            .map(|entry| entry.role.clone())
            .unwrap_or_else(|| "unresolved".to_string()),
        category_binding: entry.and_then(|entry| entry.category_binding.clone()),
        description: description.to_string(),
        required: true,
        review_order,
        can_redelegate: entry.is_some_and(|entry| entry.can_redelegate),
    }
}

pub fn validate_critic_iterations(requested: u32, max_iterations: u32) -> Result<u32, String> {
    if max_iterations == 0 {
        return Err("plan consensus max_iterations must be greater than zero".to_string());
    }
    if requested == 0 {
        return Err("critic_iterations must be greater than zero".to_string());
    }
    if requested > max_iterations {
        return Err(format!(
            "critic_iterations {requested} exceeds configured plan consensus max_iterations {max_iterations}"
        ));
    }
    Ok(requested)
}

pub fn validate_plan_consensus_artifact(
    artifact: &PlanConsensusArtifact,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if artifact.schema_version != PLAN_CONSENSUS_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version must be {PLAN_CONSENSUS_SCHEMA_VERSION}"
        ));
    }
    if artifact.workflow_id.trim().is_empty() {
        errors.push("workflow_id is required".to_string());
    }
    if artifact.plan_id.trim().is_empty() {
        errors.push("plan_id is required".to_string());
    }
    if artifact.task.trim().is_empty() {
        errors.push("task is required".to_string());
    }
    if let Err(err) =
        validate_critic_iterations(artifact.critic_iterations, artifact.max_iterations)
    {
        errors.push(err);
    }
    let lane_roles = artifact
        .lanes
        .iter()
        .map(|lane| lane.role.as_str())
        .collect::<Vec<_>>();
    for required_role in ["planner", "architect", "critic"] {
        if !lane_roles.contains(&required_role) {
            errors.push(format!("missing required consensus lane `{required_role}`"));
        }
    }
    if artifact.options.is_empty() {
        errors.push("at least one viable option is required".to_string());
    }
    if !artifact.options.is_empty()
        && !artifact
            .options
            .iter()
            .any(|option| option.id == artifact.chosen_option)
    {
        errors.push("chosen_option must reference one of the viable options".to_string());
    }
    if artifact.adr.trim().is_empty() {
        errors.push("ADR text is required".to_string());
    }
    if artifact.risks.is_empty() {
        errors.push("at least one risk or pre-mortem item is required".to_string());
    }
    if artifact.test_plan.is_empty() {
        errors.push("at least one test plan item is required".to_string());
    }
    if artifact.staffing.is_empty() {
        errors.push("at least one staffing or lane guidance item is required".to_string());
    }
    if artifact.evidence_refs.is_empty() {
        errors.push("at least one evidence ref is required".to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn plan_consensus_artifact_name(plan_id: &str) -> String {
    format!(
        "{PLAN_CONSENSUS_ARTIFACT_DIR}/{}.json",
        artifact_safe_id(plan_id)
    )
}

pub fn plan_consensus_metadata(artifact: &PlanConsensusArtifact) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::from([
        (
            METADATA_ARTIFACT_KIND.to_string(),
            PLAN_CONSENSUS_ARTIFACT_KIND.to_string(),
        ),
        (METADATA_PLAN_ID.to_string(), artifact.plan_id.clone()),
        (
            METADATA_PLAN_STATUS.to_string(),
            plan_status_from_verdict(&artifact.critic_verdict).to_string(),
        ),
        (
            METADATA_CRITIC_VERDICT.to_string(),
            artifact.critic_verdict.clone(),
        ),
        (
            METADATA_CRITIC_ITERATIONS.to_string(),
            artifact.critic_iterations.to_string(),
        ),
        (
            METADATA_MAX_ITERATIONS.to_string(),
            artifact.max_iterations.to_string(),
        ),
        (
            METADATA_LANE_COUNT.to_string(),
            artifact.lanes.len().to_string(),
        ),
    ]);
    for (index, lane) in artifact.lanes.iter().enumerate() {
        let prefix = format!("{METADATA_LANE_PREFIX}{index}.");
        metadata.insert(format!("{prefix}role"), lane.role.clone());
        metadata.insert(format!("{prefix}profile"), lane.profile.clone());
        metadata.insert(
            format!("{prefix}agent_catalog_role"),
            lane.agent_catalog_role.clone(),
        );
        metadata.insert(
            format!("{prefix}review_order"),
            lane.review_order.to_string(),
        );
        metadata.insert(format!("{prefix}required"), lane.required.to_string());
        metadata.insert(
            format!("{prefix}can_redelegate"),
            lane.can_redelegate.to_string(),
        );
        if let Some(binding) = lane.category_binding.as_ref() {
            metadata.insert(format!("{prefix}category_binding"), binding.clone());
        }
    }
    metadata
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
        "plan".to_string()
    } else {
        safe
    }
}

fn plan_status_from_verdict(verdict: &str) -> &'static str {
    match verdict {
        "revise" | "needs_revision" | "blocked" => "needs_iteration",
        "rejected" | "failed" => "failed",
        _ => "approved",
    }
}

pub fn apply_plan_consensus_evidence(
    projection: &mut BTreeMap<String, PlanConsensusProjection>,
    payload: &WorkflowEvidenceRecordedEvent,
) {
    if payload.category != PLAN_CONSENSUS_EVIDENCE_CATEGORY {
        return;
    }
    let plan_id = payload
        .metadata
        .get(METADATA_PLAN_ID)
        .cloned()
        .or_else(|| payload.acceptance_ref.clone())
        .unwrap_or_else(|| payload.workflow_id.clone());
    let critic_verdict = payload
        .metadata
        .get(METADATA_CRITIC_VERDICT)
        .cloned()
        .unwrap_or_else(|| "approved".to_string());
    let status = payload
        .metadata
        .get(METADATA_PLAN_STATUS)
        .cloned()
        .unwrap_or_else(|| plan_status_from_verdict(&critic_verdict).to_string());
    projection.insert(
        plan_id.clone(),
        PlanConsensusProjection {
            workflow_id: payload.workflow_id.clone(),
            plan_id,
            status,
            critic_verdict,
            critic_iterations: parse_metadata_u32(&payload.metadata, METADATA_CRITIC_ITERATIONS, 1),
            max_iterations: parse_metadata_u32(&payload.metadata, METADATA_MAX_ITERATIONS, 1),
            artifact_path: payload.artifact_path.clone(),
            artifact_digest: payload.artifact_digest.clone(),
            evidence_ref: payload.acceptance_ref.clone(),
            lanes: parse_lanes_from_metadata(&payload.metadata),
        },
    );
}

pub fn project_plan_consensus<'a>(
    events: impl IntoIterator<Item = &'a EventV1>,
) -> BTreeMap<String, PlanConsensusProjection> {
    let mut projection = BTreeMap::new();
    for event in events {
        if let EventV1::WorkflowEvidenceRecorded(payload) = event {
            apply_plan_consensus_evidence(&mut projection, payload);
        }
    }
    projection
}

fn parse_lanes_from_metadata(metadata: &BTreeMap<String, String>) -> Vec<PlanConsensusLane> {
    let lane_count = parse_metadata_u32(metadata, METADATA_LANE_COUNT, 0);
    (0..lane_count)
        .filter_map(|index| {
            let prefix = format!("{METADATA_LANE_PREFIX}{index}.");
            let role = metadata.get(&format!("{prefix}role"))?.clone();
            let profile = metadata.get(&format!("{prefix}profile"))?.clone();
            Some(PlanConsensusLane {
                role,
                profile,
                agent_catalog_role: metadata
                    .get(&format!("{prefix}agent_catalog_role"))
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string()),
                category_binding: metadata.get(&format!("{prefix}category_binding")).cloned(),
                description: String::new(),
                required: metadata
                    .get(&format!("{prefix}required"))
                    .is_none_or(|value| value == "true"),
                review_order: metadata
                    .get(&format!("{prefix}review_order"))
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(index + 1),
                can_redelegate: metadata
                    .get(&format!("{prefix}can_redelegate"))
                    .is_some_and(|value| value == "true"),
            })
        })
        .collect()
}

fn parse_metadata_u32(metadata: &BTreeMap<String, String>, key: &str, fallback: u32) -> u32 {
    metadata
        .get(key)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use crate::agent_catalog::resolve_agent_catalog;
    use crate::config::load_config_from_str;

    use super::*;

    #[test]
    fn consensus_lanes_resolve_through_agent_catalog() {
        let config = load_config_from_str(
            r#"
            {
              provider: {
                default: {
                  type: "openai_compatible",
                  baseURL: "http://127.0.0.1:8317/v1",
                  apiKey: "DUMMY",
                  models: { "gpt-5.4-mini": { name: "GPT-5.4 mini" } }
                }
              },
              model: "default/gpt-5.4-mini",
              permission: "ask",
              default_agent: "build"
            }
            "#,
        )
        .expect("config parses");
        let catalog = resolve_agent_catalog(&config);
        let lanes = resolve_plan_consensus_lanes(Some(&catalog));

        assert_eq!(lanes[0].role, "planner");
        assert_eq!(lanes[0].profile, "plan");
        assert_eq!(lanes[1].role, "architect");
        assert_eq!(lanes[1].profile, "oracle");
        assert_eq!(lanes[2].role, "critic");
        assert_eq!(lanes[2].profile, "momus");
        assert!(lanes
            .iter()
            .all(|lane| lane.agent_catalog_role != "unresolved"));
    }

    #[test]
    fn plan_consensus_projection_is_replay_derived_from_evidence_metadata() {
        let artifact = PlanConsensusArtifact {
            schema_version: 1,
            workflow_id: "wf_plan".to_string(),
            plan_id: "plan_1".to_string(),
            task: "Ship workflow planning".to_string(),
            snapshot_ref: Some("ctx_1".to_string()),
            lanes: resolve_plan_consensus_lanes(None),
            max_iterations: 5,
            critic_iterations: 2,
            critic_verdict: "approved".to_string(),
            principles: vec!["Prefer replay-derived status".to_string()],
            decision_drivers: vec!["Side-effect-free replay".to_string()],
            options: vec![PlanConsensusOption {
                id: "event-metadata".to_string(),
                summary: "Use workflow evidence metadata".to_string(),
                pros: vec!["Replay-safe".to_string()],
                cons: vec!["Compact".to_string()],
            }],
            chosen_option: "event-metadata".to_string(),
            rejected_alternatives: vec!["runtime-only state".to_string()],
            adr: "Record plan evidence as an artifact ref.".to_string(),
            work_breakdown: vec!["Add projection".to_string()],
            risks: vec!["Metadata drift".to_string()],
            test_plan: vec!["cargo test -p harness-core plan_consensus".to_string()],
            manual_qa_plan: vec!["Inspect JSON artifact".to_string()],
            staffing: vec!["planner/architect/critic".to_string()],
            handoff_options: vec!["workflow.run".to_string()],
            acceptance_criteria: vec!["ADR present".to_string()],
            evidence_refs: vec!["ctx_1".to_string()],
        };
        let event = EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
            workflow_id: artifact.workflow_id.clone(),
            category: PLAN_CONSENSUS_EVIDENCE_CATEGORY.to_string(),
            summary: "plan generated".to_string(),
            artifact_path: Some("artifacts/workflows/plan_consensus/plan_1.json".to_string()),
            artifact_digest: Some("digest".to_string()),
            acceptance_ref: Some(artifact.plan_id.clone()),
            metadata: plan_consensus_metadata(&artifact),
        });

        let projection = project_plan_consensus([event].iter());
        let plan = &projection["plan_1"];
        assert_eq!(plan.workflow_id, "wf_plan");
        assert_eq!(plan.status, "approved");
        assert_eq!(plan.critic_iterations, 2);
        assert_eq!(plan.max_iterations, 5);
        assert_eq!(plan.lanes.len(), 3);
    }

    #[test]
    fn critic_iterations_are_bounded() {
        assert_eq!(validate_critic_iterations(2, 5).unwrap(), 2);
        assert!(validate_critic_iterations(0, 5).is_err());
        assert!(validate_critic_iterations(6, 5).is_err());
    }
}

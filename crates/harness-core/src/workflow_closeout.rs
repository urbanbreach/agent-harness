//! Replay-derived workflow closeout contracts.
//!
//! This module defines the shared policy/readiness/report structs used by CLI,
//! dossier, doctor, and native-tool surfaces. It is deliberately a pure
//! projection/evaluation layer: it does not append events or perform side
//! effects.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::persistent_task::PersistentTaskProjection;
use crate::workflow::{
    WorkflowProjection, WorkflowQuestionProjection, WorkflowSignoffPolicy,
    WorkflowTeamCloseoutProjection, PENDING_TASK_WAIVER_DECISION, SIGNOFF_WAIVER_DECISION,
    WORKFLOW_QUESTION_STATUS_ANSWERED, WORKFLOW_QUESTION_STATUS_CLOSED,
};

pub const WORKFLOW_CLOSEOUT_SCHEMA_VERSION: u32 = 1;
pub const WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID: &str = "workflow.closeout.default";
pub const WORKFLOW_CLOSEOUT_SIMULATED_POLICY_ID: &str = "workflow.closeout.simulated";
pub const WORKFLOW_CLOSEOUT_GOAL_POLICY_ID: &str = "workflow.closeout.goal";
pub const WORKFLOW_CLOSEOUT_TEAM_POLICY_ID: &str = "workflow.closeout.team";
pub const WORKFLOW_CLOSEOUT_MISSION_POLICY_ID: &str = "workflow.closeout.mission";
pub const WORKFLOW_CLOSEOUT_WIKI_POLICY_ID: &str = "workflow.closeout.wiki";
pub const WORKFLOW_CLOSEOUT_LIVE_POLICY_ID: &str = "workflow.closeout.live";

pub const WORKFLOW_CLOSEOUT_DIMENSION_EVIDENCE: &str = "evidence";
pub const WORKFLOW_CLOSEOUT_DIMENSION_TASKS: &str = "tasks";
pub const WORKFLOW_CLOSEOUT_DIMENSION_CONTINUATIONS: &str = "continuations";
pub const WORKFLOW_CLOSEOUT_DIMENSION_ARTIFACTS: &str = "artifacts";
pub const WORKFLOW_CLOSEOUT_DIMENSION_DOSSIER: &str = "dossier";
pub const WORKFLOW_CLOSEOUT_DIMENSION_PLAN: &str = "plan";
pub const WORKFLOW_CLOSEOUT_DIMENSION_GOAL: &str = "goal";
pub const WORKFLOW_CLOSEOUT_DIMENSION_TEAM: &str = "team";
pub const WORKFLOW_CLOSEOUT_DIMENSION_MISSION: &str = "mission";
pub const WORKFLOW_CLOSEOUT_DIMENSION_WIKI: &str = "wiki";
pub const WORKFLOW_CLOSEOUT_DIMENSION_QUESTION: &str = "question";
pub const WORKFLOW_CLOSEOUT_DIMENSION_HOOKS: &str = "hooks";
pub const WORKFLOW_CLOSEOUT_DIMENSION_STATE: &str = "state";
pub const WORKFLOW_CLOSEOUT_DIMENSION_TRACE: &str = "trace";
pub const WORKFLOW_CLOSEOUT_DIMENSION_REVIEW: &str = "review";
pub const WORKFLOW_CLOSEOUT_DIMENSION_SECURITY: &str = "security";
pub const WORKFLOW_CLOSEOUT_DIMENSION_QA: &str = "qa";
pub const WORKFLOW_CLOSEOUT_DIMENSION_PERFORMANCE: &str = "performance";
pub const WORKFLOW_CLOSEOUT_DIMENSION_VISUAL: &str = "visual";
pub const WORKFLOW_CLOSEOUT_DIMENSION_ADVISOR: &str = "advisor";
pub const WORKFLOW_CLOSEOUT_DIMENSION_SETUP: &str = "setup";
pub const WORKFLOW_CLOSEOUT_DIMENSION_SKILL: &str = "skill";
pub const WORKFLOW_CLOSEOUT_DIMENSION_STATUS_HUD: &str = "status_hud";
pub const WORKFLOW_CLOSEOUT_DIMENSION_NOTE_MEMORY: &str = "note_memory";

pub const WORKFLOW_CLOSEOUT_DOSSIER_EVIDENCE_CATEGORY: &str = "evidence.dossier";
pub const WORKFLOW_CLOSEOUT_HOOK_EVIDENCE_CATEGORY: &str = "evidence.hook_decision";
pub const WORKFLOW_CLOSEOUT_STATE_EVIDENCE_CATEGORY: &str = "evidence.state";
pub const WORKFLOW_CLOSEOUT_TRACE_EVIDENCE_CATEGORY: &str = "evidence.trace";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct WorkflowCloseoutPolicyId(pub String);

impl WorkflowCloseoutPolicyId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl From<&str> for WorkflowCloseoutPolicyId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl std::fmt::Display for WorkflowCloseoutPolicyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowCloseoutPolicyConfig {
    #[serde(default = "default_policy_enabled")]
    pub enabled: bool,
    #[serde(default = "default_policy_version")]
    pub version: u32,
    #[serde(default = "default_require_evidence", alias = "requireEvidence")]
    pub require_evidence: bool,
    #[serde(default = "default_require_dossier", alias = "requireDossier")]
    pub require_dossier: bool,
    #[serde(
        default = "default_require_export_artifact",
        alias = "requireExportArtifact"
    )]
    pub require_export_artifact: bool,
    #[serde(default, alias = "allowLiveApproval")]
    pub allow_live_approval: bool,
}

impl Default for WorkflowCloseoutPolicyConfig {
    fn default() -> Self {
        Self {
            enabled: default_policy_enabled(),
            version: default_policy_version(),
            require_evidence: default_require_evidence(),
            require_dossier: default_require_dossier(),
            require_export_artifact: default_require_export_artifact(),
            allow_live_approval: false,
        }
    }
}

impl WorkflowCloseoutPolicyConfig {
    pub fn live() -> Self {
        Self {
            allow_live_approval: true,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowCloseoutPolicy {
    pub policy_id: WorkflowCloseoutPolicyId,
    pub version: u32,
    pub enabled: bool,
    pub require_evidence: bool,
    pub require_dossier: bool,
    pub require_export_artifact: bool,
    pub allow_live_approval: bool,
}

impl WorkflowCloseoutPolicy {
    pub fn from_config(
        policy_id: impl Into<String>,
        config: WorkflowCloseoutPolicyConfig,
    ) -> Result<Self, WorkflowCloseoutPolicyError> {
        let policy_id = policy_id.into();
        if !is_builtin_policy_id(&policy_id) {
            return Err(WorkflowCloseoutPolicyError::UnknownPolicy {
                policy_id,
                known_policy_ids: builtin_policy_ids()
                    .iter()
                    .map(|id| (*id).to_string())
                    .collect(),
            });
        }
        if !config.enabled {
            return Err(WorkflowCloseoutPolicyError::DisabledPolicy { policy_id });
        }
        Ok(Self {
            policy_id: WorkflowCloseoutPolicyId::new(policy_id),
            version: config.version,
            enabled: config.enabled,
            require_evidence: config.require_evidence,
            require_dossier: config.require_dossier,
            require_export_artifact: config.require_export_artifact,
            allow_live_approval: config.allow_live_approval,
        })
    }

    pub fn default_policy() -> Self {
        Self::from_config(
            WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID,
            WorkflowCloseoutPolicyConfig::default(),
        )
        .expect("built-in default closeout policy is valid")
    }

    pub fn evaluate(
        &self,
        projection: &WorkflowProjection,
        workflow_id: impl Into<String>,
        persistent_tasks: &PersistentTaskProjection,
        signoff_policy: &WorkflowSignoffPolicy,
    ) -> WorkflowCloseoutReadiness {
        evaluate_closeout_readiness(
            self,
            projection,
            workflow_id,
            persistent_tasks,
            signoff_policy,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowCloseoutPolicyError {
    UnknownPolicy {
        policy_id: String,
        known_policy_ids: Vec<String>,
    },
    DisabledPolicy {
        policy_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowSignoffDecision {
    Approve,
    Fail,
    RequestEvidence,
    Waive,
    Abort,
    Redirect,
    ApproveLive,
}

impl WorkflowSignoffDecision {
    pub fn requires_reason(&self) -> bool {
        !matches!(self, Self::Approve)
    }

    pub fn requires_scope(&self) -> bool {
        matches!(self, Self::Waive | Self::Redirect)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowLegalNextAction {
    pub action: WorkflowSignoffDecision,
    pub requires_reason: bool,
    pub requires_scope: bool,
}

impl WorkflowLegalNextAction {
    pub fn new(action: WorkflowSignoffDecision) -> Self {
        Self {
            requires_reason: action.requires_reason(),
            requires_scope: action.requires_scope(),
            action,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowCloseoutDimension {
    pub id: String,
    pub label: String,
    pub allowed: bool,
    pub waived: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocking_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recovery_hints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_seq: Option<u64>,
}

impl WorkflowCloseoutDimension {
    pub fn allowed(id: &str, label: &str) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            allowed: true,
            waived: false,
            blocking_refs: Vec::new(),
            missing_categories: Vec::new(),
            domain_reasons: Vec::new(),
            recovery_hints: Vec::new(),
            last_event_seq: None,
        }
    }

    fn blocked(
        id: &str,
        label: &str,
        waived: bool,
        blocking_refs: Vec<String>,
        missing_categories: Vec<String>,
        domain_reasons: Vec<String>,
        recovery_hints: Vec<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            allowed: waived || (blocking_refs.is_empty() && missing_categories.is_empty()),
            waived,
            blocking_refs,
            missing_categories,
            domain_reasons,
            recovery_hints,
            last_event_seq: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowCloseoutWaiver {
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub operator: String,
    pub policy_id: WorkflowCloseoutPolicyId,
    pub policy_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_seq: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowCloseoutReadiness {
    pub policy_id: WorkflowCloseoutPolicyId,
    pub policy_version: u32,
    pub schema_version: u32,
    pub workflow_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub terminal: bool,
    pub overall_allowed: bool,
    pub legal_next_actions: Vec<WorkflowLegalNextAction>,
    pub dimensions: Vec<WorkflowCloseoutDimension>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waivers: Vec<WorkflowCloseoutWaiver>,
    pub stale_export: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_seq: Option<u64>,
}

impl WorkflowCloseoutReadiness {
    pub fn dimension(&self, id: &str) -> Option<&WorkflowCloseoutDimension> {
        self.dimensions.iter().find(|dimension| dimension.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSignoffReport {
    pub workflow_id: String,
    pub decision: WorkflowSignoffDecision,
    pub audit_only: bool,
    pub accepted: bool,
    pub closeout: WorkflowCloseoutReadiness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStatusCloseoutReport {
    pub closeout: WorkflowCloseoutReadiness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDossierCloseoutSection {
    pub policy_id: WorkflowCloseoutPolicyId,
    pub policy_version: u32,
    pub schema_version: u32,
    pub matrix: Vec<WorkflowCloseoutDimension>,
    pub legal_next_actions: Vec<WorkflowLegalNextAction>,
    pub stale_export: bool,
    pub require_export_artifact: bool,
    pub overall_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowCatalogHealthReport {
    pub visible: Vec<String>,
    pub missing: Vec<String>,
    pub disabled: Vec<String>,
    pub shadowed: Vec<String>,
    pub resolution_roots: Vec<String>,
}

pub fn builtin_policy_ids() -> &'static [&'static str] {
    &[
        WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID,
        WORKFLOW_CLOSEOUT_SIMULATED_POLICY_ID,
        WORKFLOW_CLOSEOUT_GOAL_POLICY_ID,
        WORKFLOW_CLOSEOUT_TEAM_POLICY_ID,
        WORKFLOW_CLOSEOUT_MISSION_POLICY_ID,
        WORKFLOW_CLOSEOUT_WIKI_POLICY_ID,
        WORKFLOW_CLOSEOUT_LIVE_POLICY_ID,
    ]
}

pub fn is_builtin_policy_id(policy_id: &str) -> bool {
    builtin_policy_ids().contains(&policy_id)
}

pub fn default_policy_map() -> BTreeMap<String, WorkflowCloseoutPolicyConfig> {
    builtin_policy_ids()
        .iter()
        .map(|policy_id| {
            let config = if *policy_id == WORKFLOW_CLOSEOUT_LIVE_POLICY_ID {
                WorkflowCloseoutPolicyConfig::live()
            } else {
                WorkflowCloseoutPolicyConfig::default()
            };
            ((*policy_id).to_string(), config)
        })
        .collect()
}

pub fn evaluate_closeout_readiness(
    policy: &WorkflowCloseoutPolicy,
    projection: &WorkflowProjection,
    workflow_id: impl Into<String>,
    persistent_tasks: &PersistentTaskProjection,
    signoff_policy: &WorkflowSignoffPolicy,
) -> WorkflowCloseoutReadiness {
    let workflow_id = workflow_id.into();
    let run = projection.workflows.get(&workflow_id);
    let signoff = signoff_policy.evaluate(projection, workflow_id.clone());
    let tasks = projection.task_readiness(workflow_id.clone(), persistent_tasks);
    let completion =
        projection.completion_readiness(workflow_id.clone(), persistent_tasks, signoff_policy);
    let evidence = projection.evidence.get(&workflow_id);
    let evidence_categories = run
        .map(|run| run.evidence_categories.clone())
        .unwrap_or_default();
    let terminal = run.is_some_and(|run| run.terminal);
    let waivers = collect_waivers(policy, run);

    let dimensions = apply_dimension_waivers(
        vec![
            evidence_dimension(policy, &signoff, &waivers),
            tasks_dimension(&tasks),
            continuations_dimension(&completion.active_continuation_ids),
            artifacts_dimension(evidence),
            dossier_dimension(policy, &evidence_categories),
            plan_dimension(projection, &workflow_id),
            goal_dimension(projection, &workflow_id),
            team_dimension(projection, &workflow_id),
            mission_dimension(projection, &workflow_id),
            wiki_dimension(evidence),
            question_dimension(projection, &workflow_id),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_HOOKS,
                "Hook decisions",
                WORKFLOW_CLOSEOUT_HOOK_EVIDENCE_CATEGORY,
                "hook_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_STATE,
                "State/memory",
                WORKFLOW_CLOSEOUT_STATE_EVIDENCE_CATEGORY,
                "state_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_TRACE,
                "Trace",
                WORKFLOW_CLOSEOUT_TRACE_EVIDENCE_CATEGORY,
                "trace_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_REVIEW,
                "Review findings",
                crate::workflow_registry::REVIEW_EVIDENCE_CATEGORY,
                "review_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_SECURITY,
                "Security review",
                crate::workflow_registry::SECURITY_REVIEW_EVIDENCE_CATEGORY,
                "security_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_QA,
                "QA scenarios",
                crate::workflow_registry::QA_EVIDENCE_CATEGORY,
                "qa_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_PERFORMANCE,
                "Performance evaluation",
                crate::workflow_registry::PERFORMANCE_EVIDENCE_CATEGORY,
                "performance_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_VISUAL,
                "Visual verdict",
                crate::workflow_registry::VISUAL_EVIDENCE_CATEGORY,
                "visual_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_ADVISOR,
                "Advisor evidence",
                crate::workflow_registry::ADVISOR_EVIDENCE_CATEGORY,
                "advisor_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_SETUP,
                "Setup/doctor",
                crate::workflow_registry::SETUP_DOCTOR_EVIDENCE_CATEGORY,
                "setup_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_SKILL,
                "Skill management",
                crate::workflow_registry::SKILL_MANAGEMENT_EVIDENCE_CATEGORY,
                "skill_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_STATUS_HUD,
                "Status/HUD",
                crate::workflow_registry::STATUS_HUD_EVIDENCE_CATEGORY,
                "status_hud_status",
            ),
            metadata_status_dimension(
                evidence,
                WORKFLOW_CLOSEOUT_DIMENSION_NOTE_MEMORY,
                "Note/memory",
                crate::workflow_registry::NOTE_MEMORY_EVIDENCE_CATEGORY,
                "note_memory_status",
            ),
        ],
        &waivers,
    );

    let overall_allowed = policy.enabled && dimensions.iter().all(|dimension| dimension.allowed);
    let legal_next_actions = legal_next_actions(policy, overall_allowed, terminal);

    WorkflowCloseoutReadiness {
        policy_id: policy.policy_id.clone(),
        policy_version: policy.version,
        schema_version: WORKFLOW_CLOSEOUT_SCHEMA_VERSION,
        workflow_id,
        run_id: None,
        terminal,
        overall_allowed,
        legal_next_actions,
        dimensions,
        waivers,
        stale_export: false,
        last_event_seq: None,
    }
}

fn evidence_dimension(
    policy: &WorkflowCloseoutPolicy,
    signoff: &crate::workflow::WorkflowSignoffReadiness,
    waivers: &[WorkflowCloseoutWaiver],
) -> WorkflowCloseoutDimension {
    if !policy.require_evidence {
        return WorkflowCloseoutDimension::allowed(
            WORKFLOW_CLOSEOUT_DIMENSION_EVIDENCE,
            "Required evidence",
        );
    }
    let dimension_waived =
        signoff.waived || dimension_fully_waived(waivers, WORKFLOW_CLOSEOUT_DIMENSION_EVIDENCE);
    let missing_categories = if dimension_waived {
        Vec::new()
    } else {
        signoff
            .missing_evidence_categories
            .iter()
            .filter(|category| !evidence_category_waived(waivers, category))
            .cloned()
            .collect::<Vec<_>>()
    };
    let waived = dimension_waived
        || (!signoff.missing_evidence_categories.is_empty() && missing_categories.is_empty());
    WorkflowCloseoutDimension::blocked(
        WORKFLOW_CLOSEOUT_DIMENSION_EVIDENCE,
        "Required evidence",
        waived,
        missing_categories
            .iter()
            .map(|category| format!("evidence:{category}"))
            .collect(),
        missing_categories.clone(),
        missing_categories
            .iter()
            .map(|category| format!("required evidence category `{category}` is missing"))
            .collect(),
        if missing_categories.is_empty() {
            Vec::new()
        } else {
            vec![format!(
                "record evidence for: {} or append `{SIGNOFF_WAIVER_DECISION}`",
                missing_categories.join(", ")
            )]
        },
    )
}

fn tasks_dimension(tasks: &crate::workflow::WorkflowTaskReadiness) -> WorkflowCloseoutDimension {
    let incomplete = tasks.incomplete_task_ids();
    WorkflowCloseoutDimension::blocked(
        WORKFLOW_CLOSEOUT_DIMENSION_TASKS,
        "Workflow-owned tasks",
        tasks.waived,
        incomplete
            .iter()
            .map(|task_id| format!("task:{task_id}"))
            .collect(),
        Vec::new(),
        incomplete
            .iter()
            .map(|task_id| format!("workflow-owned task `{task_id}` is incomplete"))
            .collect(),
        if incomplete.is_empty() {
            Vec::new()
        } else {
            vec![format!(
                "complete/cancel workflow-owned tasks: {} or append `{PENDING_TASK_WAIVER_DECISION}`",
                incomplete.join(", ")
            )]
        },
    )
}

fn continuations_dimension(active_ids: &[String]) -> WorkflowCloseoutDimension {
    WorkflowCloseoutDimension::blocked(
        WORKFLOW_CLOSEOUT_DIMENSION_CONTINUATIONS,
        "Active continuations",
        false,
        active_ids
            .iter()
            .map(|continuation_id| format!("continuation:{continuation_id}"))
            .collect(),
        Vec::new(),
        active_ids
            .iter()
            .map(|continuation_id| {
                format!("workflow-owned continuation `{continuation_id}` is still active")
            })
            .collect(),
        if active_ids.is_empty() {
            Vec::new()
        } else {
            vec![format!(
                "stop or resolve active workflow continuations: {}",
                active_ids.join(", ")
            )]
        },
    )
}

fn artifacts_dimension(
    evidence: Option<&Vec<crate::event::WorkflowEvidenceRecordedEvent>>,
) -> WorkflowCloseoutDimension {
    let missing_artifact_refs = evidence
        .into_iter()
        .flatten()
        .filter(|event| event.category == "evidence.artifact" && event.artifact_path.is_none())
        .map(|event| {
            event
                .acceptance_ref
                .clone()
                .unwrap_or_else(|| event.summary.clone())
        })
        .collect::<Vec<_>>();
    WorkflowCloseoutDimension::blocked(
        WORKFLOW_CLOSEOUT_DIMENSION_ARTIFACTS,
        "Artifact refs",
        false,
        missing_artifact_refs
            .iter()
            .map(|artifact| format!("artifact:{artifact}"))
            .collect(),
        Vec::new(),
        missing_artifact_refs
            .iter()
            .map(|artifact| format!("artifact evidence `{artifact}` is missing artifact_path"))
            .collect(),
        if missing_artifact_refs.is_empty() {
            Vec::new()
        } else {
            vec!["record artifact_path/digest for artifact evidence".to_string()]
        },
    )
}

fn dossier_dimension(
    policy: &WorkflowCloseoutPolicy,
    evidence_categories: &BTreeSet<String>,
) -> WorkflowCloseoutDimension {
    if !policy.require_dossier {
        return WorkflowCloseoutDimension::allowed(
            WORKFLOW_CLOSEOUT_DIMENSION_DOSSIER,
            "Replay-derived dossier",
        );
    }
    let missing_export = policy.require_export_artifact
        && !evidence_categories.contains(WORKFLOW_CLOSEOUT_DOSSIER_EVIDENCE_CATEGORY);
    WorkflowCloseoutDimension::blocked(
        WORKFLOW_CLOSEOUT_DIMENSION_DOSSIER,
        "Replay-derived dossier",
        false,
        if missing_export {
            vec!["dossier:export_artifact".to_string()]
        } else {
            Vec::new()
        },
        if missing_export {
            vec![WORKFLOW_CLOSEOUT_DOSSIER_EVIDENCE_CATEGORY.to_string()]
        } else {
            Vec::new()
        },
        if missing_export {
            vec!["policy requires a dossier export artifact".to_string()]
        } else {
            Vec::new()
        },
        if missing_export {
            vec!["run workflow dossier export and record evidence.dossier".to_string()]
        } else {
            Vec::new()
        },
    )
}

fn plan_dimension(projection: &WorkflowProjection, workflow_id: &str) -> WorkflowCloseoutDimension {
    let plans = projection
        .plan_consensus
        .values()
        .filter(|plan| plan.workflow_id == workflow_id)
        .collect::<Vec<_>>();
    if plans.is_empty() {
        return WorkflowCloseoutDimension::allowed(
            WORKFLOW_CLOSEOUT_DIMENSION_PLAN,
            "Plan consensus",
        );
    }
    let mut blocking_refs = Vec::new();
    let mut missing_categories = Vec::new();
    let mut domain_reasons = Vec::new();
    for plan in plans {
        if plan.status != "approved" {
            blocking_refs.push(format!("plan:{}", plan.plan_id));
            missing_categories.push("plan_approved".to_string());
            domain_reasons.push(format!(
                "plan `{}` is `{}` rather than approved",
                plan.plan_id, plan.status
            ));
        }
        if plan.artifact_path.is_none() {
            blocking_refs.push(format!("plan_artifact:{}", plan.plan_id));
            missing_categories.push("plan_artifact".to_string());
            domain_reasons.push(format!("plan `{}` is missing artifact_path", plan.plan_id));
        }
    }
    WorkflowCloseoutDimension::blocked(
        WORKFLOW_CLOSEOUT_DIMENSION_PLAN,
        "Plan consensus",
        false,
        dedup(blocking_refs),
        dedup(missing_categories),
        dedup(domain_reasons),
        vec!["record approved plan consensus evidence with an artifact ref".to_string()],
    )
}

fn goal_dimension(projection: &WorkflowProjection, workflow_id: &str) -> WorkflowCloseoutDimension {
    let goals = projection
        .goal_ledger
        .goals
        .values()
        .filter(|goal| goal.workflow_id == workflow_id)
        .collect::<Vec<_>>();
    if goals.is_empty() {
        return WorkflowCloseoutDimension::allowed(WORKFLOW_CLOSEOUT_DIMENSION_GOAL, "Goal ledger");
    }
    let mut blocking_refs = Vec::new();
    let mut missing_categories = Vec::new();
    let mut domain_reasons = Vec::new();
    for goal in goals {
        if !goal.ready_for_completion {
            blocking_refs.push(format!("goal:{}", goal.goal_id));
            missing_categories.extend(goal.missing_completion_requirements.clone());
            domain_reasons.push(format!(
                "goal `{}` status `{}` is not ready for closeout",
                goal.goal_id, goal.status
            ));
        }
    }
    WorkflowCloseoutDimension::blocked(
        WORKFLOW_CLOSEOUT_DIMENSION_GOAL,
        "Goal ledger",
        false,
        dedup(blocking_refs),
        dedup(missing_categories),
        dedup(domain_reasons),
        vec!["checkpoint all goal stories and pass the final quality gate".to_string()],
    )
}

fn team_dimension(projection: &WorkflowProjection, workflow_id: &str) -> WorkflowCloseoutDimension {
    let teams = projection
        .teams
        .values()
        .filter(|team| team.workflow_id == workflow_id)
        .collect::<Vec<_>>();
    if teams.is_empty() {
        return WorkflowCloseoutDimension::allowed(WORKFLOW_CLOSEOUT_DIMENSION_TEAM, "Team state");
    }
    let mut blocking_refs = Vec::new();
    let mut missing_categories = Vec::new();
    let mut domain_reasons = Vec::new();
    for team in teams {
        collect_team_blockers(
            team,
            &mut blocking_refs,
            &mut missing_categories,
            &mut domain_reasons,
        );
    }
    WorkflowCloseoutDimension::blocked(
        WORKFLOW_CLOSEOUT_DIMENSION_TEAM,
        "Team state",
        false,
        dedup(blocking_refs),
        dedup(missing_categories),
        dedup(domain_reasons),
        vec![
            "complete/delete team tasks, record team verification evidence, and add synthesis or abort reason".to_string(),
        ],
    )
}

fn collect_team_blockers(
    team: &WorkflowTeamCloseoutProjection,
    blocking_refs: &mut Vec<String>,
    missing_categories: &mut Vec<String>,
    domain_reasons: &mut Vec<String>,
) {
    for (task_id, status) in &team.task_statuses {
        if matches!(status.as_str(), "pending" | "claimed" | "in_progress") {
            blocking_refs.push(format!("team_task:{}/{}", team.team_run_id, task_id));
            missing_categories.push("team_tasks_complete".to_string());
            domain_reasons.push(format!(
                "team `{}` task `{}` is `{}`",
                team.team_run_id, task_id, status
            ));
        }
    }
    if team.abort_reason.is_none() && team.verification_evidence_refs.is_empty() {
        blocking_refs.push(format!("team_verification:{}", team.team_run_id));
        missing_categories.push("team_verification_evidence".to_string());
        domain_reasons.push(format!(
            "team `{}` is missing verification evidence",
            team.team_run_id
        ));
    }
    if team.abort_reason.is_none() && team.synthesis_refs.is_empty() {
        blocking_refs.push(format!("team_synthesis:{}", team.team_run_id));
        missing_categories.push("team_synthesis".to_string());
        domain_reasons.push(format!(
            "team `{}` is missing synthesis evidence",
            team.team_run_id
        ));
    }
    for blocker in &team.blocker_refs {
        blocking_refs.push(format!("team_blocker:{}", blocker));
        missing_categories.push("team_blocker".to_string());
        domain_reasons.push(format!(
            "team `{}` has blocker `{blocker}`",
            team.team_run_id
        ));
    }
}

fn mission_dimension(
    projection: &WorkflowProjection,
    workflow_id: &str,
) -> WorkflowCloseoutDimension {
    let missions = projection
        .research_missions
        .missions
        .values()
        .filter(|mission| mission.workflow_id == workflow_id)
        .collect::<Vec<_>>();
    if missions.is_empty() {
        return WorkflowCloseoutDimension::allowed(
            WORKFLOW_CLOSEOUT_DIMENSION_MISSION,
            "Research mission",
        );
    }
    let mut blocking_refs = Vec::new();
    let mut missing_categories = Vec::new();
    let mut domain_reasons = Vec::new();
    for mission in missions {
        if !mission.ready_for_completion {
            blocking_refs.push(format!("mission:{}", mission.mission_id));
            missing_categories.extend(mission.missing_completion_requirements.clone());
            domain_reasons.push(format!(
                "research mission `{}` status `{}` is not ready",
                mission.mission_id, mission.status
            ));
        }
    }
    WorkflowCloseoutDimension::blocked(
        WORKFLOW_CLOSEOUT_DIMENSION_MISSION,
        "Research mission",
        false,
        dedup(blocking_refs),
        dedup(missing_categories),
        dedup(domain_reasons),
        vec!["record a research result with passing validator evidence".to_string()],
    )
}

fn wiki_dimension(
    evidence: Option<&Vec<crate::event::WorkflowEvidenceRecordedEvent>>,
) -> WorkflowCloseoutDimension {
    metadata_status_dimension(
        evidence,
        WORKFLOW_CLOSEOUT_DIMENSION_WIKI,
        "Wiki/project memory",
        crate::wiki::WIKI_EVIDENCE_CATEGORY,
        "wiki_lint_status",
    )
}

fn question_dimension(
    projection: &WorkflowProjection,
    workflow_id: &str,
) -> WorkflowCloseoutDimension {
    let questions = projection
        .questions
        .values()
        .filter(|question| question.workflow_id == workflow_id)
        .collect::<Vec<_>>();
    if questions.is_empty() {
        return WorkflowCloseoutDimension::allowed(
            WORKFLOW_CLOSEOUT_DIMENSION_QUESTION,
            "Question flow",
        );
    }
    let mut blocking_refs = Vec::new();
    let mut missing_categories = Vec::new();
    let mut domain_reasons = Vec::new();
    for question in questions {
        collect_question_blockers(
            question,
            &mut blocking_refs,
            &mut missing_categories,
            &mut domain_reasons,
        );
    }
    WorkflowCloseoutDimension::blocked(
        WORKFLOW_CLOSEOUT_DIMENSION_QUESTION,
        "Question flow",
        false,
        dedup(blocking_refs),
        dedup(missing_categories),
        dedup(domain_reasons),
        vec!["answer or explicitly close outstanding workflow questions".to_string()],
    )
}

fn collect_question_blockers(
    question: &WorkflowQuestionProjection,
    blocking_refs: &mut Vec<String>,
    missing_categories: &mut Vec<String>,
    domain_reasons: &mut Vec<String>,
) {
    if matches!(
        question.status.as_str(),
        WORKFLOW_QUESTION_STATUS_ANSWERED | WORKFLOW_QUESTION_STATUS_CLOSED
    ) {
        return;
    }
    blocking_refs.push(format!("question:{}", question.question_id));
    missing_categories.push(
        match question.status.as_str() {
            "timed_out" => "question_timed_out",
            "error" => "question_error",
            _ => "question_answer",
        }
        .to_string(),
    );
    let reason = question
        .reason_code
        .as_deref()
        .unwrap_or(question.status.as_str());
    domain_reasons.push(format!(
        "question `{}` is `{}` ({reason})",
        question.question_id, question.status
    ));
}

fn metadata_status_dimension(
    evidence: Option<&Vec<crate::event::WorkflowEvidenceRecordedEvent>>,
    dimension_id: &str,
    label: &str,
    category: &str,
    status_key: &str,
) -> WorkflowCloseoutDimension {
    let mut blocking_refs = Vec::new();
    let mut missing_categories = Vec::new();
    let mut domain_reasons = Vec::new();
    for event in evidence
        .into_iter()
        .flatten()
        .filter(|event| event.category == category)
    {
        let status = event
            .metadata
            .get(status_key)
            .or_else(|| event.metadata.get("status"))
            .map(String::as_str)
            .unwrap_or("passed");
        if is_blocking_status(status) {
            let reference = event
                .acceptance_ref
                .clone()
                .unwrap_or_else(|| event.summary.clone());
            blocking_refs.push(format!("{dimension_id}:{reference}"));
            missing_categories.push(format!("{dimension_id}_ready"));
            domain_reasons.push(format!(
                "{label} evidence `{reference}` has status `{status}`"
            ));
        }
    }
    WorkflowCloseoutDimension::blocked(
        dimension_id,
        label,
        false,
        dedup(blocking_refs),
        dedup(missing_categories),
        dedup(domain_reasons),
        vec![format!(
            "record passing {label} evidence or resolve blockers"
        )],
    )
}

fn is_blocking_status(status: &str) -> bool {
    matches!(
        status,
        "blocked"
            | "denied"
            | "error"
            | "failed"
            | "failing"
            | "rejected"
            | "stale"
            | "timeout"
            | "timed_out"
    )
}

fn dedup(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

fn collect_waivers(
    policy: &WorkflowCloseoutPolicy,
    run: Option<&crate::workflow::WorkflowRunProjection>,
) -> Vec<WorkflowCloseoutWaiver> {
    let Some(run) = run else {
        return Vec::new();
    };
    run.operator_decision_records
        .iter()
        .filter_map(|decision| {
            let scope = match decision.decision.as_str() {
                SIGNOFF_WAIVER_DECISION => Some("dimension:evidence".to_string()),
                PENDING_TASK_WAIVER_DECISION => Some("dimension:tasks".to_string()),
                other if other.starts_with("waive:") => {
                    Some(other.trim_start_matches("waive:").to_string())
                }
                _ => None,
            }?;
            Some(WorkflowCloseoutWaiver {
                scope,
                reason: decision.reason.clone(),
                operator: decision.operator.clone(),
                policy_id: policy.policy_id.clone(),
                policy_version: policy.version,
                event_seq: None,
            })
        })
        .collect()
}

fn apply_dimension_waivers(
    mut dimensions: Vec<WorkflowCloseoutDimension>,
    waivers: &[WorkflowCloseoutWaiver],
) -> Vec<WorkflowCloseoutDimension> {
    for dimension in &mut dimensions {
        if dimension_fully_waived(waivers, &dimension.id) {
            dimension.allowed = true;
            dimension.waived = true;
        }
    }
    dimensions
}

fn dimension_fully_waived(waivers: &[WorkflowCloseoutWaiver], dimension_id: &str) -> bool {
    let dimension_scope = format!("dimension:{dimension_id}");
    waivers.iter().any(|waiver| waiver.scope == dimension_scope)
}

fn evidence_category_waived(waivers: &[WorkflowCloseoutWaiver], category: &str) -> bool {
    let scoped_category =
        format!("dimension:{WORKFLOW_CLOSEOUT_DIMENSION_EVIDENCE}/category:{category}");
    let category_scope = format!("category:{category}");
    waivers.iter().any(|waiver| {
        waiver.scope == scoped_category
            || waiver.scope == category_scope
            || waiver.scope == category
    })
}

fn legal_next_actions(
    policy: &WorkflowCloseoutPolicy,
    overall_allowed: bool,
    terminal: bool,
) -> Vec<WorkflowLegalNextAction> {
    if terminal {
        return Vec::new();
    }
    let mut actions = Vec::new();
    if overall_allowed {
        actions.push(WorkflowLegalNextAction::new(
            WorkflowSignoffDecision::Approve,
        ));
    } else {
        actions.push(WorkflowLegalNextAction::new(
            WorkflowSignoffDecision::RequestEvidence,
        ));
        actions.push(WorkflowLegalNextAction::new(WorkflowSignoffDecision::Waive));
    }
    actions.push(WorkflowLegalNextAction::new(WorkflowSignoffDecision::Fail));
    actions.push(WorkflowLegalNextAction::new(WorkflowSignoffDecision::Abort));
    actions.push(WorkflowLegalNextAction::new(
        WorkflowSignoffDecision::Redirect,
    ));
    if policy.allow_live_approval {
        actions.push(WorkflowLegalNextAction::new(
            WorkflowSignoffDecision::ApproveLive,
        ));
    }
    actions
}

fn default_policy_enabled() -> bool {
    true
}

fn default_policy_version() -> u32 {
    1
}

fn default_require_evidence() -> bool {
    true
}

fn default_require_dossier() -> bool {
    true
}

fn default_require_export_artifact() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        builtin_policy_ids, WorkflowCloseoutPolicy, WorkflowCloseoutPolicyConfig,
        WorkflowSignoffDecision, WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID,
        WORKFLOW_CLOSEOUT_DIMENSION_ADVISOR, WORKFLOW_CLOSEOUT_DIMENSION_CONTINUATIONS,
        WORKFLOW_CLOSEOUT_DIMENSION_DOSSIER, WORKFLOW_CLOSEOUT_DIMENSION_EVIDENCE,
        WORKFLOW_CLOSEOUT_DIMENSION_GOAL, WORKFLOW_CLOSEOUT_DIMENSION_HOOKS,
        WORKFLOW_CLOSEOUT_DIMENSION_MISSION, WORKFLOW_CLOSEOUT_DIMENSION_NOTE_MEMORY,
        WORKFLOW_CLOSEOUT_DIMENSION_PERFORMANCE, WORKFLOW_CLOSEOUT_DIMENSION_PLAN,
        WORKFLOW_CLOSEOUT_DIMENSION_QA, WORKFLOW_CLOSEOUT_DIMENSION_QUESTION,
        WORKFLOW_CLOSEOUT_DIMENSION_REVIEW, WORKFLOW_CLOSEOUT_DIMENSION_SECURITY,
        WORKFLOW_CLOSEOUT_DIMENSION_SETUP, WORKFLOW_CLOSEOUT_DIMENSION_SKILL,
        WORKFLOW_CLOSEOUT_DIMENSION_STATE, WORKFLOW_CLOSEOUT_DIMENSION_STATUS_HUD,
        WORKFLOW_CLOSEOUT_DIMENSION_TASKS, WORKFLOW_CLOSEOUT_DIMENSION_TEAM,
        WORKFLOW_CLOSEOUT_DIMENSION_TRACE, WORKFLOW_CLOSEOUT_DIMENSION_VISUAL,
        WORKFLOW_CLOSEOUT_DIMENSION_WIKI, WORKFLOW_CLOSEOUT_HOOK_EVIDENCE_CATEGORY,
        WORKFLOW_CLOSEOUT_STATE_EVIDENCE_CATEGORY, WORKFLOW_CLOSEOUT_TRACE_EVIDENCE_CATEGORY,
    };
    use crate::config::{WorkflowRunRuntimeConfig, WorkflowRuntimeConfig};
    use crate::event::{
        ContinuationStartedEvent, EventV1, PersistentTask, PersistentTaskStatus, TeamBounds,
        TeamCreatedEvent, TeamSpec, TeamTask, TeamTaskCreatedEvent, TeamTaskStatus,
        TeamTaskUpdatedEvent, WorkflowEventMetadata, WorkflowEvidenceRecordedEvent,
        WorkflowOperatorDecisionRecordedEvent, WorkflowStartedEvent,
    };
    use crate::goal_ledger::{GOAL_LEDGER_ARTIFACT_KIND, GOAL_LEDGER_EVIDENCE_CATEGORY};
    use crate::persistent_task::PersistentTaskProjection;
    use crate::plan_consensus::PLAN_CONSENSUS_EVIDENCE_CATEGORY;
    use crate::research_mission::{
        RESEARCH_MISSION_ARTIFACT_KIND, RESEARCH_MISSION_EVIDENCE_CATEGORY,
        RESEARCH_RESULT_ARTIFACT_KIND,
    };
    use crate::wiki::WIKI_EVIDENCE_CATEGORY;
    use crate::workflow::{
        project_workflows, WorkflowSignoffPolicy, PENDING_TASK_WAIVER_DECISION,
        SIGNOFF_WAIVER_DECISION, SIMULATED_TOOL_EVIDENCE_CATEGORY,
        WORKFLOW_QUESTION_EVIDENCE_CATEGORY, WORKFLOW_QUESTION_METADATA_ANSWER_REF,
        WORKFLOW_QUESTION_METADATA_ID, WORKFLOW_QUESTION_METADATA_REASON_CODE,
        WORKFLOW_QUESTION_METADATA_STATUS, WORKFLOW_QUESTION_STATUS_ANSWERED,
        WORKFLOW_QUESTION_STATUS_ASKED, WORKFLOW_QUESTION_STATUS_CLOSED,
        WORKFLOW_QUESTION_STATUS_ERROR, WORKFLOW_QUESTION_STATUS_TIMED_OUT,
        WORKFLOW_TASK_METADATA_KEY,
    };
    use crate::workflow_registry::{
        ADVISOR_EVIDENCE_CATEGORY, NOTE_MEMORY_EVIDENCE_CATEGORY, PERFORMANCE_EVIDENCE_CATEGORY,
        QA_EVIDENCE_CATEGORY, REVIEW_EVIDENCE_CATEGORY, SECURITY_REVIEW_EVIDENCE_CATEGORY,
        SETUP_DOCTOR_EVIDENCE_CATEGORY, SKILL_MANAGEMENT_EVIDENCE_CATEGORY,
        STATUS_HUD_EVIDENCE_CATEGORY, VISUAL_EVIDENCE_CATEGORY,
    };

    fn start_event() -> EventV1 {
        EventV1::WorkflowStarted(WorkflowStartedEvent {
            workflow_id: "wf_closeout".to_string(),
            mode: "workflow.run".to_string(),
            owner: "leader".to_string(),
            lane: Some("lane.leader".to_string()),
            title: Some("closeout".to_string()),
            idempotency_key: None,
        })
    }

    fn pending_tasks() -> PersistentTaskProjection {
        let mut tasks = PersistentTaskProjection::default();
        tasks.tasks.insert(
            "pt_pending".to_string(),
            PersistentTask {
                version: 1,
                task_id: "pt_pending".to_string(),
                run_id: None,
                thread_id: None,
                subject: "finish closeout".to_string(),
                description: "pending workflow task".to_string(),
                status: PersistentTaskStatus::Pending,
                active_form: None,
                owner: Some("leader".to_string()),
                blocks: Vec::new(),
                blocked_by: Vec::new(),
                metadata: BTreeMap::from([(
                    WORKFLOW_TASK_METADATA_KEY.to_string(),
                    "wf_closeout".to_string(),
                )]),
            },
        );
        tasks
    }

    fn workflow_metadata() -> WorkflowEventMetadata {
        WorkflowEventMetadata {
            workflow_id: Some("wf_closeout".to_string()),
            lane: Some("lane.delivery".to_string()),
            iteration: Some(0),
            stop_reason: None,
            evidence_category: None,
            owner: Some("leader".to_string()),
        }
    }

    fn evidence_event(
        category: &str,
        acceptance_ref: &str,
        metadata: BTreeMap<String, String>,
    ) -> EventV1 {
        EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
            workflow_id: "wf_closeout".to_string(),
            category: category.to_string(),
            summary: format!("{category} {acceptance_ref}"),
            artifact_path: Some(format!("artifacts/{acceptance_ref}.json")),
            artifact_digest: Some("digest".to_string()),
            acceptance_ref: Some(acceptance_ref.to_string()),
            metadata,
        })
    }

    fn readiness_for(events: Vec<EventV1>) -> super::WorkflowCloseoutReadiness {
        let projection = project_workflows(events.iter());
        WorkflowCloseoutPolicy::from_config(
            WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID,
            WorkflowCloseoutPolicyConfig {
                require_evidence: false,
                require_dossier: false,
                ..WorkflowCloseoutPolicyConfig::default()
            },
        )
        .expect("policy")
        .evaluate(
            &projection,
            "wf_closeout",
            &PersistentTaskProjection::default(),
            &WorkflowSignoffPolicy::new(Vec::<String>::new()),
        )
    }

    fn string_array(values: &[&str]) -> String {
        serde_json::to_string(values).expect("json array")
    }

    #[test]
    fn built_in_policy_ids_are_stable_and_unknown_policy_fails_closed() {
        assert_eq!(
            builtin_policy_ids(),
            &[
                "workflow.closeout.default",
                "workflow.closeout.simulated",
                "workflow.closeout.goal",
                "workflow.closeout.team",
                "workflow.closeout.mission",
                "workflow.closeout.wiki",
                "workflow.closeout.live",
            ]
        );
        let err = WorkflowCloseoutPolicy::from_config(
            "workflow.closeout.unregistered",
            WorkflowCloseoutPolicyConfig::default(),
        )
        .expect_err("unknown policies fail closed");
        assert!(format!("{err:?}").contains("workflow.closeout.unregistered"));
    }

    #[test]
    fn closeout_readiness_reports_core_blockers_and_legal_actions() {
        let events = [
            start_event(),
            EventV1::ContinuationStarted(ContinuationStartedEvent {
                continuation_id: "cont_active".to_string(),
                mode: "ralph".to_string(),
                command: "continue".to_string(),
                max_iterations: 3,
                max_wall_clock_ms: 1000,
                max_provider_calls: 4,
                max_tool_calls: 8,
                workflow: Some(WorkflowEventMetadata {
                    workflow_id: Some("wf_closeout".to_string()),
                    lane: Some("lane.delivery".to_string()),
                    iteration: Some(0),
                    stop_reason: None,
                    evidence_category: None,
                    owner: Some("leader".to_string()),
                }),
            }),
        ];
        let projection = project_workflows(events.iter());
        let tasks = pending_tasks();
        let policy = WorkflowCloseoutPolicy::from_config(
            WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID,
            WorkflowCloseoutPolicyConfig {
                require_export_artifact: true,
                ..WorkflowCloseoutPolicyConfig::default()
            },
        )
        .expect("default policy");
        let readiness = projection.closeout_readiness(
            "wf_closeout",
            &tasks,
            &WorkflowSignoffPolicy::new([SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string()]),
            &policy,
        );

        assert!(!readiness.overall_allowed);
        assert_eq!(readiness.policy_version, 1);
        assert!(
            !readiness
                .dimension(WORKFLOW_CLOSEOUT_DIMENSION_EVIDENCE)
                .expect("evidence dimension")
                .allowed
        );
        assert!(
            !readiness
                .dimension(WORKFLOW_CLOSEOUT_DIMENSION_TASKS)
                .expect("tasks dimension")
                .allowed
        );
        assert!(
            !readiness
                .dimension(WORKFLOW_CLOSEOUT_DIMENSION_CONTINUATIONS)
                .expect("continuations dimension")
                .allowed
        );
        assert!(
            !readiness
                .dimension(WORKFLOW_CLOSEOUT_DIMENSION_DOSSIER)
                .expect("dossier dimension")
                .allowed
        );
        assert!(readiness.legal_next_actions.iter().any(|action| {
            action.action == WorkflowSignoffDecision::RequestEvidence && action.requires_reason
        }));
        assert!(readiness.legal_next_actions.iter().any(|action| {
            action.action == WorkflowSignoffDecision::Waive && action.requires_scope
        }));
    }

    #[test]
    fn workflow_family_evidence_statuses_block_closeout_until_resolved() {
        let cases = [
            (
                REVIEW_EVIDENCE_CATEGORY,
                WORKFLOW_CLOSEOUT_DIMENSION_REVIEW,
                "review_status",
            ),
            (
                SECURITY_REVIEW_EVIDENCE_CATEGORY,
                WORKFLOW_CLOSEOUT_DIMENSION_SECURITY,
                "security_status",
            ),
            (
                QA_EVIDENCE_CATEGORY,
                WORKFLOW_CLOSEOUT_DIMENSION_QA,
                "qa_status",
            ),
            (
                PERFORMANCE_EVIDENCE_CATEGORY,
                WORKFLOW_CLOSEOUT_DIMENSION_PERFORMANCE,
                "performance_status",
            ),
            (
                VISUAL_EVIDENCE_CATEGORY,
                WORKFLOW_CLOSEOUT_DIMENSION_VISUAL,
                "visual_status",
            ),
            (
                ADVISOR_EVIDENCE_CATEGORY,
                WORKFLOW_CLOSEOUT_DIMENSION_ADVISOR,
                "advisor_status",
            ),
            (
                SETUP_DOCTOR_EVIDENCE_CATEGORY,
                WORKFLOW_CLOSEOUT_DIMENSION_SETUP,
                "setup_status",
            ),
            (
                SKILL_MANAGEMENT_EVIDENCE_CATEGORY,
                WORKFLOW_CLOSEOUT_DIMENSION_SKILL,
                "skill_status",
            ),
            (
                STATUS_HUD_EVIDENCE_CATEGORY,
                WORKFLOW_CLOSEOUT_DIMENSION_STATUS_HUD,
                "status_hud_status",
            ),
            (
                NOTE_MEMORY_EVIDENCE_CATEGORY,
                WORKFLOW_CLOSEOUT_DIMENSION_NOTE_MEMORY,
                "note_memory_status",
            ),
        ];

        for (category, dimension, status_key) in cases {
            let readiness = readiness_for(vec![
                start_event(),
                evidence_event(
                    category,
                    &format!("{dimension}-blocked"),
                    BTreeMap::from([(status_key.to_string(), "failed".to_string())]),
                ),
            ]);
            let dimension = readiness.dimension(dimension).expect("family dimension");
            assert!(
                !dimension.allowed,
                "{category} with failed status should block closeout"
            );
            assert!(dimension
                .blocking_refs
                .iter()
                .any(|reference| reference.contains("blocked")));
        }
    }

    #[test]
    fn scoped_waivers_only_release_matching_dimensions() {
        let events = [
            start_event(),
            EventV1::WorkflowOperatorDecisionRecorded(WorkflowOperatorDecisionRecordedEvent {
                workflow_id: "wf_closeout".to_string(),
                decision: SIGNOFF_WAIVER_DECISION.to_string(),
                operator: "operator".to_string(),
                reason: Some("accept missing evidence".to_string()),
                correlation_id: None,
            }),
            EventV1::WorkflowOperatorDecisionRecorded(WorkflowOperatorDecisionRecordedEvent {
                workflow_id: "wf_closeout".to_string(),
                decision: PENDING_TASK_WAIVER_DECISION.to_string(),
                operator: "operator".to_string(),
                reason: Some("defer task".to_string()),
                correlation_id: None,
            }),
        ];
        let projection = project_workflows(events.iter());
        let readiness = WorkflowCloseoutPolicy::default_policy().evaluate(
            &projection,
            "wf_closeout",
            &pending_tasks(),
            &WorkflowSignoffPolicy::new([SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string()]),
        );

        assert!(
            readiness
                .dimension(WORKFLOW_CLOSEOUT_DIMENSION_EVIDENCE)
                .expect("evidence")
                .waived
        );
        assert!(
            readiness
                .dimension(WORKFLOW_CLOSEOUT_DIMENSION_TASKS)
                .expect("tasks")
                .waived
        );
        assert_eq!(readiness.waivers.len(), 2);
        assert!(readiness
            .waivers
            .iter()
            .any(|waiver| waiver.scope == "dimension:evidence"));
        assert!(readiness
            .waivers
            .iter()
            .any(|waiver| waiver.scope == "dimension:tasks"));
    }

    #[test]
    fn scoped_closeout_waivers_apply_to_matching_dimension_or_category_only() {
        let partial_events = [
            start_event(),
            EventV1::WorkflowOperatorDecisionRecorded(WorkflowOperatorDecisionRecordedEvent {
                workflow_id: "wf_closeout".to_string(),
                decision: "waive:category:evidence.simulated_tool_result".to_string(),
                operator: "operator".to_string(),
                reason: Some("accept simulated evidence gap".to_string()),
                correlation_id: None,
            }),
        ];
        let partial_projection = project_workflows(partial_events.iter());
        let partial = WorkflowCloseoutPolicy::default_policy().evaluate(
            &partial_projection,
            "wf_closeout",
            &PersistentTaskProjection::default(),
            &WorkflowSignoffPolicy::new([
                SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string(),
                "evidence.other".to_string(),
            ]),
        );

        let evidence = partial
            .dimension(WORKFLOW_CLOSEOUT_DIMENSION_EVIDENCE)
            .expect("evidence dimension");
        assert!(!evidence.allowed);
        assert_eq!(evidence.missing_categories, ["evidence.other"]);

        let dimension_events = [
            start_event(),
            EventV1::WorkflowOperatorDecisionRecorded(WorkflowOperatorDecisionRecordedEvent {
                workflow_id: "wf_closeout".to_string(),
                decision: "waive:dimension:evidence".to_string(),
                operator: "operator".to_string(),
                reason: Some("accept all missing evidence".to_string()),
                correlation_id: None,
            }),
        ];
        let dimension_projection = project_workflows(dimension_events.iter());
        let dimension = WorkflowCloseoutPolicy::default_policy().evaluate(
            &dimension_projection,
            "wf_closeout",
            &PersistentTaskProjection::default(),
            &WorkflowSignoffPolicy::new([
                SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string(),
                "evidence.other".to_string(),
            ]),
        );
        let evidence = dimension
            .dimension(WORKFLOW_CLOSEOUT_DIMENSION_EVIDENCE)
            .expect("evidence dimension");
        assert!(evidence.allowed);
        assert!(evidence.waived);
        assert!(evidence.missing_categories.is_empty());
    }

    #[test]
    fn workflow_config_exposes_effective_default_closeout_policy() {
        let config = WorkflowRuntimeConfig {
            run: WorkflowRunRuntimeConfig {
                require_dossier: false,
                require_evidence: false,
                ..WorkflowRunRuntimeConfig::default()
            },
            ..WorkflowRuntimeConfig::default()
        };
        let policy = config
            .effective_closeout_policy(WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID)
            .expect("default policy from config");
        assert!(!policy.require_dossier);
        assert!(!policy.require_evidence);
        assert_eq!(
            config.closeout.default_policy,
            WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID
        );
    }

    #[test]
    fn workflow_config_unknown_closeout_policy_fails_closed() {
        let config = WorkflowRuntimeConfig::default();
        assert!(config
            .effective_closeout_policy("workflow.closeout.nope")
            .is_err());
    }

    #[test]
    fn closeout_allows_when_required_evidence_tasks_and_dossier_are_satisfied() {
        let events = [
            start_event(),
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: "wf_closeout".to_string(),
                category: SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string(),
                summary: "verified".to_string(),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: Some("acceptance.closeout".to_string()),
                metadata: BTreeMap::new(),
            }),
        ];
        let projection = project_workflows(events.iter());
        let readiness = WorkflowCloseoutPolicy::default_policy().evaluate(
            &projection,
            "wf_closeout",
            &PersistentTaskProjection::default(),
            &WorkflowSignoffPolicy::new([SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string()]),
        );

        assert!(readiness.overall_allowed);
        assert!(readiness.legal_next_actions.iter().any(|action| {
            action.action == WorkflowSignoffDecision::Approve && !action.requires_reason
        }));
    }

    #[test]
    fn plan_and_goal_dimensions_block_until_approved_and_quality_gated() {
        let blocked_plan = readiness_for(vec![
            start_event(),
            evidence_event(
                PLAN_CONSENSUS_EVIDENCE_CATEGORY,
                "plan-blocked",
                BTreeMap::from([
                    ("plan_id".to_string(), "plan-1".to_string()),
                    ("plan_status".to_string(), "needs_iteration".to_string()),
                    ("critic_verdict".to_string(), "revise".to_string()),
                ]),
            ),
        ]);
        let plan_dimension = blocked_plan
            .dimension(WORKFLOW_CLOSEOUT_DIMENSION_PLAN)
            .expect("plan dimension");
        assert!(!plan_dimension.allowed);
        assert!(plan_dimension
            .missing_categories
            .contains(&"plan_approved".to_string()));

        let approved_plan = readiness_for(vec![
            start_event(),
            evidence_event(
                PLAN_CONSENSUS_EVIDENCE_CATEGORY,
                "plan-approved",
                BTreeMap::from([
                    ("plan_id".to_string(), "plan-1".to_string()),
                    ("plan_status".to_string(), "approved".to_string()),
                    ("critic_verdict".to_string(), "approved".to_string()),
                ]),
            ),
        ]);
        assert!(
            approved_plan
                .dimension(WORKFLOW_CLOSEOUT_DIMENSION_PLAN)
                .expect("plan dimension")
                .allowed
        );

        let pending_goal = readiness_for(vec![
            start_event(),
            evidence_event(
                GOAL_LEDGER_EVIDENCE_CATEGORY,
                "goal-pending",
                BTreeMap::from([
                    (
                        "artifact_kind".to_string(),
                        GOAL_LEDGER_ARTIFACT_KIND.to_string(),
                    ),
                    ("goal_id".to_string(), "goal-1".to_string()),
                    ("goal_status".to_string(), "active".to_string()),
                    ("story_count".to_string(), "1".to_string()),
                    ("story.0.id".to_string(), "story-1".to_string()),
                    ("story.0.status".to_string(), "complete".to_string()),
                ]),
            ),
        ]);
        let goal_dimension = pending_goal
            .dimension(WORKFLOW_CLOSEOUT_DIMENSION_GOAL)
            .expect("goal dimension");
        assert!(!goal_dimension.allowed);
        assert!(goal_dimension
            .missing_categories
            .contains(&"final_quality_gate".to_string()));

        let complete_goal = readiness_for(vec![
            start_event(),
            evidence_event(
                GOAL_LEDGER_EVIDENCE_CATEGORY,
                "goal-complete",
                BTreeMap::from([
                    (
                        "artifact_kind".to_string(),
                        GOAL_LEDGER_ARTIFACT_KIND.to_string(),
                    ),
                    ("goal_id".to_string(), "goal-1".to_string()),
                    ("goal_status".to_string(), "active".to_string()),
                    ("story_count".to_string(), "1".to_string()),
                    ("story.0.id".to_string(), "story-1".to_string()),
                    ("story.0.status".to_string(), "complete".to_string()),
                    ("quality_gate_status".to_string(), "passed".to_string()),
                    (
                        "quality_gate_verification_refs".to_string(),
                        string_array(&["verify"]),
                    ),
                    (
                        "quality_gate_review_refs".to_string(),
                        string_array(&["review"]),
                    ),
                ]),
            ),
        ]);
        assert!(
            complete_goal
                .dimension(WORKFLOW_CLOSEOUT_DIMENSION_GOAL)
                .expect("goal dimension")
                .allowed
        );
    }

    #[test]
    fn mission_wiki_hook_state_and_trace_dimensions_use_replay_metadata() {
        let blocked = readiness_for(vec![
            start_event(),
            evidence_event(
                RESEARCH_MISSION_EVIDENCE_CATEGORY,
                "mission-active",
                BTreeMap::from([
                    (
                        "artifact_kind".to_string(),
                        RESEARCH_MISSION_ARTIFACT_KIND.to_string(),
                    ),
                    ("mission_id".to_string(), "mission-1".to_string()),
                    ("mission_status".to_string(), "active".to_string()),
                    (
                        "validator_mode".to_string(),
                        "mission_validator_script".to_string(),
                    ),
                ]),
            ),
            evidence_event(
                WIKI_EVIDENCE_CATEGORY,
                "wiki-failing",
                BTreeMap::from([("wiki_lint_status".to_string(), "failed".to_string())]),
            ),
            evidence_event(
                WORKFLOW_CLOSEOUT_HOOK_EVIDENCE_CATEGORY,
                "hook-denied",
                BTreeMap::from([("hook_status".to_string(), "denied".to_string())]),
            ),
            evidence_event(
                WORKFLOW_CLOSEOUT_STATE_EVIDENCE_CATEGORY,
                "state-stale",
                BTreeMap::from([("state_status".to_string(), "stale".to_string())]),
            ),
            evidence_event(
                WORKFLOW_CLOSEOUT_TRACE_EVIDENCE_CATEGORY,
                "trace-error",
                BTreeMap::from([("trace_status".to_string(), "error".to_string())]),
            ),
        ]);
        for dimension in [
            WORKFLOW_CLOSEOUT_DIMENSION_MISSION,
            WORKFLOW_CLOSEOUT_DIMENSION_WIKI,
            WORKFLOW_CLOSEOUT_DIMENSION_HOOKS,
            WORKFLOW_CLOSEOUT_DIMENSION_STATE,
            WORKFLOW_CLOSEOUT_DIMENSION_TRACE,
        ] {
            assert!(
                !blocked.dimension(dimension).expect("dimension").allowed,
                "{dimension} should block"
            );
        }

        let passing = readiness_for(vec![
            start_event(),
            evidence_event(
                RESEARCH_MISSION_EVIDENCE_CATEGORY,
                "mission-result",
                BTreeMap::from([
                    (
                        "artifact_kind".to_string(),
                        RESEARCH_RESULT_ARTIFACT_KIND.to_string(),
                    ),
                    ("mission_id".to_string(), "mission-1".to_string()),
                    ("mission_status".to_string(), "complete".to_string()),
                    ("iteration".to_string(), "1".to_string()),
                    ("validator_status".to_string(), "passed".to_string()),
                    (
                        "validator_ref".to_string(),
                        "artifacts/validator.json".to_string(),
                    ),
                    (
                        "validator_mode".to_string(),
                        "mission_validator_script".to_string(),
                    ),
                ]),
            ),
            evidence_event(
                WIKI_EVIDENCE_CATEGORY,
                "wiki-passing",
                BTreeMap::from([("wiki_lint_status".to_string(), "passed".to_string())]),
            ),
            evidence_event(
                WORKFLOW_CLOSEOUT_HOOK_EVIDENCE_CATEGORY,
                "hook-allowed",
                BTreeMap::from([("hook_status".to_string(), "passed".to_string())]),
            ),
            evidence_event(
                WORKFLOW_CLOSEOUT_STATE_EVIDENCE_CATEGORY,
                "state-ready",
                BTreeMap::from([("state_status".to_string(), "passed".to_string())]),
            ),
            evidence_event(
                WORKFLOW_CLOSEOUT_TRACE_EVIDENCE_CATEGORY,
                "trace-ready",
                BTreeMap::from([("trace_status".to_string(), "passed".to_string())]),
            ),
        ]);
        for dimension in [
            WORKFLOW_CLOSEOUT_DIMENSION_MISSION,
            WORKFLOW_CLOSEOUT_DIMENSION_WIKI,
            WORKFLOW_CLOSEOUT_DIMENSION_HOOKS,
            WORKFLOW_CLOSEOUT_DIMENSION_STATE,
            WORKFLOW_CLOSEOUT_DIMENSION_TRACE,
        ] {
            assert!(
                passing.dimension(dimension).expect("dimension").allowed,
                "{dimension} should allow"
            );
        }
    }

    #[test]
    fn question_dimension_blocks_open_timeout_and_error_states_only() {
        for (status, allowed, missing) in [
            (WORKFLOW_QUESTION_STATUS_ASKED, false, "question_answer"),
            (WORKFLOW_QUESTION_STATUS_ANSWERED, true, ""),
            (WORKFLOW_QUESTION_STATUS_CLOSED, true, ""),
            (
                WORKFLOW_QUESTION_STATUS_TIMED_OUT,
                false,
                "question_timed_out",
            ),
            (WORKFLOW_QUESTION_STATUS_ERROR, false, "question_error"),
        ] {
            let readiness = readiness_for(vec![
                start_event(),
                evidence_event(
                    WORKFLOW_QUESTION_EVIDENCE_CATEGORY,
                    "question-1",
                    BTreeMap::from([
                        (
                            WORKFLOW_QUESTION_METADATA_ID.to_string(),
                            "question-1".to_string(),
                        ),
                        (
                            WORKFLOW_QUESTION_METADATA_STATUS.to_string(),
                            status.to_string(),
                        ),
                        (
                            WORKFLOW_QUESTION_METADATA_REASON_CODE.to_string(),
                            status.to_string(),
                        ),
                        (
                            WORKFLOW_QUESTION_METADATA_ANSWER_REF.to_string(),
                            "answers/question-1.json".to_string(),
                        ),
                    ]),
                ),
            ]);
            let dimension = readiness
                .dimension(WORKFLOW_CLOSEOUT_DIMENSION_QUESTION)
                .expect("question dimension");
            assert_eq!(dimension.allowed, allowed, "{status}");
            if !allowed {
                assert!(
                    dimension.missing_categories.contains(&missing.to_string()),
                    "{status} missing category should include {missing}: {:?}",
                    dimension.missing_categories
                );
            }
        }
    }

    #[test]
    fn team_dimension_blocks_pending_tasks_until_verification_and_synthesis_exist() {
        let team_created = EventV1::TeamCreated(TeamCreatedEvent {
            team_run_id: "team-1".to_string(),
            spec: TeamSpec {
                version: 1,
                name: "delivery".to_string(),
                description: None,
                lead: None,
                members: Vec::new(),
                bounds: TeamBounds::default(),
                metadata: BTreeMap::new(),
            },
            workflow: Some(workflow_metadata()),
        });
        let task_created = EventV1::TeamTaskCreated(TeamTaskCreatedEvent {
            team_run_id: "team-1".to_string(),
            task: TeamTask {
                version: 1,
                task_id: "task-1".to_string(),
                subject: "verify".to_string(),
                description: "verify team output".to_string(),
                status: TeamTaskStatus::Pending,
                owner: None,
                blocks: Vec::new(),
                blocked_by: Vec::new(),
                metadata: BTreeMap::new(),
            },
            workflow: None,
        });
        let blocked = readiness_for(vec![
            start_event(),
            team_created.clone(),
            task_created.clone(),
        ]);
        let team_dimension = blocked
            .dimension(WORKFLOW_CLOSEOUT_DIMENSION_TEAM)
            .expect("team dimension");
        assert!(!team_dimension.allowed);
        assert!(team_dimension
            .missing_categories
            .contains(&"team_tasks_complete".to_string()));
        assert!(team_dimension
            .missing_categories
            .contains(&"team_verification_evidence".to_string()));
        assert!(team_dimension
            .missing_categories
            .contains(&"team_synthesis".to_string()));

        let completed = EventV1::TeamTaskUpdated(TeamTaskUpdatedEvent {
            team_run_id: "team-1".to_string(),
            task_id: "task-1".to_string(),
            status: TeamTaskStatus::Completed,
            owner: Some("leader".to_string()),
            metadata: BTreeMap::from([
                (
                    "verification_evidence_ref".to_string(),
                    "artifacts/team-verify.json".to_string(),
                ),
                (
                    "synthesis_ref".to_string(),
                    "artifacts/team-synthesis.md".to_string(),
                ),
            ]),
            workflow: None,
        });
        let allowed = readiness_for(vec![start_event(), team_created, task_created, completed]);
        assert!(
            allowed
                .dimension(WORKFLOW_CLOSEOUT_DIMENSION_TEAM)
                .expect("team dimension")
                .allowed
        );
    }
}

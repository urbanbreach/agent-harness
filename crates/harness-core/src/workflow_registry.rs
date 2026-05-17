//! First-party workflow contract registry.
//!
//! This module intentionally records stable ids and ownership boundaries only.
//! Runtime state remains event-sourced through the coordinator and replay
//! projections; these specs are not an alternate workflow state store.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowModeSpec {
    pub id: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowLaneSpec {
    pub id: &'static str,
    pub owner: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowOutcomeSpec {
    pub id: &'static str,
    pub terminal: bool,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceCategorySpec {
    pub id: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionPolicySpec {
    pub id: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowDoctorCheckSpec {
    pub id: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowDocsAnchorSpec {
    pub id: &'static str,
    pub path: &'static str,
    pub heading: &'static str,
}

pub const WORKFLOW_MODES: &[WorkflowModeSpec] = &[
    WorkflowModeSpec {
        id: "workflow.run",
        description: "Coordinator-owned workflow execution spine.",
    },
    WorkflowModeSpec {
        id: "workflow.plan_consensus",
        description: "Planner/architect/critic consensus planning workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.goal_ledger",
        description: "Durable goal/story checkpoint workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.research_mission",
        description: "Validator-gated research mission workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.wiki",
        description: "Markdown-first project wiki write workflow.",
    },
];

pub const WORKFLOW_LANES: &[WorkflowLaneSpec] = &[
    WorkflowLaneSpec {
        id: "lane.leader",
        owner: "coordinator",
        description: "Single scheduling and transition authority for workflow state.",
    },
    WorkflowLaneSpec {
        id: "lane.delivery",
        owner: "agent",
        description: "Implementation or artifact-producing work owned by scheduled agents.",
    },
    WorkflowLaneSpec {
        id: "lane.verification",
        owner: "agent",
        description: "Tests, replay checks, signoff evidence, and regression validation.",
    },
    WorkflowLaneSpec {
        id: "lane.operator_decision",
        owner: "operator",
        description: "Explicit approve, redirect, request-evidence, waiver, or abort decisions.",
    },
];

pub const WORKFLOW_OUTCOMES: &[WorkflowOutcomeSpec] = &[
    WorkflowOutcomeSpec {
        id: "outcome.finished",
        terminal: true,
        description: "Workflow completed with required evidence or waiver.",
    },
    WorkflowOutcomeSpec {
        id: "outcome.blocked",
        terminal: false,
        description: "Workflow is waiting on missing authority, evidence, or dependency.",
    },
    WorkflowOutcomeSpec {
        id: "outcome.failed",
        terminal: true,
        description: "Workflow cannot continue successfully under current constraints.",
    },
    WorkflowOutcomeSpec {
        id: "outcome.cancelled",
        terminal: true,
        description: "Operator or policy cancelled the workflow.",
    },
    WorkflowOutcomeSpec {
        id: "outcome.user_interlude",
        terminal: false,
        description: "Workflow paused for a user question or clarification.",
    },
];

pub const EVIDENCE_CATEGORIES: &[EvidenceCategorySpec] = &[
    EvidenceCategorySpec {
        id: "evidence.context_snapshot",
        description: "Redacted/capped intake context snapshot artifact.",
    },
    EvidenceCategorySpec {
        id: "evidence.permission_decision",
        description: "Permission or operator decision used by the workflow.",
    },
    EvidenceCategorySpec {
        id: "evidence.task_result",
        description: "Scheduled task result or late-result projection evidence.",
    },
    EvidenceCategorySpec {
        id: "evidence.tool_result",
        description: "Native tool completion summary plus artifact refs.",
    },
    EvidenceCategorySpec {
        id: "evidence.simulated_tool_result",
        description: "Deterministic simulator no-op tool result mapped to acceptance evidence.",
    },
    EvidenceCategorySpec {
        id: "evidence.artifact",
        description: "Redacted artifact ref, digest, and summary used for acceptance.",
    },
    EvidenceCategorySpec {
        id: "evidence.verification",
        description: "Test, lint, typecheck, replay, signoff, or other verification result.",
    },
    EvidenceCategorySpec {
        id: "evidence.operator_waiver",
        description: "Explicit operator waiver for missing or deferred acceptance evidence.",
    },
    EvidenceCategorySpec {
        id: "evidence.dossier",
        description: "Run Dossier export or projection evidence.",
    },
    EvidenceCategorySpec {
        id: "evidence.plan_consensus",
        description:
            "Planner/architect/critic consensus plan artifact and review verdict evidence.",
    },
    EvidenceCategorySpec {
        id: "evidence.goal_ledger",
        description:
            "Goal/story ledger create and checkpoint evidence with final quality-gate refs.",
    },
    EvidenceCategorySpec {
        id: "evidence.research_mission",
        description:
            "Research mission, sandbox, candidate result, and validator/review artifact refs.",
    },
    EvidenceCategorySpec {
        id: "evidence.wiki",
        description: "Markdown wiki page write/delete metadata with page path and digest refs.",
    },
];

pub const TRANSITION_POLICIES: &[TransitionPolicySpec] = &[
    TransitionPolicySpec {
        id: "transition.idempotent_start",
        description: "Duplicate workflow start with the same idempotency key returns the existing projection.",
    },
    TransitionPolicySpec {
        id: "transition.owner_conflict_denied",
        description: "Conflicting owners append denied transition evidence instead of mutating state.",
    },
    TransitionPolicySpec {
        id: "transition.terminal_late_result",
        description: "Late results after a terminal state are projected as late/discarded evidence.",
    },
    TransitionPolicySpec {
        id: "transition.projection_only_read",
        description: "Status, replay, doctor, and dossier reads derive projection state and append no events.",
    },
    TransitionPolicySpec {
        id: "transition.evidence_gated_completion",
        description: "Completion requires acceptance-mapped evidence refs or an explicit operator waiver.",
    },
    TransitionPolicySpec {
        id: "transition.workflow_tasks_incomplete",
        description: "Completion is denied while workflow-owned persistent tasks are pending, claimed, or in progress without a waiver.",
    },
    TransitionPolicySpec {
        id: "transition.active_continuation_incomplete",
        description: "Completion is denied while workflow-owned continuations are still active or queued.",
    },
];

pub const WORKFLOW_DOCTOR_CHECKS: &[WorkflowDoctorCheckSpec] = &[
    WorkflowDoctorCheckSpec {
        id: "workflow_contract_registry",
        description: "Validates the first-party workflow contract ids and docs anchors.",
    },
    WorkflowDoctorCheckSpec {
        id: "workflow_context_snapshot",
        description:
            "Validates the redacted context snapshot artifact contract and projection metadata.",
    },
    WorkflowDoctorCheckSpec {
        id: "workflow_runtime_config",
        description: "Validates staged runtime.workflow defaults and operator limits.",
    },
    WorkflowDoctorCheckSpec {
        id: "workflow_closeout_policy",
        description:
            "Validates runtime.workflow.closeout policy ids, defaults, and fail-closed behavior.",
    },
    WorkflowDoctorCheckSpec {
        id: "workflow_closeout_readiness",
        description:
            "Reports replay-derived closeout blockers and legal next actions for the latest run.",
    },
    WorkflowDoctorCheckSpec {
        id: "workflow_catalog_health",
        description:
            "Reports redacted workflow skill/role catalog visibility, missing assets, disabled entries, and shadowed prompt roots.",
    },
    WorkflowDoctorCheckSpec {
        id: "workflow_simulator",
        description: "Validates deterministic simulator evidence/signoff/dossier readiness.",
    },
    WorkflowDoctorCheckSpec {
        id: "workflow_stale_work_loop",
        description: "Warns when the latest session run has active workflow-owned continuations.",
    },
    WorkflowDoctorCheckSpec {
        id: "command_registry",
        description: "Validates canonical command and alias entries never invoke shell directly.",
    },
    WorkflowDoctorCheckSpec {
        id: "team_mode",
        description: "Validates team dependencies and declared team spec health.",
    },
    WorkflowDoctorCheckSpec {
        id: "parity_ledger",
        description: "Validates the first-slice parity ledger used as workflow intake context.",
    },
];

pub const WORKFLOW_DOCS_ANCHORS: &[WorkflowDocsAnchorSpec] = &[
    WorkflowDocsAnchorSpec {
        id: "workflow_contract_registry",
        path: "docs/config.md",
        heading: "### Workflow contract registry",
    },
    WorkflowDocsAnchorSpec {
        id: "workflow_slice_source",
        path: "docs/omx-workflow-slice-spec.md",
        heading: "# OMX-style workflow slice specification",
    },
    WorkflowDocsAnchorSpec {
        id: "workflow_testing",
        path: "docs/testing.md",
        heading: "# Testing and signoff map",
    },
];

pub fn stable_id_groups() -> [(&'static str, Vec<&'static str>); 6] {
    [
        ("modes", WORKFLOW_MODES.iter().map(|spec| spec.id).collect()),
        ("lanes", WORKFLOW_LANES.iter().map(|spec| spec.id).collect()),
        (
            "outcomes",
            WORKFLOW_OUTCOMES.iter().map(|spec| spec.id).collect(),
        ),
        (
            "evidence_categories",
            EVIDENCE_CATEGORIES.iter().map(|spec| spec.id).collect(),
        ),
        (
            "transition_policies",
            TRANSITION_POLICIES.iter().map(|spec| spec.id).collect(),
        ),
        (
            "doctor_checks",
            WORKFLOW_DOCTOR_CHECKS.iter().map(|spec| spec.id).collect(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{stable_id_groups, EVIDENCE_CATEGORIES, WORKFLOW_DOCS_ANCHORS};

    #[test]
    fn workflow_contract_ids_are_unique_by_group() {
        for (group, ids) in stable_id_groups() {
            let unique = ids.iter().copied().collect::<BTreeSet<_>>();
            assert_eq!(unique.len(), ids.len(), "duplicate id in {group}");
            if group == "doctor_checks" {
                assert!(
                    ids.iter().all(|id| id
                        .chars()
                        .all(|ch| ch.is_ascii_lowercase() || ch == '_' || ch.is_ascii_digit())),
                    "{group} ids should be stable snake_case doctor ids"
                );
            } else {
                assert!(
                    ids.iter().all(|id| id.contains('.')),
                    "{group} ids should be namespaced"
                );
            }
        }
    }

    #[test]
    fn workflow_contract_has_acceptance_evidence_and_docs_anchors() {
        assert!(EVIDENCE_CATEGORIES
            .iter()
            .any(|spec| spec.id == "evidence.verification"));
        assert!(EVIDENCE_CATEGORIES
            .iter()
            .any(|spec| spec.id == "evidence.operator_waiver"));
        assert!(WORKFLOW_DOCS_ANCHORS
            .iter()
            .any(|spec| spec.path == "docs/config.md"));
    }
}

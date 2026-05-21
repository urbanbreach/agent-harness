//! First-party workflow contract registry.
//!
//! This module intentionally records stable ids and ownership boundaries only.
//! Runtime state remains event-sourced through the coordinator and replay
//! projections; these specs are not an alternate workflow state store.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowModeSpec {
    pub id: &'static str,
    pub availability: WorkflowAvailability,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowAvailability {
    Present,
    Staged,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowRejectedSurfaceSpec {
    pub id: &'static str,
    pub reason: &'static str,
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

pub const CONTINUATION_EVIDENCE_CATEGORY: &str = "evidence.continuation";
pub const REVIEW_EVIDENCE_CATEGORY: &str = "evidence.review";
pub const SECURITY_REVIEW_EVIDENCE_CATEGORY: &str = "evidence.security_review";
pub const QA_EVIDENCE_CATEGORY: &str = "evidence.qa";
pub const PERFORMANCE_EVIDENCE_CATEGORY: &str = "evidence.performance";
pub const VISUAL_EVIDENCE_CATEGORY: &str = "evidence.visual";
pub const SETUP_DOCTOR_EVIDENCE_CATEGORY: &str = "evidence.setup_doctor";
pub const SKILL_MANAGEMENT_EVIDENCE_CATEGORY: &str = "evidence.skill_management";
pub const STATUS_HUD_EVIDENCE_CATEGORY: &str = "evidence.status_hud";
pub const NOTE_MEMORY_EVIDENCE_CATEGORY: &str = "evidence.note_memory";

pub const WORKFLOW_MODES: &[WorkflowModeSpec] = &[
    WorkflowModeSpec {
        id: "workflow.run",
        availability: WorkflowAvailability::Present,
        description: "Coordinator-owned workflow execution spine.",
    },
    WorkflowModeSpec {
        id: "workflow.deep_interview",
        availability: WorkflowAvailability::Present,
        description: "One-question-at-a-time intake and context snapshot workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.plan_consensus",
        availability: WorkflowAvailability::Present,
        description: "Planner/architect/critic consensus planning workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.continuation",
        availability: WorkflowAvailability::Present,
        description: "Bounded Ralph/ultrawork continuation workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.goal_ledger",
        availability: WorkflowAvailability::Present,
        description: "Durable goal/story checkpoint workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.research_mission",
        availability: WorkflowAvailability::Present,
        description: "Validator-gated research mission workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.team_escalation",
        availability: WorkflowAvailability::Present,
        description: "Explicit operator-owned team escalation workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.review",
        availability: WorkflowAvailability::Present,
        description: "Code-review findings workflow with closeout blockers.",
    },
    WorkflowModeSpec {
        id: "workflow.security_review",
        availability: WorkflowAvailability::Present,
        description: "Security-review findings workflow with closeout blockers.",
    },
    WorkflowModeSpec {
        id: "workflow.qa",
        availability: WorkflowAvailability::Present,
        description: "Deterministic QA scenario workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.performance",
        availability: WorkflowAvailability::Present,
        description: "Evaluator-gated performance workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.visual",
        availability: WorkflowAvailability::Present,
        description: "Visual verdict/evidence workflow with env-gated live capture.",
    },
    WorkflowModeSpec {
        id: "workflow.autopilot",
        availability: WorkflowAvailability::Present,
        description: "Autonomous plan-execute-review workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.analysis",
        availability: WorkflowAvailability::Present,
        description: "Read-only repository analysis workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.doctor",
        availability: WorkflowAvailability::Present,
        description: "Runtime and installation diagnostics workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.help",
        availability: WorkflowAvailability::Present,
        description: "Command help and discovery workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.hud",
        availability: WorkflowAvailability::Present,
        description: "HUD and status projection workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.note",
        availability: WorkflowAvailability::Present,
        description: "Note and project-memory capture workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.skill_management",
        availability: WorkflowAvailability::Present,
        description: "Skill inventory and management workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.trace",
        availability: WorkflowAvailability::Present,
        description: "Trace timeline and summary workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.configure_notifications",
        availability: WorkflowAvailability::Present,
        description: "Notification configuration workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.design",
        availability: WorkflowAvailability::Present,
        description: "Design source-of-truth workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.cleanup",
        availability: WorkflowAvailability::Present,
        description: "Cleanup, refactor, and anti-slop workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.pipeline",
        availability: WorkflowAvailability::Present,
        description: "Sequenced pipeline workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.tdd",
        availability: WorkflowAvailability::Present,
        description: "Test-driven-development workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.web_clone",
        availability: WorkflowAvailability::Present,
        description: "URL-driven web clone verification workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.ecomode",
        availability: WorkflowAvailability::Present,
        description: "Token-efficient model routing workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.deepsearch",
        availability: WorkflowAvailability::Present,
        description: "Deep search and evidence capture workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.ralph_init",
        availability: WorkflowAvailability::Present,
        description: "Ralph initialization compatibility workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.start_work",
        availability: WorkflowAvailability::Present,
        description: "Work-start handoff workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.handoff",
        availability: WorkflowAvailability::Present,
        description: "Handoff artifact workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.hyperplan",
        availability: WorkflowAvailability::Present,
        description: "Parallel planning handoff workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.operator_utility",
        availability: WorkflowAvailability::Present,
        description: "Status, HUD, note, trace, doctor, notification, and skill utility workflow.",
    },
    WorkflowModeSpec {
        id: "workflow.wiki",
        availability: WorkflowAvailability::Present,
        description: "Markdown-first project wiki write workflow.",
    },
];

pub const WORKFLOW_REJECTED_SURFACES: &[WorkflowRejectedSurfaceSpec] = &[
    WorkflowRejectedSurfaceSpec {
        id: "rejected.pixel_clone",
        reason: "Exact external UI clone is not a Harness product goal.",
    },
    WorkflowRejectedSurfaceSpec {
        id: "rejected.multiple_main_agents",
        reason: "Multiple main-agent defaults conflict with single-operator orchestration.",
    },
    WorkflowRejectedSurfaceSpec {
        id: "rejected.permissive_permissions",
        reason: "External permissive defaults must not weaken coordinator permission gates.",
    },
    WorkflowRejectedSurfaceSpec {
        id: "rejected.replay_active_server",
        reason: "Server/headless surfaces must not bypass replay purity or coordinator authority.",
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
        id: CONTINUATION_EVIDENCE_CATEGORY,
        description:
            "Bounded Ralph/ultrawork continuation evidence, blockers, and verification refs.",
    },
    EvidenceCategorySpec {
        id: REVIEW_EVIDENCE_CATEGORY,
        description: "Code-review findings, verdicts, severities, and resolution status refs.",
    },
    EvidenceCategorySpec {
        id: SECURITY_REVIEW_EVIDENCE_CATEGORY,
        description: "Security-review findings, trust-boundary notes, and resolution status refs.",
    },
    EvidenceCategorySpec {
        id: QA_EVIDENCE_CATEGORY,
        description: "QA scenarios, failures, fixes, and verification status refs.",
    },
    EvidenceCategorySpec {
        id: PERFORMANCE_EVIDENCE_CATEGORY,
        description: "Performance baselines, evaluator output, and regression status refs.",
    },
    EvidenceCategorySpec {
        id: VISUAL_EVIDENCE_CATEGORY,
        description: "Visual verdicts, screenshot refs, pixel-diff refs, and live-gate status.",
    },
    EvidenceCategorySpec {
        id: SETUP_DOCTOR_EVIDENCE_CATEGORY,
        description: "Setup/doctor check results and explicit no-side-effect status refs.",
    },
    EvidenceCategorySpec {
        id: SKILL_MANAGEMENT_EVIDENCE_CATEGORY,
        description: "Skill inventory, install/update decisions, and verification status refs.",
    },
    EvidenceCategorySpec {
        id: STATUS_HUD_EVIDENCE_CATEGORY,
        description: "Status, HUD, trace, and operator utility projection evidence refs.",
    },
    EvidenceCategorySpec {
        id: NOTE_MEMORY_EVIDENCE_CATEGORY,
        description: "Note, notepad, and project-memory evidence refs with digest metadata.",
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
    TransitionPolicySpec {
        id: "transition.workflow_mode_allow",
        description: "Tracked workflow mode starts are allowed when no incompatible workflow mode is active.",
    },
    TransitionPolicySpec {
        id: "transition.workflow_mode_overlap",
        description: "Tracked workflow mode starts may overlap only for Ralph/team pairs and Ultrawork fan-out.",
    },
    TransitionPolicySpec {
        id: "transition.workflow_mode_auto_complete",
        description: "Tracked workflow mode starts auto-complete approved source modes such as deep-interview to ralplan.",
    },
    TransitionPolicySpec {
        id: "transition.workflow_mode_denied",
        description: "Tracked workflow mode starts are denied for unsupported overlaps and execution-to-planning rollback.",
    },
    TransitionPolicySpec {
        id: "transition.autopilot_review_approved",
        description: "A clean code-review verdict completes the active autopilot workflow.",
    },
    TransitionPolicySpec {
        id: "transition.autopilot_review_return_to_ralplan",
        description: "A non-clean code-review verdict returns the active autopilot workflow to the ralplan phase with evidence.",
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
        id: "dollar_alias_wiring",
        description:
            "Validates every dollar alias resolves to a native workflow, continuation, or agent action.",
    },
    WorkflowDoctorCheckSpec {
        id: "shipped_skill_loadability",
        description: "Validates every shipped .agent-harness SKILL.md file is discoverable and loadable.",
    },
    WorkflowDoctorCheckSpec {
        id: "workflow_skill_protocol_native",
        description:
            "Validates workflow SKILL.md bodies retain protocol depth relative to the OMX reference assets.",
    },
    WorkflowDoctorCheckSpec {
        id: "strict_parity_matrix",
        description:
            "Validates selected workflow parity rows against checked-in native proof dossiers.",
    },
    WorkflowDoctorCheckSpec {
        id: "workflow_transition_policy_matrix",
        description: "Validates native workflow transition allow/overlap/auto-complete/deny policy cases.",
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
        path: "docs/config.md",
        heading: "### Native workflow parity baseline",
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

pub fn evidence_category_ids() -> Vec<&'static str> {
    EVIDENCE_CATEGORIES.iter().map(|spec| spec.id).collect()
}

pub fn is_evidence_category(category: &str) -> bool {
    EVIDENCE_CATEGORIES.iter().any(|spec| spec.id == category)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        evidence_category_ids, is_evidence_category, stable_id_groups, WorkflowAvailability,
        EVIDENCE_CATEGORIES, NOTE_MEMORY_EVIDENCE_CATEGORY, PERFORMANCE_EVIDENCE_CATEGORY,
        QA_EVIDENCE_CATEGORY, REVIEW_EVIDENCE_CATEGORY, SECURITY_REVIEW_EVIDENCE_CATEGORY,
        SETUP_DOCTOR_EVIDENCE_CATEGORY, SKILL_MANAGEMENT_EVIDENCE_CATEGORY,
        STATUS_HUD_EVIDENCE_CATEGORY, VISUAL_EVIDENCE_CATEGORY, WORKFLOW_DOCS_ANCHORS,
        WORKFLOW_MODES, WORKFLOW_REJECTED_SURFACES,
    };

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

    #[test]
    fn workflow_modes_cover_native_g004_families() {
        let modes = WORKFLOW_MODES
            .iter()
            .map(|spec| spec.id)
            .collect::<BTreeSet<_>>();
        for expected in [
            "workflow.deep_interview",
            "workflow.plan_consensus",
            "workflow.continuation",
            "workflow.goal_ledger",
            "workflow.research_mission",
            "workflow.team_escalation",
            "workflow.review",
            "workflow.security_review",
            "workflow.qa",
            "workflow.performance",
            "workflow.visual",
            "workflow.operator_utility",
            "workflow.wiki",
        ] {
            assert!(modes.contains(expected), "missing workflow mode {expected}");
        }
    }

    #[test]
    fn workflow_modes_have_truthful_availability_and_operator_purpose() {
        let modes = WORKFLOW_MODES
            .iter()
            .map(|spec| (spec.id, spec))
            .collect::<std::collections::BTreeMap<_, _>>();

        for spec in WORKFLOW_MODES {
            assert!(
                !spec.description.trim().is_empty(),
                "{} should have an operator-facing purpose",
                spec.id
            );
            assert!(
                !spec.description.to_ascii_lowercase().contains("parity"),
                "{} should not claim parity from registry metadata alone",
                spec.id
            );
        }

        for present in [
            "workflow.run",
            "workflow.deep_interview",
            "workflow.plan_consensus",
            "workflow.continuation",
            "workflow.goal_ledger",
            "workflow.research_mission",
            "workflow.team_escalation",
            "workflow.review",
            "workflow.security_review",
            "workflow.qa",
            "workflow.performance",
            "workflow.visual",
            "workflow.autopilot",
            "workflow.analysis",
            "workflow.doctor",
            "workflow.help",
            "workflow.hud",
            "workflow.note",
            "workflow.skill_management",
            "workflow.trace",
            "workflow.configure_notifications",
            "workflow.design",
            "workflow.cleanup",
            "workflow.pipeline",
            "workflow.tdd",
            "workflow.web_clone",
            "workflow.ecomode",
            "workflow.deepsearch",
            "workflow.ralph_init",
            "workflow.start_work",
            "workflow.handoff",
            "workflow.hyperplan",
            "workflow.operator_utility",
            "workflow.wiki",
        ] {
            assert_eq!(
                modes[present].availability,
                WorkflowAvailability::Present,
                "{present} should be classified as present"
            );
        }

        assert!(WORKFLOW_REJECTED_SURFACES
            .iter()
            .any(|spec| spec.id == "rejected.multiple_main_agents"));
        assert!(WORKFLOW_REJECTED_SURFACES
            .iter()
            .all(|spec| !spec.reason.trim().is_empty()));
    }

    #[test]
    fn workflow_family_evidence_categories_are_registered() {
        let categories = evidence_category_ids().into_iter().collect::<BTreeSet<_>>();
        for expected in [
            REVIEW_EVIDENCE_CATEGORY,
            SECURITY_REVIEW_EVIDENCE_CATEGORY,
            QA_EVIDENCE_CATEGORY,
            PERFORMANCE_EVIDENCE_CATEGORY,
            VISUAL_EVIDENCE_CATEGORY,
            SETUP_DOCTOR_EVIDENCE_CATEGORY,
            SKILL_MANAGEMENT_EVIDENCE_CATEGORY,
            STATUS_HUD_EVIDENCE_CATEGORY,
            NOTE_MEMORY_EVIDENCE_CATEGORY,
        ] {
            assert!(categories.contains(expected), "missing evidence {expected}");
            assert!(is_evidence_category(expected));
        }
    }
}

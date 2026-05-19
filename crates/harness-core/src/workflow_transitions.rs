use std::collections::BTreeSet;

pub const TRACKED_WORKFLOW_MODES: &[&str] = &[
    "autopilot",
    "autoresearch",
    "team",
    "ralph",
    "ultrawork",
    "ultraqa",
    "ralplan",
    "deep-interview",
];

pub const POLICY_ALLOW: &str = "transition.workflow_mode_allow";
pub const POLICY_OVERLAP: &str = "transition.workflow_mode_overlap";
pub const POLICY_AUTO_COMPLETE: &str = "transition.workflow_mode_auto_complete";
pub const POLICY_DENIED: &str = "transition.workflow_mode_denied";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowTransitionAllowlistDecision {
    Allowed {
        source_auto_completes: Vec<String>,
        transition_message: Option<String>,
        policy_id: &'static str,
    },
    Overlap {
        policy_id: &'static str,
    },
    Denied {
        reason: String,
        suggested_action: String,
        policy_id: &'static str,
    },
}

pub struct WorkflowTransitionAllowlist;

impl WorkflowTransitionAllowlist {
    pub fn evaluate<'a>(
        current_active_modes: impl IntoIterator<Item = &'a str>,
        requested_mode: &str,
    ) -> WorkflowTransitionAllowlistDecision {
        let current_modes = normalize_tracked_modes(current_active_modes);
        let Some(requested_mode) = normalize_workflow_mode(requested_mode) else {
            return WorkflowTransitionAllowlistDecision::Allowed {
                source_auto_completes: Vec::new(),
                transition_message: None,
                policy_id: POLICY_ALLOW,
            };
        };

        if current_modes.iter().any(|mode| mode == requested_mode) || current_modes.is_empty() {
            return WorkflowTransitionAllowlistDecision::Allowed {
                source_auto_completes: Vec::new(),
                transition_message: None,
                policy_id: POLICY_ALLOW,
            };
        }

        let source_auto_completes = current_modes
            .iter()
            .filter(|mode| is_auto_complete_transition(mode, requested_mode))
            .cloned()
            .collect::<Vec<_>>();
        let survivable_modes = current_modes
            .iter()
            .filter(|mode| !source_auto_completes.contains(mode))
            .cloned()
            .collect::<Vec<_>>();

        if !source_auto_completes.is_empty()
            && survivable_modes
                .iter()
                .all(|mode| is_allowed_overlap(mode, requested_mode))
        {
            return WorkflowTransitionAllowlistDecision::Allowed {
                transition_message: source_auto_completes
                    .first()
                    .map(|source| build_workflow_transition_message(source, requested_mode)),
                source_auto_completes,
                policy_id: POLICY_AUTO_COMPLETE,
            };
        }

        if current_modes
            .iter()
            .all(|mode| is_allowed_overlap(mode, requested_mode))
        {
            return WorkflowTransitionAllowlistDecision::Overlap {
                policy_id: POLICY_OVERLAP,
            };
        }

        WorkflowTransitionAllowlistDecision::Denied {
            reason: build_workflow_transition_error(&current_modes, requested_mode),
            suggested_action:
                "Complete or cancel the incompatible workflow through coordinator workflow signoff/cancel, then retry."
                    .to_string(),
            policy_id: POLICY_DENIED,
        }
    }
}

pub fn normalize_workflow_mode(mode: &str) -> Option<&'static str> {
    match mode.trim() {
        "autopilot" | "workflow.autopilot" => Some("autopilot"),
        "autoresearch" | "workflow.research_mission" => Some("autoresearch"),
        "team" | "swarm" | "workflow.team_escalation" => Some("team"),
        "ralph" | "workflow.ralph" => Some("ralph"),
        "ultrawork" | "ulw" | "workflow.ultrawork" => Some("ultrawork"),
        "ultraqa" | "workflow.qa" => Some("ultraqa"),
        "ralplan" | "plan" | "workflow.plan_consensus" => Some("ralplan"),
        "deep-interview" | "deep_interview" | "workflow.deep_interview" => Some("deep-interview"),
        _ => None,
    }
}

pub fn build_workflow_transition_message(source_mode: &str, requested_mode: &str) -> String {
    format!("mode transiting: {source_mode} -> {requested_mode}")
}

fn normalize_tracked_modes<'a>(modes: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for mode in modes {
        let Some(mode) = normalize_workflow_mode(mode) else {
            continue;
        };
        if seen.insert(mode) {
            normalized.push(mode.to_string());
        }
    }
    normalized
}

fn is_allowed_overlap(left: &str, right: &str) -> bool {
    if left == "ultrawork" || right == "ultrawork" {
        return true;
    }
    [left, right] == ["ralph", "team"] || [left, right] == ["team", "ralph"]
}

fn is_auto_complete_transition(source_mode: &str, requested_mode: &str) -> bool {
    matches!(
        (source_mode, requested_mode),
        ("deep-interview", "ralplan")
            | ("deep-interview", "autoresearch")
            | ("ralplan", "team")
            | ("ralplan", "ralph")
            | ("ralplan", "autopilot")
            | ("ralplan", "autoresearch")
    )
}

fn is_rollback_transition(current_modes: &[String], requested_mode: &str) -> bool {
    is_planning_like_mode(requested_mode)
        && current_modes
            .iter()
            .any(|mode| is_execution_like_mode(mode.as_str()))
}

fn is_planning_like_mode(mode: &str) -> bool {
    matches!(mode, "deep-interview" | "ralplan")
}

fn is_execution_like_mode(mode: &str) -> bool {
    matches!(
        mode,
        "autopilot" | "autoresearch" | "team" | "ralph" | "ultrawork" | "ultraqa"
    )
}

fn format_active_modes(modes: &[String]) -> String {
    match modes {
        [] => "no tracked workflows".to_string(),
        [only] => format!("{only} is already active"),
        [left, right] => format!("{left} and {right} are already active"),
        [rest @ .., last] => {
            format!("{}, and {last} are already active", rest.join(", "))
        }
    }
}

fn build_workflow_transition_error(current_modes: &[String], requested_mode: &str) -> String {
    let active_modes = format_active_modes(current_modes);
    if is_rollback_transition(current_modes, requested_mode) {
        return format!(
            "Cannot activate {requested_mode}: {active_modes}. Execution-to-planning rollback auto-complete is not allowed."
        );
    }
    let mut overlap_modes = current_modes.to_vec();
    overlap_modes.push(requested_mode.to_string());
    let overlap = overlap_modes.join(" + ");
    format!(
        "Cannot activate {requested_mode}: {active_modes}. Unsupported workflow overlap: {overlap}. Current state is unchanged."
    )
}

#[cfg(test)]
mod tests {
    use super::{WorkflowTransitionAllowlist, WorkflowTransitionAllowlistDecision};

    #[test]
    fn deep_interview_to_ralplan_auto_completes_source() {
        let decision = WorkflowTransitionAllowlist::evaluate(["deep-interview"], "ralplan");
        assert_eq!(
            decision,
            WorkflowTransitionAllowlistDecision::Allowed {
                source_auto_completes: vec!["deep-interview".to_string()],
                transition_message: Some("mode transiting: deep-interview -> ralplan".to_string()),
                policy_id: "transition.workflow_mode_auto_complete",
            }
        );
    }

    #[test]
    fn ralph_to_ralplan_rollback_is_denied() {
        let decision = WorkflowTransitionAllowlist::evaluate(["ralph"], "ralplan");
        let WorkflowTransitionAllowlistDecision::Denied {
            reason,
            suggested_action,
            policy_id,
        } = decision
        else {
            panic!("expected denied rollback")
        };
        assert_eq!(policy_id, "transition.workflow_mode_denied");
        assert!(reason.contains("Execution-to-planning rollback"));
        assert!(suggested_action.contains("workflow signoff/cancel"));
    }

    #[test]
    fn autopilot_to_ralplan_direct_start_is_denied() {
        let decision = WorkflowTransitionAllowlist::evaluate(["autopilot"], "ralplan");
        let WorkflowTransitionAllowlistDecision::Denied {
            reason,
            suggested_action,
            policy_id,
        } = decision
        else {
            panic!("expected denied direct autopilot loopback")
        };
        assert_eq!(policy_id, "transition.workflow_mode_denied");
        assert!(reason.contains("Execution-to-planning rollback"));
        assert!(suggested_action.contains("workflow signoff/cancel"));
    }

    #[test]
    fn team_and_ralph_are_allowed_to_overlap() {
        assert_eq!(
            WorkflowTransitionAllowlist::evaluate(["team"], "ralph"),
            WorkflowTransitionAllowlistDecision::Overlap {
                policy_id: "transition.workflow_mode_overlap",
            }
        );
    }

    #[test]
    fn ultrawork_can_overlap_with_any_tracked_mode() {
        assert_eq!(
            WorkflowTransitionAllowlist::evaluate(["ultrawork"], "autopilot"),
            WorkflowTransitionAllowlistDecision::Overlap {
                policy_id: "transition.workflow_mode_overlap",
            }
        );
    }

    #[test]
    fn unknown_modes_are_outside_the_allowlist() {
        assert_eq!(
            WorkflowTransitionAllowlist::evaluate(["workflow.status"], "workflow.snapshot"),
            WorkflowTransitionAllowlistDecision::Allowed {
                source_auto_completes: Vec::new(),
                transition_message: None,
                policy_id: "transition.workflow_mode_allow",
            }
        );
    }
}

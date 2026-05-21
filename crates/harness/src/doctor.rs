use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::Args;
use harness_core::agent_catalog::{
    resolve_agent_catalog, BUILTIN_SUBAGENTS, CATEGORY_ROUTES, LEGACY_PRIMARY_PROFILE_ALIASES,
    PRIMARY_WORKFLOW_PROFILES,
};
use harness_core::command_registry::{CommandAction, CommandRegistry};
use harness_core::config::{
    configured_model_catalog, load_resolved_config, resolve_model_selection, AgentMode,
    CompatibilityImportState, HarnessConfig, McpServerConfig, PermissionMode, ProviderConfig,
};
use harness_core::context_snapshot::{
    ContextSnapshotOptions, CONTEXT_SNAPSHOT_ARTIFACT_DIR, CONTEXT_SNAPSHOT_SCHEMA_VERSION,
};
use harness_core::workflow::{
    project_workflows, WorkflowSignoffPolicy, SIMULATED_TOOL_EVIDENCE_CATEGORY,
};
use harness_core::workflow_closeout::{
    builtin_policy_ids, is_builtin_policy_id, WORKFLOW_CLOSEOUT_DOSSIER_EVIDENCE_CATEGORY,
};
use harness_core::workflow_registry::{
    stable_id_groups, TRANSITION_POLICIES, WORKFLOW_DOCS_ANCHORS, WORKFLOW_DOCTOR_CHECKS,
};
use harness_core::workflow_transitions::{
    WorkflowTransitionAllowlist, WorkflowTransitionAllowlistDecision, POLICY_ALLOW,
    POLICY_AUTO_COMPLETE, POLICY_DENIED, POLICY_OVERLAP, TRACKED_WORKFLOW_MODES,
};
use harness_tools::{coordinator_registry_with_mcp_and_editing, EditingToolSurfaceConfig};
use serde::Serialize;
use serde_json::Value;

use crate::cli_io::{load_events_from_run_dir, EVENTS_FILE_NAME};

const CONFIG_DOC_MD: &str = include_str!("../../../docs/config.md");
const TESTING_DOC_MD: &str = include_str!("../../../docs/testing.md");
const WORKFLOW_PARITY_MATRIX_JSON: &str = include_str!("../../../docs/workflow-parity-matrix.json");
const STRICT_PARITY_PROOF_ROOT_ENV: &str = "HARNESS_STRICT_PARITY_PROOF_ROOT";
const STRICT_PARITY_DEFAULT_PROOF_ROOT: &str = "target/harness-parity/latest/selected-workflows";

const BUILD_TOOLS: [&str; 5] = [
    "todowrite",
    "task",
    "background_output",
    "plan_enter",
    "edit",
];
const PLAN_TOOLS: [&str; 4] = ["todowrite", "task", "background_output", "plan_exit"];
const DISCIPLINE_TOOLS: [&str; 6] = [
    "todowrite",
    "task",
    "background_output",
    "plan_enter",
    "skill",
    "edit",
];
const FIRST_SLICE_COMPATIBILITY_TOOLS: [&str; 24] = [
    "background_cancel",
    "ast_grep_search",
    "ast_grep_replace",
    "look_at",
    "interactive_bash",
    "terminal_spawn",
    "terminal_write",
    "terminal_screenshot",
    "terminal_resize",
    "terminal_kill",
    "terminal_list",
    "session_list",
    "session_read",
    "session_search",
    "session_info",
    "task_create",
    "task_list",
    "task_get",
    "task_update",
    "team_list",
    "team_create",
    "team_status",
    "team_task_create",
    "team_task_list",
];

#[derive(Debug, Args, Clone, Default)]
pub(crate) struct DoctorCommand {
    /// Emit machine-readable JSON instead of text.
    #[arg(long, default_value_t = false)]
    json: bool,

    /// Enforce selected workflow parity matrix rows as a hard release gate.
    #[arg(long, default_value_t = false)]
    strict_parity: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
        }
    }
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    id: String,
    name: String,
    status: CheckStatus,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    config: String,
    checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    fn has_failures(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == CheckStatus::Fail)
    }

    fn status_counts(&self) -> (usize, usize, usize) {
        let mut passes = 0;
        let mut warnings = 0;
        let mut failures = 0;
        for check in &self.checks {
            match check.status {
                CheckStatus::Pass => passes += 1,
                CheckStatus::Warn => warnings += 1,
                CheckStatus::Fail => failures += 1,
            }
        }
        (passes, warnings, failures)
    }
}

pub(crate) fn execute(
    command: DoctorCommand,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
) -> ExitCode {
    let Some(loaded) = (match load_resolved_config(config_path.as_deref()) {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("doctor failed: {err}");
            return ExitCode::from(1);
        }
    }) else {
        eprintln!(
            "doctor failed: no config file found; pass --config <path>, create ./harness.jsonc or ./harness.json, or start from configs/harness.example.jsonc"
        );
        return ExitCode::from(2);
    };

    let config_display = loaded.path_display();
    let mut config = loaded.config;
    config.apply_session_dir_override(session_dir);

    let report = build_report(config_display, &config, command.strict_parity);
    if command.json {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(err) => {
                eprintln!("doctor failed to render JSON: {err}");
                return ExitCode::from(1);
            }
        }
    } else {
        print_text_report(&report);
    }

    if report.has_failures() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn build_report(
    config_display: String,
    config: &HarnessConfig,
    strict_parity: bool,
) -> DoctorReport {
    let mut checks = vec![
        check_provider_catalog(config),
        check_provider_credentials(config),
        check_model_references(config),
        check_model_capabilities(config),
        check_shipped_profiles(config),
        check_category_routes(config),
        check_agent_catalog(config),
        check_profile_tools(config),
        check_first_slice_compatibility_tool_surface(config),
        check_command_registry(),
        check_dollar_alias_wiring(),
        check_shipped_skill_loadability(),
        check_workflow_skill_protocol_native(),
        check_workflow_transition_policy_matrix(),
        check_workflow_contract_registry(),
        check_workflow_context_snapshot_contract(),
        check_workflow_runtime_config(config),
        check_workflow_closeout_policy(config),
        check_workflow_closeout_readiness(config),
        check_workflow_catalog_health(config),
        check_workflow_simulator_contract(),
        check_workflow_stale_work_loop(config),
        check_permissions(config),
        check_session_dir(&config.paths.session_dir),
        check_mcp(config),
        check_compatibility_imports(config),
        check_team_mode(),
        check_terminal_browser_media(),
        check_parity_ledger(),
        check_compatibility_parity_gaps(),
    ];
    if strict_parity {
        checks.push(check_strict_parity_matrix());
    }

    DoctorReport {
        config: config_display,
        checks,
    }
}

fn print_text_report(report: &DoctorReport) {
    let (passes, warnings, failures) = report.status_counts();
    let headline = if failures == 0 && warnings == 0 {
        "doctor ok"
    } else if failures == 0 {
        "doctor ok with warnings"
    } else {
        "doctor found issues"
    };
    println!("{headline}: {}", report.config);
    println!("checks: {passes} passed, {warnings} warnings, {failures} failures");
    for check in &report.checks {
        println!(
            "[{}] {}: {}",
            check.status.label(),
            check.name,
            check.message
        );
    }
}

fn check_command_registry() -> DoctorCheck {
    let registry = CommandRegistry::builtins();
    let required = [
        "workflow-run",
        "workflow-status",
        "workflow-signoff",
        "workflow-cancel",
        "workflow-dossier",
        "workflow-snapshot",
        "plan-consensus",
        "goal-ledger",
        "research-mission",
        "wiki",
        "init-deep",
        "ralph-loop",
        "ulw-loop",
        "cancel-ralph",
        "refactor",
        "start-work",
        "stop-continuation",
        "remove-ai-slops",
        "handoff",
        "hyperplan",
    ];
    let missing = required
        .iter()
        .filter(|name| registry.get(name).is_none())
        .copied()
        .collect::<Vec<_>>();
    let unsafe_shell = registry.commands().iter().any(|command| {
        matches!(
            command.action,
            CommandAction::NativeTool {
                tool_id: "bash" | "shell.run"
            }
        )
    });
    if !missing.is_empty() {
        return fail(
            "command_registry",
            format!("missing built-in commands: {}", missing.join(", ")),
        );
    }
    if unsafe_shell {
        return fail(
            "command_registry",
            "command templates must not execute shell directly; use native tool permissions",
        );
    }
    pass(
        "command_registry",
        format!(
            "{} built-in commands across {} custom roots",
            registry.commands().len(),
            CommandRegistry::roots().len()
        ),
    )
}

fn check_dollar_alias_wiring() -> DoctorCheck {
    let registry = CommandRegistry::builtins();
    let mut alias_owners = BTreeMap::<&str, Vec<&str>>::new();
    let mut invalid = Vec::new();
    let mut workflow_skill_aliases = 0_usize;
    let mut slash_agent_aliases = 0_usize;
    let mut continuation_aliases = 0_usize;

    for command in registry.commands() {
        for alias in command.dollar_aliases {
            alias_owners.entry(alias).or_default().push(command.name);
            match &command.action {
                CommandAction::WorkflowSkill { skill, .. } => {
                    workflow_skill_aliases += 1;
                    if skill.trim().is_empty() {
                        invalid.push(format!("${alias} has an empty workflow skill id"));
                    }
                }
                CommandAction::SlashAgent { role } => {
                    slash_agent_aliases += 1;
                    if role.trim().is_empty() {
                        invalid.push(format!("${alias} has an empty slash-agent role"));
                    }
                }
                CommandAction::StopContinuation => {
                    continuation_aliases += 1;
                }
                CommandAction::WorkflowIntent { intent } => {
                    if intent.as_str().trim().is_empty() {
                        invalid.push(format!("${alias} has an empty workflow intent"));
                    }
                }
                CommandAction::BlockedWorkflow { .. } if !command.enabled_by_default => {}
                CommandAction::PromptTemplate { .. }
                | CommandAction::LoadSkills { .. }
                | CommandAction::PlanArtifact { .. }
                | CommandAction::HandoffArtifact { .. } => invalid.push(format!(
                    "${alias} resolves to prompt/artifact placeholder `{}`",
                    command.name
                )),
                other => invalid.push(format!(
                    "${alias} resolves to unsupported dollar action `{other:?}` on `{}`",
                    command.name
                )),
            }

            if command.enabled_by_default
                && command.availability
                    != harness_core::command_registry::WorkflowCommandAvailability::Present
            {
                invalid.push(format!(
                    "${alias} is enabled but `{}` is not present",
                    command.name
                ));
            }
        }
    }

    let duplicate_aliases = alias_owners
        .iter()
        .filter(|(_, owners)| owners.len() > 1)
        .map(|(alias, owners)| format!("${alias}: {}", owners.join(", ")))
        .collect::<Vec<_>>();
    invalid.extend(duplicate_aliases);

    if !invalid.is_empty() {
        return fail(
            "dollar_alias_wiring",
            format!(
                "{} dollar alias wiring issue(s): {}",
                invalid.len(),
                invalid.join("; ")
            ),
        );
    }

    pass_with_details(
        "dollar_alias_wiring",
        format!(
            "{} dollar alias(es) resolve to native workflow, continuation, or slash-agent actions",
            alias_owners.len()
        ),
        Some(serde_json::json!({
            "alias_count": alias_owners.len(),
            "workflow_skill_aliases": workflow_skill_aliases,
            "slash_agent_aliases": slash_agent_aliases,
            "continuation_aliases": continuation_aliases,
            "aliases": alias_owners.keys().copied().collect::<Vec<_>>(),
        })),
    )
}

fn check_shipped_skill_loadability() -> DoctorCheck {
    let workspace_root = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let shipped_skills = match shipped_skill_names(&workspace_root) {
        Ok(skills) => skills,
        Err(err) => return fail("shipped_skill_loadability", err),
    };
    if shipped_skills.is_empty() {
        return pass_with_details(
            "shipped_skill_loadability",
            "no shipped .agent-harness/skills/*/SKILL.md files were found",
            Some(serde_json::json!({
                "shipped_skill_count": 0,
                "shipped_skills": [],
            })),
        );
    }

    let report = match harness_tools::workflow_catalog_health_report(&workspace_root) {
        Ok(report) => report,
        Err(err) => {
            return fail(
                "shipped_skill_loadability",
                format!("could not inspect shipped skill catalog: {err}"),
            );
        }
    };
    let visible = skill_names_from_prefixed(&report.visible);
    let missing = skill_names_from_prefixed(&report.missing);
    let disabled = skill_names_from_prefixed(&report.disabled);
    let unavailable = shipped_skills
        .iter()
        .filter(|skill| !visible.contains(skill.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let invalid_shipped = shipped_skills
        .iter()
        .filter(|skill| missing.contains(skill.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let disabled_shipped = shipped_skills
        .iter()
        .filter(|skill| disabled.contains(skill.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    if !unavailable.is_empty() || !invalid_shipped.is_empty() || !disabled_shipped.is_empty() {
        return fail(
            "shipped_skill_loadability",
            format!(
                "shipped skill loadability failed; unavailable=[{}], invalid=[{}], disabled=[{}]",
                unavailable.join(", "),
                invalid_shipped.join(", "),
                disabled_shipped.join(", ")
            ),
        );
    }

    pass_with_details(
        "shipped_skill_loadability",
        format!(
            "{} shipped SKILL.md file(s) are discoverable and loadable",
            shipped_skills.len()
        ),
        Some(serde_json::json!({
            "shipped_skill_count": shipped_skills.len(),
            "shipped_skills": shipped_skills,
            "resolution_roots": report.resolution_roots,
        })),
    )
}

fn check_workflow_skill_protocol_native() -> DoctorCheck {
    let workspace_root = repo_root_path();
    let mut workflow_skills = CommandRegistry::builtins()
        .commands()
        .iter()
        .filter_map(|spec| match &spec.action {
            CommandAction::WorkflowSkill { skill, .. } => Some(*skill),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut checked = Vec::new();
    let mut findings = Vec::new();

    for skill in std::mem::take(&mut workflow_skills) {
        let skill_path = workspace_root
            .join(".agent-harness/skills")
            .join(skill)
            .join("SKILL.md");
        let body = match fs::read_to_string(&skill_path) {
            Ok(body) => body,
            Err(err) => {
                findings.push(WorkflowSkillProtocolFinding {
                    skill: skill.to_string(),
                    path: skill_path.display().to_string(),
                    reason_code: "missing_harness_state_contract",
                    severity: "fail",
                    token: None,
                    remediation: format!("make the shipped workflow SKILL.md readable: {err}"),
                });
                continue;
            }
        };

        let skill_findings = evaluate_workflow_skill_protocol_body(skill, &skill_path, &body);
        let skill_failed = skill_findings
            .iter()
            .any(|finding| finding.severity == "fail");
        if !skill_failed {
            checked.push(serde_json::json!({
                "skill": skill,
                "path": skill_path,
                "lines": body.lines().count(),
                "warnings": skill_findings.len(),
            }));
        }
        findings.extend(skill_findings);
    }

    if findings.iter().any(|finding| finding.severity == "fail") {
        let fatal_findings = findings
            .iter()
            .filter(|finding| finding.severity == "fail")
            .collect::<Vec<_>>();
        let message = format!(
            "{} native workflow skill protocol issue(s): {}",
            fatal_findings.len(),
            fatal_findings
                .iter()
                .map(|finding| format!(
                    "{}:{}{}",
                    finding.skill,
                    finding.reason_code,
                    finding
                        .token
                        .as_deref()
                        .map(|token| format!("({token})"))
                        .unwrap_or_default()
                ))
                .collect::<Vec<_>>()
                .join("; ")
        );
        return DoctorCheck {
            id: "workflow_skill_protocol_native".to_string(),
            name: "workflow_skill_protocol_native".to_string(),
            status: CheckStatus::Fail,
            message,
            details: Some(serde_json::json!({
                "findings": findings,
                "required_sections": REQUIRED_NATIVE_SKILL_SECTIONS,
                "hard_deprecated_policy": HARD_DEPRECATED_SKILL_PHRASE,
                "forbidden_token_scan": FORBIDDEN_WORKFLOW_SKILL_TOKENS.iter().map(|rule| {
                    serde_json::json!({
                        "token": rule.token,
                        "reason_code": rule.reason_code,
                        "severity": rule.severity,
                        "remediation": rule.remediation,
                    })
                }).collect::<Vec<_>>(),
            })),
        };
    }

    pass_with_details(
        "workflow_skill_protocol_native",
        format!(
            "{} workflow SKILL.md protocol body/bodies satisfy the native Harness contract",
            checked.len()
        ),
        Some(serde_json::json!({
            "checked": checked,
            "findings": findings,
            "required_sections": REQUIRED_NATIVE_SKILL_SECTIONS,
            "hard_deprecated_policy": HARD_DEPRECATED_SKILL_PHRASE,
            "forbidden_token_scan": FORBIDDEN_WORKFLOW_SKILL_TOKENS.iter().map(|rule| {
                serde_json::json!({
                    "token": rule.token,
                    "reason_code": rule.reason_code,
                    "severity": rule.severity,
                    "remediation": rule.remediation,
                })
            }).collect::<Vec<_>>(),
            "canonical_alias_policy": {
                "team": "user-facing",
                "worker": "internal",
                "team-mode": "hidden/internal compatibility only",
                "ai-slop-cleaner": "canonical cleanup workflow",
            },
        })),
    )
}

const REQUIRED_NATIVE_SKILL_SECTIONS: [&str; 7] = [
    "Purpose",
    "Use when",
    "Harness state contract",
    "Execution protocol",
    "Evidence and closeout contract",
    "Stop/escalation conditions",
    "Verification checklist",
];

const HARD_DEPRECATED_SKILL_PHRASE: &str = "Hard-deprecated. Do not invoke or route this skill";

const FORBIDDEN_WORKFLOW_SKILL_TOKENS: [ForbiddenWorkflowSkillToken; 13] = [
    ForbiddenWorkflowSkillToken {
        token: "legacy runtime state dir",
        reason_code: "forbidden_state_file_authority",
        severity: "fail",
        remediation: "Use Harness coordinator-owned workflow events, projections, and evidence artifacts instead of old legacy state directories.",
    },
    ForbiddenWorkflowSkillToken {
        token: "legacy runtime state file",
        reason_code: "forbidden_state_file_authority",
        severity: "fail",
        remediation: "Use Harness coordinator-owned workflow events, projections, and evidence artifacts instead of external state files.",
    },
    ForbiddenWorkflowSkillToken {
        token: "legacy state command",
        reason_code: "forbidden_legacy_cli_authority",
        severity: "fail",
        remediation: "Translate upstream CLI state operations into Harness workflow evidence/projection operations.",
    },
    ForbiddenWorkflowSkillToken {
        token: "legacy ask command",
        reason_code: "forbidden_legacy_cli_authority",
        severity: "fail",
        remediation: "Use the native `$ask` workflow and record advisor output as Harness evidence.",
    },
    ForbiddenWorkflowSkillToken {
        token: "legacy performance-goal command",
        reason_code: "forbidden_legacy_cli_authority",
        severity: "fail",
        remediation: "Use the native `$performance-goal` workflow and Harness goal/evidence projections.",
    },
    ForbiddenWorkflowSkillToken {
        token: "legacy team command",
        reason_code: "forbidden_legacy_cli_authority",
        severity: "fail",
        remediation: "Use native team_create/team_task_create/team_send_message/team_list tools as the team substrate.",
    },
    ForbiddenWorkflowSkillToken {
        token: "legacy question command",
        reason_code: "forbidden_legacy_cli_authority",
        severity: "fail",
        remediation: "Use the native question tool and workflow question evidence lifecycle.",
    },
    ForbiddenWorkflowSkillToken {
        token: "LEGACY_TEAM_ENV_",
        reason_code: "forbidden_tmux_authority",
        severity: "fail",
        remediation: "Remove team runtime environment-variable authority from shipped Harness skills.",
    },
    ForbiddenWorkflowSkillToken {
        token: "LEGACY_QUESTION_ENV_",
        reason_code: "forbidden_tmux_authority",
        severity: "fail",
        remediation: "Remove pane-routing question environment-variable authority from shipped Harness skills.",
    },
    ForbiddenWorkflowSkillToken {
        token: "tmux pane",
        reason_code: "forbidden_tmux_authority",
        severity: "fail",
        remediation: "Treat terminal multiplexing as optional diagnostics, not lifecycle proof.",
    },
    ForbiddenWorkflowSkillToken {
        token: "tmux send-keys",
        reason_code: "forbidden_tmux_authority",
        severity: "fail",
        remediation: "Use coordinator messages/tasks instead of terminal keystroke routing.",
    },
    ForbiddenWorkflowSkillToken {
        token: "Codex goal mode",
        reason_code: "forbidden_goal_mode_authority",
        severity: "fail",
        remediation: "Use Harness workflow goal ledgers/evidence as authority; external goal snapshots are optional context.",
    },
    ForbiddenWorkflowSkillToken {
        token: "goal mode is the authority",
        reason_code: "forbidden_goal_mode_authority",
        severity: "fail",
        remediation: "Do not make external goal mode the Harness source of truth.",
    },
];

#[derive(Debug, Clone, Copy)]
struct ForbiddenWorkflowSkillToken {
    token: &'static str,
    reason_code: &'static str,
    severity: &'static str,
    remediation: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct WorkflowSkillProtocolFinding {
    skill: String,
    path: String,
    reason_code: &'static str,
    severity: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    remediation: String,
}

fn evaluate_workflow_skill_protocol_body(
    skill: &str,
    path: &Path,
    body: &str,
) -> Vec<WorkflowSkillProtocolFinding> {
    let mut findings = Vec::new();
    let lower = body.to_lowercase();
    let path = path.display().to_string();

    if skill_body_is_hard_deprecated(body) {
        for required in ["Hard-deprecated", "Do not invoke or route this skill"] {
            if !lower.contains(&required.to_lowercase()) {
                findings.push(WorkflowSkillProtocolFinding {
                    skill: skill.to_string(),
                    path: path.clone(),
                    reason_code: "missing_harness_deprecation_contract",
                    severity: "fail",
                    token: Some(required.to_string()),
                    remediation: "Preserve the hard-deprecated compatibility shim contract."
                        .to_string(),
                });
            }
        }
        return findings;
    }

    for section in REQUIRED_NATIVE_SKILL_SECTIONS {
        let section_lower = section.to_lowercase();
        let section_present = lower.contains(&format!("## {section_lower}"))
            || lower.contains(&format!("<{}>", section_lower.replace(' ', "_")))
            || lower.contains(&section_lower);
        if !section_present {
            let reason_code = match section {
                "Harness state contract" => "missing_harness_state_contract",
                "Evidence and closeout contract" | "Verification checklist" => {
                    "missing_evidence_closeout_contract"
                }
                _ => "missing_harness_state_contract",
            };
            findings.push(WorkflowSkillProtocolFinding {
                skill: skill.to_string(),
                path: path.clone(),
                reason_code,
                severity: "fail",
                token: None,
                remediation: format!(
                    "Add a native Harness `{section}` section to the shipped workflow skill."
                ),
            });
        }
    }

    if !lower.contains("harness")
        || !(lower.contains("workflow evidence")
            || lower.contains("workflow projection")
            || lower.contains("coordinator-owned"))
    {
        findings.push(WorkflowSkillProtocolFinding {
            skill: skill.to_string(),
            path: path.clone(),
            reason_code: "missing_harness_state_contract",
            severity: "fail",
            token: None,
            remediation: "State the Harness event/projection/evidence contract explicitly."
                .to_string(),
        });
    }

    for rule in FORBIDDEN_WORKFLOW_SKILL_TOKENS {
        if lower.contains(&rule.token.to_lowercase()) {
            findings.push(WorkflowSkillProtocolFinding {
                skill: skill.to_string(),
                path: path.clone(),
                reason_code: rule.reason_code,
                severity: rule.severity,
                token: Some(rule.token.to_string()),
                remediation: rule.remediation.to_string(),
            });
        }
    }

    findings
}

fn skill_body_is_hard_deprecated(body: &str) -> bool {
    body.to_lowercase()
        .contains(&HARD_DEPRECATED_SKILL_PHRASE.to_lowercase())
}

fn check_workflow_transition_policy_matrix() -> DoctorCheck {
    const EXPECTED_TRACKED_MODES: &[&str] = &[
        "autopilot",
        "autoresearch",
        "team",
        "ralph",
        "ultrawork",
        "ultraqa",
        "ralplan",
        "deep-interview",
    ];
    const MATRIX_CASES: &[TransitionMatrixCase] = &[
        TransitionMatrixCase {
            name: "empty_to_autopilot_allows",
            current: &[],
            requested: "autopilot",
            expected_policy: POLICY_ALLOW,
            expected_auto_completes: &[],
        },
        TransitionMatrixCase {
            name: "deep_interview_to_ralplan_auto_completes",
            current: &["deep-interview"],
            requested: "ralplan",
            expected_policy: POLICY_AUTO_COMPLETE,
            expected_auto_completes: &["deep-interview"],
        },
        TransitionMatrixCase {
            name: "ralplan_to_team_auto_completes",
            current: &["ralplan"],
            requested: "team",
            expected_policy: POLICY_AUTO_COMPLETE,
            expected_auto_completes: &["ralplan"],
        },
        TransitionMatrixCase {
            name: "ralplan_to_autopilot_auto_completes",
            current: &["ralplan"],
            requested: "autopilot",
            expected_policy: POLICY_AUTO_COMPLETE,
            expected_auto_completes: &["ralplan"],
        },
        TransitionMatrixCase {
            name: "ralph_team_overlap",
            current: &["ralph"],
            requested: "team",
            expected_policy: POLICY_OVERLAP,
            expected_auto_completes: &[],
        },
        TransitionMatrixCase {
            name: "ultrawork_fanout_overlap",
            current: &["ultrawork"],
            requested: "autopilot",
            expected_policy: POLICY_OVERLAP,
            expected_auto_completes: &[],
        },
        TransitionMatrixCase {
            name: "ralph_to_ralplan_denied",
            current: &["ralph"],
            requested: "ralplan",
            expected_policy: POLICY_DENIED,
            expected_auto_completes: &[],
        },
        TransitionMatrixCase {
            name: "autopilot_to_ralplan_denied",
            current: &["autopilot"],
            requested: "ralplan",
            expected_policy: POLICY_DENIED,
            expected_auto_completes: &[],
        },
        TransitionMatrixCase {
            name: "team_to_autopilot_denied",
            current: &["team"],
            requested: "autopilot",
            expected_policy: POLICY_DENIED,
            expected_auto_completes: &[],
        },
    ];

    let mut failures = Vec::new();
    if TRACKED_WORKFLOW_MODES != EXPECTED_TRACKED_MODES {
        failures.push(format!(
            "tracked modes drifted: expected {:?}, got {:?}",
            EXPECTED_TRACKED_MODES, TRACKED_WORKFLOW_MODES
        ));
    }

    let registered_policies = TRANSITION_POLICIES
        .iter()
        .map(|spec| spec.id)
        .collect::<BTreeSet<_>>();
    for policy in [
        POLICY_ALLOW,
        POLICY_OVERLAP,
        POLICY_AUTO_COMPLETE,
        POLICY_DENIED,
    ] {
        if !registered_policies.contains(policy) {
            failures.push(format!("transition policy `{policy}` is not registered"));
        }
    }

    for case in MATRIX_CASES {
        let decision =
            WorkflowTransitionAllowlist::evaluate(case.current.iter().copied(), case.requested);
        let actual_policy = transition_decision_policy_id(&decision);
        if actual_policy != case.expected_policy {
            failures.push(format!(
                "{} expected policy {}, got {} ({decision:?})",
                case.name, case.expected_policy, actual_policy
            ));
            continue;
        }
        if let WorkflowTransitionAllowlistDecision::Allowed {
            source_auto_completes,
            ..
        } = &decision
        {
            let expected = case
                .expected_auto_completes
                .iter()
                .map(|mode| (*mode).to_string())
                .collect::<Vec<_>>();
            if source_auto_completes != &expected {
                failures.push(format!(
                    "{} expected auto-complete {:?}, got {:?}",
                    case.name, expected, source_auto_completes
                ));
            }
        } else if !case.expected_auto_completes.is_empty() {
            failures.push(format!(
                "{} expected auto-complete {:?}, got {decision:?}",
                case.name, case.expected_auto_completes
            ));
        }
    }

    if !failures.is_empty() {
        return fail(
            "workflow_transition_policy_matrix",
            format!(
                "{} workflow transition policy issue(s): {}",
                failures.len(),
                failures.join("; ")
            ),
        );
    }

    pass_with_details(
        "workflow_transition_policy_matrix",
        format!(
            "{} tracked mode(s), {} matrix case(s), and {} registry policy id(s) match the native legacy transition contract",
            TRACKED_WORKFLOW_MODES.len(),
            MATRIX_CASES.len(),
            registered_policies.len()
        ),
        Some(serde_json::json!({
            "tracked_modes": TRACKED_WORKFLOW_MODES,
            "matrix_cases": MATRIX_CASES.iter().map(|case| case.name).collect::<Vec<_>>(),
            "policy_ids": [POLICY_ALLOW, POLICY_OVERLAP, POLICY_AUTO_COMPLETE, POLICY_DENIED],
        })),
    )
}

#[derive(Debug, Clone, Copy)]
struct TransitionMatrixCase {
    name: &'static str,
    current: &'static [&'static str],
    requested: &'static str,
    expected_policy: &'static str,
    expected_auto_completes: &'static [&'static str],
}

fn transition_decision_policy_id(decision: &WorkflowTransitionAllowlistDecision) -> &'static str {
    match decision {
        WorkflowTransitionAllowlistDecision::Allowed { policy_id, .. }
        | WorkflowTransitionAllowlistDecision::Overlap { policy_id }
        | WorkflowTransitionAllowlistDecision::Denied { policy_id, .. } => policy_id,
    }
}

fn shipped_skill_names(workspace_root: &Path) -> Result<Vec<String>, String> {
    let skill_root = workspace_root.join(".agent-harness/skills");
    if !skill_root.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(&skill_root)
        .map_err(|err| format!("could not read {}: {err}", skill_root.display()))?;
    let mut skills = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            format!(
                "could not inspect entry under {}: {err}",
                skill_root.display()
            )
        })?;
        let file_type = entry.file_type().map_err(|err| {
            format!(
                "could not inspect skill entry {}: {err}",
                entry.path().display()
            )
        })?;
        if !file_type.is_dir() || !entry.path().join("SKILL.md").is_file() {
            continue;
        }
        skills.push(entry.file_name().to_string_lossy().to_string());
    }
    skills.sort();
    skills.dedup();
    Ok(skills)
}

fn skill_names_from_prefixed(entries: &[String]) -> BTreeSet<&str> {
    entries
        .iter()
        .filter_map(|entry| entry.strip_prefix("skill:"))
        .collect()
}

fn check_workflow_contract_registry() -> DoctorCheck {
    let mut duplicate_groups = Vec::new();
    let mut group_counts = serde_json::Map::new();
    for (group, ids) in stable_id_groups() {
        let unique = ids.iter().copied().collect::<BTreeSet<_>>();
        group_counts.insert(group.to_string(), serde_json::json!(ids.len()));
        if unique.len() != ids.len() {
            duplicate_groups.push(group);
        }
    }
    if !duplicate_groups.is_empty() {
        return fail(
            "workflow_contract_registry",
            format!(
                "duplicate workflow contract id(s) in group(s): {}",
                duplicate_groups.join(", ")
            ),
        );
    }

    let missing_docs = WORKFLOW_DOCS_ANCHORS
        .iter()
        .filter(|anchor| match anchor.path {
            "docs/config.md" => !CONFIG_DOC_MD.contains(anchor.heading),
            "docs/testing.md" => !TESTING_DOC_MD.contains(anchor.heading),
            _ => true,
        })
        .map(|anchor| format!("{}:{}", anchor.path, anchor.heading))
        .collect::<Vec<_>>();
    if !missing_docs.is_empty() {
        return fail(
            "workflow_contract_registry",
            format!(
                "missing workflow docs anchor(s): {}",
                missing_docs.join(", ")
            ),
        );
    }

    pass_with_details(
        "workflow_contract_registry",
        format!(
            "{} workflow doctor check id(s), {} docs anchor(s), stable ids split by crate responsibility",
            WORKFLOW_DOCTOR_CHECKS.len(),
            WORKFLOW_DOCS_ANCHORS.len()
        ),
        Some(serde_json::json!({
            "id_groups": group_counts,
            "docs_anchors": WORKFLOW_DOCS_ANCHORS.iter().map(|anchor| {
                serde_json::json!({
                    "id": anchor.id,
                    "path": anchor.path,
                    "heading": anchor.heading,
                })
            }).collect::<Vec<_>>(),
        })),
    )
}

fn check_strict_parity_matrix() -> DoctorCheck {
    let matrix: Value = match serde_json::from_str(WORKFLOW_PARITY_MATRIX_JSON) {
        Ok(matrix) => matrix,
        Err(err) => {
            return fail(
                "strict_parity_matrix",
                format!("docs/workflow-parity-matrix.json is not valid JSON: {err}"),
            )
        }
    };
    let Some(required_fields) = matrix.get("required_row_fields").and_then(Value::as_array) else {
        return fail(
            "strict_parity_matrix",
            "docs/workflow-parity-matrix.json is missing required_row_fields",
        );
    };
    let required_fields = required_fields
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if required_fields.is_empty() {
        return fail(
            "strict_parity_matrix",
            "docs/workflow-parity-matrix.json required_row_fields must not be empty",
        );
    }
    let Some(rows) = matrix.get("rows").and_then(Value::as_array) else {
        return fail(
            "strict_parity_matrix",
            "docs/workflow-parity-matrix.json is missing rows",
        );
    };

    let mut blockers = Vec::new();
    let mut selected_count = 0usize;
    for row in rows {
        let row_label = row
            .get("canonical_harness_id")
            .and_then(Value::as_str)
            .or_else(|| row.get("registry_command").and_then(Value::as_str))
            .unwrap_or("<unknown>");
        for field in &required_fields {
            if matrix_field_is_empty(row, field) {
                blockers.push(format!("{row_label}: missing required field {field}"));
            }
        }

        if row.get("state_authority").and_then(Value::as_str)
            != Some("harness_events_and_replay_projections")
        {
            blockers.push(format!(
                "{row_label}: state_authority is not Harness-native"
            ));
        }

        let selected_scope = row
            .get("selected_scope")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let status = row
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if matches!(status, "native_partial" | "planned" | "staged") {
            blockers.push(format!(
                "{row_label}: strict parity cannot leave workflow row in incomplete status {status:?}"
            ));
        }

        if selected_scope == "retired_with_reason" || status == "compat_only" {
            validate_retired_parity_row(row_label, row, &mut blockers);
            continue;
        }

        if selected_scope != "selected_for_this_goal" {
            blockers.push(format!(
                "{row_label}: active workflow row has selected_scope {selected_scope:?}, expected selected_for_this_goal or retired_with_reason"
            ));
            continue;
        }
        selected_count += 1;

        if status != "native_complete" {
            blockers.push(format!(
                "{row_label}: selected row status is {status:?}, expected native_complete"
            ));
        }

        let minimum_scope = row
            .get("minimum_1_to_1_scope")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if minimum_scope != "native_workflow" {
            blockers.push(format!(
                "{row_label}: selected row minimum_1_to_1_scope is {minimum_scope:?}, expected native_workflow"
            ));
        }

        validate_selected_parity_dossier(row_label, row, &mut blockers);
        validate_selected_execution_proof(row_label, row, &mut blockers);

        let native_contract = row
            .get("native_behavior_contract")
            .and_then(Value::as_str)
            .unwrap_or_default();
        for forbidden in [
            "legacy state command",
            "legacy team command",
            "legacy question command",
            "native team tools api",
        ] {
            if native_contract.contains(forbidden) {
                blockers.push(format!(
                    "{row_label}: native_behavior_contract contains forbidden runtime authority token {forbidden:?}"
                ));
            }
        }

        let parity_dimensions = row
            .get("parity_dimensions")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        for dimension in [
            "invocation",
            "state",
            "artifacts",
            "permissions",
            "replay",
            "tui",
            "negative_path",
        ] {
            if !parity_dimensions.contains(dimension) {
                blockers.push(format!(
                    "{row_label}: missing selected parity dimension {dimension}"
                ));
            }
        }
    }

    if selected_count == 0 {
        blockers.push("matrix has no selected_for_this_goal rows".to_string());
    }
    validate_active_runtime_assets_no_old_authority(&mut blockers);

    if blockers.is_empty() {
        return pass_with_details(
            "strict_parity_matrix",
            format!(
                "{selected_count} active workflow parity row(s) have complete proof evidence; retired legacy runtime shims are excluded from native_complete credit"
            ),
            Some(serde_json::json!({
                "selected_rows": selected_count,
                "matrix": "docs/workflow-parity-matrix.json",
            })),
        );
    }

    fail_with_details(
        "strict_parity_matrix",
        format!(
            "{} strict parity blocker(s) across {selected_count} selected row(s)",
            blockers.len()
        ),
        Some(serde_json::json!({
            "selected_rows": selected_count,
            "blockers": blockers,
            "matrix": "docs/workflow-parity-matrix.json",
        })),
    )
}

fn matrix_field_is_empty(row: &Value, field: &str) -> bool {
    match row.get(field) {
        Some(Value::String(value)) => value.trim().is_empty(),
        Some(Value::Array(values)) => values.is_empty(),
        Some(Value::Object(values)) => values.is_empty(),
        Some(Value::Null) | None => true,
        Some(Value::Bool(_)) | Some(Value::Number(_)) => false,
    }
}

fn validate_retired_parity_row(row_label: &str, row: &Value, blockers: &mut Vec<String>) {
    let selected_scope = row
        .get("selected_scope")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = row
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let minimum_scope = row
        .get("minimum_1_to_1_scope")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let native_contract = row
        .get("native_behavior_contract")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase();

    if selected_scope != "retired_with_reason" {
        blockers.push(format!(
            "{row_label}: compatibility-only row must use selected_scope retired_with_reason"
        ));
    }
    if status != "compat_only" {
        blockers.push(format!(
            "{row_label}: retired legacy runtime shim status is {status:?}, expected compat_only"
        ));
    }
    if minimum_scope != "retired_compatibility_shim" {
        blockers.push(format!(
            "{row_label}: retired legacy runtime shim minimum_1_to_1_scope is {minimum_scope:?}, expected retired_compatibility_shim"
        ));
    }
    if !(native_contract.contains("hard-deprecated")
        || native_contract.contains("compatibility shim"))
    {
        blockers.push(format!(
            "{row_label}: retired legacy runtime shim contract must explicitly describe compatibility/deprecation behavior"
        ));
    }
}

fn validate_selected_parity_dossier(row_label: &str, row: &Value, blockers: &mut Vec<String>) {
    validate_selected_parity_dossier_with_root(row_label, row, &repo_root_path(), blockers);
}

fn validate_selected_parity_dossier_with_root(
    row_label: &str,
    row: &Value,
    repo_root: &Path,
    blockers: &mut Vec<String>,
) {
    let dossier_path = row
        .get("evidence_dossier_path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if dossier_path.contains("${") {
        blockers.push(format!(
            "{row_label}: evidence_dossier_path still contains an unresolved template: {dossier_path}"
        ));
        return;
    }
    let path = resolve_repo_relative_path(repo_root, dossier_path);
    let body = match fs::read_to_string(&path) {
        Ok(body) => body,
        Err(err) => {
            blockers.push(format!(
                "{row_label}: evidence_dossier_path does not point at a captured proof file: {dossier_path} ({err})"
            ));
            return;
        }
    };
    let dossier = match serde_json::from_str::<Value>(&body) {
        Ok(dossier) => dossier,
        Err(err) => {
            blockers.push(format!(
                "{row_label}: evidence dossier is not valid JSON at {dossier_path}: {err}"
            ));
            return;
        }
    };

    for field in [
        "canonical_harness_id",
        "registry_command",
        "state_authority",
        "status",
        "workflow_phase",
        "native_behavior_contract",
        "operator_visible_success",
        "negative_path_contract",
    ] {
        if row.get(field).and_then(Value::as_str) != dossier.get(field).and_then(Value::as_str) {
            blockers.push(format!(
                "{row_label}: evidence dossier field {field} does not match matrix row"
            ));
        }
    }
    if row.get("e2e_scenario").and_then(Value::as_str)
        != dossier.get("scenario").and_then(Value::as_str)
    {
        blockers.push(format!(
            "{row_label}: evidence dossier scenario does not match matrix e2e_scenario"
        ));
    }

    if dossier.get("proof_kind").and_then(Value::as_str) != Some("selected_workflow_e2e_parity") {
        blockers.push(format!(
            "{row_label}: evidence dossier proof_kind is not selected_workflow_e2e_parity"
        ));
    }
    if dossier.get("strict_doctor_check").and_then(Value::as_str) != Some("strict_parity_matrix") {
        blockers.push(format!(
            "{row_label}: evidence dossier strict_doctor_check is not strict_parity_matrix"
        ));
    }

    let row_entrypoints = string_set(row, "harness_entrypoint");
    let dossier_entrypoints = string_set(&dossier, "harness_entrypoint");
    if row_entrypoints.is_empty() || row_entrypoints != dossier_entrypoints {
        blockers.push(format!(
            "{row_label}: evidence dossier harness_entrypoint does not match matrix row"
        ));
    }
    let row_aliases = string_set(row, "legacy_aliases");
    let dossier_aliases = string_set(&dossier, "legacy_aliases");
    if row_aliases != dossier_aliases {
        blockers.push(format!(
            "{row_label}: evidence dossier legacy_aliases do not match matrix row"
        ));
    }

    let row_dimensions = string_set(row, "parity_dimensions");
    let dossier_dimensions = string_set(&dossier, "parity_dimensions");
    if row_dimensions.is_empty() || !dossier_dimensions.is_superset(&row_dimensions) {
        blockers.push(format!(
            "{row_label}: evidence dossier parity_dimensions do not cover matrix row"
        ));
    }

    let evidence_categories = string_set(&dossier, "evidence_categories");
    for required in ["strict_parity_doctor", "negative_path_contract"] {
        if !evidence_categories.contains(required) {
            blockers.push(format!(
                "{row_label}: evidence dossier is missing evidence category {required}"
            ));
        }
    }
    let commands = string_set(&dossier, "commands");
    if !commands
        .iter()
        .any(|command| command.contains("doctor --json --strict-parity"))
    {
        blockers.push(format!(
            "{row_label}: evidence dossier commands do not include strict parity doctor"
        ));
    }

    let artifacts = dossier.get("artifacts").unwrap_or(&Value::Null);
    if artifacts.get("docs_dossier").and_then(Value::as_str) != Some(dossier_path) {
        blockers.push(format!(
            "{row_label}: evidence dossier artifacts.docs_dossier does not point back to the matrix path"
        ));
    }

    let truth_gates = dossier.get("truth_gates").unwrap_or(&Value::Null);
    for (field, expected) in [
        ("replay_derived", true),
        ("native_only", true),
        ("external_runtime_authority", false),
        ("status_reads_append_events", false),
        ("dossier_reads_append_events", false),
        ("permission_checks_before_side_effects", true),
    ] {
        if truth_gates.get(field).and_then(Value::as_bool) != Some(expected) {
            blockers.push(format!(
                "{row_label}: evidence dossier truth_gates.{field} is not {expected}"
            ));
        }
    }
}

fn validate_selected_execution_proof(row_label: &str, row: &Value, blockers: &mut Vec<String>) {
    validate_selected_execution_proof_with_root(row_label, row, &repo_root_path(), blockers);
}

fn validate_selected_execution_proof_with_root(
    row_label: &str,
    row: &Value,
    repo_root: &Path,
    blockers: &mut Vec<String>,
) {
    let scenario = row
        .get("e2e_scenario")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let proof_root = env::var(STRICT_PARITY_PROOF_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join(STRICT_PARITY_DEFAULT_PROOF_ROOT));
    let proof_path = proof_root
        .join(strict_parity_scenario_slug(scenario))
        .join("proof-bundle.json");
    let proof_dir = proof_path.parent().unwrap_or(proof_root.as_path());
    let body = match fs::read_to_string(&proof_path) {
        Ok(body) => body,
        Err(err) => {
            blockers.push(format!(
                "{row_label}: missing generated execution proof bundle {} ({err}); run the selected workflow simulator lane first",
                proof_path.display()
            ));
            return;
        }
    };
    let proof = match serde_json::from_str::<Value>(&body) {
        Ok(proof) => proof,
        Err(err) => {
            blockers.push(format!(
                "{row_label}: execution proof bundle is not valid JSON at {}: {err}",
                proof_path.display()
            ));
            return;
        }
    };

    for (field, row_field) in [
        ("scenario", "e2e_scenario"),
        ("canonical_harness_id", "canonical_harness_id"),
        ("registry_command", "registry_command"),
        ("implementation_status", "status"),
        ("workflow_phase", "workflow_phase"),
    ] {
        if proof.get(field).and_then(Value::as_str) != row.get(row_field).and_then(Value::as_str) {
            blockers.push(format!(
                "{row_label}: execution proof field {field} does not match matrix {row_field}"
            ));
        }
    }
    if proof.get("proof_kind").and_then(Value::as_str) != Some("selected_workflow_execution_proof")
    {
        blockers.push(format!(
            "{row_label}: execution proof_kind is not selected_workflow_execution_proof"
        ));
    }
    if proof.get("old_runtime_free").and_then(Value::as_bool) != Some(true) {
        blockers.push(format!(
            "{row_label}: execution proof does not assert old_runtime_free=true"
        ));
    }
    if proof.get("implementation_status").and_then(Value::as_str) != Some("native_complete") {
        blockers.push(format!(
            "{row_label}: execution proof implementation_status is not native_complete"
        ));
    }
    validate_registry_native_credit(row_label, row, blockers);
    validate_no_old_runtime_tokens(row_label, &proof, blockers);

    let commands = proof.get("commands").and_then(Value::as_array);
    match commands {
        Some(commands) if !commands.is_empty() => {
            let read_projection_row = row_is_read_projection(row);
            let mut has_projection_read = false;
            let mut has_selected_surface = false;
            let expected_fragment = expected_selected_command_fragment(row);
            for (index, command) in commands.iter().enumerate() {
                if command.get("exit_code").and_then(Value::as_i64) != Some(0) {
                    blockers.push(format!(
                        "{row_label}: execution proof command did not exit 0"
                    ));
                }
                for field in ["stdout_path", "stderr_path", "status_path"] {
                    validate_proof_relative_file(row_label, proof_dir, command, field, blockers);
                }
                validate_command_status_file(row_label, proof_dir, command, blockers);
                let command_text = command
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !command_text.contains("cargo run -p harness --")
                    || !command_text.contains(" workflow ")
                {
                    blockers.push(format!(
                        "{row_label}: execution proof command was not captured from the Harness CLI surface"
                    ));
                }
                if command_text.contains(" workflow dispatch ")
                    || command_text.contains("--deterministic-scenario")
                {
                    blockers.push(format!(
                        "{row_label}: execution proof uses synthetic workflow dispatch rather than the selected workflow surface"
                    ));
                }
                if read_projection_row
                    && (command_text.contains(" workflow evidence record ")
                        || command_text.contains(" workflow signoff "))
                {
                    blockers.push(format!(
                        "{row_label}: read-only projection proof used mutating workflow command"
                    ));
                }
                if index == 0 && !command_text.contains(expected_fragment) {
                    blockers.push(format!(
                        "{row_label}: first execution proof command does not exercise selected surface fragment `{expected_fragment}`"
                    ));
                }
                has_selected_surface |= command_text.contains(expected_fragment);
                has_projection_read |= command_text.contains(" workflow status ")
                    || command_text.contains(" workflow dossier export ");
            }
            if !has_selected_surface {
                blockers.push(format!(
                    "{row_label}: execution proof has no command for the selected workflow surface"
                ));
            }
            if !has_projection_read {
                blockers.push(format!(
                    "{row_label}: execution proof has no replay projection read command"
                ));
            }
        }
        _ => blockers.push(format!(
            "{row_label}: execution proof has no captured commands"
        )),
    }

    let event_log = proof.get("event_log").unwrap_or(&Value::Null);
    validate_proof_relative_file(row_label, proof_dir, event_log, "path", blockers);
    if event_log.get("workflow_id").and_then(Value::as_str)
        != row.get("canonical_harness_id").and_then(Value::as_str)
    {
        blockers.push(format!(
            "{row_label}: execution proof event_log.workflow_id does not match matrix canonical_harness_id"
        ));
    }
    let mut event_types = string_set(event_log, "event_types");
    if let Some(path) = event_log.get("path").and_then(Value::as_str) {
        let resolved = resolve_relative_to(proof_dir, path);
        match event_types_from_jsonl(&resolved) {
            Ok(computed) => {
                if !event_types.is_empty() && event_types != computed {
                    blockers.push(format!(
                        "{row_label}: execution proof event_types do not match events.jsonl"
                    ));
                }
                event_types = computed;
            }
            Err(err) => blockers.push(format!(
                "{row_label}: failed to read execution proof events.jsonl: {err}"
            )),
        }
    }
    let read_projection_row = row_is_read_projection(row);
    let required_events: &[&str] = match row.get("registry_command").and_then(Value::as_str) {
        _ if read_projection_row => &[],
        Some("init-deep") => &["WorkflowEvidenceRecorded"],
        Some("ralph-loop" | "ulw-loop" | "stop-continuation") => {
            &["WorkflowStarted", "WorkflowCompleted"]
        }
        Some("plan-consensus" | "goal-ledger" | "research-mission" | "wiki") => &[
            "WorkflowStarted",
            "WorkflowEvidenceRecorded",
            "WorkflowCompleted",
        ],
        _ => &["WorkflowStarted", "WorkflowCompleted"],
    };
    for expected in required_events {
        if !event_types.contains(*expected) {
            blockers.push(format!(
                "{row_label}: execution proof event log missing event type {expected}"
            ));
        }
    }
    if read_projection_row && !event_types.is_empty() {
        blockers.push(format!(
            "{row_label}: read-only projection proof appended workflow events"
        ));
    }
    if row.get("registry_command").and_then(Value::as_str) == Some("research-mission")
        && !event_types.contains("ToolCallFinished")
    {
        blockers.push(format!(
            "{row_label}: research mission execution proof event log missing event type ToolCallFinished"
        ));
    }
    if event_log
        .get("event_count")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        == 0
        && !read_projection_row
    {
        blockers.push(format!("{row_label}: execution proof event_count is zero"));
    }
    if read_projection_row {
        let digest = event_log.get("digest").and_then(Value::as_str);
        let before = event_log.get("before_digest").and_then(Value::as_str);
        let after = event_log.get("after_digest").and_then(Value::as_str);
        if digest.is_none() || before != digest || after != digest {
            blockers.push(format!(
                "{row_label}: read-only projection proof does not prove stable event-log digest"
            ));
        }
    }

    let projections = proof.get("projections").unwrap_or(&Value::Null);
    for field in [
        "workflow_status_path",
        "workflow_dossier_path",
        "replay_status_path",
    ] {
        validate_proof_relative_file(row_label, proof_dir, projections, field, blockers);
    }

    let artifacts = proof.get("artifacts").and_then(Value::as_array);
    match artifacts {
        Some(artifacts) if !artifacts.is_empty() => {
            for artifact in artifacts {
                validate_proof_relative_file(row_label, proof_dir, artifact, "path", blockers);
                if artifact
                    .get("digest")
                    .and_then(Value::as_str)
                    .filter(|digest| !digest.trim().is_empty())
                    .is_none()
                {
                    blockers.push(format!(
                        "{row_label}: execution proof artifact is missing digest"
                    ));
                }
            }
        }
        _ => blockers.push(format!("{row_label}: execution proof has no artifacts")),
    }

    let negative = proof.get("negative_path").unwrap_or(&Value::Null);
    if negative.get("denied").and_then(Value::as_bool) != Some(true) {
        blockers.push(format!(
            "{row_label}: execution proof negative path did not record denial"
        ));
    }
    if negative
        .get("exit_code")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        == 0
    {
        blockers.push(format!(
            "{row_label}: execution proof negative path did not exit nonzero"
        ));
    }
    let negative_command = negative
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if negative_command.contains(" workflow dispatch ")
        || negative_command.contains("--deterministic-scenario")
    {
        blockers.push(format!(
            "{row_label}: execution proof negative path uses synthetic workflow dispatch"
        ));
    }
    if negative
        .get("no_success_artifacts")
        .and_then(Value::as_bool)
        != Some(true)
    {
        blockers.push(format!(
            "{row_label}: execution proof negative path allows success artifacts"
        ));
    }
    for field in ["stdout_path", "stderr_path", "status_path"] {
        validate_proof_relative_file(row_label, proof_dir, negative, field, blockers);
    }

    let truth_gates = proof.get("truth_gates").unwrap_or(&Value::Null);
    for field in [
        "replay_derived",
        "native_only",
        "old_runtime_free",
        "permission_checks_before_side_effects",
    ] {
        if truth_gates.get(field).and_then(Value::as_bool) != Some(true) {
            blockers.push(format!(
                "{row_label}: execution proof truth_gates.{field} is not true"
            ));
        }
    }
    for field in ["status_reads_append_events", "dossier_reads_append_events"] {
        if truth_gates.get(field).and_then(Value::as_bool) != Some(false) {
            blockers.push(format!(
                "{row_label}: execution proof truth_gates.{field} is not false"
            ));
        }
    }
    if read_projection_row
        && truth_gates
            .get("projection_reads_preserve_event_digest")
            .and_then(Value::as_bool)
            != Some(true)
    {
        blockers.push(format!(
            "{row_label}: read-only projection proof does not assert stable event-log digest"
        ));
    }
}

fn validate_command_status_file(
    row_label: &str,
    proof_dir: &Path,
    command: &Value,
    blockers: &mut Vec<String>,
) {
    let Some(path) = command.get("status_path").and_then(Value::as_str) else {
        return;
    };
    let resolved = resolve_relative_to(proof_dir, path);
    let body = match fs::read_to_string(&resolved) {
        Ok(body) => body,
        Err(_) => return,
    };
    let status = match serde_json::from_str::<Value>(&body) {
        Ok(status) => status,
        Err(err) => {
            blockers.push(format!(
                "{row_label}: execution proof command status file is not valid JSON: {err}"
            ));
            return;
        }
    };
    if status.get("exit_code").and_then(Value::as_i64)
        != command.get("exit_code").and_then(Value::as_i64)
    {
        blockers.push(format!(
            "{row_label}: execution proof command status exit_code does not match proof bundle"
        ));
    }
    if status.get("success").and_then(Value::as_bool) != Some(true) {
        blockers.push(format!(
            "{row_label}: execution proof command status did not record success=true"
        ));
    }
    if status.get("command").and_then(Value::as_str)
        != command.get("command").and_then(Value::as_str)
    {
        blockers.push(format!(
            "{row_label}: execution proof command status command does not match proof bundle"
        ));
    }
}

fn expected_selected_command_fragment(row: &Value) -> &'static str {
    if row_is_read_projection(row) {
        return " workflow status ";
    }
    match row.get("registry_command").and_then(Value::as_str) {
        Some("plan-consensus") => " workflow plan-consensus ",
        Some("goal-ledger") => " workflow goal ",
        Some("research-mission") => " workflow mission ",
        Some("wiki") => " workflow wiki ",
        Some("init-deep") => " workflow snapshot write ",
        Some("stop-continuation") => " workflow cancel ",
        Some("ralph-loop" | "ulw-loop") => " workflow run ",
        _ => " workflow run ",
    }
}

fn row_is_read_projection(row: &Value) -> bool {
    let Some(registry_command) = row.get("registry_command").and_then(Value::as_str) else {
        return false;
    };
    CommandRegistry::builtins()
        .get(registry_command)
        .is_some_and(|command| {
            matches!(
                command.effect,
                harness_core::command_registry::CommandEffect::ReadProjection
            )
        })
}

fn event_types_from_jsonl(path: &Path) -> Result<BTreeSet<String>, String> {
    let body = fs::read_to_string(path).map_err(|err| format!("{} ({err})", path.display()))?;
    let mut event_types = BTreeSet::new();
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        let value = serde_json::from_str::<Value>(line)
            .map_err(|err| format!("{} contains invalid JSONL event ({err})", path.display()))?;
        if let Some(raw) = value
            .get("payload")
            .and_then(Value::as_object)
            .and_then(|payload| payload.get("event_type").and_then(Value::as_str))
        {
            event_types.insert(event_type_label(raw));
        }
    }
    Ok(event_types)
}

fn event_type_label(raw: &str) -> String {
    raw.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn validate_registry_native_credit(row_label: &str, row: &Value, blockers: &mut Vec<String>) {
    let Some(registry_command) = row.get("registry_command").and_then(Value::as_str) else {
        return;
    };
    let registry = CommandRegistry::builtins();
    let Some(command) = registry.get(registry_command) else {
        blockers.push(format!(
            "{row_label}: registry_command {registry_command} is not registered"
        ));
        return;
    };
    let native_credit = match &command.action {
        CommandAction::WorkflowSkill { .. }
        | CommandAction::WorkflowIntent { .. }
        | CommandAction::StopContinuation => {
            command.enabled_by_default
                && command.availability
                    == harness_core::command_registry::WorkflowCommandAvailability::Present
                && matches!(
                    command.effect,
                    harness_core::command_registry::CommandEffect::MutateCoordinatorState
                        | harness_core::command_registry::CommandEffect::ReadProjection
                        | harness_core::command_registry::CommandEffect::ControlContinuation
                )
        }
        _ => false,
    };
    if !native_credit {
        blockers.push(format!(
            "{row_label}: registry command {registry_command} is not classified as native workflow behavior"
        ));
    }
}

fn validate_no_old_runtime_tokens(row_label: &str, proof: &Value, blockers: &mut Vec<String>) {
    let text = proof.to_string().to_lowercase();
    for forbidden in [
        "legacy runtime command",
        "native team tools api",
        "tmux send-keys",
        "tmux pane",
        "${codex_home",
        "~/.codex",
    ] {
        if text.contains(forbidden) {
            blockers.push(format!(
                "{row_label}: execution proof contains forbidden old-runtime token {forbidden:?}"
            ));
        }
    }
}

fn validate_proof_relative_file(
    row_label: &str,
    proof_dir: &Path,
    value: &Value,
    field: &str,
    blockers: &mut Vec<String>,
) {
    let Some(path) = value.get(field).and_then(Value::as_str) else {
        blockers.push(format!(
            "{row_label}: execution proof is missing path field {field}"
        ));
        return;
    };
    if path.contains("${") {
        blockers.push(format!(
            "{row_label}: execution proof path field {field} contains unresolved template"
        ));
        return;
    }
    let resolved = resolve_relative_to(proof_dir, path);
    if !resolved.is_file() {
        blockers.push(format!(
            "{row_label}: execution proof path field {field} does not exist: {}",
            resolved.display()
        ));
    }
}

fn resolve_relative_to(base: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn strict_parity_scenario_slug(case_id: &str) -> String {
    case_id
        .rsplit("::")
        .next()
        .unwrap_or(case_id)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn validate_active_runtime_assets_no_old_authority(blockers: &mut Vec<String>) {
    let root = repo_root_path().join(".agent-harness");
    if !root.exists() {
        return;
    }
    let forbidden = [
        "legacy runtime state dir",
        "legacy ask command",
        "legacy performance-goal command",
        "legacy goal command",
        "legacy wiki command",
        "legacy explore command",
        "legacy shell command",
        "legacy setup command",
        "legacy doctor command",
        "legacy hud command",
        "legacy team command",
        "legacy question command",
        "legacy state command",
        "native team tools api",
        "tmux send-keys",
        "tmux pane",
        "LEGACY_TEAM_ENV_",
        "LEGACY_QUESTION_ENV_",
        "CODEX_HOME",
        "~/.codex",
        "Codex goal mode",
        "goal mode is the authority",
    ];
    let mut findings = Vec::new();
    collect_old_runtime_asset_findings(&root, &forbidden, &mut findings);
    findings.sort();
    blockers.extend(findings.into_iter().map(|finding| {
        format!("active runtime asset contains old workflow authority token: {finding}")
    }));
}

fn collect_old_runtime_asset_findings(path: &Path, forbidden: &[&str], findings: &mut Vec<String>) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.is_dir() {
        if path.file_name().and_then(|name| name.to_str()) == Some("sessions") {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect_old_runtime_asset_findings(&entry.path(), forbidden, findings);
        }
        return;
    }
    if !metadata.is_file() {
        return;
    }
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return;
    };
    if !matches!(extension, "md" | "toml" | "json" | "txt") {
        return;
    }
    let Ok(body) = fs::read_to_string(path) else {
        return;
    };
    if skill_body_is_hard_deprecated(&body) {
        return;
    }
    for (index, line) in body.lines().enumerate() {
        let lower = line.to_lowercase();
        let lineage_only = lower.contains("deprecated")
            || lower.contains("migration lineage")
            || lower.contains("compatibility alias");
        if lineage_only {
            continue;
        }
        for token in forbidden {
            if line.contains(token) {
                findings.push(format!(
                    "{}:{}:{token}",
                    path.display(),
                    index.saturating_add(1)
                ));
            }
        }
    }
}

fn repo_root_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn resolve_repo_relative_path(repo_root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    repo_root.join(path)
}

fn string_set(value: &Value, field: &str) -> BTreeSet<String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default()
}

fn check_workflow_context_snapshot_contract() -> DoctorCheck {
    let options = ContextSnapshotOptions::default();
    if options.max_text_chars == 0 || options.max_list_items == 0 {
        return fail(
            "workflow_context_snapshot",
            "context snapshot caps must be non-zero",
        );
    }

    pass_with_details(
        "workflow_context_snapshot",
        format!(
            "context snapshot schema v{CONTEXT_SNAPSHOT_SCHEMA_VERSION} writes redacted artifacts under artifacts/{CONTEXT_SNAPSHOT_ARTIFACT_DIR}/ and is replay-projected from workflow evidence"
        ),
        Some(serde_json::json!({
            "schema_version": CONTEXT_SNAPSHOT_SCHEMA_VERSION,
            "artifact_dir": CONTEXT_SNAPSHOT_ARTIFACT_DIR,
            "max_text_chars": options.max_text_chars,
            "max_list_items": options.max_list_items,
        })),
    )
}

fn check_provider_credentials(config: &HarnessConfig) -> DoctorCheck {
    if config.providers.is_empty() {
        return fail("provider_credentials", "no providers are configured");
    }

    let mut inline_credentials = 0;
    let mut env_credentials = 0;
    let mut missing = Vec::new();

    for (id, provider) in &config.providers {
        let ProviderConfig::OpenAiCompatible(provider) = provider;
        let inline_available = non_empty(&provider.api_key);
        let env_available = provider.api_key_env.iter().any(|name| env_var_is_set(name));

        if inline_available {
            inline_credentials += 1;
        } else if env_available {
            env_credentials += 1;
        } else if provider.api_key_env.is_empty() {
            missing.push(format!("{id} (set apiKey or apiKeyEnv)"));
        } else {
            missing.push(format!(
                "{id} (set one of: {})",
                provider.api_key_env.join(", ")
            ));
        }
    }

    if !missing.is_empty() {
        return warn(
            "provider_credentials",
            format!(
                "{} provider(s) lack an available API key: {}",
                missing.len(),
                missing.join("; ")
            ),
        );
    }

    pass(
        "provider_credentials",
        format!(
            "{} provider(s) have credentials available; {inline_credentials} inline, {env_credentials} via environment",
            config.providers.len()
        ),
    )
}

fn check_provider_catalog(config: &HarnessConfig) -> DoctorCheck {
    if config.providers.is_empty() {
        return fail("provider_catalog", "no providers are configured");
    }

    let providers_without_models = config
        .providers
        .iter()
        .filter_map(|(id, provider)| {
            let ProviderConfig::OpenAiCompatible(provider) = provider;
            provider.models.is_empty().then_some(id.as_str())
        })
        .collect::<Vec<_>>();

    if !providers_without_models.is_empty() {
        return warn(
            "provider_catalog",
            format!(
                "{} provider(s) configured; providers without model metadata: {}",
                config.providers.len(),
                providers_without_models.join(", ")
            ),
        );
    }

    let model_count = config
        .providers
        .values()
        .map(|provider| {
            let ProviderConfig::OpenAiCompatible(provider) = provider;
            provider.models.len()
        })
        .sum::<usize>();
    pass(
        "provider_catalog",
        format!(
            "{} provider(s) and {model_count} model(s) configured",
            config.providers.len()
        ),
    )
}

fn check_model_references(config: &HarnessConfig) -> DoctorCheck {
    let mut failures = Vec::new();
    let mut agent_selections = 0;
    let mut model_profile_selections = 0;
    let mut fallback_targets = 0;

    for profile_name in config.model_profiles.keys() {
        match resolve_model_selection(config, profile_name, None) {
            Ok(selection) => {
                model_profile_selections += 1;
                fallback_targets += selection.fallback.len();
            }
            Err(err) => failures.push(format!("model_profile.{profile_name}: {err}")),
        }
    }

    for (agent_name, agent) in &config.agents {
        match resolve_model_selection(config, &agent.model_ref, agent.variant.as_deref()) {
            Ok(selection) => {
                agent_selections += 1;
                fallback_targets += selection.fallback.len();
            }
            Err(err) => failures.push(format!("agent.{agent_name}: {err}")),
        }
    }

    if !failures.is_empty() {
        return fail(
            "model_references",
            format!(
                "{} model reference(s) failed to resolve: {}",
                failures.len(),
                failures.join("; ")
            ),
        );
    }

    let default_agent = config.default_agent.as_deref().unwrap_or("none");
    pass(
        "model_references",
        format!(
            "{agent_selections} agent model selection(s) and {model_profile_selections} model profile(s) resolve; {fallback_targets} fallback target(s); default_agent={default_agent}"
        ),
    )
}

fn check_model_capabilities(config: &HarnessConfig) -> DoctorCheck {
    let catalog = configured_model_catalog(config);
    let model_count = catalog.len();
    let mut warnings = Vec::new();
    let mut fallback_edges = 0_usize;

    for (agent_name, agent) in &config.agents {
        let Ok(selection) =
            resolve_model_selection(config, &agent.model_ref, agent.variant.as_deref())
        else {
            continue;
        };
        fallback_edges += selection.fallback.len();
        let requires_tools = !agent.tools.is_empty();
        let targets = std::iter::once(&selection.primary).chain(selection.fallback.iter());
        for target in targets {
            let Some(provider) = config.providers.get(&target.provider) else {
                continue;
            };
            let ProviderConfig::OpenAiCompatible(provider) = provider;
            let Some(model) = provider.models.get(&target.model) else {
                continue;
            };
            if requires_tools && model.metadata.supports_tool_calls == Some(false) {
                warnings.push(format!(
                    "agent `{agent_name}` uses tools but target `{}` declares supports_tool_calls=false",
                    target.model_ref
                ));
            }
            if requires_tools && model.metadata.supports_tool_calls.is_none() {
                warnings.push(format!(
                    "agent `{agent_name}` uses tools but target `{}` has unknown tool-call capability",
                    target.model_ref
                ));
            }
            if model.modalities.input.is_empty() || model.modalities.output.is_empty() {
                warnings.push(format!(
                    "target `{}` has incomplete modality metadata (input={}, output={})",
                    target.model_ref,
                    model.modalities.input.len(),
                    model.modalities.output.len()
                ));
            }
        }
    }

    let details = serde_json::json!({
        "catalog_entries": model_count,
        "fallback_edges": fallback_edges,
        "warnings": warnings,
    });

    if warnings.is_empty() {
        pass_with_details(
            "model_capabilities",
            format!(
                "{model_count} model catalog entrie(s) cached; {fallback_edges} fallback edge(s) checked for tool/modality capability"
            ),
            Some(details),
        )
    } else {
        DoctorCheck {
            id: "model_capabilities".to_string(),
            name: "model_capabilities".to_string(),
            status: CheckStatus::Warn,
            message: format!(
                "{} model capability warning(s); fallback edge(s) checked={fallback_edges}",
                warnings.len()
            ),
            details: Some(details),
        }
    }
}

fn check_shipped_profiles(config: &HarnessConfig) -> DoctorCheck {
    let mut missing = Vec::new();
    for profile in PRIMARY_WORKFLOW_PROFILES
        .iter()
        .copied()
        .chain(LEGACY_PRIMARY_PROFILE_ALIASES.iter().copied())
        .chain(BUILTIN_SUBAGENTS.iter().copied())
    {
        if !config.agents.contains_key(profile) {
            missing.push(profile);
        }
    }
    if !missing.is_empty() {
        return warn(
            "workflow_profiles",
            format!(
                "missing recommended shipped profile(s): {}; enable them under `agent` for full orchestration parity",
                missing.join(", ")
            ),
        );
    }

    let invalid_primary = PRIMARY_WORKFLOW_PROFILES
        .iter()
        .copied()
        .filter_map(|profile| {
            let agent = config.agents.get(profile)?;
            (agent.hidden || agent.mode == AgentMode::Subagent).then_some(profile)
        })
        .collect::<Vec<_>>();
    if !invalid_primary.is_empty() {
        return fail(
            "workflow_profiles",
            format!(
                "primary workflow profile(s) are hidden or subagent-only: {}",
                invalid_primary.join(", ")
            ),
        );
    }

    let invalid_legacy = LEGACY_PRIMARY_PROFILE_ALIASES
        .iter()
        .copied()
        .filter_map(|profile| {
            let agent = config.agents.get(profile)?;
            (!agent.hidden || agent.mode == AgentMode::Subagent).then_some(profile)
        })
        .collect::<Vec<_>>();
    if !invalid_legacy.is_empty() {
        return fail(
            "workflow_profiles",
            format!(
                "legacy workflow profile alias(es) must be hidden compatibility lanes: {}",
                invalid_legacy.join(", ")
            ),
        );
    }

    let invalid_subagents = BUILTIN_SUBAGENTS
        .iter()
        .copied()
        .filter_map(|profile| {
            let agent = config.agents.get(profile)?;
            (agent.hidden || agent.mode == AgentMode::Primary).then_some(profile)
        })
        .collect::<Vec<_>>();
    if !invalid_subagents.is_empty() {
        return fail(
            "workflow_profiles",
            format!(
                "subagent workflow profile(s) are hidden or primary-only: {}",
                invalid_subagents.join(", ")
            ),
        );
    }

    pass(
        "workflow_profiles",
        "operator is the visible workflow profile; build, plan, discipline are hidden compatibility lanes; subagent specialists are available",
    )
}

fn check_category_routes(config: &HarnessConfig) -> DoctorCheck {
    let missing = CATEGORY_ROUTES
        .iter()
        .copied()
        .filter(|profile| !config.agents.contains_key(*profile))
        .collect::<Vec<_>>();

    let invalid_routes = CATEGORY_ROUTES
        .iter()
        .copied()
        .filter_map(|profile| {
            let agent = config.agents.get(profile)?;
            (agent.hidden || agent.mode == AgentMode::Primary).then_some(profile)
        })
        .collect::<Vec<_>>();

    let recursive_routes = CATEGORY_ROUTES
        .iter()
        .copied()
        .filter_map(|profile| {
            let agent = config.agents.get(profile)?;
            let task_permission = agent
                .permissions
                .as_ref()
                .and_then(|permissions| permissions.task.as_ref());
            (!matches!(task_permission, Some(PermissionMode::Deny))).then_some(profile)
        })
        .collect::<Vec<_>>();

    let mut details = Vec::new();
    if !invalid_routes.is_empty() {
        details.push(format!(
            "task category route profile(s) are hidden or primary-only: {}",
            invalid_routes.join(", ")
        ));
    }
    if !missing.is_empty() {
        details.push(format!(
            "missing recommended task category route profile(s): {}; task(category=...) falls back to general when no matching route exists",
            missing.join(", ")
        ));
    }
    if !recursive_routes.is_empty() {
        details.push(format!(
            "task category route profile(s) can redelegate or inherit task permission: {}; shipped category routes deny recursive delegation by default",
            recursive_routes.join(", ")
        ));
    }

    if !invalid_routes.is_empty() {
        return fail("category_routes", details.join("; "));
    }
    if !details.is_empty() {
        return warn("category_routes", details.join("; "));
    }

    pass(
        "category_routes",
        "visual-engineering, artistry, ultrabrain, deep, quick, unspecified-low, unspecified-high, and writing category routes are available",
    )
}

fn check_agent_catalog(config: &HarnessConfig) -> DoctorCheck {
    let catalog = resolve_agent_catalog(config);
    let model_errors = catalog
        .entries
        .iter()
        .filter(|entry| entry.model_error.is_some())
        .collect::<Vec<_>>();
    if !model_errors.is_empty() {
        return fail(
            "agent_catalog",
            format!(
                "{} catalog profile(s) have unresolved models: {}",
                model_errors.len(),
                model_errors
                    .iter()
                    .map(|entry| entry.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    let specialists = catalog
        .entries
        .iter()
        .filter(|entry| entry.role == "specialist")
        .count();
    let categories = catalog
        .entries
        .iter()
        .filter(|entry| entry.role == "category")
        .count();
    let fallback_targets = catalog
        .entries
        .iter()
        .map(|entry| entry.fallback_model_refs.len())
        .sum::<usize>();

    pass_with_details(
        "agent_catalog",
        format!(
            "{} profile(s) resolved through AgentCatalog; {specialists} specialist(s), {categories} category route(s), {fallback_targets} fallback target(s)",
            catalog.entries.len()
        ),
        serde_json::to_value(&catalog).ok(),
    )
}

fn check_profile_tools(config: &HarnessConfig) -> DoctorCheck {
    let native_tools = coordinator_registry_with_mcp_and_editing(
        config.permissions.shell_allowlist.clone(),
        Default::default(),
        EditingToolSurfaceConfig {
            hashline_edit: config.hashline_edit,
        },
    )
    .tool_ids()
    .into_iter()
    .collect::<BTreeSet<_>>();

    let unknown_tools = config
        .agents
        .iter()
        .flat_map(|(profile, agent)| {
            let native_tools = &native_tools;
            agent
                .tools
                .iter()
                .filter(move |tool| !native_tools.contains(*tool) && !tool.starts_with("mcp."))
                .map(move |tool| format!("{profile}.{tool}"))
        })
        .collect::<Vec<_>>();
    if !unknown_tools.is_empty() {
        return fail(
            "tool_surface",
            format!(
                "configured profile tool ids are not registered: {}",
                unknown_tools.join(", ")
            ),
        );
    }

    let missing_core_tools = [
        ("build", &BUILD_TOOLS[..]),
        ("plan", &PLAN_TOOLS[..]),
        ("discipline", &DISCIPLINE_TOOLS[..]),
    ]
    .into_iter()
    .flat_map(|(profile, expected)| {
        expected.iter().filter_map(move |tool| {
            let agent = config.agents.get(profile)?;
            (!agent.tools.iter().any(|configured| configured == tool))
                .then(|| format!("{profile}.{tool}"))
        })
    })
    .collect::<Vec<_>>();
    if !missing_core_tools.is_empty() {
        return warn(
            "tool_surface",
            format!(
                "recommended workflow tool(s) are missing: {}",
                missing_core_tools.join(", ")
            ),
        );
    }

    pass("tool_surface", "configured profile tools are registered")
}

fn check_first_slice_compatibility_tool_surface(config: &HarnessConfig) -> DoctorCheck {
    let native_tools = coordinator_registry_with_mcp_and_editing(
        config.permissions.shell_allowlist.clone(),
        Default::default(),
        EditingToolSurfaceConfig {
            hashline_edit: config.hashline_edit,
        },
    )
    .tool_ids()
    .into_iter()
    .collect::<BTreeSet<_>>();

    let missing = FIRST_SLICE_COMPATIBILITY_TOOLS
        .into_iter()
        .filter(|tool| !native_tools.contains(*tool))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return warn(
            "compatibility_tool_surface",
            format!(
                "missing first-slice compatibility tool surface id(s): {}; see docs/parity-ledger.json",
                missing.join(", ")
            ),
        );
    }

    pass(
        "compatibility_tool_surface",
        "first-slice compatibility tool ids are registered; unsupported tools return explicit diagnostics",
    )
}

fn check_workflow_simulator_contract() -> DoctorCheck {
    let policy = WorkflowSignoffPolicy::simulator_default();
    let required = policy.required_evidence_categories();
    if !required
        .iter()
        .any(|category| category == SIMULATED_TOOL_EVIDENCE_CATEGORY)
    {
        return fail(
            "workflow_simulator",
            "simulator signoff policy must require mapped no-op tool evidence",
        );
    }

    pass_with_details(
        "workflow_simulator",
        "deterministic workflow simulator contract is available without live provider, terminal, browser, or network dependencies",
        Some(serde_json::json!({
            "required_evidence_categories": required,
            "side_effect_adapter": "bash true via permission-gated no-op",
            "dossier": "replay-derived run dossier",
        })),
    )
}

fn check_workflow_stale_work_loop(config: &HarnessConfig) -> DoctorCheck {
    let Some(run_dir) = latest_event_run_dir(&config.paths.session_dir) else {
        return pass_with_details(
            "workflow_stale_work_loop",
            "no session event logs found; no active workflow work loops to inspect",
            Some(serde_json::json!({
                "session_dir": config.paths.session_dir,
            })),
        );
    };
    let events = match load_events_from_run_dir(&run_dir) {
        Ok(events) => events,
        Err(err) => {
            return warn(
                "workflow_stale_work_loop",
                format!(
                    "could not inspect latest workflow run {} for stale work loops: {err}",
                    run_dir.display()
                ),
            );
        }
    };
    let projection = project_workflows(events.iter().map(|event| &event.payload));
    let active = projection
        .continuations
        .values()
        .filter(|continuation| {
            continuation.status == "active" || continuation.status == "reminder_queued"
        })
        .map(|continuation| {
            serde_json::json!({
                "continuation_id": continuation.continuation_id,
                "workflow_id": continuation.workflow_id,
                "status": continuation.status,
                "iteration": continuation.iteration,
                "last_schedule_reason": continuation.last_schedule_reason,
            })
        })
        .collect::<Vec<_>>();
    if active.is_empty() {
        return pass_with_details(
            "workflow_stale_work_loop",
            "latest workflow run has no active workflow-owned continuation loops",
            Some(serde_json::json!({
                "run_dir": run_dir,
            })),
        );
    }
    warn_with_details(
        "workflow_stale_work_loop",
        "latest workflow run has active workflow-owned continuation loops; inspect status/dossier before claiming completion",
        Some(serde_json::json!({
            "run_dir": run_dir,
            "active_continuations": active,
        })),
    )
}

fn latest_event_run_dir(session_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(session_dir).ok()?;
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.join(EVENTS_FILE_NAME).is_file() {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        candidates.push((modified, path));
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    candidates.pop().map(|(_, path)| path)
}

fn check_permissions(config: &HarnessConfig) -> DoctorCheck {
    let task_permission = config.permissions.defaults.task.as_ref();
    if matches!(task_permission, Some(PermissionMode::Deny)) {
        return warn(
            "permissions",
            "default task permission is deny; delegation profiles require per-agent task rules to run",
        );
    }

    let shell_roots = config.permissions.shell_allowlist.cwd_roots.len();
    let executables = config.permissions.shell_allowlist.executables.len();
    pass(
        "permissions",
        format!(
            "default permissions loaded; shell allowlist has {executables} executable(s) and {shell_roots} cwd root(s)"
        ),
    )
}

fn check_session_dir(session_dir: &Path) -> DoctorCheck {
    if session_dir.exists() {
        return match session_dir.metadata() {
            Ok(metadata) if metadata.is_dir() && !metadata.permissions().readonly() => pass(
                "session_dir",
                format!("session directory is present: {}", session_dir.display()),
            ),
            Ok(metadata) if metadata.is_dir() => warn(
                "session_dir",
                format!("session directory is read-only: {}", session_dir.display()),
            ),
            Ok(_) => fail(
                "session_dir",
                format!(
                    "session path exists but is not a directory: {}",
                    session_dir.display()
                ),
            ),
            Err(err) => fail(
                "session_dir",
                format!(
                    "failed to inspect session directory {}: {err}",
                    session_dir.display()
                ),
            ),
        };
    }

    let parent = session_dir.parent().unwrap_or_else(|| Path::new("."));
    match parent.metadata() {
        Ok(metadata) if metadata.is_dir() && !metadata.permissions().readonly() => pass(
            "session_dir",
            format!(
                "session directory can be created under existing parent: {}",
                parent.display()
            ),
        ),
        Ok(metadata) if metadata.is_dir() => warn(
            "session_dir",
            format!(
                "session directory parent is read-only: {}",
                parent.display()
            ),
        ),
        Ok(_) => fail(
            "session_dir",
            format!(
                "session directory parent is not a directory: {}",
                parent.display()
            ),
        ),
        Err(err) => fail(
            "session_dir",
            format!(
                "session directory parent {} is not accessible: {err}",
                parent.display()
            ),
        ),
    }
}

fn check_mcp(config: &HarnessConfig) -> DoctorCheck {
    let total = config.integrations.mcp.servers.len();
    let enabled = config
        .integrations
        .mcp
        .servers
        .values()
        .filter(|server| server.enabled())
        .count();
    if total == 0 {
        return pass("mcp", "no MCP servers configured");
    }

    let stdio_enabled = config
        .integrations
        .mcp
        .servers
        .values()
        .filter(|server| server.enabled())
        .filter(|server| matches!(server, McpServerConfig::Stdio { .. }))
        .count();

    pass(
        "mcp",
        format!(
            "{enabled}/{total} MCP server(s) enabled; {stdio_enabled} enabled stdio server(s) will launch only at runtime"
        ),
    )
}

fn check_compatibility_imports(config: &HarnessConfig) -> DoctorCheck {
    let total = config.compatibility.imports.len();
    let imported = config
        .compatibility
        .imports
        .iter()
        .filter(|item| item.status == CompatibilityImportState::Imported)
        .count();
    let disabled = config
        .compatibility
        .imports
        .iter()
        .filter(|item| item.status == CompatibilityImportState::Disabled)
        .count();
    let errors = config
        .compatibility
        .imports
        .iter()
        .filter(|item| item.status == CompatibilityImportState::Error)
        .collect::<Vec<_>>();
    let manifests = config.compatibility.extension_manifests.len();
    let commands = config.compatibility.command_templates.len();
    let details = serde_json::json!({
        "required": config.compatibility.required,
        "total": total,
        "imported": imported,
        "disabled": disabled,
        "errors": errors.len(),
        "command_templates": commands,
        "extension_manifests": manifests,
        "items": config.compatibility.imports,
    });

    if !errors.is_empty() {
        return warn_with_details(
            "compatibility_imports",
            format!(
                "{imported}/{total} compatibility item(s) imported; {disabled} disabled; {} import error(s) surfaced without executing external code",
                errors.len()
            ),
            Some(details),
        );
    }

    pass_with_details(
        "compatibility_imports",
        format!(
            "{imported}/{total} compatibility item(s) imported; {disabled} disabled; {commands} command template(s), {manifests} extension manifest(s); executable plugin loading remains disabled"
        ),
        Some(details),
    )
}

fn check_team_mode() -> DoctorCheck {
    let declared_report = inspect_team_specs(Path::new(".agent-harness/teams"));
    let git_available = command_available("git");
    let tmux_available = command_available("tmux");

    let message = format!(
        "{} declared team spec(s), {} invalid; git {}; tmux {}; active team runs, metadata diagnostics, and shutdown proof are replay-derived through team_list/team_status",
        declared_report.total,
        declared_report.invalid,
        availability_label(git_available),
        availability_label(tmux_available)
    );

    if declared_report.invalid > 0 {
        return warn(
            "team_mode",
            format!(
                "{message}; invalid declared specs: {}",
                declared_report.errors.join("; ")
            ),
        );
    }

    if git_available && tmux_available {
        pass("team_mode", message)
    } else {
        warn(
            "team_mode",
            format!(
                "{message}; missing optional dependency means worktree or tmux visualization parity will degrade gracefully"
            ),
        )
    }
}

#[derive(Debug, Default)]
struct TeamSpecDoctorReport {
    total: usize,
    invalid: usize,
    errors: Vec<String>,
}

fn inspect_team_specs(root: &Path) -> TeamSpecDoctorReport {
    let Ok(entries) = std::fs::read_dir(root) else {
        return TeamSpecDoctorReport::default();
    };
    let mut report = TeamSpecDoctorReport::default();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        report.total += 1;
        let error = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) => validate_team_spec_value(&path, &value).err(),
                Err(source) => Some(format!("invalid JSON: {source}")),
            },
            Err(source) => Some(format!("cannot read: {source}")),
        };
        if let Some(error) = error {
            report.invalid += 1;
            report.errors.push(format!("{}: {error}", path.display()));
        }
    }
    report
}

fn validate_team_spec_value(path: &Path, value: &Value) -> Result<(), String> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "missing non-empty name".to_string())?;
    if path
        .file_stem()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| file_name != name)
    {
        return Err("file name must match declared team name".to_string());
    }
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("version must be 1".to_string());
    }
    if value
        .get("members")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err("members must be a non-empty array".to_string());
    }
    Ok(())
}

fn command_available(command: &str) -> bool {
    let version_arg = if command == "tmux" { "-V" } else { "--version" };
    Command::new(command)
        .arg(version_arg)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn availability_label(available: bool) -> &'static str {
    if available {
        "available"
    } else {
        "missing"
    }
}

fn check_terminal_browser_media() -> DoctorCheck {
    let tmux_available = command_available("tmux");
    let npx_available = command_available("npx");
    let agent_browser_available = command_available("agent-browser");
    let skills = ["playwright", "agent-browser", "dev-browser"];
    let missing_skills = skills
        .into_iter()
        .filter(|skill| {
            !Path::new(".agent-harness/skills")
                .join(skill)
                .join("SKILL.md")
                .exists()
        })
        .collect::<Vec<_>>();

    let message = format!(
        "terminal tmux {}; browser deps: npx {}, agent-browser {}; browser skills missing: {}",
        availability_label(tmux_available),
        availability_label(npx_available),
        availability_label(agent_browser_available),
        if missing_skills.is_empty() {
            "none".to_string()
        } else {
            missing_skills.join(", ")
        }
    );

    if !tmux_available || !missing_skills.is_empty() {
        return warn(
            "terminal_browser_media",
            format!(
                "{message}; terminal/browser parity degrades gracefully and tools return actionable dependency diagnostics"
            ),
        );
    }
    if !npx_available || !agent_browser_available {
        return warn(
            "terminal_browser_media",
            format!(
                "{message}; optional browser automation dependencies are missing, use skill diagnostics before live/browser work"
            ),
        );
    }
    pass(
        "terminal_browser_media",
        format!("{message}; look_at and terminal tools are registered"),
    )
}

fn check_workflow_runtime_config(config: &HarnessConfig) -> DoctorCheck {
    let workflow = &config.runtime.workflow;
    if workflow.interview.max_rounds == 0 {
        return fail(
            "workflow_runtime_config",
            "runtime.workflow.interview.max_rounds must be greater than zero",
        );
    }
    if !(0.0..=1.0).contains(&workflow.interview.threshold) {
        return fail(
            "workflow_runtime_config",
            "runtime.workflow.interview.threshold must be between 0.0 and 1.0",
        );
    }
    if workflow.team.max_parallel_members > workflow.team.max_members {
        return fail(
            "workflow_runtime_config",
            "runtime.workflow.team.max_parallel_members must not exceed max_members",
        );
    }

    let status = if workflow.enabled {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn
    };
    DoctorCheck {
        id: "workflow_runtime_config".to_string(),
        name: "workflow_runtime_config".to_string(),
        status,
        message: if workflow.enabled {
            "runtime.workflow enabled with staged command/config defaults".to_string()
        } else {
            "runtime.workflow is disabled; CLI projection reads still work for existing logs"
                .to_string()
        },
        details: Some(serde_json::json!({
            "enabled": workflow.enabled,
            "aliases": workflow.aliases,
            "default_lane": workflow.run.default_lane,
            "require_dossier": workflow.run.require_dossier,
            "require_evidence": workflow.run.require_evidence,
            "team": {
                "max_members": workflow.team.max_members,
                "max_parallel_members": workflow.team.max_parallel_members,
                "tmux_visualization": workflow.team.tmux_visualization,
                "worktrees": workflow.team.worktrees,
            },
            "wiki": {
                "enabled": workflow.wiki.enabled,
                "root": workflow.wiki.root,
                "auto_capture": workflow.wiki.auto_capture,
            },
            "closeout": {
                "default_policy": workflow.closeout.default_policy,
                "require_replay_equivalence": workflow.closeout.require_replay_equivalence,
                "allow_audit_only": workflow.closeout.allow_audit_only,
                "policy_count": workflow.closeout.policies.len(),
            }
        })),
    }
}

fn check_workflow_closeout_policy(config: &HarnessConfig) -> DoctorCheck {
    let workflow = &config.runtime.workflow;
    let builtin_ids = builtin_policy_ids();
    let unknown = workflow
        .closeout
        .policies
        .keys()
        .filter(|policy_id| !is_builtin_policy_id(policy_id))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return fail(
            "workflow_closeout_policy",
            format!(
                "unknown workflow closeout policy id(s): {}",
                unknown.join(", ")
            ),
        );
    }
    let default_policy_id = workflow.closeout.default_policy.as_str();
    let policy = match workflow.effective_closeout_policy(default_policy_id) {
        Ok(policy) => policy,
        Err(err) => {
            return fail(
                "workflow_closeout_policy",
                format!("runtime.workflow.closeout.default_policy is invalid: {err:?}"),
            );
        }
    };
    pass_with_details(
        "workflow_closeout_policy",
        format!(
            "workflow closeout policy `{}` v{} is enabled; unknown ids fail closed",
            policy.policy_id, policy.version
        ),
        Some(serde_json::json!({
            "default_policy": workflow.closeout.default_policy,
            "known_builtin_policy_ids": builtin_ids,
            "require_evidence": policy.require_evidence,
            "require_dossier": policy.require_dossier,
            "require_export_artifact": policy.require_export_artifact,
            "require_replay_equivalence": workflow.closeout.require_replay_equivalence,
            "allow_audit_only": workflow.closeout.allow_audit_only,
        })),
    )
}

fn check_workflow_catalog_health(config: &HarnessConfig) -> DoctorCheck {
    let workspace_root = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut report = match harness_tools::workflow_catalog_health_report(&workspace_root) {
        Ok(report) => report,
        Err(err) => {
            return warn(
                "workflow_catalog_health",
                format!("could not inspect workflow skill/role catalog health: {err}"),
            );
        }
    };

    report.disabled.extend(
        config
            .compatibility
            .disabled_agents
            .iter()
            .map(|name| format!("role:{name}")),
    );
    report.disabled.sort();
    report.disabled.dedup();

    let message = format!(
        "workflow catalog health: {} visible, {} missing, {} disabled, {} shadowed; prompt contents are redacted",
        report.visible.len(),
        report.missing.len(),
        report.disabled.len(),
        report.shadowed.len()
    );
    let details = serde_json::to_value(report).ok();
    if details
        .as_ref()
        .and_then(|value| value.get("missing"))
        .and_then(Value::as_array)
        .is_some_and(|missing| !missing.is_empty())
    {
        warn_with_details("workflow_catalog_health", message, details)
    } else {
        pass_with_details("workflow_catalog_health", message, details)
    }
}

fn check_workflow_closeout_readiness(config: &HarnessConfig) -> DoctorCheck {
    let Some(run_dir) = latest_event_run_dir(&config.paths.session_dir) else {
        return pass_with_details(
            "workflow_closeout_readiness",
            "no session event logs found; no workflow closeout blockers to inspect",
            Some(serde_json::json!({
                "session_dir": config.paths.session_dir,
            })),
        );
    };
    let events = match load_events_from_run_dir(&run_dir) {
        Ok(events) => events,
        Err(err) => {
            return warn(
                "workflow_closeout_readiness",
                format!(
                    "could not inspect latest workflow run {} for closeout readiness: {err}",
                    run_dir.display()
                ),
            );
        }
    };
    let projection = project_workflows(events.iter().map(|event| &event.payload));
    let persistent_tasks = harness_core::persistent_task::project_persistent_tasks(&events);
    let signoff_policy = WorkflowSignoffPolicy::simulator_default();
    let closeout_policy = match config
        .runtime
        .workflow
        .effective_closeout_policy(&config.runtime.workflow.closeout.default_policy)
    {
        Ok(policy) => policy,
        Err(err) => {
            return fail(
                "workflow_closeout_readiness",
                format!("cannot evaluate workflow closeout readiness: {err:?}"),
            );
        }
    };
    let blockers = projection
        .workflows
        .values()
        .filter_map(|workflow| {
            if workflow.terminal && workflow.status != "outcome.finished" {
                return None;
            }
            let readiness = projection.closeout_readiness(
                workflow.workflow_id.clone(),
                &persistent_tasks,
                &signoff_policy,
                &closeout_policy,
            );
            (!readiness.overall_allowed).then(|| {
                let blocking_dimensions = readiness
                    .dimensions
                    .iter()
                    .filter(|dimension| !dimension.allowed)
                    .map(|dimension| dimension.id.clone())
                    .collect::<Vec<_>>();
                serde_json::json!({
                    "workflow_id": workflow.workflow_id,
                    "blocking_dimensions": blocking_dimensions,
                    "legal_next_actions": readiness.legal_next_actions,
                    "stale_export": readiness.stale_export,
                })
            })
        })
        .collect::<Vec<_>>();
    if blockers.is_empty() {
        return pass_with_details(
            "workflow_closeout_readiness",
            "latest workflow run has no closeout blockers under the default policy",
            Some(serde_json::json!({
                "run_dir": run_dir,
                "policy_id": closeout_policy.policy_id,
                "dossier_evidence_category": WORKFLOW_CLOSEOUT_DOSSIER_EVIDENCE_CATEGORY,
            })),
        );
    }
    warn_with_details(
        "workflow_closeout_readiness",
        "latest workflow run has closeout blockers; inspect workflow status/dossier legal_next_actions",
        Some(serde_json::json!({
            "run_dir": run_dir,
            "policy_id": closeout_policy.policy_id,
            "blockers": blockers,
        })),
    )
}

fn check_parity_ledger() -> DoctorCheck {
    let Some(ledger) = (match parse_parity_ledger() {
        Ok(ledger) => ledger,
        Err(err) => return fail("parity_ledger", err),
    }) else {
        return warn(
            "parity_ledger",
            "docs/parity-ledger.json is not present; use the Harness workflow parity matrix as the current parity source",
        );
    };
    let Some(items) = ledger.get("items").and_then(Value::as_array) else {
        return fail(
            "parity_ledger",
            "docs/parity-ledger.json is missing an items array",
        );
    };

    let missing_fields = items
        .iter()
        .filter(|item| {
            !has_non_empty_string(item, "id")
                || !has_non_empty_string(item, "owner")
                || !has_non_empty_string(item, "status")
                || item
                    .get("evidence")
                    .and_then(Value::as_array)
                    .is_none_or(|evidence| evidence.is_empty())
        })
        .count();
    if missing_fields > 0 {
        return fail(
            "parity_ledger",
            format!(
                "docs/parity-ledger.json has {missing_fields} item(s) missing id, owner, status, or evidence"
            ),
        );
    }

    pass(
        "parity_ledger",
        format!(
            "{} parity item(s) loaded from docs/parity-ledger.json",
            items.len()
        ),
    )
}

fn check_compatibility_parity_gaps() -> DoctorCheck {
    let Some(ledger) = (match parse_parity_ledger() {
        Ok(ledger) => ledger,
        Err(err) => return fail("compatibility_parity_gaps", err),
    }) else {
        return warn(
            "compatibility_parity_gaps",
            "docs/parity-ledger.json is not present; use the Harness workflow parity matrix for the current parity gap list",
        );
    };
    let Some(items) = ledger.get("items").and_then(Value::as_array) else {
        return fail(
            "compatibility_parity_gaps",
            "docs/parity-ledger.json is missing an items array",
        );
    };

    let open_items = items
        .iter()
        .filter(|item| {
            item.get("status")
                .and_then(Value::as_str)
                .is_none_or(|status| !matches!(status, "present" | "stronger"))
        })
        .collect::<Vec<_>>();
    if open_items.is_empty() {
        return pass(
            "compatibility_parity_gaps",
            "compatibility parity ledger has no open gaps",
        );
    }

    let preview = open_items
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .take(6)
        .collect::<Vec<_>>()
        .join(", ");
    warn(
        "compatibility_parity_gaps",
        format!(
            "{} open compatibility parity ledger item(s); next gaps include: {preview}; see docs/parity-ledger.json",
            open_items.len()
        ),
    )
}

fn parse_parity_ledger() -> Result<Option<Value>, String> {
    let path = Path::new("docs/parity-ledger.json");
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
    };
    serde_json::from_str(&body)
        .map(Some)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn has_non_empty_string(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
}

fn non_empty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn env_var_is_set(name: &str) -> bool {
    non_empty(name) && env::var(name).is_ok_and(|value| non_empty(&value))
}

fn pass(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    let name = name.into();
    DoctorCheck {
        id: name.clone(),
        name,
        status: CheckStatus::Pass,
        message: message.into(),
        details: None,
    }
}

fn pass_with_details(
    name: impl Into<String>,
    message: impl Into<String>,
    details: Option<Value>,
) -> DoctorCheck {
    let name = name.into();
    DoctorCheck {
        id: name.clone(),
        name,
        status: CheckStatus::Pass,
        message: message.into(),
        details,
    }
}

fn warn(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    let name = name.into();
    DoctorCheck {
        id: name.clone(),
        name,
        status: CheckStatus::Warn,
        message: message.into(),
        details: None,
    }
}

fn warn_with_details(
    name: impl Into<String>,
    message: impl Into<String>,
    details: Option<Value>,
) -> DoctorCheck {
    let name = name.into();
    DoctorCheck {
        id: name.clone(),
        name,
        status: CheckStatus::Warn,
        message: message.into(),
        details,
    }
}

fn fail(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    let name = name.into();
    DoctorCheck {
        id: name.clone(),
        name,
        status: CheckStatus::Fail,
        message: message.into(),
        details: None,
    }
}

fn fail_with_details(
    name: impl Into<String>,
    message: impl Into<String>,
    details: Option<Value>,
) -> DoctorCheck {
    let name = name.into();
    DoctorCheck {
        id: name.clone(),
        name,
        status: CheckStatus::Fail,
        message: message.into(),
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_workflow_skill_protocol_body, validate_selected_parity_dossier_with_root,
    };
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn native_workflow_protocol_detects_forbidden_substrate_fixture() {
        let body = r#"
---
name: bad
description: bad
---

## Purpose
Bad fixture.
## Use when
Testing.
## Harness state contract
Harness workflow evidence exists.
## Execution protocol
Run legacy state command and route through a tmux pane.
## Evidence and closeout contract
Close with workflow evidence.
## Verification checklist
Verify.
"#;
        let findings =
            evaluate_workflow_skill_protocol_body("bad", Path::new("bad/SKILL.md"), body);
        let reason_codes = findings
            .iter()
            .map(|finding| finding.reason_code)
            .collect::<Vec<_>>();
        assert!(reason_codes.contains(&"forbidden_legacy_cli_authority"));
        assert!(reason_codes.contains(&"forbidden_tmux_authority"));
    }

    #[test]
    fn strict_parity_dossier_validation_rejects_mirrored_field_drift() {
        let temp = tempdir().expect("tempdir");
        let dossier_path = temp.path().join("proofs/plan/dossier.json");
        fs::create_dir_all(dossier_path.parent().expect("proof parent")).expect("proof dir");
        fs::write(
            &dossier_path,
            serde_json::to_vec_pretty(&json!({
                "canonical_harness_id": "harness.workflow.plan_consensus",
                "registry_command": "wrong-command",
                "state_authority": "harness_events_and_replay_projections",
                "status": "native_complete",
                "scenario": "simulator::plan_happy_path",
                "workflow_phase": "planning",
                "native_behavior_contract": "native contract",
                "operator_visible_success": "operator success",
                "negative_path_contract": "negative contract",
                "proof_kind": "selected_workflow_e2e_parity",
                "strict_doctor_check": "strict_parity_matrix",
                "harness_entrypoint": ["$plan"],
                "legacy_aliases": ["plan"],
                "parity_dimensions": ["invocation", "state", "artifacts", "permissions", "replay", "tui", "negative_path"],
                "evidence_categories": ["strict_parity_doctor", "negative_path_contract"],
                "commands": ["cargo run -p harness -- --config configs/harness.example.jsonc doctor --json --strict-parity"],
                "artifacts": { "docs_dossier": "proofs/plan/dossier.json" },
                "truth_gates": {
                    "replay_derived": true,
                    "native_only": true,
                    "external_runtime_authority": false,
                    "status_reads_append_events": false,
                    "dossier_reads_append_events": false,
                    "permission_checks_before_side_effects": true
                }
            }))
            .expect("serialize dossier"),
        )
        .expect("write dossier");

        let row = json!({
            "canonical_harness_id": "harness.workflow.plan_consensus",
            "registry_command": "plan-consensus",
            "state_authority": "harness_events_and_replay_projections",
            "status": "native_complete",
            "e2e_scenario": "simulator::plan_happy_path",
            "workflow_phase": "planning",
            "native_behavior_contract": "native contract",
            "operator_visible_success": "operator success",
            "negative_path_contract": "negative contract",
            "evidence_dossier_path": "proofs/plan/dossier.json",
            "harness_entrypoint": ["$plan"],
            "legacy_aliases": ["plan"],
            "parity_dimensions": ["invocation", "state", "artifacts", "permissions", "replay", "tui", "negative_path"]
        });
        let mut blockers = Vec::new();
        validate_selected_parity_dossier_with_root(
            "harness.workflow.plan_consensus",
            &row,
            temp.path(),
            &mut blockers,
        );

        assert!(
            blockers
                .iter()
                .any(|blocker| blocker.contains("registry_command")),
            "{blockers:#?}"
        );
    }
}

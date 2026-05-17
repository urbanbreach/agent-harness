use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::Args;
use harness_core::agent_catalog::resolve_agent_catalog;
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
    stable_id_groups, WORKFLOW_DOCS_ANCHORS, WORKFLOW_DOCTOR_CHECKS,
};
use harness_tools::{coordinator_registry_with_mcp_and_editing, EditingToolSurfaceConfig};
use serde::Serialize;
use serde_json::Value;

use crate::cli_io::{load_events_from_run_dir, EVENTS_FILE_NAME};

const PARITY_LEDGER_JSON: &str = include_str!("../../../docs/parity-ledger.json");
const CONFIG_DOC_MD: &str = include_str!("../../../docs/config.md");
const TESTING_DOC_MD: &str = include_str!("../../../docs/testing.md");
const WORKFLOW_SLICE_SPEC_MD: &str = include_str!("../../../docs/omx-workflow-slice-spec.md");

const REQUIRED_PRIMARY_AGENTS: [&str; 3] = ["build", "plan", "discipline"];
const REQUIRED_SUBAGENTS: [&str; 2] = ["explore", "general"];
const REQUIRED_OMO_SPECIALISTS: [&str; 10] = [
    "oracle",
    "librarian",
    "metis",
    "momus",
    "multimodal-looker",
    "sisyphus-junior",
    "atlas",
    "prometheus",
    "sisyphus",
    "hephaestus",
];
const REQUIRED_CATEGORY_ROUTES: [&str; 8] = [
    "visual-engineering",
    "artistry",
    "ultrabrain",
    "deep",
    "quick",
    "unspecified-low",
    "unspecified-high",
    "writing",
];
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
const FIRST_SLICE_OMO_TOOLS: [&str; 24] = [
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

    let report = build_report(config_display, &config);
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

fn build_report(config_display: String, config: &HarnessConfig) -> DoctorReport {
    let checks = vec![
        check_provider_catalog(config),
        check_provider_credentials(config),
        check_model_references(config),
        check_model_capabilities(config),
        check_shipped_profiles(config),
        check_category_routes(config),
        check_agent_catalog(config),
        check_profile_tools(config),
        check_first_slice_omo_tool_surface(config),
        check_command_registry(),
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
        check_omo_parity_gaps(),
    ];

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
            "docs/omx-workflow-slice-spec.md" => !WORKFLOW_SLICE_SPEC_MD.contains(anchor.heading),
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
    for profile in REQUIRED_PRIMARY_AGENTS
        .into_iter()
        .chain(REQUIRED_SUBAGENTS)
        .chain(REQUIRED_OMO_SPECIALISTS)
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

    let invalid_primary = REQUIRED_PRIMARY_AGENTS
        .into_iter()
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

    let invalid_subagents = REQUIRED_SUBAGENTS
        .into_iter()
        .chain(REQUIRED_OMO_SPECIALISTS)
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
        "build, plan, discipline, explore, general, and OMO specialist profiles are available",
    )
}

fn check_category_routes(config: &HarnessConfig) -> DoctorCheck {
    let missing = REQUIRED_CATEGORY_ROUTES
        .into_iter()
        .filter(|profile| !config.agents.contains_key(*profile))
        .collect::<Vec<_>>();

    let invalid_routes = REQUIRED_CATEGORY_ROUTES
        .into_iter()
        .filter_map(|profile| {
            let agent = config.agents.get(profile)?;
            (agent.hidden || agent.mode == AgentMode::Primary).then_some(profile)
        })
        .collect::<Vec<_>>();

    let recursive_routes = REQUIRED_CATEGORY_ROUTES
        .into_iter()
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

fn check_first_slice_omo_tool_surface(config: &HarnessConfig) -> DoctorCheck {
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

    let missing = FIRST_SLICE_OMO_TOOLS
        .into_iter()
        .filter(|tool| !native_tools.contains(*tool))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return warn(
            "omo_tool_surface",
            format!(
                "missing first-slice OMO tool surface id(s): {}; see docs/parity-ledger.json",
                missing.join(", ")
            ),
        );
    }

    pass(
        "omo_tool_surface",
        "first-slice OMO tool ids are registered; unsupported tools return explicit diagnostics",
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
        .keys()
        .filter_map(|workflow_id| {
            let readiness = projection.closeout_readiness(
                workflow_id.clone(),
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
                    "workflow_id": workflow_id,
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
    let ledger = match parse_parity_ledger() {
        Ok(ledger) => ledger,
        Err(err) => return fail("parity_ledger", err),
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

fn check_omo_parity_gaps() -> DoctorCheck {
    let ledger = match parse_parity_ledger() {
        Ok(ledger) => ledger,
        Err(err) => return fail("omo_parity_gaps", err),
    };
    let Some(items) = ledger.get("items").and_then(Value::as_array) else {
        return fail(
            "omo_parity_gaps",
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
        return pass("omo_parity_gaps", "OMO parity ledger has no open gaps");
    }

    let preview = open_items
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .take(6)
        .collect::<Vec<_>>()
        .join(", ");
    warn(
        "omo_parity_gaps",
        format!(
            "{} open OMO parity ledger item(s); next gaps include: {preview}; see docs/parity-ledger.json",
            open_items.len()
        ),
    )
}

fn parse_parity_ledger() -> Result<Value, String> {
    serde_json::from_str(PARITY_LEDGER_JSON)
        .map_err(|err| format!("failed to parse docs/parity-ledger.json: {err}"))
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

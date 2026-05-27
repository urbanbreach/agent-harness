use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Args;
use harness_core::config::{
    resolve_model_selection, AgentMode, HarnessConfig, McpServerConfig, PermissionMode,
    ProviderConfig, ResolvedModelTarget,
};
use harness_core::coord::{
    TASK_CATEGORY_FALLBACK_DISABLED_PARENT_PROFILES, TASK_CATEGORY_FALLBACK_PROFILE,
};
use harness_tools::{coordinator_registry_with_mcp_and_editing, EditingToolSurfaceConfig};
use serde::Serialize;
use serde_json::{json, Value};

use crate::{CliDeps, CliIo};

const REQUIRED_PRIMARY_AGENTS: [&str; 3] = ["build", "plan", "discipline"];
const REQUIRED_SUBAGENTS: [&str; 2] = ["explore", "general"];
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
    name: String,
    status: CheckStatus,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    config: String,
    no_network_probes: bool,
    provider_execution_proof: bool,
    readiness_scope: &'static str,
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

pub(crate) fn execute_with_io(
    command: DoctorCommand,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let config_context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "doctor failed: failed to resolve config context: {err}"
            );
            return 2;
        }
    };

    let Some(loaded) = (match harness_core::config::load_resolved_config_with_context(
        config_path.as_deref(),
        &config_context,
    ) {
        Ok(loaded) => loaded,
        Err(err) => {
            let _ = writeln!(io.stderr, "doctor failed: {err}");
            return 1;
        }
    }) else {
        let _ = writeln!(
            io.stderr,
            "doctor failed: no config file found; pass --config <path>, create ./harness.jsonc or ./harness.json, or start from configs/harness.example.jsonc"
        );
        return 2;
    };

    let config_display = loaded.path_display();
    let mut config = loaded.config;
    config.apply_session_dir_override(session_dir);

    let report = build_report(config_display, &config, &|name| deps.env_var_is_set(name));
    if command.json {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => {
                let _ = writeln!(io.stdout, "{json}");
            }
            Err(err) => {
                let _ = writeln!(io.stderr, "doctor failed to render JSON: {err}");
                return 1;
            }
        }
    } else {
        print_text_report(&report, io.stdout);
    }

    if report.has_failures() {
        1
    } else {
        0
    }
}

pub(crate) fn support_report_json(
    config_display: String,
    config: &HarnessConfig,
    env_var_is_set: &dyn Fn(&str) -> bool,
) -> Value {
    let report = build_report(config_display.clone(), config, env_var_is_set);
    let mut value = serde_json::to_value(report).unwrap_or_else(|err| {
        json!({
            "config": config_display,
            "checks": [],
            "serialization_error": err.to_string(),
        })
    });
    if let Some(object) = value.as_object_mut() {
        object.insert("no_network_probes".to_string(), json!(true));
    }
    value
}

fn build_report(
    config_display: String,
    config: &HarnessConfig,
    env_var_is_set: &dyn Fn(&str) -> bool,
) -> DoctorReport {
    let checks = vec![
        check_provider_catalog(config),
        check_provider_credentials(config, env_var_is_set),
        check_model_references(config),
        check_shipped_profiles(config),
        check_category_routes(config),
        check_resolved_routes(config),
        check_profile_tools(config),
        check_permissions(config),
        check_session_dir(&config.paths.session_dir),
        check_mcp(config),
    ];

    DoctorReport {
        config: config_display,
        no_network_probes: true,
        provider_execution_proof: false,
        readiness_scope: "local_readiness_only",
        checks,
    }
}

fn check_resolved_routes(config: &HarnessConfig) -> DoctorCheck {
    let mut routes = serde_json::Map::new();
    let mut missing = Vec::new();

    for profile in REQUIRED_PRIMARY_AGENTS
        .into_iter()
        .chain(REQUIRED_SUBAGENTS)
        .chain(REQUIRED_CATEGORY_ROUTES)
    {
        let Some(agent) = config.agents.get(profile) else {
            missing.push(profile);
            continue;
        };
        routes.insert(profile.to_string(), route_metadata(config, profile, agent));
    }

    let route_count = routes.len();
    let details = json!({
        "routes": routes,
        "skills": skill_readiness_metadata(config),
        "category_fallback": {
            "unknown_category_profile": TASK_CATEGORY_FALLBACK_PROFILE,
            "disabled_for_parent": TASK_CATEGORY_FALLBACK_DISABLED_PARENT_PROFILES,
            "policy_source": "harness_core::coord::task_category_fallback_profile",
            "visibility": "task output reports requested category, resolved route, runtime metadata, and fallback policy; doctor reports the same policy without provider or MCP network calls"
        },
        "no_network_probes": true,
    });

    if !missing.is_empty() {
        return warn_with_details(
            "resolved_routes",
            format!(
                "resolved metadata omitted missing shipped route(s): {}",
                missing.join(", ")
            ),
            details,
        );
    }

    pass_with_details(
        "resolved_routes",
        format!("{route_count} shipped route(s) resolved with prompt, tool, model, and permission metadata"),
        details,
    )
}

fn route_metadata(
    config: &HarnessConfig,
    profile: &str,
    agent: &harness_core::config::ProfileConfig,
) -> Value {
    let model = resolve_model_selection(config, &agent.model_ref, agent.variant.as_deref())
        .map(|selection| {
            json!({
                "model_ref": selection.primary.model_ref,
                "provider": selection.primary.provider,
                "model": selection.primary.model,
                "variant": selection.primary.variant,
                "tool_call_support": tool_call_support_metadata(config, &selection.primary),
                "fallback_chain": selection
                    .fallback
                    .into_iter()
                    .map(|target| target.model_ref)
                    .collect::<Vec<_>>(),
            })
        })
        .unwrap_or_else(|err| {
            json!({
                "model_ref": agent.model_ref,
                "variant": agent.variant,
                "tool_call_support": unknown_tool_call_support_metadata("model_resolution_failed"),
                "resolution_error": err.to_string(),
            })
        });

    json!({
        "profile_id": profile,
        "role": route_role(profile, agent),
        "hidden": agent.hidden,
        "prompt": prompt_status(profile, agent),
        "model": model,
        "toolset": agent.tools,
        "skills": route_skill_metadata(config, agent),
        "permission_posture": permission_posture(config, agent),
    })
}

fn skill_readiness_metadata(config: &HarnessConfig) -> Value {
    let configured_permission_patterns = config
        .skills
        .permissions
        .iter()
        .map(|(pattern, mode)| {
            json!({
                "pattern": pattern,
                "permission": permission_mode_label(Some(mode)),
            })
        })
        .collect::<Vec<_>>();
    let skill_tool_profiles = config
        .agents
        .iter()
        .filter_map(|(profile, agent)| {
            agent
                .tools
                .iter()
                .any(|tool| tool == "skill")
                .then_some(profile.as_str())
        })
        .collect::<Vec<_>>();
    let root_count = config.skills.project_roots.len() + config.skills.global_roots.len();

    json!({
        "status": if root_count == 0 { "no_roots_configured" } else { "configured" },
        "project_roots": config.skills.project_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "global_roots": config.skills.global_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "walk_to_git_root": config.skills.walk_to_git_root,
        "permission_rules": configured_permission_patterns,
        "skill_tool_profiles": skill_tool_profiles,
        "no_network_probes": true,
    })
}

fn route_skill_metadata(
    config: &HarnessConfig,
    agent: &harness_core::config::ProfileConfig,
) -> Value {
    json!({
        "tool_enabled": agent.tools.iter().any(|tool| tool == "skill"),
        "configured_permission_patterns": config.skills.permissions.keys().cloned().collect::<Vec<_>>(),
    })
}

fn tool_call_support_metadata(config: &HarnessConfig, target: &ResolvedModelTarget) -> Value {
    let Some(provider) = config.providers.get(&target.provider) else {
        return unknown_tool_call_support_metadata("provider_metadata_missing");
    };
    let ProviderConfig::OpenAiCompatible(provider) = provider;
    let Some(model) = provider.models.get(&target.model) else {
        return unknown_tool_call_support_metadata("model_metadata_missing");
    };

    match model.metadata.supports_tool_calls {
        Some(supports_tool_calls) => json!({
            "status": if supports_tool_calls { "supported" } else { "unsupported" },
            "supports_tool_calls": supports_tool_calls,
            "source": "provider_model_metadata",
            "no_network_probes": true,
        }),
        None => unknown_tool_call_support_metadata("unknown_not_declared"),
    }
}

fn unknown_tool_call_support_metadata(reason: &str) -> Value {
    json!({
        "status": "unknown_not_probed",
        "supports_tool_calls": null,
        "source": reason,
        "no_network_probes": true,
    })
}

fn route_role(profile: &str, agent: &harness_core::config::ProfileConfig) -> &'static str {
    if agent.hidden {
        return "hidden";
    }
    if REQUIRED_CATEGORY_ROUTES.contains(&profile) {
        return "category";
    }
    if REQUIRED_PRIMARY_AGENTS.contains(&profile) || agent.mode == AgentMode::Primary {
        return "primary";
    }
    if agent.mode == AgentMode::Subagent {
        return "subagent";
    }
    "all"
}

fn prompt_status(profile: &str, agent: &harness_core::config::ProfileConfig) -> Value {
    if agent.system_prompt.as_deref().is_some_and(non_empty) {
        return json!({
            "status": "available",
            "source": "configured_or_discovered",
        });
    }
    if bundled_prompt_available(profile) {
        return json!({
            "status": "available",
            "source": "bundled_shipped_asset",
        });
    }
    json!({
        "status": "missing",
        "source": null,
    })
}

fn bundled_prompt_available(profile: &str) -> bool {
    REQUIRED_PRIMARY_AGENTS.contains(&profile)
        || REQUIRED_SUBAGENTS.contains(&profile)
        || REQUIRED_CATEGORY_ROUTES.contains(&profile)
}

fn permission_posture(
    config: &HarnessConfig,
    agent: &harness_core::config::ProfileConfig,
) -> Value {
    let permissions = agent.permissions.as_ref();
    json!({
        "fallback": permission_mode_label(permissions.and_then(|value| value.fallback.as_ref()).or(config.permissions.fallback.as_ref())),
        "edit": permission_mode_label(permissions.and_then(|value| value.edit.as_ref()).or(permissions.and_then(|value| value.fallback.as_ref())).or(Some(&config.permissions.defaults.edit))),
        "bash": permission_mode_label(permissions.and_then(|value| value.shell.as_ref()).or(permissions.and_then(|value| value.fallback.as_ref())).or(Some(&config.permissions.defaults.shell))),
        "question": permission_mode_label(permissions.and_then(|value| value.question.as_ref()).or(permissions.and_then(|value| value.fallback.as_ref())).or(config.permissions.defaults.question.as_ref()).or(config.permissions.fallback.as_ref())),
        "task": permission_mode_label(permissions.and_then(|value| value.task.as_ref()).or(permissions.and_then(|value| value.fallback.as_ref())).or(config.permissions.defaults.task.as_ref()).or(config.permissions.fallback.as_ref())),
        "webfetch": permission_mode_label(permissions.and_then(|value| value.webfetch.as_ref()).or(permissions.and_then(|value| value.network.as_ref())).or(permissions.and_then(|value| value.fallback.as_ref())).or(config.permissions.defaults.webfetch.as_ref()).or(Some(&config.permissions.defaults.network))),
        "websearch": permission_mode_label(permissions.and_then(|value| value.websearch.as_ref()).or(permissions.and_then(|value| value.network.as_ref())).or(permissions.and_then(|value| value.fallback.as_ref())).or(config.permissions.defaults.websearch.as_ref()).or(Some(&config.permissions.defaults.network))),
        "codesearch": permission_mode_label(permissions.and_then(|value| value.codesearch.as_ref()).or(permissions.and_then(|value| value.network.as_ref())).or(permissions.and_then(|value| value.fallback.as_ref())).or(config.permissions.defaults.codesearch.as_ref()).or(Some(&config.permissions.defaults.network))),
        "lsp": permission_mode_label(permissions.and_then(|value| value.lsp.as_ref()).or(permissions.and_then(|value| value.fallback.as_ref())).or(config.permissions.defaults.lsp.as_ref()).or(config.permissions.fallback.as_ref())),
    })
}

fn permission_mode_label(mode: Option<&PermissionMode>) -> Value {
    match mode {
        Some(PermissionMode::Allow) => json!("allow"),
        Some(PermissionMode::Ask) => json!("ask"),
        Some(PermissionMode::Deny) => json!("deny"),
        None => Value::Null,
    }
}

fn print_text_report(report: &DoctorReport, out: &mut dyn Write) {
    let (passes, warnings, failures) = report.status_counts();
    let headline = if failures == 0 && warnings == 0 {
        "doctor ok"
    } else if failures == 0 {
        "doctor ok with warnings"
    } else {
        "doctor found issues"
    };
    let _ = writeln!(out, "{headline}: {}", report.config);
    let _ = writeln!(
        out,
        "checks: {passes} passed, {warnings} warnings, {failures} failures"
    );
    let _ = writeln!(
        out,
        "scope: local readiness only; no provider or MCP network probes; not provider execution proof"
    );
    for check in &report.checks {
        let _ = writeln!(
            out,
            "[{}] {}: {}",
            check.status.label(),
            check.name,
            check.message
        );
    }
}

fn check_provider_credentials(
    config: &HarnessConfig,
    env_var_is_set: &dyn Fn(&str) -> bool,
) -> DoctorCheck {
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

fn check_shipped_profiles(config: &HarnessConfig) -> DoctorCheck {
    let mut missing = Vec::new();
    for profile in REQUIRED_PRIMARY_AGENTS
        .into_iter()
        .chain(REQUIRED_SUBAGENTS)
    {
        if !config.agents.contains_key(profile) {
            missing.push(profile);
        }
    }
    if !missing.is_empty() {
        return warn(
            "workflow_profiles",
            format!(
                "missing recommended shipped profile(s): {}; enable them under `agent` for the complete V1 local coding path",
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
        "build, plan, discipline, explore, and general profiles are available",
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

fn non_empty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn pass(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status: CheckStatus::Pass,
        message: message.into(),
        details: None,
    }
}

fn pass_with_details(
    name: impl Into<String>,
    message: impl Into<String>,
    details: Value,
) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status: CheckStatus::Pass,
        message: message.into(),
        details: Some(details),
    }
}

fn warn(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status: CheckStatus::Warn,
        message: message.into(),
        details: None,
    }
}

fn warn_with_details(
    name: impl Into<String>,
    message: impl Into<String>,
    details: Value,
) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status: CheckStatus::Warn,
        message: message.into(),
        details: Some(details),
    }
}

fn fail(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status: CheckStatus::Fail,
        message: message.into(),
        details: None,
    }
}

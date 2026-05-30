use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Args;
use harness_core::agent_catalog::{
    resolve_agent_catalog, SHIPPED_CATEGORY_ROUTES, SHIPPED_PRIMARY_PROFILES, SHIPPED_SUBAGENTS,
};
use harness_core::config::{
    resolve_model_selection, AgentMode, HarnessConfig, McpServerConfig, PermissionMode,
    ProviderConfig,
};
use harness_core::event::EventEnvelopeV1;
use harness_core::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
use harness_core::proj::{project_team_state, TeamRunStatus};
use harness_tools::{
    coordinator_registry_with_mcp_and_editing, discover_skill_catalog_with_config,
    native_tool_catalog_entries, EditingToolSurfaceConfig, SkillCatalogStatus,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::readiness::ast_grep_adapter_readiness;
use crate::{CliDeps, CliIo};

const REQUIRED_PRIMARY_AGENTS: [&str; 2] = ["build", "plan"];
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

    let report = build_report(
        config_display,
        &config,
        &config_context.discovery.current_dir,
        &|name| deps.env_var_is_set(name),
    );
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
    workspace_root: &Path,
    env_var_is_set: &dyn Fn(&str) -> bool,
) -> Value {
    let report = build_report(
        config_display.clone(),
        config,
        workspace_root,
        env_var_is_set,
    );
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
    workspace_root: &Path,
    env_var_is_set: &dyn Fn(&str) -> bool,
) -> DoctorReport {
    let checks = vec![
        check_provider_catalog(config),
        check_provider_credentials(config, env_var_is_set),
        check_model_references(config),
        check_shipped_profiles(config),
        check_category_routes(config),
        check_resolved_routes(config, workspace_root),
        check_profile_tools(config),
        check_native_tool_catalog(config),
        check_extension_roadmap_readiness(config),
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

fn check_resolved_routes(config: &HarnessConfig, workspace_root: &Path) -> DoctorCheck {
    let catalog = resolve_agent_catalog(config);
    let mut missing = Vec::new();

    for profile in SHIPPED_PRIMARY_PROFILES
        .iter()
        .copied()
        .chain(SHIPPED_SUBAGENTS.iter().copied())
        .chain(SHIPPED_CATEGORY_ROUTES.iter().copied())
    {
        if catalog.get(profile).is_none() {
            missing.push(profile);
        }
    }

    let routes = catalog
        .entries
        .iter()
        .map(|entry| {
            let mut value = serde_json::to_value(entry).unwrap_or_else(|_| json!({}));
            attach_doctor_model_metadata(config, &mut value);
            (entry.id.clone(), value)
        })
        .collect::<BTreeMap<_, _>>();
    let route_count = routes.len();
    let details = json!({
        "routes": routes,
        "skills": skill_readiness_metadata(config, workspace_root),
        "category_fallback": {
            "unknown_category_profile": catalog.category_fallback.unknown_category_profile,
            "disabled_parent_profiles": catalog.category_fallback.disabled_parent_profiles.clone(),
            "disabled_for_parent": catalog.category_fallback.disabled_parent_profiles,
            "policy_source": catalog.category_fallback.policy_source,
        },
        "catalog_source": "harness_core::agent_catalog",
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

fn check_native_tool_catalog(config: &HarnessConfig) -> DoctorCheck {
    let registry = coordinator_registry_with_mcp_and_editing(
        config.permissions.shell_allowlist.clone(),
        Default::default(),
        EditingToolSurfaceConfig {
            hashline_edit: config.hashline_edit,
        },
    );
    let catalog = native_tool_catalog_entries(&registry);
    let required = [
        "session_list",
        "session_read",
        "session_search",
        "session_info",
        "background_cancel",
        "team_list",
        "ast_grep_search",
        "ast_grep_replace",
    ];
    let missing = required
        .iter()
        .filter_map(|tool_id| {
            (!catalog.iter().any(|entry| entry.canonical_id == **tool_id)).then_some(*tool_id)
        })
        .collect::<Vec<_>>();
    let details = json!({
        "catalog_source": "harness_tools::tool_catalog",
        "tool_count": catalog.len(),
        "required_v1_tools": required,
        "tools": catalog,
        "readiness": {
            "session_tools": catalog.iter().filter(|entry| entry.canonical_id.starts_with("session_")).count(),
            "background_cancel": catalog.iter().any(|entry| entry.canonical_id == "background_cancel"),
            "team_list": catalog.iter().any(|entry| entry.canonical_id == "team_list"),
            "team_projection": active_team_projection_summary(&config.paths.session_dir),
            "ast_grep_search": catalog.iter().any(|entry| entry.canonical_id == "ast_grep_search"),
            "ast_grep_adapter": ast_grep_adapter_readiness(),
            "ast_grep_replace": "shipped_edit_safe",
        },
        "no_network_probes": true,
    });

    if !missing.is_empty() {
        return fail(
            "native_tool_catalog",
            format!(
                "missing required V1 tool catalog entries: {}",
                missing.join(", ")
            ),
        );
    }
    pass_with_details(
        "native_tool_catalog",
        format!(
            "{} native tool catalog entries are available",
            catalog.len()
        ),
        details,
    )
}

fn check_extension_roadmap_readiness(config: &HarnessConfig) -> DoctorCheck {
    let details = json!({
        "scope": "roadmap_readiness_not_runtime_health",
        "separate_from_runtime_health": true,
        "core_capabilities": {
            "markdown_skills": "shipped",
            "config_backed_mcp": "shipped",
            "native_tool_catalog": "shipped",
            "agent_category_routes": "shipped",
        },
        "built_in_capabilities": {
            "configured_agent_profiles": config.agents.len(),
            "project_skill_roots": config.skills.project_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            "global_skill_roots": config.skills.global_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            "disabled_skill_selectors": config.skills.disabled.clone(),
        },
        "descriptor_seams": {
            "typed_extension_manifest": {
                "status": "shipped_descriptor_only",
                "schema_version": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
                "schema_path": "configs/extension-manifest.v1.schema.json",
                "runtime_effects_scope": "descriptor_only",
                "runtime_effects": {
                    "registers_tools": false,
                    "executes_commands": false,
                    "launches_mcp": false,
                    "invokes_provider_decorators": false,
                    "loads_external_code": false,
                    "mutates_sessions": false,
                },
            },
        },
        "planned_seams": {
            "command_hooks": "final_slice",
            "ast_grep_replace": "shipped_descriptor",
            "desktop_mobile_web_clients": "post_v1",
            "browser_media_automation": "post_v1",
            "team_mode": "primitive_only_v1",
        },
        "no_network_probes": true,
    });

    pass_with_details(
        "extension_roadmap_readiness",
        "roadmap and extension readiness are reported separately from local runtime health",
        details,
    )
}

fn active_team_projection_summary(session_dir: &Path) -> Value {
    if !session_dir.exists() {
        return json!({
            "available": false,
            "active_team_count": 0,
            "sessions_scanned": 0,
            "parse_error_count": 0,
            "reason": "session_dir_missing",
            "source": "event_replay",
            "no_network_probes": true,
        });
    }

    let mut active_team_count = 0usize;
    let mut sessions_scanned = 0usize;
    let mut parse_error_count = 0usize;
    if let Ok(entries) = std::fs::read_dir(session_dir) {
        for entry in entries.filter_map(Result::ok) {
            let events_path = entry.path().join("events.jsonl");
            if !events_path.is_file() {
                continue;
            }
            let Ok(body) = std::fs::read_to_string(&events_path) else {
                parse_error_count = parse_error_count.saturating_add(1);
                continue;
            };
            let mut events = Vec::new();
            for line in body.lines().filter(|line| !line.trim().is_empty()) {
                match serde_json::from_str::<EventEnvelopeV1>(line) {
                    Ok(event) => events.push(event),
                    Err(_) => parse_error_count = parse_error_count.saturating_add(1),
                }
            }
            sessions_scanned = sessions_scanned.saturating_add(1);
            if let Ok(projection) = project_team_state(events.iter()) {
                active_team_count = active_team_count.saturating_add(
                    projection
                        .teams
                        .values()
                        .filter(|team| team.status == TeamRunStatus::Active)
                        .count(),
                );
            }
        }
    }

    json!({
        "available": true,
        "active_team_count": active_team_count,
        "sessions_scanned": sessions_scanned,
        "parse_error_count": parse_error_count,
        "source": "event_replay",
        "no_network_probes": true,
    })
}

fn attach_doctor_model_metadata(config: &HarnessConfig, value: &mut Value) {
    let Some(model) = value.get_mut("model").and_then(Value::as_object_mut) else {
        return;
    };
    let provider_id = model.get("provider").and_then(Value::as_str);
    let model_id = model.get("model").and_then(Value::as_str);
    model.insert(
        "tool_call_support".to_string(),
        tool_call_support_metadata(config, provider_id, model_id),
    );
}

fn tool_call_support_metadata(
    config: &HarnessConfig,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> Value {
    let Some(provider_id) = provider_id else {
        return json!({
            "status": "unknown",
            "supports_tool_calls": Value::Null,
            "source": "model_resolution_unavailable",
            "no_network_probes": true,
        });
    };
    let Some(model_id) = model_id else {
        return json!({
            "status": "unknown",
            "supports_tool_calls": Value::Null,
            "source": "model_resolution_unavailable",
            "no_network_probes": true,
        });
    };
    let Some(provider) = config.providers.get(provider_id) else {
        return json!({
            "status": "unknown",
            "supports_tool_calls": Value::Null,
            "source": "provider_model_metadata_missing",
            "no_network_probes": true,
        });
    };
    let ProviderConfig::OpenAiCompatible(provider) = provider;
    let Some(model) = provider.models.get(model_id) else {
        return json!({
            "status": "unknown",
            "supports_tool_calls": Value::Null,
            "source": "provider_model_metadata_missing",
            "no_network_probes": true,
        });
    };

    match model.metadata.supports_tool_calls {
        Some(true) => json!({
            "status": "supported",
            "supports_tool_calls": true,
            "source": "provider_model_metadata",
            "no_network_probes": true,
        }),
        Some(false) => json!({
            "status": "unsupported",
            "supports_tool_calls": false,
            "source": "provider_model_metadata",
            "no_network_probes": true,
        }),
        None => json!({
            "status": "unknown",
            "supports_tool_calls": Value::Null,
            "source": "provider_model_metadata",
            "no_network_probes": true,
        }),
    }
}

fn skill_readiness_metadata(config: &HarnessConfig, workspace_root: &Path) -> Value {
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

    let catalog = discover_skill_catalog_with_config(workspace_root, &config.skills);
    let (catalog, readiness, catalog_status) = match catalog {
        Ok(catalog) => {
            let loadable_count = catalog
                .entries
                .iter()
                .filter(|entry| entry.status == SkillCatalogStatus::Loadable)
                .count();
            let denied_count = catalog
                .entries
                .iter()
                .filter(|entry| entry.status == SkillCatalogStatus::Denied)
                .count();
            let disabled_count = catalog
                .entries
                .iter()
                .filter(|entry| entry.status == SkillCatalogStatus::Disabled)
                .count();
            let malformed_count = catalog
                .entries
                .iter()
                .filter(|entry| entry.status == SkillCatalogStatus::Malformed)
                .count();
            let shadowed_count = catalog
                .entries
                .iter()
                .filter(|entry| entry.status == SkillCatalogStatus::Shadowed)
                .count();
            (
                json!(catalog),
                json!({
                    "entry_count": catalog.entries.len(),
                    "loadable_count": loadable_count,
                    "denied_count": denied_count,
                    "disabled_count": disabled_count,
                    "malformed_count": malformed_count,
                    "shadowed_count": shadowed_count,
                }),
                "available",
            )
        }
        Err(err) => (
            json!({ "entries": [] }),
            json!({
                "entry_count": 0,
                "loadable_count": 0,
                "denied_count": 0,
                "disabled_count": 0,
                "malformed_count": 0,
                "shadowed_count": 0,
                "error": err.to_string(),
            }),
            "unavailable",
        ),
    };

    json!({
        "status": if root_count == 0 { "no_roots_configured" } else { "configured" },
        "catalog_status": catalog_status,
        "catalog_source": "harness_tools::skill_catalog",
        "project_roots": config.skills.project_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "global_roots": config.skills.global_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "walk_to_git_root": config.skills.walk_to_git_root,
        "permission_rules": configured_permission_patterns,
        "skill_tool_profiles": skill_tool_profiles,
        "workspace_root": workspace_root.display().to_string(),
        "catalog": catalog,
        "readiness": readiness,
        "no_network_probes": true,
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
        "build, plan, explore, and general profiles are available",
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

    let missing_core_tools = [("build", &BUILD_TOOLS[..]), ("plan", &PLAN_TOOLS[..])]
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

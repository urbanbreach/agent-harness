// allow: SIZE_OK — doctor diagnostic check registry (14 checks + helpers, indivisible)
use super::*;
use crate::doctor::{DoctorCheck, DoctorReport};
use std::collections::BTreeMap;
use std::path::Path;

pub(super) fn check_resolved_routes(config: &HarnessConfig, workspace_root: &Path) -> DoctorCheck {
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
            attach_doctor_model_metadata(config, workspace_root, &mut value);
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

pub(super) fn check_native_tool_catalog(config: &HarnessConfig) -> DoctorCheck {
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
        "ast_grep_search",
        "ast_grep_replace",
    ];
    let missing = required
        .iter()
        .filter_map(|tool_id| {
            (!catalog.iter().any(|entry| entry.canonical_id == **tool_id)).then_some(*tool_id)
        })
        .collect::<Vec<_>>();
    let profile_description_overrides = profile_description_overrides_by_tool(config);
    let tools = catalog
        .iter()
        .map(|entry| {
            let mut value = serde_json::to_value(entry).unwrap_or_else(|_| json!({}));
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "profile_description_overrides".to_string(),
                    json!(profile_description_overrides
                        .get(&entry.canonical_id)
                        .cloned()
                        .unwrap_or_default()),
                );
            }
            value
        })
        .collect::<Vec<_>>();
    let details = json!({
        "catalog_source": "harness_tools::tool_catalog",
        "tool_count": catalog.len(),
        "required_v1_tools": required,
        "tools": tools,
        "readiness": {
            "session_tools": catalog.iter().filter(|entry| entry.canonical_id.starts_with("session_")).count(),
            "background_cancel": catalog.iter().any(|entry| entry.canonical_id == "background_cancel"),
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

pub(super) fn profile_description_overrides_by_tool(
    config: &HarnessConfig,
) -> BTreeMap<String, Vec<String>> {
    let mut overrides = BTreeMap::new();
    overrides.insert(
        "task".to_string(),
        config.agents.keys().cloned().collect::<Vec<_>>(),
    );
    overrides.insert(
        "skill".to_string(),
        config
            .agents
            .iter()
            .filter(|(_, profile)| profile.tools.iter().any(|tool| tool == "skill"))
            .map(|(profile_name, _)| profile_name.clone())
            .collect::<Vec<_>>(),
    );
    overrides
}

pub(super) fn check_extension_roadmap_readiness(config: &HarnessConfig) -> DoctorCheck {
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
        },
        "no_network_probes": true,
    });

    pass_with_details(
        "extension_roadmap_readiness",
        "roadmap and extension readiness are reported separately from local runtime health",
        details,
    )
}

pub(super) fn print_text_report(report: &DoctorReport, out: &mut dyn Write) {
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

pub(super) fn check_provider_credentials(
    config: &HarnessConfig,
    env_var_is_set: &dyn Fn(&str) -> bool,
    credential_store: &Option<CredentialStore>,
) -> DoctorCheck {
    if config.providers.is_empty() {
        return fail("provider_credentials", "no providers are configured");
    }

    let mut inline_credentials = 0;
    let mut env_credentials = 0;
    let mut stored_oauth_credentials = 0;
    let mut stored_api_key_credentials = 0;
    let mut missing = Vec::new();
    let mut credential_errors = Vec::new();

    for (id, provider) in &config.providers {
        let ProviderConfig::OpenAiCompatible(provider) = provider else {
            continue;
        };
        let stored = match (provider.auth_provider.clone(), credential_store.as_ref()) {
            (Some(auth_provider), Some(store)) => match store.load(&auth_provider) {
                Ok(stored) => stored,
                Err(err) => {
                    credential_errors.push(format!("{id} ({auth_provider}: {err})"));
                    None
                }
            },
            _ => None,
        };
        if let Some(stored) = stored {
            match stored.kind {
                StoredCredentialKind::Oauth => stored_oauth_credentials += 1,
                StoredCredentialKind::ApiKey => stored_api_key_credentials += 1,
                StoredCredentialKind::WellKnown => stored_oauth_credentials += 1,
            }
            continue;
        }

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

    let auth_status =
        auth_cmd::auth_statuses(Some(config), env_var_is_set, credential_store.as_ref());
    let details = Some(json!({
        "auth": auth_status,
        "redacted": true,
        "no_network_probes": true,
    }));

    if !credential_errors.is_empty() || !missing.is_empty() {
        let mut findings = Vec::new();
        if !credential_errors.is_empty() {
            findings.push(format!(
                "{} provider(s) have unreadable stored credentials: {}",
                credential_errors.len(),
                credential_errors.join("; ")
            ));
        }
        if !missing.is_empty() {
            findings.push(format!(
                "{} provider(s) lack an available API key or stored auth credential: {}",
                missing.len(),
                missing.join("; ")
            ));
        }
        return DoctorCheck {
            name: "provider_credentials".to_string(),
            status: CheckStatus::Warn,
            message: findings.join("; "),
            details,
        };
    }

    DoctorCheck {
        name: "provider_credentials".to_string(),
        status: CheckStatus::Pass,
        message: format!(
            "{} provider(s) have credentials available; {stored_oauth_credentials} stored oauth, {stored_api_key_credentials} stored api_key, {inline_credentials} inline, {env_credentials} via environment",
            config.providers.len()
        ),
        details,
    }
}

pub(super) fn check_provider_catalog(config: &HarnessConfig) -> DoctorCheck {
    if config.providers.is_empty() {
        return fail("provider_catalog", "no providers are configured");
    }

    let providers_without_models = config
        .providers
        .iter()
        .filter_map(|(id, provider)| {
            let ProviderConfig::OpenAiCompatible(provider) = provider else {
                return None;
            };
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
            let ProviderConfig::OpenAiCompatible(provider) = provider else {
                return 0;
            };
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

pub(super) fn check_model_references(config: &HarnessConfig) -> DoctorCheck {
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

pub(super) fn check_shipped_profiles(config: &HarnessConfig) -> DoctorCheck {
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

pub(super) fn check_category_routes(config: &HarnessConfig) -> DoctorCheck {
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

pub(super) fn check_profile_tools(config: &HarnessConfig) -> DoctorCheck {
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

pub(super) fn check_permissions(config: &HarnessConfig) -> DoctorCheck {
    let task_permission = config.permissions.defaults.task.as_ref();
    if matches!(task_permission, Some(PermissionMode::Deny)) {
        return warn(
            "permissions",
            "default task permission is deny; delegation profiles require per-agent task rules to run",
        );
    }

    let shell_allowlist = &config.permissions.shell_allowlist;
    let shell_roots = shell_allowlist.cwd_roots.len();
    let executables = shell_allowlist.executables.len();
    let mode_label = match shell_allowlist.mode {
        ShellAllowlistMode::PermissionPatterns => "permission_patterns",
        ShellAllowlistMode::LegacyExecutables => "legacy_executables",
    };
    pass(
        "permissions",
        format!(
            "default permissions loaded; shell allowlist mode is {mode_label}, with {executables} legacy executable(s) and {shell_roots} cwd root(s)"
        ),
    )
}

pub(super) fn check_session_dir(session_dir: &Path) -> DoctorCheck {
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

pub(super) fn check_mcp(config: &HarnessConfig) -> DoctorCheck {
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

pub(super) fn check_formatters(config: &HarnessConfig, workspace_root: &Path) -> DoctorCheck {
    let formatter_config = &config.formatter;
    if !formatter_config.enabled {
        return pass("formatters", "formatters are disabled");
    }

    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            return warn(
                "formatters",
                format!("failed to create runtime for formatter discovery: {err}"),
            )
        }
    };

    let target_path = workspace_root.display().to_string();
    let statuses: Vec<FormatterStatus> = runtime.block_on(formatter_status(
        formatter_config,
        workspace_root,
        &target_path,
        &RealFormatterDiscovery,
    ));

    let enabled_count = statuses.iter().filter(|s| s.enabled).count();
    let formatter_entries: Vec<Value> = statuses
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "extensions": s.extensions,
                "enabled": s.enabled,
            })
        })
        .collect();
    let details = json!({
        "formatters": formatter_entries,
        "no_network_probes": true,
    });

    if statuses.is_empty() {
        return warn_with_details("formatters", "no formatter statuses available", details);
    }

    if enabled_count == 0 {
        return warn_with_details(
            "formatters",
            format!(
                "{} formatter(s) configured, none enabled on this system",
                statuses.len()
            ),
            details,
        );
    }

    pass_with_details(
        "formatters",
        format!("{enabled_count}/{} formatter(s) enabled", statuses.len()),
        details,
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

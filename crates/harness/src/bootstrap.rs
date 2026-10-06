#[path = "bootstrap/secrets.rs"]
mod secrets;
#[path = "bootstrap/subagents.rs"]
mod subagents;
use harness::CliDeps;
use harness_core::{
    agent::AgentProfile,
    auth::{CredentialStore, ProviderCredentialManager, ProviderId},
    config::{
        resolve_model_selection, HarnessConfig, OpenAiApiMode, PermissionRuleSet, ProviderConfig,
    },
    coord::CoordinatorConfig,
    perm::{PermissionPolicy, PermissionRule, PermissionRuleset},
};
use harness_providers::{HttpProvider, Protocol, Provider, ProviderRouter};
pub(crate) use secrets::secret_values;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

pub(crate) fn interactive_profile_name(config: &HarnessConfig) -> String {
    config
        .ui
        .default_profile
        .clone()
        .unwrap_or_else(|| "default".into())
}
pub(crate) fn interactive_config_guidance() -> String {
    "Configure a provider and model, connect with `harness auth login`, or use --mock.".into()
}
pub(crate) fn interactive_agent_profiles(
    config: &HarnessConfig,
) -> Result<BTreeMap<String, AgentProfile>, String> {
    config
        .agents
        .iter()
        .map(|(name, source)| {
            let mut profile = AgentProfile::fallback(name);
            profile.model_ref = source.model_ref.clone();
            profile.model_ref_explicit = source.model_ref_explicit;
            profile.system_prompt = source
                .system_prompt
                .iter()
                .map(String::as_str)
                .chain(
                    config
                        .instruction_files
                        .iter()
                        .map(|file| file.content.as_str()),
                )
                .collect::<Vec<_>>()
                .join("\n\n");
            profile.temperature = source.temperature;
            profile.max_iters = source.max_iters;
            profile.tool_failure_mode = source.tool_failure_mode;
            let mut seen = BTreeSet::new();
            profile.toolset = source
                .tools
                .iter()
                .filter(|name| seen.insert(*name))
                .cloned()
                .collect();
            if let Some(permissions) = &source.permissions {
                profile.permission_ruleset = permission_rules(permissions, &permissions.rules)?;
            }
            Ok((name.clone(), profile))
        })
        .collect()
}
pub(crate) fn build_interactive_coordinator_config(
    config: &HarnessConfig,
) -> Result<CoordinatorConfig, String> {
    let mut result = build(config, &CliDeps::real(), false)?;
    result.interactive = true;
    Ok(result)
}
pub(crate) fn build(
    config: &HarnessConfig,
    deps: &CliDeps,
    mock: bool,
) -> Result<CoordinatorConfig, String> {
    config.validate().map_err(|e| e.to_string())?;
    let mut result = CoordinatorConfig::new(config.runtime.session_dir.clone());
    result.model_catalog = harness_core::config::configured_model_catalog(config).into();
    subagents::configure(config, deps, &mut result)?;
    let mut rules = permission_rules(&config.permissions.defaults, &config.permissions.rules)?;
    if let Some(action) = config.permissions.fallback {
        rules.insert(
            0,
            PermissionRule {
                permission: "*".into(),
                pattern: "*".into(),
                action,
            },
        );
    }
    rules.extend(
        config
            .skills
            .permissions
            .iter()
            .map(|(pattern, action)| PermissionRule {
                permission: "skill".into(),
                pattern: pattern.clone().into(),
                action: *action,
            }),
    );
    result.permission_policy = PermissionPolicy::from_rules(rules)
        .map_err(|e| e.to_string())?
        .with_ask_timeout_ms(config.runtime.permissions.ask_timeout_ms);
    result.yolo_on_start = config.runtime.yolo;
    result.skills = config.skills.clone();
    result.skill_catalog_discovery = Some(Arc::new(harness_tools::NativeSkillCatalogDiscovery));
    result.tool_concurrency = config.runtime.background_tasks.default_concurrency;
    result.provider_model_concurrency = config.runtime.background_tasks.model_concurrency;
    result.compaction = config.runtime.compaction.clone();
    result.provider_retry = config.runtime.provider_retry.clone();
    result.formatter = Arc::new(config.formatter.clone());
    result.hook_runtime_config = harness_core::config::HookRuntimeConfig {
        hooks: config.hooks.clone(),
        shell_allowlist: config.permissions.shell_allowlist.clone(),
        suppress_execution: config.runtime.deterministic.enabled,
    };
    let mut registry = harness_tools::coordinator_registry_with_skills(
        config.permissions.shell_allowlist.clone(),
        config.skills.clone(),
    );
    harness_tools::register_subagent_tools(
        &mut registry,
        &result.subagents,
        result
            .subagent_definitions
            .as_ref()
            .ok_or("subagent definitions unavailable")?,
        result.subagent_model_catalog.as_ref(),
    );
    harness_tools::register_remote_search_tools(
        &mut registry,
        remote_search_config(deps, config.integrations.remote_search.clone())?,
    );
    harness_tools::register_mcp_tools(&mut registry, config.integrations.mcp.clone())
        .map_err(|e| e.to_string())?;
    harness_tools::register_lsp_tools(&mut registry, config.lsp.clone());
    harness_tools::register_eval_tool(&mut registry, config.eval.clone());
    harness_tools::register_github_tools(&mut registry, &|key| deps.env_var_value(key));
    harness_tools::register_shell_tool(
        &mut registry,
        config.permissions.shell_allowlist.clone(),
        &|key| deps.env_var_value(key),
    );
    let mcp_tools: Vec<_> = registry
        .tool_ids()
        .into_iter()
        .filter(|id| id.starts_with("mcp."))
        .collect();
    result.tool_registry = Arc::new(registry);
    if !config.agents.is_empty() {
        result.agent_profiles = interactive_agent_profiles(config)?;
    }
    for (name, profile) in &mut result.agent_profiles {
        if config
            .agents
            .get(name)
            .is_some_and(|p| p.mode == harness_core::config::AgentMode::Primary)
        {
            for tool in &mcp_tools {
                if !profile.toolset.contains(tool) {
                    profile.toolset.push(tool.clone());
                }
            }
        }
    }
    let store = (!mock)
        .then(|| CredentialStore::from_lookup(&|name| deps.env_var_value(name)))
        .flatten();
    result.secret_values = secret_values(config, deps, store.as_ref())?;
    let instructions = config
        .instruction_files
        .iter()
        .map(|file| file.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    for (name, profile) in &mut result.agent_profiles {
        let source = harness_core::system_prompt::PromptSource {
            user_prompt_dir: {
                let path = |key| {
                    deps.env_var_value(key)
                        .filter(|value| !value.is_empty())
                        .map(PathBuf::from)
                };
                path("XDG_CONFIG_HOME")
                    .or_else(|| path("HOME").map(|home| home.join(".config")))
                    .map(|base| base.join("harness/prompts"))
            },
            configured: config
                .agents
                .get(name)
                .and_then(|p| p.system_prompt.clone()),
            suffix: if instructions.trim().is_empty() {
                String::new()
            } else {
                format!("\n\nProject instructions:\n{instructions}")
            },
            ..Default::default()
        };
        profile.system_prompt = format!(
            "{}{}",
            source.configured.as_deref().unwrap_or_default(),
            source.suffix
        );
        result
            .agent_prompt_sources
            .insert(name.clone(), Arc::new(source));
        if mock {
            profile.model_ref = "mock:default".into();
            continue;
        }
        if profile.model_ref == "mock:default" && !config.providers.contains_key("mock") {
            continue;
        }
        let selection = resolve_model_selection(
            config,
            &profile.model_ref,
            config.agents.get(name).and_then(|p| p.variant.as_deref()),
        )
        .map_err(|e| e.to_string())?;
        profile.model_ref = selection.primary.model_ref.clone();
        if let Some(ProviderConfig::OpenAiCompatible(provider)) =
            config.providers.get(&selection.primary.provider)
        {
            profile.cache_retention = provider.cache_retention;
        }
        result
            .agent_model_targets
            .insert(name.clone(), selection.primary);
        result
            .agent_model_fallbacks
            .insert(name.clone(), selection.fallback);
    }
    result.provider = match deps.provider_override() {
        Some(provider) => provider,
        _ => {
            if mock {
                Arc::new(harness_providers::mock::MockProvider::default())
            } else {
                providers(config, deps, &result.secret_registry)?
            }
        }
    };
    result.config_digest = blake3::hash(&serde_json::to_vec(config).map_err(|e| e.to_string())?)
        .to_hex()
        .to_string();
    Ok(result)
}

fn providers(
    config: &HarnessConfig,
    deps: &CliDeps,
    secrets: &Arc<harness_core::redact::SecretRegistry>,
) -> Result<Arc<dyn Provider>, String> {
    let store = CredentialStore::from_lookup(&|name| deps.env_var_value(name));
    let mut providers = BTreeMap::<String, Arc<dyn Provider>>::new();
    for (name, definition) in &config.providers {
        if config.disabled_providers.contains(name)
            || !config.enabled_providers.is_empty() && !config.enabled_providers.contains(name)
        {
            continue;
        }
        let (base, protocol, key, env, timeout, headers) = match definition {
            ProviderConfig::OpenAiCompatible(p) => (
                &p.base_url,
                if p.api_mode != OpenAiApiMode::ChatCompletions {
                    Protocol::Responses
                } else {
                    Protocol::Chat
                },
                &p.api_key,
                &p.api_key_env,
                p.timeout_ms,
                &p.headers,
            ),
            ProviderConfig::Anthropic(p) => (
                &p.base_url,
                Protocol::Anthropic,
                &p.api_key,
                &p.api_key_env,
                p.timeout_ms,
                &p.headers,
            ),
        };
        let suffix = match protocol {
            Protocol::Chat => "/chat/completions",
            Protocol::Responses => "/responses",
            Protocol::Anthropic => "/messages",
        };
        let mut endpoint = reqwest::Url::parse(base).map_err(|_| "invalid provider URL")?;
        let path = endpoint.path().trim_end_matches('/');
        let path = if path.ends_with(suffix) {
            path.to_owned()
        } else {
            format!("{path}{suffix}")
        };
        endpoint.set_path(&path);
        let mut provider =
            HttpProvider::new(endpoint.as_str(), protocol, Duration::from_millis(timeout))?;
        if matches!(definition, ProviderConfig::OpenAiCompatible(p) if p.api_mode == OpenAiApiMode::Auto)
        {
            provider = provider.with_chat_fallback();
        }
        let mut extra_headers = reqwest::header::HeaderMap::new();
        for (key, value) in headers {
            extra_headers.insert(
                reqwest::header::HeaderName::try_from(key)
                    .map_err(|_| "invalid provider header name")?,
                reqwest::header::HeaderValue::try_from(value)
                    .map_err(|_| "invalid provider header value")?,
            );
        }
        provider = provider.with_headers(extra_headers);
        let auth = definition
            .credential_provider(name)
            .ok_or("invalid provider ID")?;
        if auth == ProviderId::codex() {
            provider = provider.with_auth_profile(harness_providers::ProviderAuthProfile::Codex);
        } else if auth == ProviderId::github_copilot() {
            provider =
                provider.with_auth_profile(harness_providers::ProviderAuthProfile::GithubCopilot);
        }
        let stored = store
            .as_ref()
            .is_some_and(|store| store.credential_path(&auth).exists());
        if stored || !key.is_empty() || !env.is_empty() {
            let deps = deps.clone();
            let mut manager = ProviderCredentialManager::new(
                store.clone(),
                auth.clone(),
                env.clone(),
                key,
                move |name| deps.env_var_value(name),
            )
            .with_secret_registry(Arc::clone(secrets))
            .map_err(|e| e.to_string())?;
            if auth == ProviderId::codex() {
                let http =
                    harness_core::auth::ReqwestAuthHttpClient::new().map_err(|e| e.to_string())?;
                manager = manager.with_refresher(Arc::new(
                    harness_core::auth::codex::CodexOAuthClient::new(Arc::new(http)),
                ));
            }
            provider = provider.with_credentials(Arc::new(manager));
        }
        providers.insert(name.clone(), Arc::new(provider));
    }
    if providers.is_empty() {
        return Err(interactive_config_guidance());
    }
    Ok(Arc::new(ProviderRouter::new(providers)))
}

fn permission_rules(
    defaults: &impl serde::Serialize,
    selectors: &PermissionRuleSet,
) -> Result<PermissionRuleset, String> {
    let mut rules = Vec::new();
    let defaults = serde_json::to_value(defaults).map_err(|e| e.to_string())?;
    if let Some(defaults) = defaults.as_object() {
        for (name, action) in defaults {
            if action.is_string() {
                rules.push(PermissionRule {
                    permission: if name == "shell" {
                        "bash".into()
                    } else {
                        name.clone()
                    },
                    pattern: "*".into(),
                    action: serde_json::from_value(action.clone()).map_err(|e| e.to_string())?,
                });
            }
        }
    }
    for (name, selectors) in [
        ("bash", &selectors.shell),
        ("edit", &selectors.edit),
        ("task", &selectors.task),
        ("read", &selectors.read),
        ("external_directory", &selectors.external_directory),
    ] {
        for rule in selectors {
            rules.push(PermissionRule {
                permission: name.into(),
                pattern: rule.selector.clone(),
                action: rule.mode,
            });
        }
    }
    Ok(rules)
}

fn remote_search_config(
    deps: &CliDeps,
    mut config: harness_core::config::RemoteSearchConfig,
) -> Result<harness_core::config::RemoteSearchConfig, String> {
    let first = |names: &[&str]| {
        names.iter().find_map(|name| {
            deps.env_var_value(name)
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.trim().to_owned())
        })
    };
    if let Some(endpoint) = first(&["HARNESS_REMOTE_SEARCH_ENDPOINT", "HARNESS_EXA_MCP_ENDPOINT"]) {
        config.endpoint = endpoint;
    }
    if let Some(token) = first(&[
        "HARNESS_REMOTE_SEARCH_AUTH_TOKEN",
        "HARNESS_EXA_MCP_AUTH_TOKEN",
        "EXA_API_KEY",
    ]) {
        config.auth_token = Some(token);
    }
    if let Some(value) = deps.env_var_value("HARNESS_REMOTE_SEARCH_REQUIRE_AUTH") {
        config.require_auth = match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => true,
            "false" | "0" | "no" | "off" => false,
            _ => return Err("HARNESS_REMOTE_SEARCH_REQUIRE_AUTH must be true or false".into()),
        };
    }
    for (name, target) in [
        (
            "HARNESS_REMOTE_SEARCH_TIMEOUT_SECS",
            &mut config.timeout_secs,
        ),
        (
            "HARNESS_REMOTE_SEARCH_RETRY_BACKOFF_MS",
            &mut config.retry_backoff_ms,
        ),
    ] {
        if let Some(value) = deps.env_var_value(name) {
            *target = value
                .trim()
                .parse()
                .map_err(|_| format!("{name} must be a nonnegative integer"))?;
        }
    }
    if let Some(value) = deps.env_var_value("HARNESS_REMOTE_SEARCH_MAX_RETRIES") {
        config.max_retries = value.trim().parse().map_err(|_| {
            "HARNESS_REMOTE_SEARCH_MAX_RETRIES must be a nonnegative integer".to_owned()
        })?;
    }
    Ok(config)
}

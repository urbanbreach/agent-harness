// allow: SIZE_OK — CLI bootstrap (runtime catalog + profile + provider assembly)
use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::agent::AgentProfile;
use harness_core::auth::codex::{
    AuthHttpClient, AuthHttpMethod, AuthHttpRequest, AuthHttpResponse, CodexOAuthClient,
    CodexOAuthError,
};
use harness_core::auth::{AuthProviderId, CredentialStore, ProviderCredentialManager};
use harness_core::config::{
    refresh_profile_model_metadata_registry, resolve_model_selection, AgentMode, HarnessConfig,
    OpenAiApiMode as CoreOpenAiApiMode, ProviderConfig,
};
use harness_core::coord::CoordinatorConfig;
use harness_core::perm::{PermissionKind, PermissionPolicy, PermissionRuleRequest, PolicyDecision};
use harness_core::tool::ToolRegistry;
use harness_providers::openai::{
    OpenAiApiMode as ProviderOpenAiApiMode, OpenAiAuthProfile, OpenAiCompatibleProvider,
    OpenAiCompatibleProviderConfig,
};
use harness_providers::{Provider, ProviderRouter};
use harness_tools::{
    coordinator_registry_with_mcp_and_editing, discover_skill_catalog_with_config,
    EditingToolSurfaceConfig, SkillCatalogEntry, SkillCatalogStatus,
};

use crate::dynamic_prompt::{self, DynamicPromptContext};
use crate::UnwrapOrAbort;

pub const DEFAULT_INTERACTIVE_PROFILE: &str = "default";

const CONFIG_SEARCH_LOCATIONS: [&str; 4] = [
    "./harness.jsonc",
    "./harness.json",
    "$XDG_CONFIG_HOME/harness/harness.jsonc (fallback: ~/.config/harness/harness.jsonc)",
    "$XDG_CONFIG_HOME/harness/harness.json (fallback: ~/.config/harness/harness.json)",
];

pub fn interactive_config_guidance() -> String {
    format!(
        "interactive mode requires a config file; pass --config <path> or create {}. A starting point lives at configs/harness.example.jsonc and uses the generic agent. If you want the demo/mock UI instead, re-run with --mock",
        CONFIG_SEARCH_LOCATIONS.join(" or ")
    )
}

pub fn build_interactive_coordinator_config(
    cfg: &HarnessConfig,
) -> Result<CoordinatorConfig, String> {
    let mut coordinator_config = CoordinatorConfig::new(cfg.paths.session_dir.clone());
    coordinator_config.permission_policy = PermissionPolicy::from_config(cfg);
    let mut tool_registry = coordinator_registry_with_mcp_and_editing(
        cfg.permissions.shell_allowlist.clone(),
        cfg.integrations.mcp.clone(),
        EditingToolSurfaceConfig {
            hashline_edit: cfg.hashline_edit,
        },
    );
    install_task_tool_subagent_descriptions(&mut tool_registry, cfg);
    install_skill_tool_descriptions(&mut tool_registry, cfg);
    let auto_tool_ids = auto_mcp_tool_ids(&tool_registry);
    coordinator_config.tool_registry = Arc::new(tool_registry);
    coordinator_config.tool_concurrency = cfg.background_task.default_concurrency;
    coordinator_config.provider_model_concurrency = cfg.background_task.model_concurrency;
    coordinator_config.stale_timeout_ms = cfg.background_task.stale_timeout_ms;
    coordinator_config.compaction = cfg.runtime.compaction.clone();
    coordinator_config.provider_retry = cfg.runtime.provider_retry;
    coordinator_config.provider = Arc::new(build_provider_router(cfg)?);
    coordinator_config.title_model_ref = cfg.small_model.clone();
    let (agent_profiles, agent_model_fallbacks) =
        interactive_agent_profiles_with_extra_tools(cfg, &auto_tool_ids)?;
    coordinator_config.agent_profiles = agent_profiles;
    coordinator_config.agent_model_fallbacks = agent_model_fallbacks;
    coordinator_config.formatter = cfg.formatter.clone();
    Ok(coordinator_config)
}

pub fn interactive_profile_name(cfg: &HarnessConfig) -> String {
    let _ = cfg;
    DEFAULT_INTERACTIVE_PROFILE.to_string()
}

fn install_task_tool_subagent_descriptions(tool_registry: &mut ToolRegistry, cfg: &HarnessConfig) {
    let Some(task_tool) = tool_registry.get("task") else {
        return;
    };
    let base_description = task_tool.description().to_string();
    let permission_policy = PermissionPolicy::from_config(cfg);

    for parent_profile in cfg.agents.keys() {
        let description = task_tool_description_for_profile(
            &base_description,
            cfg,
            &permission_policy,
            parent_profile,
        );
        tool_registry.set_profile_tool_description("task", parent_profile, description);
    }
}

fn install_skill_tool_descriptions(tool_registry: &mut ToolRegistry, cfg: &HarnessConfig) {
    if tool_registry.get("skill").is_none() {
        return;
    }

    let workspace_root = harness_core::workspace::WorkspaceEnvironment::current().workspace_root;
    let available_skills = available_skills_prompt(cfg, &workspace_root);
    let description = skill_tool_description(&available_skills);

    for (profile_name, profile) in &cfg.agents {
        if profile.tools.iter().any(|tool| tool == "skill") {
            tool_registry.set_profile_tool_description("skill", profile_name, description.clone());
        }
    }
}

fn available_skills_prompt(cfg: &HarnessConfig, workspace_root: &std::path::Path) -> String {
    let Ok(catalog) = discover_skill_catalog_with_config(workspace_root, &cfg.skills) else {
        return "No skills are currently available.".to_string();
    };
    format_available_skills(catalog.entries.iter())
}

fn format_available_skills<'a>(entries: impl Iterator<Item = &'a SkillCatalogEntry>) -> String {
    let mut skills = entries
        .filter(|entry| entry.status == SkillCatalogStatus::Loadable)
        .collect::<Vec<_>>();
    skills.sort_by(|left, right| left.name.cmp(&right.name));

    if skills.is_empty() {
        return "No skills are currently available.".to_string();
    }

    let mut output = String::from("<available_skills>");
    for skill in skills {
        output.push_str("\n  <skill>\n    <name>");
        output.push_str(&xml_escape(&skill.name));
        output.push_str("</name>\n    <description>");
        output.push_str(&xml_escape(&skill.description));
        output.push_str("</description>\n  </skill>");
    }
    output.push_str("\n</available_skills>");
    output
}

fn skill_tool_description(available_skills: &str) -> String {
    [
        "Load a specialized skill that provides domain-specific instructions and workflows.",
        "Use this tool when the task at hand matches one of the skills listed in available_skills.",
        "The `name` argument must be an exact skill name from available_skills.",
        available_skills,
    ]
    .join("\n\n")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn task_tool_description_for_profile(
    base_description: &str,
    cfg: &HarnessConfig,
    permission_policy: &PermissionPolicy,
    parent_profile: &str,
) -> String {
    let mut subagents = cfg
        .agents
        .iter()
        .filter(|(_, profile)| profile.mode != AgentMode::Primary)
        .filter(|(name, _)| task_profile_allowed(parent_profile, name, permission_policy))
        .map(|(name, profile)| {
            let tools = if profile.tools.is_empty() {
                "no tools".to_string()
            } else {
                profile.tools.join(", ")
            };
            format!("- {name}: {} Tools: {tools}.", profile.description)
        })
        .collect::<Vec<_>>();
    subagents.sort();

    let mut description = String::from(base_description);
    description.push_str("\n\nAvailable subagents:");
    if subagents.is_empty() {
        description.push_str("\n- none");
    } else {
        for subagent in subagents {
            description.push('\n');
            description.push_str(&subagent);
        }
    }
    description
}

fn task_profile_allowed(
    parent_profile: &str,
    child_profile: &str,
    permission_policy: &PermissionPolicy,
) -> bool {
    !matches!(
        permission_policy.evaluate_request(
            Some(parent_profile),
            PermissionKind::Task,
            Some(&PermissionRuleRequest::TaskAgent(child_profile.to_string())),
        ),
        PolicyDecision::Deny
    )
}

fn build_provider_router(cfg: &HarnessConfig) -> Result<ProviderRouter, String> {
    let providers = cfg
        .providers
        .iter()
        .map(|(provider_id, provider)| {
            build_provider(provider_id, provider).map(|provider| (provider_id.clone(), provider))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    Ok(ProviderRouter::new(providers))
}

fn build_provider(
    provider_id: &str,
    provider: &ProviderConfig,
) -> Result<Arc<dyn Provider>, String> {
    match provider {
        ProviderConfig::OpenAiCompatible(provider) => {
            let api_key = provider
                .api_key_env
                .iter()
                .find_map(|name| {
                    std::env::var(name)
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                })
                .unwrap_or_else(|| provider.api_key.clone());

            let mut openai_provider =
                OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
                    base_url: provider.base_url.clone(),
                    api_key,
                    api_mode: map_openai_api_mode(provider.api_mode.clone()),
                    timeout_ms: provider.timeout_ms,
                    headers: provider.headers.clone(),
                })
                .map_err(|err| format!("failed to build provider `{provider_id}`: {err}"))?;

            if let Some(auth_provider) = provider.auth_provider.clone() {
                let auth_profile = if auth_provider == AuthProviderId::codex() {
                    OpenAiAuthProfile::Codex
                } else if auth_provider == AuthProviderId::github_copilot() {
                    OpenAiAuthProfile::GithubCopilot
                } else {
                    return Err(format!(
                        "unsupported auth provider `{auth_provider}` for provider `{provider_id}`"
                    ));
                };
                openai_provider = openai_provider.with_auth_profile(auth_profile);
                if let Some(store) = CredentialStore::from_env() {
                    let mut manager = ProviderCredentialManager::new(
                        store,
                        auth_provider.clone(),
                        provider.api_key_env.clone(),
                        provider.api_key.clone(),
                        |name| std::env::var(name).ok(),
                    );
                    if auth_provider == AuthProviderId::codex() {
                        manager = manager.with_refresher(Arc::new(CodexOAuthClient::new(
                            Arc::new(ReqwestAuthHttpClient::default()),
                        )));
                    }
                    openai_provider = openai_provider.with_credential_source(Arc::new(manager));
                }
            }

            Ok(Arc::new(openai_provider) as Arc<dyn Provider>)
        }
        ProviderConfig::Anthropic(provider) => {
            let api_key = provider
                .api_key_env
                .iter()
                .find_map(|name| {
                    std::env::var(name)
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                })
                .unwrap_or_else(|| provider.api_key.clone());

            let anthropic_provider = harness_providers::anthropic::AnthropicProvider::new(
                harness_providers::anthropic::AnthropicProviderConfig {
                    base_url: provider.base_url.clone(),
                    api_key,
                    timeout_ms: provider.timeout_ms,
                    headers: provider.headers.clone(),
                },
            )
            .map_err(|err| format!("failed to build provider `{provider_id}`: {err}"))?;

            Ok(Arc::new(anthropic_provider) as Arc<dyn Provider>)
        }
    }
}

#[derive(Debug, Default)]
struct ReqwestAuthHttpClient {
    client: reqwest::Client,
}

#[async_trait]
impl AuthHttpClient for ReqwestAuthHttpClient {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CodexOAuthError> {
        let mut builder = match request.method {
            AuthHttpMethod::Post => self.client.post(&request.url),
        };
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        let response =
            builder
                .body(request.body)
                .send()
                .await
                .map_err(|err| CodexOAuthError::Http {
                    message: err.without_url().to_string(),
                })?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|err| CodexOAuthError::Http {
            message: err.without_url().to_string(),
        })?;
        Ok(AuthHttpResponse { status, body })
    }
}

fn map_openai_api_mode(mode: CoreOpenAiApiMode) -> ProviderOpenAiApiMode {
    match mode {
        CoreOpenAiApiMode::Responses => ProviderOpenAiApiMode::Responses,
        CoreOpenAiApiMode::ChatCompletions => ProviderOpenAiApiMode::ChatCompletions,
        CoreOpenAiApiMode::Auto => ProviderOpenAiApiMode::Auto,
    }
}

fn compose_interactive_system_prompt(
    cfg: &HarnessConfig,
    profile_name: &str,
    profile_cfg: &harness_core::config::ProfileConfig,
    model: &harness_core::config::ResolvedModelTarget,
    toolset: &[String],
) -> Result<String, String> {
    let bundled_prompt = profile_cfg
        .system_prompt
        .is_none()
        .then(|| bundled_shipped_agent_prompt(profile_name))
        .flatten();

    if profile_cfg.system_prompt.is_none() && bundled_prompt.is_none() {
        return Err(format!(
            "agent `{profile_name}` is missing a system prompt; define `agent.system_prompt` or ship `.agent-harness/agents/default.md`"
        ));
    }

    let instruction_prompt = cfg.instruction_prompt_prefix();
    Ok(dynamic_prompt::compose(DynamicPromptContext {
        configured_prompt: profile_cfg
            .system_prompt
            .as_deref()
            .or(bundled_prompt.as_deref()),
        model,
        instruction_prompt: instruction_prompt.as_deref(),
        skill_tool_enabled: toolset.iter().any(|tool| tool == "skill"),
    }))
}

fn bundled_shipped_agent_prompt(profile_name: &str) -> Option<String> {
    let markdown = match profile_name {
        DEFAULT_INTERACTIVE_PROFILE => include_str!("../../../.agent-harness/agents/default.md"),
        "explore" => include_str!("../../../.agent-harness/agents/explore.md"),
        "general" => include_str!("../../../.agent-harness/agents/general.md"),
        "librarian" => include_str!("../../../.agent-harness/agents/librarian.md"),
        _ => return None,
    };
    Some(markdown_prompt_body(markdown))
}

fn markdown_prompt_body(markdown: &str) -> String {
    let mut lines = markdown.lines();
    if lines.next() == Some("---") {
        for line in &mut lines {
            if line == "---" {
                break;
            }
        }
        return lines.collect::<Vec<_>>().join("\n").trim().to_string();
    }
    markdown.trim().to_string()
}

pub fn interactive_agent_profiles(
    cfg: &HarnessConfig,
) -> Result<BTreeMap<String, AgentProfile>, String> {
    Ok(interactive_agent_profiles_with_extra_tools(cfg, &[])?.0)
}

fn interactive_agent_profiles_with_extra_tools(
    cfg: &HarnessConfig,
    extra_tool_ids: &[String],
) -> Result<
    (
        BTreeMap<String, AgentProfile>,
        BTreeMap<String, Vec<String>>,
    ),
    String,
> {
    refresh_profile_model_metadata_registry(cfg).map_err(|err| err.to_string())?;

    let mut profiles = BTreeMap::new();
    let mut model_fallbacks = BTreeMap::new();

    let editing_surface = EditingToolSurfaceConfig {
        hashline_edit: cfg.hashline_edit,
    };

    for (profile_name, profile_cfg) in &cfg.agents {
        let model_selection =
            resolve_model_selection(cfg, &profile_cfg.model_ref, profile_cfg.variant.as_deref())
                .map_err(|err| {
                    format!(
                        "agent `{profile_name}` has invalid model selection `{}`: {err}",
                        profile_cfg.model_ref
                    )
                })?;

        let toolset: Vec<String> = normalize_profile_toolset(&profile_cfg.tools, editing_surface)
            .iter()
            .map(String::as_str)
            .chain(extra_tool_ids.iter().map(String::as_str))
            .map(ToOwned::to_owned)
            .collect();
        let system_prompt = compose_interactive_system_prompt(
            cfg,
            profile_name,
            profile_cfg,
            &model_selection.primary,
            &toolset,
        )?;
        let cache_retention = cfg
            .providers
            .get(&model_selection.primary.provider)
            .map(provider_cache_retention)
            .unwrap_or_default();

        let fallback_refs: Vec<String> = model_selection
            .fallback
            .iter()
            .map(|target| target.model_ref.clone())
            .collect();
        if !fallback_refs.is_empty() {
            model_fallbacks.insert(profile_name.clone(), fallback_refs);
        }

        profiles.insert(
            profile_name.clone(),
            AgentProfile {
                name: profile_name.clone(),
                model_ref: model_selection.primary.model_ref,
                model_ref_explicit: profile_cfg.model_ref_explicit,
                system_prompt,
                max_iters: profile_cfg.max_iters,
                temperature: profile_cfg.temperature,
                cache_retention,
                tool_failure_mode: profile_cfg.tool_failure_mode,
                toolset,
                permission_ruleset: profile_cfg
                    .permissions
                    .as_ref()
                    .map(harness_core::perm::from_profile_permissions)
                    .unwrap_or_default(),
            },
        );
    }

    Ok((profiles, model_fallbacks))
}

fn provider_cache_retention(provider: &ProviderConfig) -> harness_providers::CacheRetention {
    match provider {
        ProviderConfig::OpenAiCompatible(provider) => provider.cache_retention,
        ProviderConfig::Anthropic(_) => harness_providers::CacheRetention::None,
    }
}

fn normalize_profile_toolset(
    configured: &[String],
    editing_surface: EditingToolSurfaceConfig,
) -> Vec<String> {
    let mut ordered = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for tool_id in configured {
        push_tool(&mut ordered, &mut seen, tool_id);
    }

    if editing_surface.hashline_edit && seen.contains("edit") && !seen.contains("read") {
        push_tool(&mut ordered, &mut seen, "read");
    }

    ordered
}

fn push_tool(
    ordered: &mut Vec<String>,
    seen: &mut std::collections::BTreeSet<String>,
    tool_id: &str,
) {
    if seen.insert(tool_id.to_string()) {
        ordered.push(tool_id.to_string());
    }
}

fn auto_mcp_tool_ids(tool_registry: &ToolRegistry) -> Vec<String> {
    tool_registry
        .tool_ids()
        .into_iter()
        .filter(|tool_id| {
            tool_id.starts_with("mcp.")
                && !matches!(
                    tool_id
                        .splitn(4, '.')
                        .skip(2)
                        .collect::<Vec<_>>()
                        .as_slice(),
                    ["tools", "list"]
                        | ["tool", "call"]
                        | ["resources", "list"]
                        | ["resource", "read"]
                        | ["prompts", "list"]
                        | ["prompt", "get"]
                )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use harness_core::agent::build_provider_tool_defs;
    use harness_core::config::{load_config_from_file, load_config_from_str};

    use super::*;
    use crate::UnwrapOrAbort;

    fn config_fixture(agents: &str) -> HarnessConfig {
        let raw = format!(
            r#"
            {{
              provider: {{
                default: {{
                  type: "openai_compatible",
                  options: {{
                    baseURL: "http://127.0.0.1:8317/v1",
                    apiKey: "test-openai-api-key",
                    apiMode: "responses",
                    timeoutMs: 60000,
                  }},
                  models: {{
                    "gpt-5.4-mini": {{
                      name: "GPT-5.4 mini",
                    }},
                    "gpt-5.4": {{
                      name: "GPT-5.4",
                      variants: {{
                        mini: {{
                          name: "Mini",
                        }},
                      }},
                    }},
                  }},
                }},
              }},
              model_profile: {{
                fast: {{
                  model: "default:gpt-5.4",
                  variant: "mini",
                }},
              }},
              model: "default/gpt-5.4-mini",
              agent: {{
                {agents}
              }},
              permission: "allow",
            }}
            "#,
            agents = agents,
        );

        load_config_from_str(&raw).unwrap_or_abort()
    }

    #[test]
    fn interactive_agent_profiles_preserve_optional_max_iters_and_temperature() {
        // arrange
        // act
        // assert
        let cfg = config_fixture(
            r#"
            default: {
              system_prompt: "Deep prompt",
              model: "default/gpt-5.4-mini",
              temperature: 0.7,
              tools: ["read"],
            },
            general: {
              system_prompt: "Review prompt",
              model: "default/gpt-5.4-mini",
              max_iters: 20,
              tools: ["read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles(&cfg).unwrap_or_abort();
        assert_eq!(profiles["default"].max_iters, None);
        assert_eq!(profiles["default"].temperature, Some(0.7));
        assert_eq!(profiles["general"].max_iters, Some(20));
        assert_eq!(profiles["general"].temperature, None);
    }

    #[test]
    fn interactive_agents_preserve_configured_system_prompt_in_runtime_config() {
        // arrange
        // act
        // assert
        let configured_prompt =
            "Audit the configured tool flow exactly.\nCollect hooks evidence before signoff.";
        let configured_prompt_json = configured_prompt.replace('\n', "\\n");
        let cfg = config_fixture(&format!(
            r#"
            default: {{
              system_prompt: "{configured_prompt_json}",
              model: "default/gpt-5.4-mini",
              tools: ["read"],
            }},
            "#
        ));

        let profiles = interactive_agent_profiles(&cfg).unwrap_or_abort();
        assert!(profiles["default"]
            .system_prompt
            .starts_with(configured_prompt));
        assert!(profiles["default"]
            .system_prompt
            .contains("The exact model ID is default/gpt-5.4-mini"));

        let coordinator_config = build_interactive_coordinator_config(&cfg).unwrap_or_abort();
        assert!(coordinator_config.agent_profiles["default"]
            .system_prompt
            .starts_with(configured_prompt));
        assert!(coordinator_config.agent_profiles["default"]
            .system_prompt
            .contains("The exact model ID is default/gpt-5.4-mini"));
    }

    #[test]
    fn interactive_agent_profiles_apply_model_profile_selection_to_runtime_model_ref() {
        // arrange
        // act
        // assert
        let cfg = config_fixture(
            r#"
            default: {
              system_prompt: "Build prompt",
              model: "fast",
              tools: ["read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles(&cfg).unwrap_or_abort();
        assert_eq!(profiles["default"].model_ref, "default:gpt-5.4");
    }

    #[test]
    fn interactive_agents_use_shipped_prompt_when_not_overridden() {
        // arrange
        // act
        // assert
        let cfg = config_fixture(
            r#"
            default: {
              model: "default/gpt-5.4-mini",
              tools: ["read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles(&cfg).unwrap_or_abort();
        assert!(profiles["default"]
            .system_prompt
            .starts_with("You are an expert coding assistant"));
    }

    #[test]
    fn interactive_agent_profiles_append_auto_mcp_tools() {
        // arrange
        // act
        // assert
        let cfg = config_fixture(
            r#"
            default: {
              system_prompt: "Build prompt",
              model: "default/gpt-5.4-mini",
              tools: ["read"],
            },
            "#,
        );

        let (profiles, _fallbacks) = interactive_agent_profiles_with_extra_tools(
            &cfg,
            &[
                "mcp.docs-rs.search_in_crate".to_string(),
                "mcp.gh_grep.searchGitHub".to_string(),
            ],
        )
        .unwrap_or_abort();

        assert!(profiles["default"].toolset.contains(&"read".to_string()));
        assert!(profiles["default"]
            .toolset
            .contains(&"mcp.docs-rs.search_in_crate".to_string()));
        assert!(profiles["default"]
            .toolset
            .contains(&"mcp.gh_grep.searchGitHub".to_string()));
        assert!(!profiles["default"]
            .toolset
            .contains(&"mcp.docs-rs.tool.call".to_string()));
    }

    #[test]
    fn interactive_profile_name_is_always_default() {
        // arrange
        // act
        // assert
        let cfg = config_fixture(
            r#"
            default: {
              model: "default/gpt-5.4-mini",
              tools: ["read"],
            },
            "#,
        );

        assert_eq!(interactive_profile_name(&cfg), "default");
    }

    #[test]
    fn shipped_example_config_seeds_default_and_named_subagents() {
        // arrange
        // act
        // assert
        let config_path = crate::cli_config::shipped_example_config_path();
        let cfg = load_config_from_file(&config_path).unwrap_or_abort();

        assert!(cfg.agents.contains_key("default"));
        assert!(cfg.agents.contains_key("explore"));
        assert!(cfg.agents.contains_key("general"));
        assert!(cfg.agents.contains_key("librarian"));

        let profiles = interactive_agent_profiles(&cfg).unwrap_or_abort();
        assert!(profiles["default"].toolset.contains(&"edit".to_string()));
        assert!(profiles["default"].toolset.contains(&"bash".to_string()));
        assert!(profiles["default"].toolset.contains(&"task".to_string()));
        assert!(profiles["default"]
            .toolset
            .contains(&"background_output".to_string()));
        assert!(profiles["default"]
            .toolset
            .contains(&"todowrite".to_string()));
        assert!(!profiles["default"]
            .toolset
            .contains(&"plan_enter".to_string()));
        assert!(!profiles["default"]
            .toolset
            .contains(&"plan_exit".to_string()));
        assert!(profiles["explore"].toolset.contains(&"read".to_string()));
        assert!(profiles["explore"].toolset.contains(&"grep".to_string()));
        assert!(!profiles["explore"].toolset.contains(&"edit".to_string()));
        assert!(profiles["explore"].toolset.contains(&"bash".to_string()));
        assert!(profiles["explore"]
            .toolset
            .contains(&"webfetch".to_string()));
        assert!(profiles["explore"]
            .toolset
            .contains(&"websearch".to_string()));
        assert!(profiles["general"].toolset.contains(&"edit".to_string()));
        assert!(profiles["general"].toolset.contains(&"bash".to_string()));
        assert!(!profiles["general"].toolset.contains(&"task".to_string()));
        assert!(!profiles["general"]
            .toolset
            .contains(&"background_output".to_string()));
        assert!(!profiles["general"]
            .toolset
            .contains(&"todowrite".to_string()));
        assert!(profiles["librarian"]
            .toolset
            .contains(&"webfetch".to_string()));
        assert!(profiles["default"]
            .system_prompt
            .starts_with("You are an expert coding assistant"));
        assert!(profiles["default"].system_prompt.contains("inside Harness"));
        assert!(!profiles["default"]
            .system_prompt
            .to_lowercase()
            .contains(&["open", "code"].concat()));
    }

    #[test]
    fn task_tool_description_lists_available_subagents_for_default() {
        // arrange
        // act
        // assert
        let config_path = crate::cli_config::shipped_example_config_path();
        let cfg = load_config_from_file(&config_path).unwrap_or_abort();
        let coordinator_config = build_interactive_coordinator_config(&cfg).unwrap_or_abort();
        let profile = &coordinator_config.agent_profiles["default"];
        let task_description = task_description_for_profile(&coordinator_config, profile);

        assert!(task_description.contains("Available subagents:"));
        assert!(task_description.contains("- explore: Read-only codebase exploration subagent."));
        assert!(task_description
            .contains("- general: General-purpose implementation and research subagent."));
        assert!(
            task_description.contains("- librarian: Documentation and external research subagent.")
        );
        assert!(!task_description.contains("- default:"));
    }

    #[test]
    fn skill_tool_description_lists_available_skills_for_default() {
        // arrange
        // act
        // assert
        let config_path = crate::cli_config::shipped_example_config_path();
        let cfg = load_config_from_file(&config_path).unwrap_or_abort();
        let coordinator_config = build_interactive_coordinator_config(&cfg).unwrap_or_abort();
        let profile = &coordinator_config.agent_profiles["default"];
        let skill_description = skill_description_for_profile(&coordinator_config, profile);

        assert!(skill_description.contains("<available_skills>"));
        assert!(skill_description.contains("<name>git-master</name>"));
        assert!(skill_description.contains("<name>review-work</name>"));
        assert!(skill_description.contains("<name>rust-best-practices</name>"));
        assert!(skill_description.contains("The `name` argument must be an exact skill name"));
        assert!(!skill_description.contains("user_message"));
    }

    #[test]
    fn task_tool_description_filters_denied_subagents() {
        // arrange
        // act
        // assert
        let cfg = config_fixture(
            r#"
            default: {
              system_prompt: "Default prompt",
              model: "default/gpt-5.4-mini",
              permission: {
                task: {
                  general: "deny",
                  "*": "allow",
                },
              },
              tools: ["task"],
            },
            explore: {
              system_prompt: "Explore prompt",
              model: "default/gpt-5.4-mini",
              tools: ["read"],
            },
            general: {
              system_prompt: "General prompt",
              model: "default/gpt-5.4-mini",
              tools: ["read"],
            },
            "#,
        );
        let coordinator_config = build_interactive_coordinator_config(&cfg).unwrap_or_abort();
        let profile = &coordinator_config.agent_profiles["default"];
        let task_description = task_description_for_profile(&coordinator_config, profile);

        assert!(task_description.contains("- explore: Read-only codebase exploration subagent."));
        assert!(!task_description.contains("- general:"));
    }

    fn task_description_for_profile(
        coordinator_config: &CoordinatorConfig,
        profile: &AgentProfile,
    ) -> String {
        build_provider_tool_defs(profile, coordinator_config.tool_registry.as_ref())
            .unwrap_or_abort()
            .into_iter()
            .find(|tool| tool.tool_id == "task")
            .unwrap_or_abort()
            .description
            .unwrap_or_abort()
    }

    fn skill_description_for_profile(
        coordinator_config: &CoordinatorConfig,
        profile: &AgentProfile,
    ) -> String {
        build_provider_tool_defs(profile, coordinator_config.tool_registry.as_ref())
            .unwrap_or_abort()
            .into_iter()
            .find(|tool| tool.tool_id == "skill")
            .unwrap_or_abort()
            .description
            .unwrap_or_abort()
    }
}

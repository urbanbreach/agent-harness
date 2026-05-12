use std::collections::BTreeMap;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::config::{
    refresh_profile_model_metadata_registry, resolve_model_selection, AgentMode, HarnessConfig,
    OpenAiApiMode as CoreOpenAiApiMode, ProviderConfig,
};
use harness_core::coord::CoordinatorConfig;
use harness_core::perm::{PermissionKind, PermissionPolicy, PermissionRuleRequest, PolicyDecision};
use harness_core::tool::ToolRegistry;
use harness_providers::openai::{
    OpenAiApiMode as ProviderOpenAiApiMode, OpenAiCompatibleProvider,
    OpenAiCompatibleProviderConfig,
};
use harness_providers::{Provider, ProviderRouter};
use harness_tools::{coordinator_registry_with_mcp_and_editing, EditingToolSurfaceConfig};

use crate::dynamic_prompt::{self, DynamicPromptContext};

const DEFAULT_INTERACTIVE_PROFILE: &str = "build";

const CONFIG_SEARCH_LOCATIONS: [&str; 4] = [
    "./harness.jsonc",
    "./harness.json",
    "$XDG_CONFIG_HOME/harness/harness.jsonc (fallback: ~/.config/harness/harness.jsonc)",
    "$XDG_CONFIG_HOME/harness/harness.json (fallback: ~/.config/harness/harness.json)",
];

pub fn interactive_config_guidance() -> String {
    format!(
        "interactive mode requires a config file; pass --config <path> or create {}. A starting point lives at configs/harness.example.jsonc and defaults to the build agent. If you want the demo/mock UI instead, re-run with --mock",
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
    let auto_tool_ids = auto_mcp_tool_ids(&tool_registry);
    coordinator_config.tool_registry = Arc::new(tool_registry);
    coordinator_config.tool_concurrency = cfg.background_task.default_concurrency;
    coordinator_config.provider_model_concurrency = cfg.background_task.model_concurrency;
    coordinator_config.stale_timeout_ms = cfg.background_task.stale_timeout_ms;
    coordinator_config.compaction = cfg.runtime.compaction.clone();
    coordinator_config.provider = Arc::new(build_provider_router(cfg)?);
    coordinator_config.agent_profiles =
        interactive_agent_profiles_with_extra_tools(cfg, &auto_tool_ids)?;
    Ok(coordinator_config)
}

pub fn interactive_profile_name(cfg: &HarnessConfig) -> String {
    cfg.default_agent
        .clone()
        .or_else(|| {
            cfg.agents
                .contains_key(DEFAULT_INTERACTIVE_PROFILE)
                .then(|| DEFAULT_INTERACTIVE_PROFILE.to_string())
        })
        .or_else(|| {
            cfg.agents
                .keys()
                .find(|name| name.as_str() != harness_core::session_title::TITLE_AGENT_NAME)
                .cloned()
        })
        .unwrap_or_else(|| DEFAULT_INTERACTIVE_PROFILE.to_string())
}

fn install_task_tool_subagent_descriptions(tool_registry: &mut ToolRegistry, cfg: &HarnessConfig) {
    let Some(task_tool) = tool_registry.get("task") else {
        return;
    };
    let base_description = task_tool.description().to_string();
    let permission_policy = PermissionPolicy::from_config(cfg);
    let parent_profiles = cfg.agents.keys().cloned().collect::<Vec<_>>();

    for parent_profile in parent_profiles {
        let description = task_tool_description_for_profile(
            &base_description,
            cfg,
            &permission_policy,
            &parent_profile,
        );
        tool_registry.set_profile_tool_description("task", parent_profile, description);
    }
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
    description.push_str(
        "\n\nAvailable subagents for this caller are listed below. Profiles omitted from this list are unavailable to this caller.",
    );
    if subagents.is_empty() {
        description.push_str("\n\nAvailable subagents:\n- none");
    } else {
        description.push_str("\n\nAvailable subagents:");
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
    if parent_profile == harness_core::plan::PLAN_AGENT_NAME && child_profile != "explore" {
        return false;
    }

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
    let ProviderConfig::OpenAiCompatible(provider) = provider;

    OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        api_mode: map_openai_api_mode(provider.api_mode.clone()),
        timeout_ms: provider.timeout_ms,
        headers: provider.headers.clone(),
    })
    .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
    .map_err(|err| format!("failed to build provider `{provider_id}`: {err}"))
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
    if profile_name == harness_core::session_title::TITLE_AGENT_NAME {
        return Ok(profile_cfg.system_prompt.clone().unwrap_or_else(|| {
            harness_core::session_title::TITLE_AGENT_SYSTEM_PROMPT.to_string()
        }));
    }

    if profile_cfg.system_prompt.is_none()
        && !matches!(
            profile_name,
            harness_core::plan::BUILD_AGENT_NAME | harness_core::plan::PLAN_AGENT_NAME
        )
    {
        return Err(format!(
            "agent `{profile_name}` is missing a system prompt; define `agents.{profile_name}.system_prompt` or ship `.agent-harness/agents/{profile_name}.md`"
        ));
    }

    let instruction_prompt = cfg.instruction_prompt_prefix();
    Ok(dynamic_prompt::compose(DynamicPromptContext {
        configured_prompt: profile_cfg.system_prompt.as_deref(),
        model,
        instruction_prompt: instruction_prompt.as_deref(),
        skill_tool_enabled: toolset.iter().any(|tool| tool == "skill"),
    }))
}

pub fn interactive_agent_profiles(
    cfg: &HarnessConfig,
) -> Result<BTreeMap<String, AgentProfile>, String> {
    interactive_agent_profiles_with_extra_tools(cfg, &[])
}

fn interactive_agent_profiles_with_extra_tools(
    cfg: &HarnessConfig,
    extra_tool_ids: &[String],
) -> Result<BTreeMap<String, AgentProfile>, String> {
    refresh_profile_model_metadata_registry(cfg).map_err(|err| err.to_string())?;

    let mut profiles = BTreeMap::new();

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

        profiles.insert(
            profile_name.clone(),
            AgentProfile {
                name: profile_name.clone(),
                category: profile_name.clone(),
                model_ref: model_selection.primary.model_ref,
                model_ref_explicit: profile_cfg.model_ref_explicit,
                system_prompt,
                max_iters: profile_cfg.max_iters,
                temperature: profile_cfg.temperature,
                tool_failure_mode: profile_cfg.tool_failure_mode,
                toolset,
            },
        );
    }

    Ok(profiles)
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

    fn config_fixture(agents: &str) -> HarnessConfig {
        let raw = format!(
            r#"
            {{
              providers: {{
                default: {{
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-openai-api-key",
                  api_mode: "responses",
                  timeout_ms: 60000,
                  models: {{
                    "gpt-5.4-mini": {{
                      display_name: "GPT-5.4 mini",
                    }},
                    "gpt-5.4": {{
                      display_name: "GPT-5.4",
                      variants: {{
                        mini: {{
                          display_name: "Mini",
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
              agents: {{
                {agents}
              }},
              permissions: {{
                defaults: {{
                  edit: "ask",
                  shell: "ask",
                  network: "deny",
                  question: "ask",
                  task: "ask",
                  webfetch: "deny",
                  websearch: "deny",
                  codesearch: "deny",
                  lsp: "allow",
                }},
                shell_allowlist: {{
                  executables: ["git"],
                  cwd_roots: ["."],
                }},
              }},
              runtime: {{
                background_tasks: {{
                  default_concurrency: 2,
                  provider_concurrency: 2,
                  model_concurrency: 2,
                  stale_timeout_ms: 15000,
                  message_staleness_timeout_ms: 5000,
                }},
                session_dir: ".agent-harness/sessions",
                permissions: {{
                  ask_timeout_ms: 45000,
                }},
                prompt: {{
                  wait_timeout_ms: 15000,
                }},
                deterministic: {{
                  enabled: false,
                  seed: 42,
                }},
              }},
              integrations: {{
                remote_search: {{
                  endpoint: "https://mcp.exa.ai/mcp",
                }},
              }},
            }}
            "#,
            agents = agents,
        );

        load_config_from_str(&raw).expect("fixture config should parse")
    }

    #[test]
    fn interactive_agent_profiles_preserve_optional_max_iters_and_temperature() {
        let cfg = config_fixture(
            r#"
            deep: {
              description: "Default iteration budget",
              system_prompt: "Deep prompt",
              model_ref: "default:gpt-5.4-mini",
              temperature: 0.7,
              tools: ["fs.read"],
            },
            review: {
              description: "Longer review budget",
              system_prompt: "Review prompt",
              model_ref: "default:gpt-5.4-mini",
              max_iters: 20,
              tools: ["fs.read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert_eq!(profiles["deep"].max_iters, None);
        assert_eq!(profiles["deep"].temperature, Some(0.7));
        assert_eq!(profiles["review"].max_iters, Some(20));
        assert_eq!(profiles["review"].temperature, None);
    }

    #[test]
    fn interactive_agents_preserve_configured_system_prompt_in_runtime_config() {
        let configured_prompt =
            "Audit the configured tool flow exactly.\nCollect hooks evidence before signoff.";
        let configured_prompt_json = configured_prompt.replace('\n', "\\n");
        let cfg = config_fixture(&format!(
            r#"
            review: {{
              description: "Review profile",
              system_prompt: "{configured_prompt_json}",
              model_ref: "default:gpt-5.4-mini",
              tools: ["read"],
            }},
            "#
        ));

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert!(profiles["review"]
            .system_prompt
            .starts_with(configured_prompt));
        assert!(profiles["review"]
            .system_prompt
            .contains("The exact model ID is default/gpt-5.4-mini"));

        let coordinator_config =
            build_interactive_coordinator_config(&cfg).expect("coordinator config");
        assert!(coordinator_config.agent_profiles["review"]
            .system_prompt
            .starts_with(configured_prompt));
        assert!(coordinator_config.agent_profiles["review"]
            .system_prompt
            .contains("The exact model ID is default/gpt-5.4-mini"));
    }

    #[test]
    fn interactive_agent_profiles_apply_model_profile_selection_to_runtime_model_ref() {
        let cfg = config_fixture(
            r#"
            build: {
              description: "Build lane",
              system_prompt: "Build prompt",
              model_ref: "fast",
              tools: ["read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert_eq!(profiles["build"].model_ref, "default:gpt-5.4");
    }

    #[test]
    fn interactive_agents_require_explicit_or_discovered_system_prompt() {
        let cfg = config_fixture(
            r#"
            deep: {
              description: "Default deep execution profile",
              model_ref: "default:gpt-5.4-mini",
              tools: ["read"],
            },
            "#,
        );

        let err = interactive_agent_profiles(&cfg)
            .expect_err("interactive profiles should fail without a prompt");
        assert!(err.contains("agent `deep` is missing a system prompt"));
    }

    #[test]
    fn interactive_agent_profiles_append_auto_mcp_tools() {
        let cfg = config_fixture(
            r#"
            build: {
              description: "Build lane",
              system_prompt: "Build prompt",
              model_ref: "default:gpt-5.4-mini",
              tools: ["read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles_with_extra_tools(
            &cfg,
            &[
                "mcp.docs-rs.search_in_crate".to_string(),
                "mcp.gh_grep.searchGitHub".to_string(),
            ],
        )
        .expect("interactive profiles");

        assert!(profiles["build"].toolset.contains(&"read".to_string()));
        assert!(profiles["build"]
            .toolset
            .contains(&"mcp.docs-rs.search_in_crate".to_string()));
        assert!(profiles["build"]
            .toolset
            .contains(&"mcp.gh_grep.searchGitHub".to_string()));
        assert!(!profiles["build"]
            .toolset
            .contains(&"mcp.docs-rs.tool.call".to_string()));
    }

    #[test]
    fn interactive_profile_name_defaults_to_build_when_present() {
        let cfg = config_fixture(
            r#"
            build: {
              description: "Build lane",
              model_ref: "default:gpt-5.4-mini",
              tools: ["fs.read"],
            },
            "#,
        );

        assert_eq!(interactive_profile_name(&cfg), "build");
    }

    #[test]
    fn interactive_profile_name_uses_first_available_profile_without_build() {
        let cfg = config_fixture(
            r#"
            deep: {
              description: "Deep lane",
              model_ref: "default:gpt-5.4-mini",
              tools: ["fs.read"],
            },
            "#,
        );

        assert_eq!(interactive_profile_name(&cfg), "deep");
    }

    #[test]
    fn shipped_example_config_seeds_build_plan_and_subagents() {
        let config_path = crate::cli_config::shipped_example_config_path();
        let cfg = load_config_from_file(&config_path).expect("shipped example config should parse");

        assert!(cfg.agents.contains_key("build"));
        assert!(cfg.agents.contains_key("plan"));
        assert!(cfg.agents.contains_key("explore"));
        assert!(cfg.agents.contains_key("general"));
        assert_eq!(cfg.default_agent.as_deref(), Some("build"));

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert!(profiles["build"].toolset.contains(&"edit".to_string()));
        assert!(profiles["build"].toolset.contains(&"bash".to_string()));
        assert!(profiles["build"].toolset.contains(&"task".to_string()));
        assert!(profiles["build"]
            .toolset
            .contains(&"plan_enter".to_string()));
        assert!(profiles["build"]
            .toolset
            .contains(&"background_output".to_string()));
        assert!(profiles["build"].toolset.contains(&"todowrite".to_string()));
        assert!(profiles["plan"].toolset.contains(&"edit".to_string()));
        assert!(profiles["plan"].toolset.contains(&"plan_exit".to_string()));
        assert!(profiles["plan"].toolset.contains(&"task".to_string()));
        assert!(profiles["plan"]
            .toolset
            .contains(&"background_output".to_string()));
        assert!(profiles["plan"].toolset.contains(&"bash".to_string()));
        assert!(!profiles["plan"].toolset.contains(&"plan_enter".to_string()));
        assert!(profiles["explore"].toolset.contains(&"read".to_string()));
        assert!(profiles["explore"].toolset.contains(&"grep".to_string()));
        assert!(!profiles["explore"].toolset.contains(&"edit".to_string()));
        assert!(!profiles["explore"].toolset.contains(&"bash".to_string()));
        assert!(profiles["general"].toolset.contains(&"edit".to_string()));
        assert!(profiles["general"].toolset.contains(&"bash".to_string()));
        assert!(!profiles["general"].toolset.contains(&"task".to_string()));
        assert_eq!(
            profiles[harness_core::session_title::TITLE_AGENT_NAME].system_prompt,
            harness_core::session_title::TITLE_AGENT_SYSTEM_PROMPT
        );
        assert_eq!(
            profiles[harness_core::session_title::TITLE_AGENT_NAME].temperature,
            Some(harness_core::session_title::TITLE_AGENT_TEMPERATURE)
        );
        assert!(profiles[harness_core::session_title::TITLE_AGENT_NAME]
            .toolset
            .is_empty());
        assert!(profiles["build"]
            .system_prompt
            .starts_with("You are agent-harness, You and the user"));
        assert!(profiles["plan"]
            .system_prompt
            .starts_with("You are agent-harness, You and the user"));
        assert!(!profiles["build"]
            .system_prompt
            .to_lowercase()
            .contains(&["open", "code"].concat()));
        assert!(!profiles["plan"]
            .system_prompt
            .to_lowercase()
            .contains(&["open", "code"].concat()));
    }

    #[test]
    fn task_tool_description_lists_available_subagents_for_build() {
        let config_path = crate::cli_config::shipped_example_config_path();
        let cfg = load_config_from_file(&config_path).expect("shipped example config should parse");
        let coordinator_config =
            build_interactive_coordinator_config(&cfg).expect("coordinator config");
        let profile = &coordinator_config.agent_profiles["build"];
        let task_description = task_description_for_profile(&coordinator_config, profile);

        assert!(task_description.contains("Available subagents:"));
        assert!(task_description.contains("- explore: Read-only contextual codebase search agent"));
        assert!(task_description.contains("- general: General-purpose implementation"));
        assert!(!task_description.contains("- build:"));
        assert!(!task_description.contains("- plan:"));
        assert!(!task_description.contains("- title:"));
    }

    #[test]
    fn task_tool_description_respects_plan_delegation_boundary() {
        let config_path = crate::cli_config::shipped_example_config_path();
        let cfg = load_config_from_file(&config_path).expect("shipped example config should parse");
        let coordinator_config =
            build_interactive_coordinator_config(&cfg).expect("coordinator config");
        let profile = &coordinator_config.agent_profiles["plan"];
        let task_description = task_description_for_profile(&coordinator_config, profile);

        assert!(task_description.contains("- explore: Read-only contextual codebase search agent"));
        assert!(!task_description.contains("- general:"));
    }

    #[test]
    fn task_tool_description_filters_denied_subagents() {
        let cfg = config_fixture(
            r#"
            build: {
              description: "Build lane",
              system_prompt: "Build prompt",
              model_ref: "default:gpt-5.4-mini",
              permissions: {
                rules: {
                  task: [
                    { selector: { type: "exact", value: "general" }, mode: "deny" },
                  ],
                },
              },
              tools: ["task"],
            },
            explore: {
              description: "Explore lane",
              system_prompt: "Explore prompt",
              model_ref: "default:gpt-5.4-mini",
              mode: "subagent",
              tools: ["read"],
            },
            general: {
              description: "General lane",
              system_prompt: "General prompt",
              model_ref: "default:gpt-5.4-mini",
              mode: "subagent",
              tools: ["read"],
            },
            "#,
        );
        let coordinator_config =
            build_interactive_coordinator_config(&cfg).expect("coordinator config");
        let profile = &coordinator_config.agent_profiles["build"];
        let task_description = task_description_for_profile(&coordinator_config, profile);

        assert!(task_description.contains("- explore: Explore lane"));
        assert!(!task_description.contains("- general:"));
    }

    fn task_description_for_profile(
        coordinator_config: &CoordinatorConfig,
        profile: &AgentProfile,
    ) -> String {
        build_provider_tool_defs(profile, coordinator_config.tool_registry.as_ref())
            .expect("tool defs")
            .into_iter()
            .find(|tool| tool.tool_id == "task")
            .expect("task tool")
            .description
            .expect("task description")
    }
}

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::config::{
    load_config_from_file, refresh_profile_model_metadata_registry, HarnessConfig,
    OpenAiApiMode as CoreOpenAiApiMode, ProviderConfig,
};
use harness_core::coord::{CoordinatorConfig, PlanProfileConfig};
use harness_core::perm::PermissionPolicy;
use harness_core::tool::ToolRegistry;
use harness_providers::openai::{
    OpenAiApiMode as ProviderOpenAiApiMode, OpenAiCompatibleProvider,
    OpenAiCompatibleProviderConfig,
};
use harness_providers::{Provider, ProviderRouter};
use harness_tools::coordinator_registry_with_mcp;

const DEFAULT_INTERACTIVE_PROFILE: &str = "build";
const LEGACY_INTERACTIVE_PROFILE: &str = "deep";

const CONFIG_SEARCH_LOCATIONS: [&str; 2] = [
    "./harness.jsonc",
    "$XDG_CONFIG_HOME/harness/config.jsonc (fallback: ~/.config/harness/config.jsonc)",
];

pub fn load_harness_config(path: &Path) -> Result<HarnessConfig, String> {
    load_config_from_file(path).map_err(|err| format!("{} ({})", err, path.display()))
}

pub fn interactive_config_guidance() -> String {
    format!(
        "interactive mode requires a config file; pass --config <path> or create {}. A starting point lives at configs/harness.example.jsonc and defaults to the build agent while keeping the plan -> build handoff available. If you want the demo/mock UI instead, re-run with --mock",
        CONFIG_SEARCH_LOCATIONS.join(" or ")
    )
}

pub fn build_interactive_coordinator_config(
    cfg: &HarnessConfig,
) -> Result<CoordinatorConfig, String> {
    let mut coordinator_config = CoordinatorConfig::new(cfg.paths.session_dir.clone());
    coordinator_config.permission_policy = PermissionPolicy::from_config(cfg);
    let tool_registry = coordinator_registry_with_mcp(
        cfg.permissions.shell_allowlist.clone(),
        cfg.integrations.mcp.clone(),
    );
    let auto_tool_ids = auto_mcp_tool_ids(&tool_registry);
    coordinator_config.tool_registry = Arc::new(tool_registry);
    coordinator_config.tool_concurrency = cfg.background_task.default_concurrency;
    coordinator_config.provider_model_concurrency = cfg.background_task.model_concurrency;
    coordinator_config.stale_timeout_ms = cfg.background_task.stale_timeout_ms;
    coordinator_config.provider = Arc::new(build_provider_router(cfg)?);
    coordinator_config.agent_profiles =
        interactive_agent_profiles_with_extra_tools(cfg, &auto_tool_ids)?;
    coordinator_config.plan_profiles = interactive_plan_profiles(cfg);
    Ok(coordinator_config)
}

pub fn interactive_profile_name(cfg: &HarnessConfig) -> String {
    cfg.ui
        .default_profile
        .clone()
        .or_else(|| {
            cfg.agents
                .contains_key(DEFAULT_INTERACTIVE_PROFILE)
                .then(|| DEFAULT_INTERACTIVE_PROFILE.to_string())
        })
        .or_else(|| {
            cfg.agents
                .contains_key(LEGACY_INTERACTIVE_PROFILE)
                .then(|| LEGACY_INTERACTIVE_PROFILE.to_string())
        })
        .or_else(|| cfg.agents.keys().next().cloned())
        .unwrap_or_else(|| DEFAULT_INTERACTIVE_PROFILE.to_string())
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

fn default_interactive_system_prompt(profile_name: &str, description: &str) -> String {
    format!("You are the {profile_name} agent. {description}")
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

    for (profile_name, profile_cfg) in &cfg.agents {
        profiles.insert(
            profile_name.clone(),
            AgentProfile {
                name: profile_name.clone(),
                category: profile_name.clone(),
                model_ref: profile_cfg.model_ref.clone(),
                system_prompt: profile_cfg.system_prompt.clone().unwrap_or_else(|| {
                    default_interactive_system_prompt(profile_name, &profile_cfg.description)
                }),
                max_iters: profile_cfg.max_iters,
                temperature: profile_cfg.temperature,
                tool_failure_mode: profile_cfg.tool_failure_mode,
                toolset: profile_cfg
                    .tools
                    .iter()
                    .map(String::as_str)
                    .chain(extra_tool_ids.iter().map(String::as_str))
                    .map(ToOwned::to_owned)
                    .collect(),
            },
        );
    }

    Ok(profiles)
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

fn interactive_plan_profiles(cfg: &HarnessConfig) -> BTreeMap<String, PlanProfileConfig> {
    cfg.agents
        .iter()
        .map(|(name, profile)| {
            (
                name.clone(),
                PlanProfileConfig {
                    plan_mode: profile.plan_mode,
                    exit_target_profile: profile.exit_target_profile.clone(),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use harness_core::config::load_config_from_str;

    use super::*;

    fn config_fixture(agents: &str) -> HarnessConfig {
        let raw = format!(
            r#"
            {{
              providers: {{
                default: {{
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "sk-zerolimit",
                  api_mode: "responses",
                  timeout_ms: 60000,
                  models: {{
                    "gpt-5.4-mini": {{
                      display_name: "GPT-5.4 mini",
                    }},
                  }},
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
    fn interactive_agent_profiles_preserve_default_max_iters_and_temperature() {
        let cfg = config_fixture(
            r#"
            deep: {
              description: "Default iteration budget",
              model_ref: "default:gpt-5.4-mini",
              temperature: 0.7,
              tools: ["fs.read"],
            },
            tool_audit: {
              description: "Longer tool audit budget",
              model_ref: "default:gpt-5.4-mini",
              max_iters: 20,
              tools: ["fs.read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert_eq!(profiles["deep"].max_iters, 12);
        assert_eq!(profiles["deep"].temperature, Some(0.7));
        assert_eq!(profiles["tool_audit"].max_iters, 20);
        assert_eq!(profiles["tool_audit"].temperature, None);
    }

    #[test]
    fn interactive_agents_preserve_configured_system_prompt_in_runtime_config() {
        let configured_prompt =
            "Audit the configured tool flow exactly.\nCollect hooks evidence before signoff.";
        let configured_prompt_json = configured_prompt.replace('\n', "\\n");
        let cfg = config_fixture(&format!(
            r#"
            tool_audit: {{
              description: "Audit profile",
              system_prompt: "{configured_prompt_json}",
              model_ref: "default:gpt-5.4-mini",
              tools: ["read"],
            }},
            "#
        ));

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert_eq!(profiles["tool_audit"].system_prompt, configured_prompt);

        let coordinator_config =
            build_interactive_coordinator_config(&cfg).expect("coordinator config");
        assert_eq!(
            coordinator_config.agent_profiles["tool_audit"].system_prompt,
            configured_prompt
        );
    }

    #[test]
    fn interactive_agents_fall_back_to_generated_system_prompt_when_missing() {
        let cfg = config_fixture(
            r#"
            deep: {
              description: "Default deep execution profile",
              model_ref: "default:gpt-5.4-mini",
              tools: ["read"],
            },
            "#,
        );

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles");
        assert_eq!(
            profiles["deep"].system_prompt,
            default_interactive_system_prompt("deep", "Default deep execution profile")
        );
    }

    #[test]
    fn interactive_agent_profiles_append_auto_mcp_tools() {
        let cfg = config_fixture(
            r#"
            build: {
              description: "Build lane",
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
            plan: {
              description: "Plan lane",
              model_ref: "default:gpt-5.4-mini",
              tools: ["fs.read"],
            },
            "#,
        );

        assert_eq!(interactive_profile_name(&cfg), "build");
    }

    #[test]
    fn interactive_profile_name_preserves_legacy_deep_fallback_without_build() {
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
}

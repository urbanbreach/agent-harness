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
use harness_core::tool::resolve_tool_ids_for_surface;
use harness_providers::openai::{
    OpenAiApiMode as ProviderOpenAiApiMode, OpenAiCompatibleProvider,
    OpenAiCompatibleProviderConfig,
};
use harness_providers::{Provider, ProviderRouter};
use harness_tools::coordinator_registry_with_mcp;

const DEFAULT_INTERACTIVE_PROFILE: &str = "deep";

const CONFIG_SEARCH_LOCATIONS: [&str; 2] = [
    "./harness.jsonc",
    "$XDG_CONFIG_HOME/harness/config.jsonc (fallback: ~/.config/harness/config.jsonc)",
];

pub fn load_harness_config(path: &Path) -> Result<HarnessConfig, String> {
    load_config_from_file(path).map_err(|err| format!("{} ({})", err, path.display()))
}

pub fn interactive_config_guidance() -> String {
    format!(
        "interactive mode requires a config file; pass --config <path> or create {}. A starting point lives at configs/harness.example.jsonc and defaults to the plan -> build handoff. If you want the demo/mock UI instead, re-run with --mock",
        CONFIG_SEARCH_LOCATIONS.join(" or ")
    )
}

pub fn build_interactive_coordinator_config(
    cfg: &HarnessConfig,
) -> Result<CoordinatorConfig, String> {
    let mut coordinator_config = CoordinatorConfig::new(cfg.paths.session_dir.clone());
    coordinator_config.permission_policy = PermissionPolicy::from_config(cfg);
    coordinator_config.tool_registry = Arc::new(coordinator_registry_with_mcp(
        cfg.permissions.shell_allowlist.clone(),
        cfg.integrations.mcp.clone(),
    ));
    coordinator_config.tool_concurrency = cfg.background_task.default_concurrency;
    coordinator_config.provider_model_concurrency = cfg.background_task.model_concurrency;
    coordinator_config.stale_timeout_ms = cfg.background_task.stale_timeout_ms;
    coordinator_config.provider = Arc::new(build_provider_router(cfg)?);
    coordinator_config.agent_profiles = interactive_agent_profiles(cfg)?;
    coordinator_config.plan_profiles = interactive_plan_profiles(cfg);
    Ok(coordinator_config)
}

pub fn interactive_profile_name(cfg: &HarnessConfig) -> String {
    cfg.ui
        .default_profile
        .clone()
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

pub fn interactive_agent_profiles(
    cfg: &HarnessConfig,
) -> Result<BTreeMap<String, AgentProfile>, String> {
    refresh_profile_model_metadata_registry(cfg).map_err(|err| err.to_string())?;

    let mut profiles = BTreeMap::new();

    for (profile_name, profile_cfg) in &cfg.profiles {
        profiles.insert(
            profile_name.clone(),
            AgentProfile {
                name: profile_name.clone(),
                category: profile_name.clone(),
                model_ref: profile_cfg.model_ref.clone(),
                system_prompt: format!(
                    "You are the {profile_name} agent. {}",
                    profile_cfg.description
                ),
                max_iters: profile_cfg.max_iters,
                tool_failure_mode: profile_cfg.tool_failure_mode,
                tool_surface: profile_cfg.tool_surface,
                toolset: resolve_tool_ids_for_surface(
                    profile_cfg.tools.iter().map(String::as_str),
                    profile_cfg.tool_surface,
                ),
            },
        );
    }

    Ok(profiles)
}

fn interactive_plan_profiles(cfg: &HarnessConfig) -> BTreeMap<String, PlanProfileConfig> {
    cfg.profiles
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
    use super::interactive_agent_profiles;
    use harness_core::config::load_config_from_str;

    #[test]
    fn interactive_agent_profiles_preserve_default_and_custom_max_iters() {
        let cfg = load_config_from_str(
            r#"
            {
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "https://example.invalid/v1",
                  api_key: "test-key",
                  models: {
                    "gpt-5.4-mini": {
                      display_name: "GPT-5.4 mini"
                    }
                  }
                }
              },
              profiles: {
                deep: {
                  description: "Default iteration budget",
                  model_ref: "default:gpt-5.4-mini",
                  tools: ["fs.read"]
                },
                tool_audit: {
                  description: "Longer tool audit budget",
                  model_ref: "default:gpt-5.4-mini",
                  max_iters: 20,
                  tools: ["fs.read"]
                }
              },
              permissions: {
                defaults: {
                  edit: "deny",
                  shell: "deny",
                  network: "deny",
                  question: "deny",
                  task: "deny",
                  webfetch: "deny",
                  websearch: "deny",
                  codesearch: "deny",
                  lsp: "deny"
                },
                shell_allowlist: {
                  executables: ["git"],
                  cwd_roots: ["."]
                }
              },
              runtime: {
                session_dir: "/tmp/harness-tests",
                background_tasks: {
                  default_concurrency: 1,
                  provider_concurrency: 1,
                  model_concurrency: 1,
                  stale_timeout_ms: 1000,
                  message_staleness_timeout_ms: 1000
                },
                permissions: {
                  ask_timeout_ms: 1000
                },
                prompt: {
                  wait_timeout_ms: 1000
                },
                deterministic: {
                  enabled: false,
                  seed: 42
                }
              },
              integrations: {}
            }
            "#,
        )
        .expect("config should parse");

        let profiles = interactive_agent_profiles(&cfg).expect("interactive profiles should build");

        assert_eq!(profiles["deep"].max_iters, 12);
        assert_eq!(profiles["tool_audit"].max_iters, 20);
    }
}

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use harness_core::agent::{AgentModelRef, AgentProfile};
use harness_core::config::{
    load_config_from_file, HarnessConfig, OpenAiApiMode as CoreOpenAiApiMode, ProviderConfig,
};
use harness_core::coord::CoordinatorConfig;
use harness_core::perm::PermissionPolicy;
use harness_providers::openai::{
    OpenAiApiMode as ProviderOpenAiApiMode, OpenAiCompatibleProvider,
    OpenAiCompatibleProviderConfig,
};
use harness_tools::coordinator_registry;

const DEFAULT_PROVIDER_ID: &str = "default";
const DEFAULT_INTERACTIVE_PROFILE: &str = "deep";

pub fn load_harness_config(path: &Path) -> Result<HarnessConfig, String> {
    load_config_from_file(path).map_err(|err| format!("{} ({})", err, path.display()))
}

pub fn build_interactive_coordinator_config(
    cfg: &HarnessConfig,
) -> Result<CoordinatorConfig, String> {
    let mut coordinator_config = CoordinatorConfig::new(cfg.paths.session_dir.clone());
    coordinator_config.permission_policy = PermissionPolicy::from_config(cfg);
    coordinator_config.tool_registry = Arc::new(coordinator_registry(
        cfg.permissions.shell_allowlist.clone(),
    ));
    coordinator_config.tool_concurrency = cfg.background_task.default_concurrency;
    coordinator_config.provider_model_concurrency = cfg.background_task.model_concurrency;
    coordinator_config.stale_timeout_ms = cfg.background_task.stale_timeout_ms;
    coordinator_config.provider = Arc::new(build_default_provider(cfg)?);
    coordinator_config.agent_profiles = build_agent_profiles(cfg)?;
    Ok(coordinator_config)
}

pub fn interactive_profile_name(cfg: &HarnessConfig) -> String {
    cfg.ui
        .default_profile
        .clone()
        .unwrap_or_else(|| DEFAULT_INTERACTIVE_PROFILE.to_string())
}

fn build_default_provider(cfg: &HarnessConfig) -> Result<OpenAiCompatibleProvider, String> {
    let Some(provider) = cfg.providers.get(DEFAULT_PROVIDER_ID) else {
        return Err("interactive mode requires providers.default".to_string());
    };

    let ProviderConfig::OpenAiCompatible(provider) = provider;

    OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        api_mode: map_openai_api_mode(provider.api_mode.clone()),
        timeout_ms: provider.timeout_ms,
        headers: provider.headers.clone(),
    })
    .map_err(|err| format!("failed to build providers.default: {err}"))
}

fn map_openai_api_mode(mode: CoreOpenAiApiMode) -> ProviderOpenAiApiMode {
    match mode {
        CoreOpenAiApiMode::Responses => ProviderOpenAiApiMode::Responses,
        CoreOpenAiApiMode::ChatCompletions => ProviderOpenAiApiMode::ChatCompletions,
        CoreOpenAiApiMode::Auto => ProviderOpenAiApiMode::Auto,
    }
}

fn build_agent_profiles(cfg: &HarnessConfig) -> Result<BTreeMap<String, AgentProfile>, String> {
    let mut profiles = BTreeMap::new();

    for (category_name, category_cfg) in &cfg.categories {
        let model_ref = AgentModelRef::parse(&category_cfg.model_ref);
        if model_ref.provider_id != DEFAULT_PROVIDER_ID {
            return Err(format!(
                "category `{category_name}` must use provider_id `default` (got `{}`)",
                model_ref.provider_id
            ));
        }

        profiles.insert(
            category_name.clone(),
            AgentProfile {
                name: category_name.clone(),
                category: category_name.clone(),
                model_ref: category_cfg.model_ref.clone(),
                system_prompt: format!(
                    "You are the {category_name} agent. {}",
                    category_cfg.description
                ),
                toolset: category_cfg.tools.clone(),
            },
        );
    }

    Ok(profiles)
}

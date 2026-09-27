//! The unchanged TUI reads these snapshots. Runtime execution receives its config directly.
use super::*;
use std::sync::{LazyLock, PoisonError, RwLock};

#[derive(Default)]
struct Registered {
    integrations: Option<IntegrationsConfig>,
    lsp: LspConfig,
    formatter: Option<FormatterConfig>,
    skills: SkillsConfig,
    hooks: HookRuntimeConfig,
    models: BTreeMap<String, ResolvedProfileModelMetadata>,
    connections: BTreeMap<String, McpServerConnectionState>,
    tool_ids: BTreeMap<String, BTreeMap<String, String>>,
}
static REGISTERED: LazyLock<RwLock<Registered>> =
    LazyLock::new(|| RwLock::new(Registered::default()));

pub fn registered_integrations_config() -> Option<IntegrationsConfig> {
    REGISTERED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .integrations
        .clone()
}
pub fn registered_lsp_config() -> LspConfig {
    REGISTERED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .lsp
        .clone()
}
pub fn registered_formatter_config() -> Option<FormatterConfig> {
    REGISTERED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .formatter
        .clone()
}
pub fn registered_skills_config() -> SkillsConfig {
    REGISTERED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .skills
        .clone()
}
pub fn registered_hook_runtime_config() -> HookRuntimeConfig {
    REGISTERED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .hooks
        .clone()
}
pub fn registered_profile_model_metadata(profile: &str) -> Option<ResolvedProfileModelMetadata> {
    REGISTERED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .models
        .get(profile)
        .cloned()
}
pub fn registered_mcp_server_connection_state(name: &str) -> Option<McpServerConnectionState> {
    REGISTERED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .connections
        .get(name)
        .cloned()
}
pub fn registered_mcp_server_first_class_tool_id(server: &str, tool: &str) -> Option<String> {
    REGISTERED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .tool_ids
        .get(server)?
        .get(tool)
        .cloned()
}
pub fn update_registered_mcp_server_connection(
    name: &str,
    state: Option<McpServerConnectionState>,
) {
    let mut registered = REGISTERED.write().unwrap_or_else(PoisonError::into_inner);
    if let Some(state) = state {
        registered.connections.insert(name.into(), state);
    } else {
        registered.connections.remove(name);
    }
}
pub fn update_registered_mcp_server_tools(name: &str, tools: BTreeMap<String, String>) {
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .tool_ids
        .insert(name.into(), tools);
}

pub fn set_registered_integrations_config(config: IntegrationsConfig) {
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .integrations = Some(config);
}
pub fn set_registered_lsp_config(config: LspConfig) {
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .lsp = config;
}
pub fn set_registered_formatter_config(config: FormatterConfig) {
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .formatter = Some(config);
}
pub fn set_registered_hook_runtime_config(config: HookRuntimeConfig) {
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .hooks = config;
}
pub fn set_registered_mcp_server_connection_states(
    states: BTreeMap<String, McpServerConnectionState>,
) {
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .connections = states;
}
pub fn set_registered_mcp_server_first_class_tool_ids(
    ids: BTreeMap<String, BTreeMap<String, String>>,
) {
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .tool_ids = ids;
}

pub fn clear_registered_integrations_config() {
    let mut state = REGISTERED.write().unwrap_or_else(PoisonError::into_inner);
    state.integrations = None;
    state.connections.clear();
    state.tool_ids.clear();
}
pub fn clear_registered_mcp_server_connection_states() {
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .connections
        .clear();
}
pub fn clear_registered_mcp_server_first_class_tool_ids() {
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .tool_ids
        .clear();
}
pub fn refresh_integrations_config_registry(config: &HarnessConfig) {
    let mut state = REGISTERED.write().unwrap_or_else(PoisonError::into_inner);
    state.integrations = Some(config.integrations.clone());
    state.connections.clear();
    state.tool_ids.clear();
}
pub fn refresh_lsp_config_registry(config: &HarnessConfig) {
    set_registered_lsp_config(config.lsp.clone());
}
pub fn refresh_skills_config_registry(config: &HarnessConfig) {
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .skills = config.skills.clone();
}
pub fn refresh_hook_runtime_config_registry(config: &HarnessConfig) {
    set_registered_hook_runtime_config(HookRuntimeConfig {
        hooks: config.hooks.clone(),
        shell_allowlist: config.permissions.shell_allowlist.clone(),
        suppress_execution: false,
    });
}
pub fn refresh_profile_model_metadata_registry(config: &HarnessConfig) -> Result<(), ConfigError> {
    let mut models = BTreeMap::new();
    for (name, agent) in &config.agents {
        if agent.model_ref == "mock:default" && !config.providers.contains_key("mock") {
            continue;
        }
        models.insert(name.clone(), resolve_profile_model_metadata(config, name)?);
    }
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .models = models;
    Ok(())
}

pub(super) fn register(config: &HarnessConfig) -> Result<(), ConfigError> {
    refresh_profile_model_metadata_registry(config)?;
    refresh_hook_runtime_config_registry(config);
    refresh_skills_config_registry(config);
    refresh_lsp_config_registry(config);
    refresh_integrations_config_registry(config);
    set_registered_formatter_config(config.formatter.clone());
    Ok(())
}

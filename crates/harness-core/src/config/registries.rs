use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

use super::{
    resolve_profile_model_metadata, ConfigError, FormatterConfig, HarnessConfig, HookRuntimeConfig,
    IntegrationsConfig, LspConfig, McpServerConnectionState, ResolvedProfileModelMetadata,
    SkillsConfig,
};

static PROFILE_MODEL_METADATA_REGISTRY: OnceLock<
    Mutex<BTreeMap<String, ResolvedProfileModelMetadata>>,
> = OnceLock::new();
static HOOK_RUNTIME_CONFIG_REGISTRY: OnceLock<Mutex<HookRuntimeConfig>> = OnceLock::new();
static SKILLS_CONFIG_REGISTRY: OnceLock<Mutex<SkillsConfig>> = OnceLock::new();
static LSP_CONFIG_REGISTRY: OnceLock<Mutex<LspConfig>> = OnceLock::new();
static INTEGRATIONS_CONFIG_REGISTRY: OnceLock<Mutex<Option<IntegrationsConfig>>> = OnceLock::new();
static MCP_SERVER_CONNECTION_REGISTRY: OnceLock<Mutex<BTreeMap<String, McpServerConnectionState>>> =
    OnceLock::new();
static MCP_SERVER_FIRST_CLASS_TOOL_ID_REGISTRY: OnceLock<
    Mutex<BTreeMap<String, BTreeMap<String, String>>>,
> = OnceLock::new();
static FORMATTER_CONFIG_REGISTRY: OnceLock<Mutex<Option<FormatterConfig>>> = OnceLock::new();

fn profile_model_metadata_registry(
) -> &'static Mutex<BTreeMap<String, ResolvedProfileModelMetadata>> {
    PROFILE_MODEL_METADATA_REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn hook_runtime_config_registry() -> &'static Mutex<HookRuntimeConfig> {
    HOOK_RUNTIME_CONFIG_REGISTRY.get_or_init(|| Mutex::new(HookRuntimeConfig::default()))
}

fn skills_config_registry() -> &'static Mutex<SkillsConfig> {
    SKILLS_CONFIG_REGISTRY.get_or_init(|| Mutex::new(SkillsConfig::default()))
}

fn lsp_config_registry() -> &'static Mutex<LspConfig> {
    LSP_CONFIG_REGISTRY.get_or_init(|| Mutex::new(LspConfig::default()))
}

fn integrations_config_registry() -> &'static Mutex<Option<IntegrationsConfig>> {
    INTEGRATIONS_CONFIG_REGISTRY.get_or_init(|| Mutex::new(None))
}

fn mcp_server_connection_registry() -> &'static Mutex<BTreeMap<String, McpServerConnectionState>> {
    MCP_SERVER_CONNECTION_REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn mcp_server_first_class_tool_id_registry(
) -> &'static Mutex<BTreeMap<String, BTreeMap<String, String>>> {
    MCP_SERVER_FIRST_CLASS_TOOL_ID_REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn formatter_config_registry() -> &'static Mutex<Option<FormatterConfig>> {
    FORMATTER_CONFIG_REGISTRY.get_or_init(|| Mutex::new(None))
}

fn with_registry_lock<T, U>(registry: &'static Mutex<T>, f: impl FnOnce(&mut T) -> U) -> U {
    match registry.lock() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            f(&mut guard)
        }
    }
}

fn with_profile_model_metadata_registry<T>(
    f: impl FnOnce(&mut BTreeMap<String, ResolvedProfileModelMetadata>) -> T,
) -> T {
    with_registry_lock(profile_model_metadata_registry(), f)
}

fn with_hook_runtime_config_registry<T>(f: impl FnOnce(&mut HookRuntimeConfig) -> T) -> T {
    with_registry_lock(hook_runtime_config_registry(), f)
}

fn with_skills_config_registry<T>(f: impl FnOnce(&mut SkillsConfig) -> T) -> T {
    with_registry_lock(skills_config_registry(), f)
}

fn with_lsp_config_registry<T>(f: impl FnOnce(&mut LspConfig) -> T) -> T {
    with_registry_lock(lsp_config_registry(), f)
}

fn with_integrations_config_registry<T>(f: impl FnOnce(&mut Option<IntegrationsConfig>) -> T) -> T {
    with_registry_lock(integrations_config_registry(), f)
}

fn with_mcp_server_connection_registry<T>(
    f: impl FnOnce(&mut BTreeMap<String, McpServerConnectionState>) -> T,
) -> T {
    with_registry_lock(mcp_server_connection_registry(), f)
}

fn with_mcp_server_first_class_tool_id_registry<T>(
    f: impl FnOnce(&mut BTreeMap<String, BTreeMap<String, String>>) -> T,
) -> T {
    with_registry_lock(mcp_server_first_class_tool_id_registry(), f)
}

fn with_formatter_config_registry<T>(f: impl FnOnce(&mut Option<FormatterConfig>) -> T) -> T {
    with_registry_lock(formatter_config_registry(), f)
}

pub fn refresh_profile_model_metadata_registry(cfg: &HarnessConfig) -> Result<(), ConfigError> {
    let resolved = cfg
        .agents
        .keys()
        .map(|profile_name| {
            resolve_profile_model_metadata(cfg, profile_name)
                .map(|metadata| (profile_name.clone(), metadata))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    with_profile_model_metadata_registry(|registry| {
        *registry = resolved;
    });

    Ok(())
}

pub fn refresh_hook_runtime_config_registry(cfg: &HarnessConfig) {
    set_registered_hook_runtime_config(HookRuntimeConfig {
        hooks: cfg.hooks.clone(),
        shell_allowlist: cfg.permissions.shell_allowlist.clone(),
        suppress_execution: false,
    });
}

pub fn refresh_skills_config_registry(cfg: &HarnessConfig) {
    with_skills_config_registry(|registered| {
        *registered = cfg.skills.clone();
    });
}

pub fn set_registered_hook_runtime_config(config: HookRuntimeConfig) {
    with_hook_runtime_config_registry(|registered| {
        *registered = config;
    });
}

pub fn registered_hook_runtime_config() -> HookRuntimeConfig {
    with_hook_runtime_config_registry(|registered| registered.clone())
}

pub fn refresh_lsp_config_registry(cfg: &HarnessConfig) {
    set_registered_lsp_config(cfg.lsp.clone());
}

pub fn refresh_integrations_config_registry(cfg: &HarnessConfig) {
    set_registered_integrations_config(cfg.integrations.clone());
    clear_registered_mcp_server_connection_states();
    clear_registered_mcp_server_first_class_tool_ids();
}

pub fn registered_skills_config() -> SkillsConfig {
    with_skills_config_registry(|registered| registered.clone())
}

pub fn set_registered_integrations_config(config: IntegrationsConfig) {
    with_integrations_config_registry(|registered| {
        *registered = Some(config);
    });
}

pub fn clear_registered_integrations_config() {
    with_integrations_config_registry(|registered| {
        *registered = None;
    });
    clear_registered_mcp_server_connection_states();
    clear_registered_mcp_server_first_class_tool_ids();
}

pub fn registered_integrations_config() -> Option<IntegrationsConfig> {
    with_integrations_config_registry(|registered| registered.clone())
}

pub fn set_registered_mcp_server_connection_states(
    states: BTreeMap<String, McpServerConnectionState>,
) {
    with_mcp_server_connection_registry(|registered| {
        *registered = states;
    });
}

pub fn clear_registered_mcp_server_connection_states() {
    with_mcp_server_connection_registry(|registered| {
        registered.clear();
    });
}

pub fn set_registered_mcp_server_first_class_tool_ids(
    tool_ids: BTreeMap<String, BTreeMap<String, String>>,
) {
    with_mcp_server_first_class_tool_id_registry(|registered| {
        *registered = tool_ids;
    });
}

pub fn clear_registered_mcp_server_first_class_tool_ids() {
    with_mcp_server_first_class_tool_id_registry(|registered| {
        registered.clear();
    });
}

pub fn registered_mcp_server_first_class_tool_id(
    server_name: &str,
    remote_tool_name: &str,
) -> Option<String> {
    with_mcp_server_first_class_tool_id_registry(|registered| {
        registered
            .get(server_name)
            .and_then(|tool_ids| tool_ids.get(remote_tool_name))
            .cloned()
    })
}

pub fn registered_mcp_server_connection_state(
    server_name: &str,
) -> Option<McpServerConnectionState> {
    with_mcp_server_connection_registry(|registered| registered.get(server_name).cloned())
}

pub fn set_registered_lsp_config(config: LspConfig) {
    with_lsp_config_registry(|registered| {
        *registered = config;
    });
}

pub fn registered_lsp_config() -> LspConfig {
    with_lsp_config_registry(|registered| registered.clone())
}

pub fn set_registered_formatter_config(config: FormatterConfig) {
    with_formatter_config_registry(|registered| {
        *registered = Some(config);
    });
}

pub fn registered_formatter_config() -> Option<FormatterConfig> {
    with_formatter_config_registry(|registered| registered.clone())
}

pub fn registered_profile_model_metadata(profile: &str) -> Option<ResolvedProfileModelMetadata> {
    with_profile_model_metadata_registry(|registry| registry.get(profile).cloned())
}

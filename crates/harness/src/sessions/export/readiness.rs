// allow: SIZE_OK — session export readiness assembly (single responsibility: gather config, credentials, catalog, and tool status for export)

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use harness_core::agent_catalog::resolve_agent_catalog;
use harness_core::auth::CredentialStore;
use harness_core::config::{
    AnthropicProviderConfig, HarnessConfig, OpenAiCompatibleProviderConfig, ProviderConfig,
};
use harness_tools::{
    coordinator_registry_with_mcp_and_editing, discover_skill_catalog_with_config,
    native_tool_catalog_entries, EditingToolSurfaceConfig, SkillCatalogStatus,
};
use serde_json::{json, Value};

use crate::readiness::ast_grep_adapter_readiness;
use crate::CliDeps;

use super::credentials::{
    session_export_config_credential_values, session_export_credential_store_manifest,
};
use super::SessionExportReadiness;

pub(super) fn session_export_readiness(
    config_path: Option<&Path>,
    session_dir_override: Option<PathBuf>,
    session_workspace_root: Option<PathBuf>,
    deps: &CliDeps,
) -> SessionExportReadiness {
    let mut context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            return unavailable_session_export_readiness(format!(
                "failed to resolve config context: {err}"
            ));
        }
    };
    if config_path.is_none() {
        if let Some(workspace_root) = session_workspace_root.as_ref() {
            context = context.with_current_dir(workspace_root.clone());
        }
    }
    let skill_workspace_root = session_workspace_root
        .as_deref()
        .unwrap_or(context.discovery.current_dir.as_path())
        .to_path_buf();

    let loaded =
        match harness_core::config::load_resolved_config_with_context(config_path, &context) {
            Ok(Some(loaded)) => loaded,
            Ok(None) => {
                return unavailable_session_export_readiness(
                    "no config file found; support export includes replay-only evidence",
                );
            }
            Err(err) => {
                return unavailable_session_export_readiness(format!(
                    "failed to load config: {err}"
                ));
            }
        };

    let config_display = loaded.path_display();
    let paths = loaded.paths;
    let mut config = loaded.config;
    config.apply_session_dir_override(session_dir_override);
    let credential_store = CredentialStore::from_lookup(&|name| deps.env_var_value(name));
    let credential_values = session_export_config_credential_values(&config, deps);

    SessionExportReadiness {
        doctor_json: crate::doctor::support_report_json(
            config_display.clone(),
            &config,
            &skill_workspace_root,
            &|name| deps.env_var_is_set(name),
        ),
        config_summary: session_export_config_summary(&config_display, &paths, &config),
        provider_summary: session_export_provider_summary(&config),
        agent_catalog_summary: session_export_agent_catalog_summary(&config),
        skill_catalog_summary: session_export_skill_catalog_summary(&skill_workspace_root, &config),
        native_tool_catalog_summary: session_export_native_tool_catalog_summary(&config),
        session_tool_readiness: session_export_session_tool_readiness(&config),
        credential_store_manifest: session_export_credential_store_manifest(
            &config,
            credential_store.as_ref(),
        ),
        credential_values,
    }
}

fn unavailable_session_export_readiness(reason: impl Into<String>) -> SessionExportReadiness {
    let reason = reason.into();
    SessionExportReadiness {
        doctor_json: json!({
            "available": false,
            "no_network_probes": true,
            "reason": reason,
        }),
        config_summary: json!({
            "loaded": false,
            "no_network_probes": true,
            "reason": reason,
        }),
        provider_summary: json!({
            "loaded": false,
            "provider_count": 0,
            "providers": [],
            "no_network_probes": true,
            "reason": reason,
        }),
        agent_catalog_summary: json!({
            "loaded": false,
            "source": "harness_core::agent_catalog",
            "no_network_probes": true,
            "reason": reason,
        }),
        skill_catalog_summary: json!({
            "loaded": false,
            "source": "harness_tools::skill_catalog",
            "no_network_probes": true,
            "reason": reason,
        }),
        native_tool_catalog_summary: json!({
            "loaded": false,
            "source": "harness_tools::tool_catalog",
            "no_network_probes": true,
            "reason": reason,
        }),
        session_tool_readiness: json!({
            "available": false,
            "source": "event_replay",
            "redacted_by_default": true,
            "no_network_probes": true,
            "reason": reason,
        }),
        credential_store_manifest: json!({
            "available": false,
            "redacted_by_default": true,
            "excluded_from_bundle": true,
            "reason": reason,
        }),
        credential_values: Vec::new(),
    }
}

fn session_export_config_summary(
    config_display: &str,
    paths: &[PathBuf],
    config: &HarnessConfig,
) -> Value {
    json!({
        "loaded": true,
        "config": config_display,
        "paths": paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        "agent_count": config.agents.len(),
        "provider_count": config.providers.len(),
        "model_profile_count": config.model_profiles.len(),
        "session_dir": config.paths.session_dir.display().to_string(),
        "no_network_probes": true,
    })
}

fn session_export_provider_summary(config: &HarnessConfig) -> Value {
    let providers = config
        .providers
        .iter()
        .map(|(id, provider)| match provider {
            ProviderConfig::OpenAiCompatible(provider) => {
                openai_compatible_provider_summary(id, provider)
            }
            ProviderConfig::Anthropic(provider) => anthropic_provider_summary(id, provider),
        })
        .collect::<Vec<_>>();

    json!({
        "loaded": true,
        "provider_count": providers.len(),
        "providers": providers,
        "no_network_probes": true,
    })
}

fn session_export_agent_catalog_summary(config: &HarnessConfig) -> Value {
    let catalog = resolve_agent_catalog(config);
    json!({
        "loaded": true,
        "source": "harness_core::agent_catalog",
        "entry_count": catalog.entries.len(),
        "entries": catalog.entries,
        "no_network_probes": true,
    })
}

fn session_export_skill_catalog_summary(workspace_root: &Path, config: &HarnessConfig) -> Value {
    match discover_skill_catalog_with_config(workspace_root, &config.skills) {
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
            json!({
                "loaded": true,
                "source": "harness_tools::skill_catalog",
                "workspace_root": workspace_root.display().to_string(),
                "entry_count": catalog.entries.len(),
                "loadable_count": loadable_count,
                "denied_count": denied_count,
                "disabled_count": disabled_count,
                "malformed_count": malformed_count,
                "shadowed_count": shadowed_count,
                "entries": catalog.entries,
                "no_network_probes": true,
            })
        }
        Err(err) => json!({
            "loaded": false,
            "source": "harness_tools::skill_catalog",
            "workspace_root": workspace_root.display().to_string(),
            "entry_count": 0,
            "loadable_count": 0,
            "denied_count": 0,
            "disabled_count": 0,
            "malformed_count": 0,
            "shadowed_count": 0,
            "entries": [],
            "reason": err.to_string(),
            "no_network_probes": true,
        }),
    }
}

fn session_export_native_tool_catalog_summary(config: &HarnessConfig) -> Value {
    let registry = coordinator_registry_with_mcp_and_editing(
        config.permissions.shell_allowlist.clone(),
        Default::default(),
        EditingToolSurfaceConfig {
            hashline_edit: config.hashline_edit,
        },
    );
    let catalog = native_tool_catalog_entries(&registry);
    let required_v1_tools = [
        "session_list",
        "session_read",
        "session_search",
        "session_info",
        "background_cancel",
        "team_list",
        "ast_grep_search",
    ];
    json!({
        "loaded": true,
        "source": "harness_tools::tool_catalog",
        "tool_count": catalog.len(),
        "required_v1_tools": required_v1_tools,
        "missing_required_v1_tools": required_v1_tools
            .into_iter()
            .filter(|tool_id| !catalog.iter().any(|entry| entry.canonical_id == *tool_id))
            .collect::<Vec<_>>(),
        "tools": catalog,
        "ast_grep_adapter": ast_grep_adapter_readiness(),
        "no_network_probes": true,
    })
}

fn session_export_session_tool_readiness(config: &HarnessConfig) -> Value {
    let registry = coordinator_registry_with_mcp_and_editing(
        config.permissions.shell_allowlist.clone(),
        Default::default(),
        EditingToolSurfaceConfig {
            hashline_edit: config.hashline_edit,
        },
    );
    let ids = registry.tool_ids().into_iter().collect::<BTreeSet<_>>();
    let session_tools = [
        "session_list",
        "session_read",
        "session_search",
        "session_info",
    ];
    json!({
        "available": session_tools.iter().all(|tool_id| ids.contains(*tool_id)),
        "source": "event_replay",
        "redacted_by_default": true,
        "side_effect_free": true,
        "tools": session_tools,
        "missing": session_tools
            .into_iter()
            .filter(|tool_id| !ids.contains(*tool_id))
            .collect::<Vec<_>>(),
        "no_network_probes": true,
    })
}

fn openai_compatible_provider_summary(
    id: &str,
    provider: &OpenAiCompatibleProviderConfig,
) -> Value {
    json!({
        "id": id,
        "type": "openai_compatible",
        "name": provider.name.as_deref(),
        "base_url": provider.base_url.as_str(),
        "model_count": provider.models.len(),
        "models": provider.models.keys().cloned().collect::<Vec<_>>(),
        "credentials": provider_credentials_summary(provider),
        "api_key_env": provider.api_key_env.clone(),
        "timeout_ms": provider.timeout_ms,
        "header_count": provider.headers.len(),
    })
}

fn anthropic_provider_summary(id: &str, provider: &AnthropicProviderConfig) -> Value {
    json!({
        "id": id,
        "type": "anthropic_messages",
        "name": provider.name.as_deref(),
        "base_url": provider.base_url.as_str(),
        "model_count": provider.models.len(),
        "models": provider.models.keys().cloned().collect::<Vec<_>>(),
        "credentials": anthropic_credentials_summary(provider),
        "api_key_env": provider.api_key_env.clone(),
        "timeout_ms": provider.timeout_ms,
        "header_count": provider.headers.len(),
    })
}

fn provider_credentials_summary(provider: &OpenAiCompatibleProviderConfig) -> &'static str {
    if !provider.api_key.trim().is_empty() {
        "inline_redacted"
    } else if !provider.api_key_env.is_empty() {
        "env_reference"
    } else {
        "missing"
    }
}

fn anthropic_credentials_summary(provider: &AnthropicProviderConfig) -> &'static str {
    if !provider.api_key.trim().is_empty() {
        "inline_redacted"
    } else if !provider.api_key_env.is_empty() {
        "env_reference"
    } else {
        "missing"
    }
}

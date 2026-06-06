use std::path::Path;

use harness_core::config::{HarnessConfig, PermissionMode, ProviderConfig};
use harness_core::model_resolution::{resolve_model, ModelResolutionInput};
use harness_tools::{discover_skill_catalog_with_config, SkillCatalogStatus};
use serde_json::{json, Value};

use crate::dynamic_prompt;

pub(super) fn attach_doctor_model_metadata(
    config: &HarnessConfig,
    workspace_root: &Path,
    value: &mut Value,
) {
    let Some(model) = value.get_mut("model").and_then(Value::as_object_mut) else {
        return;
    };
    let provider_id = model
        .get("provider")
        .and_then(Value::as_str)
        .map(str::to_string);
    let model_id = model
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string);
    let provider_id_ref = provider_id.as_deref();
    let model_id_ref = model_id.as_deref();
    model.insert(
        "tool_call_support".to_string(),
        tool_call_support_metadata(config, provider_id_ref, model_id_ref),
    );
    model.insert(
        "prompt_family_asset".to_string(),
        prompt_family_asset_metadata(config, workspace_root, provider_id_ref, model_id_ref),
    );
}

fn tool_call_support_metadata(
    config: &HarnessConfig,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> Value {
    let Some(provider_id) = provider_id else {
        return json!({
            "status": "unknown",
            "supports_tool_calls": Value::Null,
            "source": "model_resolution_unavailable",
            "no_network_probes": true,
        });
    };
    let Some(model_id) = model_id else {
        return json!({
            "status": "unknown",
            "supports_tool_calls": Value::Null,
            "source": "model_resolution_unavailable",
            "no_network_probes": true,
        });
    };
    let Some(provider) = config.providers.get(provider_id) else {
        return json!({
            "status": "unknown",
            "supports_tool_calls": Value::Null,
            "source": "provider_model_metadata_missing",
            "no_network_probes": true,
        });
    };
    let ProviderConfig::OpenAiCompatible(provider) = provider;
    let Some(model) = provider.models.get(model_id) else {
        return json!({
            "status": "unknown",
            "supports_tool_calls": Value::Null,
            "source": "provider_model_metadata_missing",
            "no_network_probes": true,
        });
    };

    match model.metadata.supports_tool_calls {
        Some(true) => json!({
            "status": "supported",
            "supports_tool_calls": true,
            "source": "provider_model_metadata",
            "no_network_probes": true,
        }),
        Some(false) => json!({
            "status": "unsupported",
            "supports_tool_calls": false,
            "source": "provider_model_metadata",
            "no_network_probes": true,
        }),
        None => json!({
            "status": "unknown",
            "supports_tool_calls": Value::Null,
            "source": "provider_model_metadata",
            "no_network_probes": true,
        }),
    }
}

fn prompt_family_asset_metadata(
    config: &HarnessConfig,
    workspace_root: &Path,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> Value {
    let Some(provider_id) = provider_id else {
        return json!({
            "status": "unknown",
            "source": "model_resolution_unavailable",
            "no_network_probes": true,
        });
    };
    let Some(model_id) = model_id else {
        return json!({
            "status": "unknown",
            "source": "model_resolution_unavailable",
            "no_network_probes": true,
        });
    };
    let Some(provider) = config.providers.get(provider_id) else {
        return json!({
            "status": "unknown",
            "source": "provider_model_metadata_missing",
            "no_network_probes": true,
        });
    };
    let ProviderConfig::OpenAiCompatible(provider) = provider;
    let Some(model) = provider.models.get(model_id) else {
        return json!({
            "status": "unknown",
            "source": "provider_model_metadata_missing",
            "no_network_probes": true,
        });
    };
    let resolution = resolve_model(ModelResolutionInput {
        provider: provider_id,
        model: model_id,
        metadata_family: model.metadata.family.as_deref(),
        input_modalities: &model.modalities.input,
        context_window_tokens: model.metadata.context_window_tokens.or(model.limit.context),
        max_input_tokens: model.max_input_tokens.or(model.limit.input),
        max_output_tokens: model.max_output_tokens.or(model.limit.output),
        supports_tool_calls: model.metadata.supports_tool_calls,
        supports_reasoning_summaries: model.metadata.supports_reasoning_summaries,
    });
    let status =
        dynamic_prompt::prompt_family_asset_status(resolution.prompt_family, workspace_root);
    json!({
        "family": status.family,
        "status": status.status,
        "source": status.source,
        "path": status.path.map(|path| path.display().to_string()),
        "warning": status.warning,
        "no_network_probes": true,
    })
}

pub(super) fn skill_readiness_metadata(config: &HarnessConfig, workspace_root: &Path) -> Value {
    let configured_permission_patterns = config
        .skills
        .permissions
        .iter()
        .map(|(pattern, mode)| {
            json!({
                "pattern": pattern,
                "permission": permission_mode_label(Some(mode)),
            })
        })
        .collect::<Vec<_>>();
    let skill_tool_profiles = config
        .agents
        .iter()
        .filter_map(|(profile, agent)| {
            agent
                .tools
                .iter()
                .any(|tool| tool == "skill")
                .then_some(profile.as_str())
        })
        .collect::<Vec<_>>();
    let root_count = config.skills.project_roots.len() + config.skills.global_roots.len();

    let catalog = discover_skill_catalog_with_config(workspace_root, &config.skills);
    let (catalog, readiness, catalog_status) = match catalog {
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
            (
                json!(catalog),
                json!({
                    "entry_count": catalog.entries.len(),
                    "loadable_count": loadable_count,
                    "denied_count": denied_count,
                    "disabled_count": disabled_count,
                    "malformed_count": malformed_count,
                    "shadowed_count": shadowed_count,
                }),
                "available",
            )
        }
        Err(err) => (
            json!({ "entries": [] }),
            json!({
                "entry_count": 0,
                "loadable_count": 0,
                "denied_count": 0,
                "disabled_count": 0,
                "malformed_count": 0,
                "shadowed_count": 0,
                "error": err.to_string(),
            }),
            "unavailable",
        ),
    };

    json!({
        "status": if root_count == 0 { "no_roots_configured" } else { "configured" },
        "catalog_status": catalog_status,
        "catalog_source": "harness_tools::skill_catalog",
        "project_roots": config.skills.project_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "global_roots": config.skills.global_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "walk_to_git_root": config.skills.walk_to_git_root,
        "permission_rules": configured_permission_patterns,
        "skill_tool_profiles": skill_tool_profiles,
        "workspace_root": workspace_root.display().to_string(),
        "catalog": catalog,
        "readiness": readiness,
        "no_network_probes": true,
    })
}

fn permission_mode_label(mode: Option<&PermissionMode>) -> Value {
    match mode {
        Some(PermissionMode::Allow) => json!("allow"),
        Some(PermissionMode::Ask) => json!("ask"),
        Some(PermissionMode::Deny) => json!("deny"),
        None => Value::Null,
    }
}

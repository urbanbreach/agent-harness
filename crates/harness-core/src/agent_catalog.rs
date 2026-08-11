// allow: SIZE_OK — serialized agent metadata contract and its resolver stay colocated
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{
    resolve_model_selection, AgentMode, HarnessConfig, PermissionMode, ProfileConfig,
    ResolvedModelTarget,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCatalog {
    pub entries: Vec<AgentCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub mode: AgentMode,
    pub prompt: AgentPromptCatalogMetadata,
    pub model: AgentModelCatalogMetadata,
    pub toolset: Vec<String>,
    pub permission_posture: AgentPermissionPosture,
    pub skills: AgentSkillCatalogMetadata,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readiness_warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPromptCatalogMetadata {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentModelCatalogMetadata {
    pub model_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_chain: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPermissionPosture {
    #[serde(rename = "fallback")]
    pub fallback: Option<String>,
    pub edit: String,
    pub bash: String,
    pub question: String,
    pub task: String,
    pub webfetch: String,
    pub websearch: String,
    pub codesearch: String,
    pub lsp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSkillCatalogMetadata {
    pub tool_enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configured_permission_patterns: Vec<String>,
}

impl AgentCatalog {
    pub fn get(&self, id: &str) -> Option<&AgentCatalogEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn entries_by_id(&self) -> BTreeMap<String, AgentCatalogEntry> {
        self.entries
            .iter()
            .cloned()
            .map(|entry| (entry.id.clone(), entry))
            .collect()
    }
}

pub fn resolve_agent_catalog(config: &HarnessConfig) -> AgentCatalog {
    AgentCatalog {
        entries: config
            .agents
            .iter()
            .map(|(id, profile)| resolve_agent_catalog_entry(config, id, profile))
            .collect(),
    }
}

pub fn resolve_agent_catalog_entry(
    config: &HarnessConfig,
    id: &str,
    profile: &ProfileConfig,
) -> AgentCatalogEntry {
    let prompt = prompt_metadata(profile);
    let model = model_metadata(config, profile);
    let mut readiness_warnings = Vec::new();
    if prompt.status == "missing" {
        readiness_warnings.push("prompt_asset_missing".to_string());
    }
    if model.resolution_error.is_some() {
        readiness_warnings.push("model_resolution_failed".to_string());
    }

    AgentCatalogEntry {
        id: id.to_string(),
        display_name: display_name(id),
        description: profile.description.clone(),
        mode: profile.mode,
        prompt,
        model,
        toolset: profile.tools.clone(),
        permission_posture: permission_posture(config, profile),
        skills: AgentSkillCatalogMetadata {
            tool_enabled: profile.tools.iter().any(|tool| tool == "skill"),
            configured_permission_patterns: config.skills.permissions.keys().cloned().collect(),
        },
        readiness_warnings,
    }
}

fn display_name(id: &str) -> String {
    match id {
        "default" => "Harness",
        "explore" => "Explore",
        "general" => "General",
        "librarian" => "Librarian",
        _ => id,
    }
    .to_string()
}

fn prompt_metadata(profile: &ProfileConfig) -> AgentPromptCatalogMetadata {
    match profile
        .system_prompt
        .as_deref()
        .filter(|prompt| !prompt.trim().is_empty())
    {
        Some(_) => AgentPromptCatalogMetadata {
            status: "available".to_string(),
            source: Some("configured".to_string()),
        },
        None => AgentPromptCatalogMetadata {
            status: "missing".to_string(),
            source: None,
        },
    }
}

fn model_metadata(config: &HarnessConfig, profile: &ProfileConfig) -> AgentModelCatalogMetadata {
    match resolve_model_selection(config, &profile.model_ref, profile.variant.as_deref()) {
        Ok(selection) => AgentModelCatalogMetadata {
            model_ref: selection.primary.model_ref.clone(),
            provider: Some(selection.primary.provider.clone()),
            model: selection.primary.model.clone(),
            variant: selection.primary.variant.clone(),
            fallback_chain: selection.fallback.iter().map(model_target_label).collect(),
            resolution_error: None,
        },
        Err(error) => {
            let (_, model, variant) = split_model_ref(&profile.model_ref);
            AgentModelCatalogMetadata {
                model_ref: profile.model_ref.clone(),
                provider: None,
                model,
                variant: profile.variant.clone().or(variant),
                fallback_chain: Vec::new(),
                resolution_error: Some(error.to_string()),
            }
        }
    }
}

fn model_target_label(target: &ResolvedModelTarget) -> String {
    match target.variant.as_deref() {
        Some(variant) => format!("{}:{variant}", target.model_ref),
        None => target.model_ref.clone(),
    }
}

fn split_model_ref(model_ref: &str) -> (Option<String>, String, Option<String>) {
    let mut slash_parts = model_ref.split('/');
    let first = slash_parts.next();
    let second = slash_parts.next();
    let third = slash_parts.next();
    if let (Some(provider), Some(model)) = (first, second) {
        return (
            Some(provider.to_string()),
            model.to_string(),
            third.map(str::to_string),
        );
    }

    let mut colon_parts = model_ref.splitn(2, ':');
    match (colon_parts.next(), colon_parts.next()) {
        (Some(provider), Some(model)) => (Some(provider.to_string()), model.to_string(), None),
        _ => (None, model_ref.to_string(), None),
    }
}

fn permission_posture(config: &HarnessConfig, profile: &ProfileConfig) -> AgentPermissionPosture {
    let permissions = profile.permissions.as_ref();
    AgentPermissionPosture {
        fallback: permissions
            .and_then(|value| value.fallback.as_ref())
            .or(config.permissions.fallback.as_ref())
            .map(permission_mode_label),
        edit: permission_mode_label(
            permissions
                .and_then(|value| value.edit.as_ref())
                .or(permissions.and_then(|value| value.fallback.as_ref()))
                .unwrap_or(&config.permissions.defaults.edit),
        ),
        bash: permission_mode_label(
            permissions
                .and_then(|value| value.shell.as_ref())
                .or(permissions.and_then(|value| value.fallback.as_ref()))
                .unwrap_or(&config.permissions.defaults.shell),
        ),
        question: permission_mode_label(
            permissions
                .and_then(|value| value.question.as_ref())
                .or(permissions.and_then(|value| value.fallback.as_ref()))
                .or(config.permissions.defaults.question.as_ref())
                .or(config.permissions.fallback.as_ref())
                .unwrap_or(&PermissionMode::Ask),
        ),
        task: permission_mode_label(
            permissions
                .and_then(|value| value.task.as_ref())
                .or(permissions.and_then(|value| value.fallback.as_ref()))
                .or(config.permissions.defaults.task.as_ref())
                .or(config.permissions.fallback.as_ref())
                .unwrap_or(&PermissionMode::Allow),
        ),
        webfetch: permission_mode_label(
            permissions
                .and_then(|value| value.webfetch.as_ref())
                .or(permissions.and_then(|value| value.network.as_ref()))
                .or(permissions.and_then(|value| value.fallback.as_ref()))
                .or(config.permissions.defaults.webfetch.as_ref())
                .unwrap_or(&config.permissions.defaults.network),
        ),
        websearch: permission_mode_label(
            permissions
                .and_then(|value| value.websearch.as_ref())
                .or(permissions.and_then(|value| value.network.as_ref()))
                .or(permissions.and_then(|value| value.fallback.as_ref()))
                .or(config.permissions.defaults.websearch.as_ref())
                .unwrap_or(&config.permissions.defaults.network),
        ),
        codesearch: permission_mode_label(
            permissions
                .and_then(|value| value.codesearch.as_ref())
                .or(permissions.and_then(|value| value.network.as_ref()))
                .or(permissions.and_then(|value| value.fallback.as_ref()))
                .or(config.permissions.defaults.codesearch.as_ref())
                .unwrap_or(&config.permissions.defaults.network),
        ),
        lsp: permission_mode_label(
            permissions
                .and_then(|value| value.lsp.as_ref())
                .or(permissions.and_then(|value| value.fallback.as_ref()))
                .or(config.permissions.defaults.lsp.as_ref())
                .or(config.permissions.fallback.as_ref())
                .unwrap_or(&PermissionMode::Allow),
        ),
    }
}

fn permission_mode_label(mode: &PermissionMode) -> String {
    match mode {
        PermissionMode::Allow => "allow".to_string(),
        PermissionMode::Ask => "ask".to_string(),
        PermissionMode::Deny => "deny".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_agent_catalog;
    use crate::config::load_config_from_str;
    use crate::UnwrapOrAbort;

    #[test]
    fn catalog_contains_primary_and_named_subagents() {
        // Given
        let config = load_config_from_str(
            r#"
            {
              provider: {
                mock: {
                  type: "openai_compatible",
                  options: {
                    baseURL: "http://127.0.0.1:8317/v1",
                    apiKey: "test-key"
                  },
                  models: { model: { name: "Mock model" } }
                }
              },
              model: "mock/model",
              agent: {
                default: {
                  system_prompt: "Do the work",
                  tools: ["task", "read"]
                }
              },
              permission: "allow"
            }
            "#,
        )
        .unwrap_or_abort();

        // When
        let catalog = resolve_agent_catalog(&config);

        // Then
        assert_eq!(
            catalog
                .entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["default", "explore", "general", "librarian"]
        );
        let agent = catalog.get("default").unwrap_or_abort();
        assert_eq!(agent.display_name, "Harness");
        assert_eq!(agent.mode, crate::config::AgentMode::Primary);
        assert_eq!(agent.model.provider.as_deref(), Some("mock"));
        assert_eq!(agent.toolset, ["task", "read"]);
        assert_eq!(agent.permission_posture.task, "allow");
        for name in ["explore", "general", "librarian"] {
            assert_eq!(
                catalog.get(name).unwrap_or_abort().mode,
                crate::config::AgentMode::Subagent
            );
        }
    }
}

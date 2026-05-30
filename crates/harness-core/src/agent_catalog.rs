use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{
    resolve_model_selection, AgentMode, HarnessConfig, PermissionMode, ProfileConfig,
    ResolvedModelTarget,
};
use crate::coord::{
    task_category_fallback_chain, TASK_CATEGORY_FALLBACK_DISABLED_PARENT_PROFILES,
    TASK_CATEGORY_FALLBACK_PROFILE,
};

pub const SHIPPED_PRIMARY_PROFILES: &[&str] = &["build", "plan"];
pub const SHIPPED_SUBAGENTS: &[&str] = &["explore", "general"];
pub const SHIPPED_CATEGORY_ROUTES: &[&str] = &[
    "visual-engineering",
    "artistry",
    "ultrabrain",
    "deep",
    "quick",
    "unspecified-low",
    "unspecified-high",
    "writing",
];
pub const SHIPPED_HIDDEN_PROFILES: &[&str] = &["title", "summary", "compaction"];

const DISPLAY_ORDER: &[&str] = &[
    "build",
    "plan",
    "explore",
    "general",
    "visual-engineering",
    "artistry",
    "ultrabrain",
    "deep",
    "quick",
    "unspecified-low",
    "unspecified-high",
    "writing",
    "title",
    "summary",
    "compaction",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCatalog {
    pub entries: Vec<AgentCatalogEntry>,
    pub category_fallback: CategoryFallbackCatalog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub role: AgentCatalogRole,
    pub mode: AgentMode,
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_binding: Option<String>,
    pub display_order: usize,
    pub prompt: AgentPromptCatalogMetadata,
    pub model: AgentModelCatalogMetadata,
    pub toolset: Vec<String>,
    pub permission_posture: AgentPermissionPosture,
    pub skills: AgentSkillCatalogMetadata,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readiness_warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCatalogRole {
    Primary,
    Subagent,
    Category,
    Hidden,
    Profile,
    All,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryFallbackCatalog {
    pub unknown_category_profile: String,
    pub disabled_parent_profiles: Vec<String>,
    pub policy_source: String,
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
    let mut entries = config
        .agents
        .iter()
        .map(|(id, profile)| resolve_agent_catalog_entry(config, id, profile))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| (entry.display_order, entry.id.clone()));

    AgentCatalog {
        entries,
        category_fallback: CategoryFallbackCatalog {
            unknown_category_profile: TASK_CATEGORY_FALLBACK_PROFILE.to_string(),
            disabled_parent_profiles: TASK_CATEGORY_FALLBACK_DISABLED_PARENT_PROFILES
                .iter()
                .map(|profile| profile.to_string())
                .collect(),
            policy_source: "harness_core::coord::task_category_fallback_profile".to_string(),
        },
    }
}

pub fn resolve_agent_catalog_entry(
    config: &HarnessConfig,
    id: &str,
    profile: &ProfileConfig,
) -> AgentCatalogEntry {
    let role = agent_catalog_role(id, profile);
    let prompt = prompt_metadata(id, profile);
    let model = model_metadata(config, profile);
    let permission_posture = permission_posture(config, profile);
    let toolset = profile.tools.clone();
    let mut readiness_warnings = Vec::new();

    if prompt.status == "missing" {
        readiness_warnings.push("prompt_asset_missing".to_string());
    }
    if model.resolution_error.is_some() {
        readiness_warnings.push("model_resolution_failed".to_string());
    }
    if matches!(role, AgentCatalogRole::Category) {
        let task_permission = profile
            .permissions
            .as_ref()
            .and_then(|permissions| permissions.task.as_ref());
        if !matches!(task_permission, Some(PermissionMode::Deny)) {
            readiness_warnings.push("category_route_can_redelegate".to_string());
        }
    }

    AgentCatalogEntry {
        id: id.to_string(),
        display_name: profile
            .name
            .clone()
            .unwrap_or_else(|| display_name_from_id(id)),
        description: profile.description.clone(),
        role,
        mode: profile.mode,
        hidden: profile.hidden,
        category_binding: matches!(role, AgentCatalogRole::Category).then(|| id.to_string()),
        display_order: display_order(id),
        prompt,
        model,
        toolset,
        permission_posture,
        skills: AgentSkillCatalogMetadata {
            tool_enabled: profile.tools.iter().any(|tool| tool == "skill"),
            configured_permission_patterns: config.skills.permissions.keys().cloned().collect(),
        },
        readiness_warnings,
    }
}

pub fn agent_catalog_role(id: &str, profile: &ProfileConfig) -> AgentCatalogRole {
    if profile.hidden || SHIPPED_HIDDEN_PROFILES.contains(&id) {
        return AgentCatalogRole::Hidden;
    }
    if SHIPPED_CATEGORY_ROUTES.contains(&id) {
        return AgentCatalogRole::Category;
    }
    if SHIPPED_PRIMARY_PROFILES.contains(&id) || profile.mode == AgentMode::Primary {
        return AgentCatalogRole::Primary;
    }
    if SHIPPED_SUBAGENTS.contains(&id) || profile.mode == AgentMode::Subagent {
        return AgentCatalogRole::Subagent;
    }
    if profile.mode == AgentMode::All {
        return AgentCatalogRole::All;
    }
    AgentCatalogRole::Profile
}

pub fn category_fallback_chain(category: Option<&str>) -> Vec<String> {
    task_category_fallback_chain(category)
}

fn prompt_metadata(id: &str, profile: &ProfileConfig) -> AgentPromptCatalogMetadata {
    if profile.system_prompt.as_deref().is_some_and(non_empty) {
        return AgentPromptCatalogMetadata {
            status: "available".to_string(),
            source: Some("configured_or_discovered".to_string()),
        };
    }
    if bundled_prompt_available(id) {
        return AgentPromptCatalogMetadata {
            status: "available".to_string(),
            source: Some("bundled_shipped_asset".to_string()),
        };
    }
    AgentPromptCatalogMetadata {
        status: "missing".to_string(),
        source: None,
    }
}

fn bundled_prompt_available(id: &str) -> bool {
    SHIPPED_PRIMARY_PROFILES.contains(&id)
        || SHIPPED_SUBAGENTS.contains(&id)
        || SHIPPED_CATEGORY_ROUTES.contains(&id)
}

fn model_metadata(config: &HarnessConfig, profile: &ProfileConfig) -> AgentModelCatalogMetadata {
    match resolve_model_selection(config, &profile.model_ref, profile.variant.as_deref()) {
        Ok(selection) => AgentModelCatalogMetadata {
            model_ref: selection.primary.model_ref.clone(),
            provider: Some(selection.primary.provider.clone()),
            model: selection.primary.model.clone(),
            variant: selection.primary.variant.clone(),
            fallback_chain: selection
                .fallback
                .iter()
                .map(model_target_label)
                .collect::<Vec<_>>(),
            resolution_error: None,
        },
        Err(err) => {
            let (_, model, variant) = split_model_ref(&profile.model_ref);
            AgentModelCatalogMetadata {
                model_ref: profile.model_ref.clone(),
                provider: None,
                model,
                variant: profile.variant.clone().or(variant),
                fallback_chain: Vec::new(),
                resolution_error: Some(err.to_string()),
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

fn display_order(id: &str) -> usize {
    DISPLAY_ORDER
        .iter()
        .position(|candidate| *candidate == id)
        .unwrap_or(DISPLAY_ORDER.len() + 100)
}

fn display_name_from_id(id: &str) -> String {
    id.split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let mut chars = part.chars();
            let first = chars.next()?;
            Some(format!("{}{}", first.to_uppercase(), chars.as_str()))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn non_empty(value: &str) -> bool {
    !value.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_agent_catalog, AgentCatalogRole, SHIPPED_CATEGORY_ROUTES, SHIPPED_PRIMARY_PROFILES,
        SHIPPED_SUBAGENTS,
    };
    use crate::config::{load_config_from_str, AgentMode, HarnessConfig};

    fn config() -> HarnessConfig {
        load_config_from_str(
            r#"
            {
              provider: {
                mock: {
                  type: "openai_compatible",
                  options: {
                    baseURL: "http://127.0.0.1:8317/v1",
                    apiKey: "test-key",
                  },
                  models: {
                    model: {
                      name: "Mock model"
                    }
                  }
                }
              },
              model: "mock/model",
              small_model: "mock/model",
              agent: {
                build: { system_prompt: "Build work" },
                plan: { system_prompt: "Plan work" },
              },
              default_agent: "build",
              permission: {
                edit: "allow",
                bash: "allow",
                question: "allow",
                task: "allow",
                webfetch: "allow",
                websearch: "allow",
                codesearch: "allow",
                lsp: "allow"
              }
            }
            "#,
        )
        .expect("agent catalog fixture config should parse")
    }

    #[test]
    fn catalog_resolves_shipped_roles_and_hidden_profiles() {
        let catalog = resolve_agent_catalog(&config());

        for id in SHIPPED_PRIMARY_PROFILES {
            assert_eq!(
                catalog.get(id).expect("primary").role,
                AgentCatalogRole::Primary
            );
        }
        for id in SHIPPED_SUBAGENTS {
            assert_eq!(
                catalog.get(id).expect("subagent").role,
                AgentCatalogRole::Subagent
            );
        }
        for id in SHIPPED_CATEGORY_ROUTES {
            let entry = catalog.get(id).expect("category");
            assert_eq!(entry.role, AgentCatalogRole::Category);
            assert_eq!(entry.category_binding.as_deref(), Some(*id));
        }
        assert_eq!(
            catalog.get("title").expect("hidden title").role,
            AgentCatalogRole::Hidden
        );
        assert_eq!(
            catalog.get("summary").expect("hidden summary").role,
            AgentCatalogRole::Hidden
        );
        assert_eq!(
            catalog.get("compaction").expect("hidden compaction").role,
            AgentCatalogRole::Hidden
        );
    }

    #[test]
    fn catalog_reports_prompt_model_tools_permissions_and_fallback_policy() {
        let catalog = resolve_agent_catalog(&config());
        let build = catalog.get("build").expect("build route");

        assert_eq!(build.prompt.status, "available");
        assert_eq!(build.model.provider.as_deref(), Some("mock"));
        assert_eq!(build.model.model, "model");
        assert!(build.toolset.iter().any(|tool| tool == "task"));
        assert_eq!(build.permission_posture.task, "allow");
        assert_eq!(
            catalog.category_fallback.unknown_category_profile.as_str(),
            "general"
        );
    }

    #[test]
    fn catalog_keeps_custom_all_profile_visible() {
        let mut fixture = config();
        let mut custom = fixture.agents.get("build").expect("build").clone();
        custom.mode = AgentMode::All;
        custom.hidden = false;
        fixture.agents.insert("ops".to_string(), custom);

        assert_eq!(
            resolve_agent_catalog(&config())
                .get("build")
                .expect("build")
                .role,
            AgentCatalogRole::Primary
        );
        assert_eq!(
            resolve_agent_catalog(&fixture)
                .get("ops")
                .expect("ops")
                .role,
            AgentCatalogRole::All
        );
    }
}

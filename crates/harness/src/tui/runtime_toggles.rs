use std::path::Path;

use harness_core::config::{AgentMode, HarnessConfig};
use harness_tools::{discover_skill_catalog, SkillCatalogEntry};
use harness_tui::app::{ToggleEntryConfig, ToggleEntryKind, TogglesConfig};

pub(super) fn runtime_toggles_config(
    config: Option<&HarnessConfig>,
    workspace_root: &Path,
) -> TogglesConfig {
    let mut toggles = TogglesConfig::default();
    let Some(config) = config else {
        return toggles;
    };

    let skill_catalog_entries = discover_skill_catalog(workspace_root)
        .ok()
        .map(|catalog| catalog.entries)
        .unwrap_or_default();

    for (name, profile) in &config.agents {
        if profile.hidden {
            continue;
        }
        if !matches!(profile.mode, AgentMode::Subagent) {
            toggles.entries.push(ToggleEntryConfig {
                kind: ToggleEntryKind::Agent { name: name.clone() },
                label: name.clone(),
                description: profile.description.clone(),
                enabled: true,
            });
        }
        if !matches!(profile.mode, AgentMode::Primary) {
            toggles.entries.push(ToggleEntryConfig {
                kind: ToggleEntryKind::Subagent { name: name.clone() },
                label: name.clone(),
                description: profile.description.clone(),
                enabled: true,
            });
        }
        for tool in &profile.tools {
            toggles.entries.push(ToggleEntryConfig {
                kind: ToggleEntryKind::AgentTool {
                    agent: name.clone(),
                    tool: tool.clone(),
                },
                label: format!("{name}: {tool}"),
                description: format!("Configured tool `{tool}` for `{name}`"),
                enabled: true,
            });
        }
        if profile.tools.iter().any(|tool| tool == "skill") {
            let skill_entries = if skill_catalog_entries.is_empty() {
                fallback_skill_toggle_entries(config)
            } else {
                skill_catalog_entries
                    .iter()
                    .map(skill_catalog_toggle_entry)
                    .collect()
            };
            for skill in skill_entries {
                toggles.entries.push(ToggleEntryConfig {
                    kind: ToggleEntryKind::AgentSkill {
                        agent: name.clone(),
                        skill: skill.id,
                    },
                    label: format!("{name}: {}", skill.label),
                    description: skill.description,
                    enabled: skill.enabled,
                });
            }
        }
    }

    for (index, hook) in config.hooks.lifecycle.iter().enumerate() {
        let id = hook
            .id
            .clone()
            .unwrap_or_else(|| format!("{} #{index}", hook.event.as_str()));
        toggles.entries.push(ToggleEntryConfig {
            kind: ToggleEntryKind::Hook { id: id.clone() },
            label: id,
            description: format!("{} lifecycle hook", hook.event.as_str()),
            enabled: true,
        });
    }

    for (name, server) in &config.integrations.mcp.servers {
        toggles.entries.push(ToggleEntryConfig {
            kind: ToggleEntryKind::McpServer { name: name.clone() },
            label: name.clone(),
            description: "Configured MCP server state".to_string(),
            enabled: server.enabled(),
        });
    }

    toggles
}

struct SkillToggleEntry {
    id: String,
    label: String,
    description: String,
    enabled: bool,
}

fn skill_catalog_toggle_entry(entry: &SkillCatalogEntry) -> SkillToggleEntry {
    let mut description = format!(
        "{} skill `{}` from {} root {}",
        entry.status.as_str(),
        entry.name,
        entry.source_scope,
        entry.root_path.display()
    );
    if let Some(reason) = entry.reason.as_deref() {
        description.push_str(&format!(" ({reason})"));
    } else if !entry.description.is_empty() {
        description.push_str(&format!(": {}", entry.description));
    }

    SkillToggleEntry {
        id: entry.stable_id.clone(),
        label: entry.name.clone(),
        description,
        enabled: entry.loadable,
    }
}

fn fallback_skill_toggle_entries(config: &HarnessConfig) -> Vec<SkillToggleEntry> {
    if config.skills.permissions.is_empty() {
        return vec![SkillToggleEntry {
            id: "skill-loading".to_string(),
            label: "skill loading".to_string(),
            description: "Configured skill loading surface".to_string(),
            enabled: true,
        }];
    }

    config
        .skills
        .permissions
        .keys()
        .map(|pattern| SkillToggleEntry {
            id: format!("permission:{pattern}"),
            label: pattern.clone(),
            description: format!("Configured skill permission pattern `{pattern}`"),
            enabled: true,
        })
        .collect()
}

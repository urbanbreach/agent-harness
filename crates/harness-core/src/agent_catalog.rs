use std::collections::BTreeMap;

use serde::Serialize;

use crate::config::{resolve_model_selection, AgentMode, HarnessConfig, PermissionMode};

pub const PRIMARY_WORKFLOW_PROFILES: &[&str] = &["build", "plan", "discipline"];
pub const BUILTIN_SUBAGENTS: &[&str] = &[
    "explore",
    "general",
    "oracle",
    "librarian",
    "metis",
    "momus",
    "multimodal-looker",
    "sisyphus-junior",
    "atlas",
    "prometheus",
    "sisyphus",
    "hephaestus",
];
pub const CATEGORY_ROUTES: &[&str] = &[
    "visual-engineering",
    "artistry",
    "ultrabrain",
    "deep",
    "quick",
    "unspecified-low",
    "unspecified-high",
    "writing",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentCatalog {
    pub entries: Vec<AgentCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentCatalogEntry {
    pub name: String,
    pub role: String,
    pub mode: AgentMode,
    pub hidden: bool,
    pub display_order: usize,
    pub description: String,
    pub model_ref: String,
    pub resolved_model_ref: Option<String>,
    pub fallback_model_refs: Vec<String>,
    pub model_error: Option<String>,
    pub tools: Vec<String>,
    pub permissions: BTreeMap<String, String>,
    pub category_binding: Option<String>,
    pub can_redelegate: bool,
}

pub fn resolve_agent_catalog(config: &HarnessConfig) -> AgentCatalog {
    let entries = config
        .agents
        .iter()
        .enumerate()
        .map(|(config_order, (name, profile))| {
            let model =
                resolve_model_selection(config, &profile.model_ref, profile.variant.as_deref());
            let (resolved_model_ref, fallback_model_refs, model_error) = match model {
                Ok(selection) => (
                    Some(selection.primary.model_ref),
                    selection
                        .fallback
                        .into_iter()
                        .map(|target| target.model_ref)
                        .collect(),
                    None,
                ),
                Err(err) => (None, Vec::new(), Some(err.to_string())),
            };
            let permissions = profile
                .permissions
                .as_ref()
                .map(|permissions| {
                    BTreeMap::from([
                        (
                            "edit".to_string(),
                            permission_label(permissions.edit.as_ref()),
                        ),
                        (
                            "bash".to_string(),
                            permission_label(permissions.shell.as_ref()),
                        ),
                        (
                            "network".to_string(),
                            permission_label(permissions.network.as_ref()),
                        ),
                        (
                            "question".to_string(),
                            permission_label(permissions.question.as_ref()),
                        ),
                        (
                            "task".to_string(),
                            permission_label(permissions.task.as_ref()),
                        ),
                        (
                            "webfetch".to_string(),
                            permission_label(permissions.webfetch.as_ref()),
                        ),
                        (
                            "websearch".to_string(),
                            permission_label(permissions.websearch.as_ref()),
                        ),
                        (
                            "codesearch".to_string(),
                            permission_label(permissions.codesearch.as_ref()),
                        ),
                        (
                            "lsp".to_string(),
                            permission_label(permissions.lsp.as_ref()),
                        ),
                    ])
                })
                .unwrap_or_default();
            let can_redelegate = permissions
                .get("task")
                .is_some_and(|permission| permission == "allow");

            AgentCatalogEntry {
                name: name.clone(),
                role: agent_catalog_role(name).to_string(),
                mode: profile.mode,
                hidden: profile.hidden,
                display_order: agent_catalog_display_order(name, config_order),
                description: profile.description.clone(),
                model_ref: profile.model_ref.clone(),
                resolved_model_ref,
                fallback_model_refs,
                model_error,
                tools: profile.tools.clone(),
                permissions,
                category_binding: agent_catalog_category_binding(name),
                can_redelegate,
            }
        })
        .collect();
    AgentCatalog { entries }
}

pub fn agent_catalog_role(name: &str) -> &'static str {
    if PRIMARY_WORKFLOW_PROFILES.contains(&name) {
        "primary"
    } else if CATEGORY_ROUTES.contains(&name) {
        "category"
    } else if BUILTIN_SUBAGENTS.contains(&name) {
        "specialist"
    } else {
        "custom"
    }
}

pub fn agent_catalog_display_order(name: &str, config_order: usize) -> usize {
    if let Some(rank) = PRIMARY_WORKFLOW_PROFILES
        .iter()
        .position(|profile| *profile == name)
    {
        return rank;
    }
    if let Some(rank) = BUILTIN_SUBAGENTS
        .iter()
        .position(|profile| *profile == name)
    {
        return 1_000 + rank;
    }
    if let Some(rank) = CATEGORY_ROUTES.iter().position(|profile| *profile == name) {
        return 2_000 + rank;
    }
    10_000 + config_order
}

pub fn agent_catalog_category_binding(name: &str) -> Option<String> {
    CATEGORY_ROUTES.contains(&name).then(|| name.to_string())
}

fn permission_label(mode: Option<&PermissionMode>) -> String {
    match mode {
        Some(PermissionMode::Allow) => "allow",
        Some(PermissionMode::Ask) => "ask",
        Some(PermissionMode::Deny) => "deny",
        None => "inherit",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{resolve_agent_catalog, BUILTIN_SUBAGENTS, CATEGORY_ROUTES};
    use crate::config::load_config_from_str;

    #[test]
    fn catalog_marks_omo_specialists_and_category_routes() {
        let config = load_config_from_str(
            r#"
            {
              provider: {
                default: {
                  type: "openai_compatible",
                  baseURL: "http://127.0.0.1:8317/v1",
                  apiKey: "DUMMY",
                  models: { "gpt-5.4-mini": { name: "GPT-5.4 mini" } }
                }
              },
              model: "default/gpt-5.4-mini",
              permission: "ask"
            }
            "#,
        )
        .expect("config parses");

        let catalog = resolve_agent_catalog(&config);
        let oracle = catalog
            .entries
            .iter()
            .find(|entry| entry.name == "oracle")
            .expect("oracle profile is shipped");
        assert_eq!(oracle.role, "specialist");
        assert_eq!(oracle.permissions["edit"], "deny");

        for profile in BUILTIN_SUBAGENTS {
            let entry = catalog
                .entries
                .iter()
                .find(|entry| entry.name == *profile)
                .unwrap_or_else(|| panic!("{profile} profile is shipped"));
            assert_eq!(entry.role, "specialist");
        }

        for read_only_profile in [
            "oracle",
            "librarian",
            "metis",
            "momus",
            "prometheus",
            "multimodal-looker",
            "explore",
        ] {
            let entry = catalog
                .entries
                .iter()
                .find(|entry| entry.name == read_only_profile)
                .unwrap_or_else(|| panic!("{read_only_profile} profile is shipped"));
            assert_eq!(
                entry.permissions["edit"], "deny",
                "{read_only_profile} must remain read-only"
            );
            assert!(
                !entry.can_redelegate,
                "{read_only_profile} must not be able to redelegate"
            );
        }

        let deep = catalog
            .entries
            .iter()
            .find(|entry| entry.name == "deep")
            .expect("deep category route is shipped");
        assert_eq!(deep.role, "category");
        assert_eq!(deep.category_binding.as_deref(), Some("deep"));
        assert!(!deep.can_redelegate);

        for category in CATEGORY_ROUTES {
            let entry = catalog
                .entries
                .iter()
                .find(|entry| entry.name == *category)
                .unwrap_or_else(|| panic!("{category} category route is shipped"));
            assert_eq!(entry.role, "category");
            assert_eq!(entry.category_binding.as_deref(), Some(*category));
        }

        let mut primary = catalog
            .entries
            .iter()
            .filter(|entry| entry.role == "primary")
            .map(|entry| (entry.display_order, entry.name.as_str()))
            .collect::<Vec<_>>();
        primary.sort_unstable();
        assert_eq!(
            primary
                .into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>(),
            vec!["build", "plan", "discipline"]
        );
    }
}

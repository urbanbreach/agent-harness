use std::collections::BTreeMap;

use serde::Serialize;

use crate::config::{resolve_model_selection, AgentMode, HarnessConfig, PermissionMode};

pub const OPERATOR_AGENT_NAME: &str = "operator";
pub const PRIMARY_WORKFLOW_PROFILES: &[&str] = &[OPERATOR_AGENT_NAME];
pub const LEGACY_PRIMARY_PROFILE_ALIASES: &[&str] = &["build", "plan", "discipline"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SlashAgentDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub reasoning_effort: &'static str,
    pub posture: &'static str,
    pub model_class: &'static str,
    pub routing_role: &'static str,
    pub tools: &'static str,
    pub category: &'static str,
}

pub const SLASH_AGENT_DEFINITIONS: &[SlashAgentDefinition] = &[
    slash_agent(
        "executor",
        "Code implementation, refactoring, feature work",
        "medium",
        "deep-worker",
        "standard",
        "executor",
        "execution",
        "build",
    ),
    slash_agent(
        "team-executor",
        "Supervised team execution for conservative delivery lanes",
        "medium",
        "deep-worker",
        "frontier",
        "executor",
        "execution",
        "build",
    ),
    slash_agent(
        "explore",
        "Fast codebase search and file/symbol mapping",
        "low",
        "fast-lane",
        "fast",
        "specialist",
        "read-only",
        "build",
    ),
    slash_agent(
        "analyst",
        "Requirements clarity, acceptance criteria, hidden constraints",
        "medium",
        "frontier-orchestrator",
        "frontier",
        "leader",
        "analysis",
        "build",
    ),
    slash_agent(
        "planner",
        "Task sequencing, execution plans, risk flags",
        "medium",
        "frontier-orchestrator",
        "frontier",
        "leader",
        "analysis",
        "build",
    ),
    slash_agent(
        "architect",
        "System design, boundaries, interfaces, long-horizon tradeoffs",
        "high",
        "frontier-orchestrator",
        "frontier",
        "leader",
        "read-only",
        "build",
    ),
    slash_agent(
        "debugger",
        "Root-cause analysis, regression isolation, failure diagnosis",
        "high",
        "deep-worker",
        "standard",
        "executor",
        "analysis",
        "build",
    ),
    slash_agent(
        "verifier",
        "Completion evidence, claim validation, test adequacy",
        "high",
        "frontier-orchestrator",
        "standard",
        "leader",
        "analysis",
        "build",
    ),
    slash_agent(
        "style-reviewer",
        "Formatting, naming, idioms, lint conventions",
        "low",
        "fast-lane",
        "fast",
        "specialist",
        "read-only",
        "review",
    ),
    slash_agent(
        "quality-reviewer",
        "Logic defects, maintainability, anti-patterns",
        "medium",
        "frontier-orchestrator",
        "standard",
        "leader",
        "read-only",
        "review",
    ),
    slash_agent(
        "api-reviewer",
        "API contracts, versioning, backward compatibility",
        "medium",
        "frontier-orchestrator",
        "standard",
        "leader",
        "read-only",
        "review",
    ),
    slash_agent(
        "security-reviewer",
        "Vulnerabilities, trust boundaries, authn/authz",
        "medium",
        "frontier-orchestrator",
        "frontier",
        "leader",
        "read-only",
        "review",
    ),
    slash_agent(
        "performance-reviewer",
        "Hotspots, complexity, memory/latency optimization",
        "medium",
        "frontier-orchestrator",
        "standard",
        "leader",
        "read-only",
        "review",
    ),
    slash_agent(
        "code-reviewer",
        "Comprehensive review across all concerns",
        "high",
        "frontier-orchestrator",
        "frontier",
        "leader",
        "read-only",
        "review",
    ),
    slash_agent(
        "dependency-expert",
        "External SDK/API/package evaluation",
        "high",
        "frontier-orchestrator",
        "standard",
        "specialist",
        "analysis",
        "domain",
    ),
    slash_agent(
        "test-engineer",
        "Test strategy, coverage, flaky-test hardening",
        "medium",
        "deep-worker",
        "frontier",
        "executor",
        "execution",
        "domain",
    ),
    slash_agent(
        "quality-strategist",
        "Quality strategy, release readiness, risk assessment",
        "medium",
        "frontier-orchestrator",
        "standard",
        "leader",
        "analysis",
        "domain",
    ),
    slash_agent(
        "build-fixer",
        "Build/toolchain/type failures resolution",
        "high",
        "deep-worker",
        "standard",
        "executor",
        "execution",
        "domain",
    ),
    slash_agent(
        "designer",
        "UX/UI architecture, interaction design",
        "high",
        "deep-worker",
        "standard",
        "executor",
        "execution",
        "domain",
    ),
    slash_agent(
        "writer",
        "Documentation, migration notes, user guidance",
        "high",
        "fast-lane",
        "standard",
        "specialist",
        "execution",
        "domain",
    ),
    slash_agent(
        "qa-tester",
        "Interactive CLI/service runtime validation",
        "low",
        "deep-worker",
        "standard",
        "executor",
        "execution",
        "domain",
    ),
    slash_agent(
        "git-master",
        "Commit strategy, history hygiene, rebasing",
        "high",
        "deep-worker",
        "standard",
        "executor",
        "execution",
        "domain",
    ),
    slash_agent(
        "code-simplifier",
        "Simplifies recently modified code for clarity and consistency without changing behavior",
        "high",
        "deep-worker",
        "frontier",
        "executor",
        "execution",
        "domain",
    ),
    slash_agent(
        "researcher",
        "External documentation and reference research",
        "high",
        "fast-lane",
        "standard",
        "specialist",
        "analysis",
        "domain",
    ),
    slash_agent(
        "product-manager",
        "Problem framing, personas/JTBD, PRDs",
        "medium",
        "frontier-orchestrator",
        "standard",
        "leader",
        "analysis",
        "product",
    ),
    slash_agent(
        "ux-researcher",
        "Heuristic audits, usability, accessibility",
        "medium",
        "frontier-orchestrator",
        "standard",
        "specialist",
        "analysis",
        "product",
    ),
    slash_agent(
        "information-architect",
        "Taxonomy, navigation, findability",
        "low",
        "frontier-orchestrator",
        "standard",
        "specialist",
        "analysis",
        "product",
    ),
    slash_agent(
        "product-analyst",
        "Product metrics, funnel analysis, experiments",
        "low",
        "frontier-orchestrator",
        "standard",
        "specialist",
        "analysis",
        "product",
    ),
    slash_agent(
        "critic",
        "Plan/design critical challenge and review",
        "high",
        "frontier-orchestrator",
        "frontier",
        "leader",
        "read-only",
        "coordination",
    ),
    slash_agent(
        "vision",
        "Image/screenshot/diagram analysis",
        "low",
        "fast-lane",
        "frontier",
        "specialist",
        "read-only",
        "coordination",
    ),
];

#[expect(
    clippy::too_many_arguments,
    reason = "const data-table constructor mirrors SlashAgentDefinition fields"
)]
const fn slash_agent(
    name: &'static str,
    description: &'static str,
    reasoning_effort: &'static str,
    posture: &'static str,
    model_class: &'static str,
    routing_role: &'static str,
    tools: &'static str,
    category: &'static str,
) -> SlashAgentDefinition {
    SlashAgentDefinition {
        name,
        description,
        reasoning_effort,
        posture,
        model_class,
        routing_role,
        tools,
        category,
    }
}

pub const SLASH_AGENT_NAMES: &[&str] = &[
    "executor",
    "team-executor",
    "explore",
    "analyst",
    "planner",
    "architect",
    "debugger",
    "verifier",
    "style-reviewer",
    "quality-reviewer",
    "api-reviewer",
    "security-reviewer",
    "performance-reviewer",
    "code-reviewer",
    "dependency-expert",
    "test-engineer",
    "quality-strategist",
    "build-fixer",
    "designer",
    "writer",
    "qa-tester",
    "git-master",
    "code-simplifier",
    "researcher",
    "product-manager",
    "ux-researcher",
    "information-architect",
    "product-analyst",
    "critic",
    "vision",
];

pub const LEGACY_BUILTIN_SUBAGENTS: &[&str] = &[
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

pub const BUILTIN_SUBAGENTS: &[&str] = &[
    "executor",
    "team-executor",
    "explore",
    "analyst",
    "planner",
    "architect",
    "debugger",
    "verifier",
    "style-reviewer",
    "quality-reviewer",
    "api-reviewer",
    "security-reviewer",
    "performance-reviewer",
    "code-reviewer",
    "dependency-expert",
    "test-engineer",
    "quality-strategist",
    "build-fixer",
    "designer",
    "writer",
    "qa-tester",
    "git-master",
    "code-simplifier",
    "researcher",
    "product-manager",
    "ux-researcher",
    "information-architect",
    "product-analyst",
    "critic",
    "vision",
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

pub fn slash_agent_definitions() -> &'static [SlashAgentDefinition] {
    SLASH_AGENT_DEFINITIONS
}

pub fn slash_agent_definition(name: &str) -> Option<&'static SlashAgentDefinition> {
    SLASH_AGENT_DEFINITIONS
        .iter()
        .find(|definition| definition.name == name)
}

pub fn is_slash_agent_role(name: &str) -> bool {
    slash_agent_definition(name).is_some()
}

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
    } else if LEGACY_PRIMARY_PROFILE_ALIASES.contains(&name) {
        "compatibility"
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
    if let Some(rank) = LEGACY_PRIMARY_PROFILE_ALIASES
        .iter()
        .position(|profile| *profile == name)
    {
        return 100 + rank;
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
    use super::{
        is_slash_agent_role, resolve_agent_catalog, slash_agent_definition,
        slash_agent_definitions, BUILTIN_SUBAGENTS, CATEGORY_ROUTES,
        LEGACY_PRIMARY_PROFILE_ALIASES, OPERATOR_AGENT_NAME, SLASH_AGENT_NAMES,
    };
    use std::collections::BTreeSet;

    use crate::config::{load_config_from_str, AgentMode, HarnessConfig};

    const EXPECTED_SLASH_AGENT_NAMES: &[&str] = &[
        "executor",
        "team-executor",
        "explore",
        "analyst",
        "planner",
        "architect",
        "debugger",
        "verifier",
        "style-reviewer",
        "quality-reviewer",
        "api-reviewer",
        "security-reviewer",
        "performance-reviewer",
        "code-reviewer",
        "dependency-expert",
        "test-engineer",
        "quality-strategist",
        "build-fixer",
        "designer",
        "writer",
        "qa-tester",
        "git-master",
        "code-simplifier",
        "researcher",
        "product-manager",
        "ux-researcher",
        "information-architect",
        "product-analyst",
        "critic",
        "vision",
    ];

    fn catalog_test_config() -> HarnessConfig {
        load_config_from_str(
            r#"
            {
              provider: {
                default: {
                  type: "openai_compatible",
                  baseURL: "http://127.0.0.1:8317/v1",
                  apiKey: "DUMMY",
                  models: {
                    "gpt-5.5": { name: "GPT-5.5" },
                    "gpt-5.4-mini": { name: "GPT-5.4 mini" }
                  }
                }
              },
              model: "default/gpt-5.5",
              small_model: "default/gpt-5.4-mini",
              permission: "ask"
            }
            "#,
        )
        .expect("config parses")
    }

    #[test]
    fn catalog_marks_operator_default_and_subordinate_profile_routes() {
        let config = catalog_test_config();

        let catalog = resolve_agent_catalog(&config);
        let operator = catalog
            .entries
            .iter()
            .find(|entry| entry.name == OPERATOR_AGENT_NAME)
            .expect("operator profile is shipped");
        assert_eq!(operator.role, "primary");

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
            vec![OPERATOR_AGENT_NAME]
        );

        for legacy in LEGACY_PRIMARY_PROFILE_ALIASES {
            let entry = catalog
                .entries
                .iter()
                .find(|entry| entry.name == *legacy)
                .unwrap_or_else(|| panic!("{legacy} compatibility profile is shipped"));
            assert_eq!(entry.role, "compatibility");
            assert_ne!(entry.role, "primary");
        }
    }

    #[test]
    fn slash_agent_definitions_are_complete_and_shipped_with_metadata() {
        let config = catalog_test_config();
        let catalog = resolve_agent_catalog(&config);
        let definition_names = slash_agent_definitions()
            .iter()
            .map(|definition| definition.name)
            .collect::<Vec<_>>();

        assert_eq!(SLASH_AGENT_NAMES, EXPECTED_SLASH_AGENT_NAMES);
        assert_eq!(definition_names, EXPECTED_SLASH_AGENT_NAMES);
        assert_eq!(definition_names.len(), 30);
        assert_eq!(
            definition_names
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len(),
            definition_names.len(),
            "slash-agent names must stay unique"
        );

        for definition in slash_agent_definitions() {
            assert!(is_slash_agent_role(definition.name));
            assert_eq!(
                slash_agent_definition(definition.name).map(|found| found.name),
                Some(definition.name)
            );
            assert!(
                BUILTIN_SUBAGENTS.contains(&definition.name),
                "{} must be part of the shipped subordinate roster",
                definition.name
            );

            let profile = config
                .agents
                .get(definition.name)
                .unwrap_or_else(|| panic!("{} profile is shipped", definition.name));
            assert_eq!(profile.mode, AgentMode::Subagent);
            assert_eq!(profile.description, definition.description);

            let metadata = profile
                .options
                .get("slash_agent")
                .unwrap_or_else(|| panic!("{} has slash-agent metadata", definition.name));
            assert_eq!(metadata["name"], definition.name);
            assert_eq!(metadata["description"], definition.description);
            assert_eq!(metadata["reasoning_effort"], definition.reasoning_effort);
            assert_eq!(metadata["posture"], definition.posture);
            assert_eq!(metadata["model_class"], definition.model_class);
            assert_eq!(metadata["routing_role"], definition.routing_role);
            assert_eq!(metadata["tools"], definition.tools);
            assert_eq!(metadata["category"], definition.category);
            assert_eq!(metadata["command"], format!("/{}", definition.name));

            let entry = catalog
                .entries
                .iter()
                .find(|entry| entry.name == definition.name)
                .unwrap_or_else(|| panic!("{} appears in resolved catalog", definition.name));
            assert_eq!(entry.role, "specialist");
        }
    }

    #[test]
    fn slash_agent_permissions_preserve_read_only_and_redelegation_boundaries() {
        let config = catalog_test_config();
        let catalog = resolve_agent_catalog(&config);

        for definition in slash_agent_definitions() {
            let entry = catalog
                .entries
                .iter()
                .find(|entry| entry.name == definition.name)
                .unwrap_or_else(|| panic!("{} appears in resolved catalog", definition.name));

            if definition.tools == "read-only" {
                assert_eq!(
                    entry.permissions["edit"], "deny",
                    "{} read-only role must deny edits",
                    definition.name
                );
                assert_eq!(
                    entry.permissions["task"], "deny",
                    "{} read-only role must deny redelegation",
                    definition.name
                );
                assert!(
                    !entry.can_redelegate,
                    "{} read-only role must not be able to redelegate",
                    definition.name
                );
            }

            if definition.tools == "execution" {
                assert_eq!(
                    entry.permissions["edit"], "allow",
                    "{} execution role must allow edits",
                    definition.name
                );
                assert_eq!(
                    entry.permissions["bash"], "allow",
                    "{} execution role must allow bash",
                    definition.name
                );
                assert_eq!(
                    entry.permissions["task"], "deny",
                    "{} execution role must deny redelegation",
                    definition.name
                );
                assert!(
                    !entry.can_redelegate,
                    "{} execution role must not be able to redelegate",
                    definition.name
                );
            }
        }
    }
}

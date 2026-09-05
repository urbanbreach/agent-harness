//! Permission ruleset evaluation and tool visibility.
//!
//! Ruleset semantics (evaluate / disabled / visibleTools / fromConfig / merge):
//! - `evaluate` — last matching rule wins; default action is **ask**
//! - `disabled` / `visibleTools` — catch-all (`pattern: "*"` + `action: deny`) hides tools
//! - `fromConfig` / `merge` — config shape expansion and flat merge

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::config::{
    PermissionMode, PermissionRuleSet, PermissionSelector, PermissionSelectorRule,
    ProfilePermissions,
};

/// Permission action for ruleset evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionAction {
    Allow,
    Ask,
    Deny,
}

impl From<PermissionMode> for PermissionAction {
    fn from(mode: PermissionMode) -> Self {
        match mode {
            PermissionMode::Allow => Self::Allow,
            PermissionMode::Ask => Self::Ask,
            PermissionMode::Deny => Self::Deny,
        }
    }
}

impl From<PermissionAction> for PermissionMode {
    fn from(action: PermissionAction) -> Self {
        match action {
            PermissionAction::Allow => Self::Allow,
            PermissionAction::Ask => Self::Ask,
            PermissionAction::Deny => Self::Deny,
        }
    }
}

/// Single permission rule (permission + pattern + action).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRule {
    pub permission: String,
    pub pattern: String,
    pub action: PermissionAction,
}

/// Ordered ruleset (merge order = evaluation order; last match wins).
pub type PermissionRuleset = Vec<PermissionRule>;

const EDIT_TOOLS: &[&str] = &[
    "edit",
    "write",
    "apply_patch",
    "multiedit",
    "ast_grep_replace",
];
const READ_MCP_TOOLS: &[&str] = &[
    "list_mcp_resources",
    "list_mcp_resource_templates",
    "read_mcp_resource",
];

/// Expand `~/` and `$HOME` prefixes with home-directory expansion.
pub fn expand_pattern(pattern: &str) -> String {
    if let Some(rest) = pattern.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}/{rest}", home.to_string_lossy());
        }
    }
    if pattern == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return home.to_string_lossy().into_owned();
        }
    }
    if let Some(rest) = pattern.strip_prefix("$HOME/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}/{rest}", home.to_string_lossy());
        }
    }
    if let Some(rest) = pattern.strip_prefix("$HOME") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}{rest}", home.to_string_lossy());
        }
    }
    pattern.to_string()
}

/// Build a ruleset from a nested permission config map.
///
/// Values may be scalar actions (`"allow"`) or pattern maps
/// (`{ "git *": "allow", "*": "ask" }`).
pub fn from_config_map(config: &BTreeMap<String, ConfigPermissionValue>) -> PermissionRuleset {
    let mut rules = Vec::new();
    for (permission, value) in config {
        match value {
            ConfigPermissionValue::Action(action) => rules.push(PermissionRule {
                permission: permission.clone(),
                pattern: "*".to_string(),
                action: *action,
            }),
            ConfigPermissionValue::Patterns(patterns) => {
                for (pattern, action) in patterns {
                    rules.push(PermissionRule {
                        permission: permission.clone(),
                        pattern: expand_pattern(pattern),
                        action: *action,
                    });
                }
            }
        }
    }
    rules
}

/// Config value for one permission key (scalar or pattern map).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigPermissionValue {
    Action(PermissionAction),
    Patterns(BTreeMap<String, PermissionAction>),
}

/// Flat-merge rulesets (later rules override earlier ones).
pub fn merge(rulesets: impl IntoIterator<Item = PermissionRuleset>) -> PermissionRuleset {
    rulesets.into_iter().flatten().collect()
}

/// Evaluate permission+pattern against rulesets (last match wins; default ask).
///
/// Last matching rule wins. When no rule matches, returns
/// `{ action: ask, permission, pattern: "*" }`.
pub fn evaluate(
    permission: &str,
    pattern: &str,
    rulesets: impl IntoIterator<Item = impl AsRef<[PermissionRule]>>,
) -> PermissionRule {
    let merged: Vec<PermissionRule> = rulesets
        .into_iter()
        .flat_map(|set| set.as_ref().to_vec())
        .collect();
    let match_rule = merged.into_iter().rev().find(|rule| {
        wildcard_match(permission, &rule.permission) && wildcard_match(pattern, &rule.pattern)
    });

    match_rule.unwrap_or(PermissionRule {
        permission: permission.to_string(),
        pattern: "*".to_string(),
        action: PermissionAction::Ask,
    })
}

/// Map a tool id to the permission name used by catch-all deny / disabled-tool filtering.
pub fn tool_permission_name(tool_id: &str) -> &str {
    if EDIT_TOOLS.contains(&tool_id) {
        "edit"
    } else if READ_MCP_TOOLS.contains(&tool_id) {
        "read"
    } else {
        tool_id
    }
}

/// Tools whose catch-all permission rule is deny (hidden from the model).
pub fn disabled_tools<'a>(
    tools: impl IntoIterator<Item = &'a str>,
    ruleset: &[PermissionRule],
) -> BTreeSet<String> {
    tools
        .into_iter()
        .filter(|tool| {
            let permission = tool_permission_name(tool);
            // findLast on permission only (not pattern).
            let rule = ruleset
                .iter()
                .rfind(|rule| wildcard_match(permission, &rule.permission));
            matches!(
                rule,
                Some(PermissionRule {
                    pattern,
                    action: PermissionAction::Deny,
                    ..
                }) if pattern == "*"
            )
        })
        .map(str::to_string)
        .collect()
}

/// Whether a single tool is catch-all denied (hidden from the model).
pub fn is_tool_disabled(tool_id: &str, ruleset: &[PermissionRule]) -> bool {
    disabled_tools(std::iter::once(tool_id), ruleset).contains(tool_id)
}

/// Filter tools to those not catch-all denied (not catch-all denied).
pub fn visible_tools<'a>(
    tools: impl IntoIterator<Item = &'a str>,
    ruleset: &[PermissionRule],
) -> Vec<String> {
    let tools: Vec<&str> = tools.into_iter().collect();
    let hidden = disabled_tools(tools.iter().copied(), ruleset);
    tools
        .into_iter()
        .filter(|tool| !hidden.contains(*tool))
        .map(str::to_string)
        .collect()
}

/// Convert Harness [`ProfilePermissions`] into a flat permission ruleset.
///
/// Scalar fields become `pattern: "*"` rules. Selector rules append afterward
/// so last-match semantics preserve plan-path allows under catch-all deny.
pub fn from_profile_permissions(permissions: &ProfilePermissions) -> PermissionRuleset {
    let mut rules = Vec::new();

    if let Some(fallback) = permissions.fallback.clone() {
        rules.push(PermissionRule {
            permission: "*".to_string(),
            pattern: "*".to_string(),
            action: fallback.into(),
        });
    }

    push_scalar(&mut rules, "edit", permissions.edit.clone());
    push_scalar(&mut rules, "bash", permissions.shell.clone());
    push_scalar(&mut rules, "shell", permissions.shell.clone());
    push_scalar(&mut rules, "network", permissions.network.clone());
    push_scalar(&mut rules, "question", permissions.question.clone());
    push_scalar(&mut rules, "task", permissions.task.clone());
    push_scalar(&mut rules, "todowrite", permissions.todowrite.clone());
    push_scalar(&mut rules, "webfetch", permissions.webfetch.clone());
    push_scalar(&mut rules, "websearch", permissions.websearch.clone());
    push_scalar(&mut rules, "codesearch", permissions.codesearch.clone());
    push_scalar(&mut rules, "lsp", permissions.lsp.clone());
    push_scalar(&mut rules, "read", permissions.read.clone());
    push_scalar(
        &mut rules,
        "external_directory",
        permissions.external_directory.clone(),
    );
    push_scalar(&mut rules, "doom_loop", permissions.doom_loop.clone());

    append_selector_rules(&mut rules, "edit", &permissions.rules.edit);
    append_selector_rules(&mut rules, "bash", &permissions.rules.shell);
    append_selector_rules(&mut rules, "shell", &permissions.rules.shell);
    append_selector_rules(&mut rules, "task", &permissions.rules.task);
    append_selector_rules(&mut rules, "read", &permissions.rules.read);
    append_selector_rules(
        &mut rules,
        "external_directory",
        &permissions.rules.external_directory,
    );

    rules
}

/// Build a ruleset from global permission defaults + optional profile overlay.
pub fn from_defaults_and_profile(
    defaults: &BTreeMap<String, PermissionAction>,
    profile: Option<&ProfilePermissions>,
) -> PermissionRuleset {
    let mut rules: PermissionRuleset = defaults
        .iter()
        .map(|(permission, action)| PermissionRule {
            permission: permission.clone(),
            pattern: "*".to_string(),
            action: *action,
        })
        .collect();
    if let Some(profile) = profile {
        rules.extend(from_profile_permissions(profile));
    }
    rules
}

fn push_scalar(rules: &mut PermissionRuleset, permission: &str, mode: Option<PermissionMode>) {
    if let Some(mode) = mode {
        rules.push(PermissionRule {
            permission: permission.to_string(),
            pattern: "*".to_string(),
            action: mode.into(),
        });
    }
}

fn append_selector_rules(
    rules: &mut PermissionRuleset,
    permission: &str,
    selector_rules: &[PermissionSelectorRule],
) {
    for rule in selector_rules {
        let pattern = match &rule.selector {
            PermissionSelector::CatchAll => "*".to_string(),
            PermissionSelector::Exact(value) => value.clone(),
            PermissionSelector::Prefix(prefix) => format!("{prefix}*"),
            PermissionSelector::Glob(pattern) => pattern.clone(),
        };
        rules.push(PermissionRule {
            permission: permission.to_string(),
            pattern,
            action: rule.mode.clone().into(),
        });
    }
}

/// Wildcard match (`*` / `?`) for permission patterns.
pub fn wildcard_match(input: &str, pattern: &str) -> bool {
    let normalized = input.replace('\\', "/");
    let pattern = pattern.replace('\\', "/");
    if pattern == "*" {
        return true;
    }
    glob_like_match(&normalized, &pattern)
}

fn glob_like_match(input: &str, pattern: &str) -> bool {
    // Convert simple glob to recursive match without full regex dependency.
    let input_chars = input.chars().peekable();
    let pattern_chars = pattern.chars().peekable();

    fn match_rest(
        mut input: std::iter::Peekable<std::str::Chars<'_>>,
        mut pattern: std::iter::Peekable<std::str::Chars<'_>>,
    ) -> bool {
        loop {
            match pattern.next() {
                None => return input.next().is_none(),
                Some('*') => {
                    // Greedy-then-backtrack
                    if pattern.peek().is_none() {
                        return true;
                    }
                    loop {
                        if match_rest(input.clone(), pattern.clone()) {
                            return true;
                        }
                        if input.next().is_none() {
                            return false;
                        }
                    }
                }
                Some('?') => {
                    if input.next().is_none() {
                        return false;
                    }
                }
                Some(expected) => match input.next() {
                    Some(actual) if actual == expected => {}
                    _ => return false,
                },
            }
        }
    }

    match_rest(input_chars, pattern_chars)
}

/// Derive which task subagent names are denied under a ruleset.
pub fn denied_task_agents<'a>(
    agent_names: impl IntoIterator<Item = &'a str>,
    ruleset: &[PermissionRule],
) -> BTreeSet<String> {
    agent_names
        .into_iter()
        .filter(|name| evaluate("task", name, [ruleset]).action == PermissionAction::Deny)
        .map(str::to_string)
        .collect()
}

/// Derive subagent session permission from parent deny + subagent defaults.
///
/// 1. Parent deny + external_directory rules
/// 2. Default todowrite/task deny unless subagent ruleset already permits them
pub fn derive_subagent_session_permission(
    parent_session_permission: &[PermissionRule],
    subagent_permission: &[PermissionRule],
) -> PermissionRuleset {
    let can_task = subagent_permission
        .iter()
        .any(|rule| rule.permission == "task");
    let can_todo = subagent_permission
        .iter()
        .any(|rule| rule.permission == "todowrite");

    let mut rules: PermissionRuleset = parent_session_permission
        .iter()
        .filter(|rule| {
            rule.permission == "external_directory" || rule.action == PermissionAction::Deny
        })
        .cloned()
        .collect();

    if !can_todo {
        rules.push(PermissionRule {
            permission: "todowrite".to_string(),
            pattern: "*".to_string(),
            action: PermissionAction::Deny,
        });
    }
    if !can_task {
        rules.push(PermissionRule {
            permission: "task".to_string(),
            pattern: "*".to_string(),
            action: PermissionAction::Deny,
        });
    }
    rules
}

// Re-export selector conversion helper for PermissionRuleSet → ruleset fragments.
pub fn from_permission_rule_set(rules: &PermissionRuleSet) -> PermissionRuleset {
    let mut out = Vec::new();
    append_selector_rules(&mut out, "edit", &rules.edit);
    append_selector_rules(&mut out, "bash", &rules.shell);
    append_selector_rules(&mut out, "task", &rules.task);
    append_selector_rules(&mut out, "read", &rules.read);
    append_selector_rules(&mut out, "external_directory", &rules.external_directory);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_last_match_wins_and_defaults_to_ask() {
        let rules = vec![
            PermissionRule {
                permission: "bash".into(),
                pattern: "*".into(),
                action: PermissionAction::Ask,
            },
            PermissionRule {
                permission: "bash".into(),
                pattern: "git *".into(),
                action: PermissionAction::Allow,
            },
        ];
        assert_eq!(
            evaluate("bash", "git status", [&rules]).action,
            PermissionAction::Allow
        );
        assert_eq!(
            evaluate("bash", "rm -rf /", [&rules]).action,
            PermissionAction::Ask
        );
        let empty: &[PermissionRule] = &[];
        assert_eq!(
            evaluate("unknown", "*", [empty]).action,
            PermissionAction::Ask
        );
    }

    #[test]
    fn disabled_hides_only_catch_all_deny() {
        // Plan-style: edit * deny then path allow → edit stays visible
        let plan_rules = vec![
            PermissionRule {
                permission: "edit".into(),
                pattern: "*".into(),
                action: PermissionAction::Deny,
            },
            PermissionRule {
                permission: "edit".into(),
                pattern: ".agent-harness/plans/*".into(),
                action: PermissionAction::Allow,
            },
        ];
        assert!(!is_tool_disabled("edit", &plan_rules));
        assert!(!is_tool_disabled("write", &plan_rules));

        // Explore-style: * deny then allows — edit remains denied via *
        let explore_rules = vec![
            PermissionRule {
                permission: "*".into(),
                pattern: "*".into(),
                action: PermissionAction::Deny,
            },
            PermissionRule {
                permission: "read".into(),
                pattern: "*".into(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "bash".into(),
                pattern: "*".into(),
                action: PermissionAction::Allow,
            },
        ];
        assert!(is_tool_disabled("edit", &explore_rules));
        assert!(is_tool_disabled("write", &explore_rules));
        assert!(is_tool_disabled("task", &explore_rules));
        assert!(!is_tool_disabled("read", &explore_rules));
        assert!(!is_tool_disabled("bash", &explore_rules));
    }

    #[test]
    fn denied_task_agents_filters_by_evaluate() {
        let rules = vec![
            PermissionRule {
                permission: "task".into(),
                pattern: "*".into(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "task".into(),
                pattern: "general".into(),
                action: PermissionAction::Deny,
            },
        ];
        let denied = denied_task_agents(["explore", "general", "deep"], &rules);
        assert!(denied.contains("general"));
        assert!(!denied.contains("explore"));
    }

    #[test]
    fn derive_subagent_injects_task_and_todowrite_deny() {
        let parent = vec![PermissionRule {
            permission: "edit".into(),
            pattern: "*".into(),
            action: PermissionAction::Deny,
        }];
        let subagent = vec![PermissionRule {
            permission: "bash".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        }];
        let derived = derive_subagent_session_permission(&parent, &subagent);
        assert!(derived.iter().any(|r| {
            r.permission == "task" && r.pattern == "*" && r.action == PermissionAction::Deny
        }));
        assert!(derived.iter().any(|r| {
            r.permission == "todowrite" && r.pattern == "*" && r.action == PermissionAction::Deny
        }));
        assert!(derived
            .iter()
            .any(|r| { r.permission == "edit" && r.action == PermissionAction::Deny }));
    }

    #[test]
    fn profile_permissions_plan_keeps_edit_visible() {
        let plan = ProfilePermissions {
            edit: None,
            shell: Some(PermissionMode::Ask),
            read: None,
            external_directory: None,
            doom_loop: None,
            rules: PermissionRuleSet {
                edit: vec![
                    PermissionSelectorRule {
                        selector: PermissionSelector::CatchAll,
                        mode: PermissionMode::Deny,
                    },
                    PermissionSelectorRule {
                        selector: PermissionSelector::Prefix(".agent-harness/plans/".into()),
                        mode: PermissionMode::Allow,
                    },
                ],
                ..PermissionRuleSet::default()
            },
            ..ProfilePermissions::default()
        };
        let ruleset = from_profile_permissions(&plan);
        assert!(!is_tool_disabled("edit", &ruleset));
    }
}

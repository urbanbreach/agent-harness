use super::{
    permission_kind_for_capability, permission_kind_for_tool, PermissionDecision, PermissionKind,
    PermissionPolicy, PermissionRuleRequest, PolicyDecision,
};
use crate::config::{
    CategoryPermissions, PermissionMode, PermissionRuleSet, PermissionSelector,
    PermissionSelectorRule,
};
use crate::tool::ToolCapability;

fn ask_decision(timeout_ms: u64) -> PolicyDecision {
    PolicyDecision::Ask {
        timeout_ms,
        default_decision: PermissionDecision::Deny,
    }
}

#[test]
fn evaluate_uses_global_defaults() {
    let policy = PermissionPolicy::new(
        PermissionMode::Ask,
        PermissionMode::Allow,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(1_234);

    assert_eq!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        ask_decision(1_234)
    );
}

#[test]
fn evaluate_uses_category_override_when_present() {
    let policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Deny,
        PermissionMode::Deny,
    )
    .with_category_override(
        "deep",
        CategoryPermissions {
            edit: Some(PermissionMode::Allow),
            shell: Some(PermissionMode::Ask),
            network: None,
            ..CategoryPermissions::default()
        },
    )
    .with_ask_timeout_ms(55);

    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::Shell),
        ask_decision(55)
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::Network),
        PolicyDecision::Deny
    );
}

#[test]
fn native_permission_kinds_follow_explicit_and_migration_defaults() {
    let policy = PermissionPolicy::new(
        PermissionMode::Ask,
        PermissionMode::Deny,
        PermissionMode::Allow,
    )
    .with_category_override(
        "deep",
        CategoryPermissions {
            question: Some(PermissionMode::Deny),
            task: Some(PermissionMode::Ask),
            websearch: Some(PermissionMode::Deny),
            lsp: Some(PermissionMode::Ask),
            ..CategoryPermissions::default()
        },
    )
    .with_ask_timeout_ms(77);

    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        ask_decision(77)
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::Question),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::Task),
        ask_decision(77)
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::WebSearch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::CodeSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::Lsp),
        ask_decision(77)
    );
}

#[test]
fn permission_rule_precedence_for_bash_exact_prefix_and_catch_all() {
    let policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Deny,
    )
    .with_category_override(
        "build",
        CategoryPermissions {
            rules: PermissionRuleSet {
                shell: vec![
                    PermissionSelectorRule {
                        selector: PermissionSelector::CatchAll,
                        mode: PermissionMode::Deny,
                    },
                    PermissionSelectorRule {
                        selector: PermissionSelector::Prefix("cargo test".to_string()),
                        mode: PermissionMode::Ask,
                    },
                    PermissionSelectorRule {
                        selector: PermissionSelector::Exact(
                            "cargo test -p harness-core".to_string(),
                        ),
                        mode: PermissionMode::Allow,
                    },
                ],
                edit: Vec::new(),
                task: Vec::new(),
            },
            ..CategoryPermissions::default()
        },
    );

    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::Shell,
            Some(&PermissionRuleRequest::ShellCommand(
                "cargo test -p harness-core".to_string()
            ))
        ),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::Shell,
            Some(&PermissionRuleRequest::ShellCommand(
                "cargo test -p harness-core --lib".to_string()
            ))
        ),
        ask_decision(0)
    );
    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::Shell,
            Some(&PermissionRuleRequest::ShellCommand(
                "git status".to_string()
            ))
        ),
        PolicyDecision::Deny
    );
}

#[test]
fn permission_rule_precedence_for_task_exact_glob_and_catch_all() {
    let policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Deny,
    )
    .with_category_override(
        "build",
        CategoryPermissions {
            rules: PermissionRuleSet {
                shell: Vec::new(),
                edit: Vec::new(),
                task: vec![
                    PermissionSelectorRule {
                        selector: PermissionSelector::CatchAll,
                        mode: PermissionMode::Deny,
                    },
                    PermissionSelectorRule {
                        selector: PermissionSelector::Glob("review-*".to_string()),
                        mode: PermissionMode::Ask,
                    },
                    PermissionSelectorRule {
                        selector: PermissionSelector::Exact("review-code".to_string()),
                        mode: PermissionMode::Allow,
                    },
                ],
            },
            ..CategoryPermissions::default()
        },
    );

    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::Task,
            Some(&PermissionRuleRequest::TaskAgent("review-code".to_string()))
        ),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::Task,
            Some(&PermissionRuleRequest::TaskAgent("review-docs".to_string()))
        ),
        ask_decision(0)
    );
    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::Task,
            Some(&PermissionRuleRequest::TaskAgent("build".to_string()))
        ),
        PolicyDecision::Deny
    );
}

#[test]
fn config_permission_rule_precedence_for_bash_and_edit_exact_prefix_and_catch_all() {
    let policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Deny,
    )
    .with_category_override(
        "build",
        CategoryPermissions {
            rules: PermissionRuleSet {
                shell: vec![
                    PermissionSelectorRule {
                        selector: PermissionSelector::CatchAll,
                        mode: PermissionMode::Deny,
                    },
                    PermissionSelectorRule {
                        selector: PermissionSelector::Prefix("cargo test".to_string()),
                        mode: PermissionMode::Ask,
                    },
                    PermissionSelectorRule {
                        selector: PermissionSelector::Exact(
                            "cargo test -p harness-core".to_string(),
                        ),
                        mode: PermissionMode::Allow,
                    },
                ],
                edit: vec![
                    PermissionSelectorRule {
                        selector: PermissionSelector::CatchAll,
                        mode: PermissionMode::Deny,
                    },
                    PermissionSelectorRule {
                        selector: PermissionSelector::Prefix("docs/".to_string()),
                        mode: PermissionMode::Allow,
                    },
                    PermissionSelectorRule {
                        selector: PermissionSelector::Exact("docs/locked.md".to_string()),
                        mode: PermissionMode::Ask,
                    },
                ],
                task: Vec::new(),
            },
            ..CategoryPermissions::default()
        },
    );

    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::Shell,
            Some(&PermissionRuleRequest::ShellCommand(
                "cargo test -p harness-core".to_string()
            ))
        ),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::Shell,
            Some(&PermissionRuleRequest::ShellCommand(
                "cargo test -p harness-core --lib".to_string()
            ))
        ),
        ask_decision(0)
    );
    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::Shell,
            Some(&PermissionRuleRequest::ShellCommand(
                "git status".to_string()
            ))
        ),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::EditFs,
            Some(&PermissionRuleRequest::WorkspacePath(
                "docs/locked.md".to_string()
            ))
        ),
        ask_decision(0)
    );
    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::EditFs,
            Some(&PermissionRuleRequest::WorkspacePath(
                "docs/guide.md".to_string()
            ))
        ),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate_request(
            Some("build"),
            PermissionKind::EditFs,
            Some(&PermissionRuleRequest::WorkspacePath(
                "src/main.rs".to_string()
            ))
        ),
        PolicyDecision::Deny
    );
}

#[test]
fn permission_rule_profile_override_beats_top_level_edit_rule() {
    let parsed = crate::config::load_config_from_str(
        r#"
            {
              provider: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  models: {
                    "gpt-4o-mini": { name: "GPT-4o mini" }
                  }
                }
              },
              model: "default/gpt-4o-mini",
              agent: {
                worker: {
                  system_prompt: "Deep work",
                  permission: { edit: "allow" },
                  tools: ["edit"]
                }
              },
              default_agent: "worker",
              permission: {
                edit: { "*": "deny" },
                bash: "allow",
                webfetch: "allow",
                websearch: "allow",
                codesearch: "allow",
                lsp: "allow",
                question: "allow",
                task: "allow"
              }
            }
            "#,
    )
    .expect("config should parse");
    let policy = PermissionPolicy::from_config(&parsed);

    assert_eq!(
        policy.evaluate_request(
            Some("worker"),
            PermissionKind::EditFs,
            Some(&PermissionRuleRequest::WorkspacePath(
                "docs/readme.md".to_string()
            ))
        ),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate_request(
            None,
            PermissionKind::EditFs,
            Some(&PermissionRuleRequest::WorkspacePath(
                "docs/readme.md".to_string()
            ))
        ),
        PolicyDecision::Deny
    );
}

#[test]
fn shipped_plan_agent_allows_only_plan_file_edits() {
    let parsed = crate::config::load_config_from_str(
        r#"
            {
              provider: {
                default: {
                  type: "openai_compatible",
                  options: { baseURL: "http://127.0.0.1:8317/v1", apiKey: "test-key" },
                  models: { "gpt-4o-mini": { name: "GPT-4o mini" } }
                }
              },
              model: "default/gpt-4o-mini",
              default_agent: "build",
              permission: "allow"
            }
            "#,
    )
    .expect("public config should parse");
    let policy = PermissionPolicy::from_config(&parsed);

    assert_eq!(
        policy.evaluate_request(
            Some("plan"),
            PermissionKind::EditFs,
            Some(&PermissionRuleRequest::WorkspacePath(
                ".agent-harness/plans/run-001.md".to_string()
            ))
        ),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate_request(
            Some("plan"),
            PermissionKind::EditFs,
            Some(&PermissionRuleRequest::WorkspacePath(
                "src/lib.rs".to_string()
            ))
        ),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate_request(Some("plan"), PermissionKind::Shell, None),
        ask_decision(0)
    );
    assert_eq!(
        policy.evaluate_request(Some("plan"), PermissionKind::Task, None),
        PolicyDecision::Allow
    );
}

#[test]
fn native_tool_ids_resolve_to_permission_kinds_without_aliases() {
    assert_eq!(
        permission_kind_for_tool("question"),
        Some(PermissionKind::Question)
    );
    assert_eq!(
        permission_kind_for_tool("plan_enter"),
        Some(PermissionKind::Question)
    );
    assert_eq!(
        permission_kind_for_tool("plan_exit"),
        Some(PermissionKind::Question)
    );
    assert_eq!(permission_kind_for_tool("task"), Some(PermissionKind::Task));
    assert_eq!(
        permission_kind_for_tool("skill"),
        Some(PermissionKind::Task)
    );
    assert_eq!(
        permission_kind_for_tool("webfetch"),
        Some(PermissionKind::WebFetch)
    );
    assert_eq!(
        permission_kind_for_tool("websearch"),
        Some(PermissionKind::WebSearch)
    );
    assert_eq!(
        permission_kind_for_tool("codesearch"),
        Some(PermissionKind::CodeSearch)
    );
    assert_eq!(permission_kind_for_tool("lsp"), Some(PermissionKind::Lsp));
    assert_eq!(
        permission_kind_for_tool("todoread"),
        Some(PermissionKind::Task)
    );
    assert_eq!(
        permission_kind_for_tool("todowrite"),
        Some(PermissionKind::Task)
    );
    assert_eq!(
        permission_kind_for_tool("lsp.rename"),
        Some(PermissionKind::EditFs)
    );
    assert_eq!(
        permission_kind_for_tool("ast_grep_replace"),
        Some(PermissionKind::EditFs)
    );
    assert_eq!(permission_kind_for_tool("user.question"), None);
    assert_eq!(permission_kind_for_tool("agent.spawn"), None);
    assert_eq!(permission_kind_for_tool("web.fetch"), None);
    assert_eq!(permission_kind_for_tool("search.web"), None);
    assert_eq!(permission_kind_for_tool("search.code"), None);
    assert_eq!(permission_kind_for_tool("code.lsp"), None);
    assert_eq!(permission_kind_for_tool("tool.batch"), None);
    assert_eq!(permission_kind_for_tool("batch"), None);
    assert_eq!(permission_kind_for_tool("todo.write"), None);
    assert_eq!(permission_kind_for_tool("invalid"), None);
    assert_eq!(
        permission_kind_for_capability(ToolCapability::SpawnAgent),
        Some(PermissionKind::Task)
    );
}

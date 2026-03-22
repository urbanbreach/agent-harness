use std::path::PathBuf;

use harness_core::config::load_config_from_str;
use harness_core::perm::{
    permission_kind_for_capability, permission_kind_for_tool, PermissionDecision, PermissionKind,
    PermissionPolicy, PolicyDecision,
};
use harness_core::tool::ToolCapability;

#[test]
fn permission_policy_supports_native_tool_permission_kinds() {
    let config = load_config_from_str(
        r#"
        {
          backgroundTask: {
            defaultConcurrency: 2,
            providerConcurrency: 2,
            modelConcurrency: 2,
            staleTimeoutMs: 15000,
            messageStalenessTimeoutMs: 5000,
          },
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {
                "gpt-4o-mini": {
                  display_name: "GPT-4o mini",
                },
              },
            },
          },
          categories: {
            deep: {
              description: "Deep work",
              model_ref: "default:gpt-4o-mini",
              permissions: {
                task: "deny",
                websearch: "allow",
                lsp: "ask",
              },
              tools: ["agent.spawn", "search.web", "code.lsp", "tool.batch"],
            },
          },
          permissions: {
            edit: "ask",
            shell: "deny",
            network: "deny",
          },
          paths: {
            session_dir: ".agent-harness/sessions",
          },
        }
        "#,
    )
    .expect("config should parse");

    assert_eq!(
        config.paths.session_dir,
        PathBuf::from(".agent-harness/sessions")
    );

    let policy = PermissionPolicy::from_config(&config).with_ask_timeout_ms(321);

    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Ask {
            timeout_ms: 321,
            default_decision: PermissionDecision::Deny,
        }
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );

    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::Task),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::WebFetch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::WebSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::Lsp),
        PolicyDecision::Ask {
            timeout_ms: 321,
            default_decision: PermissionDecision::Deny,
        }
    );

    assert_eq!(
        permission_kind_for_tool("user.question"),
        Some(PermissionKind::Question)
    );
    assert_eq!(
        permission_kind_for_tool("question"),
        Some(PermissionKind::Question)
    );
    assert_eq!(
        permission_kind_for_tool("agent.spawn"),
        Some(PermissionKind::Task)
    );
    assert_eq!(permission_kind_for_tool("task"), Some(PermissionKind::Task));
    assert_eq!(
        permission_kind_for_tool("web.fetch"),
        Some(PermissionKind::WebFetch)
    );
    assert_eq!(
        permission_kind_for_tool("webfetch"),
        Some(PermissionKind::WebFetch)
    );
    assert_eq!(
        permission_kind_for_tool("search.web"),
        Some(PermissionKind::WebSearch)
    );
    assert_eq!(
        permission_kind_for_tool("websearch"),
        Some(PermissionKind::WebSearch)
    );
    assert_eq!(
        permission_kind_for_tool("search.code"),
        Some(PermissionKind::CodeSearch)
    );
    assert_eq!(
        permission_kind_for_tool("codesearch"),
        Some(PermissionKind::CodeSearch)
    );
    assert_eq!(
        permission_kind_for_tool("code.lsp"),
        Some(PermissionKind::Lsp)
    );
    assert_eq!(permission_kind_for_tool("lsp"), Some(PermissionKind::Lsp));
    assert_eq!(permission_kind_for_tool("tool.batch"), None);
    assert_eq!(permission_kind_for_tool("batch"), None);
    assert_eq!(permission_kind_for_tool("todo.write"), None);
    assert_eq!(permission_kind_for_tool("invalid"), None);
    assert_eq!(
        permission_kind_for_capability(ToolCapability::SpawnAgent),
        Some(PermissionKind::Task)
    );
}

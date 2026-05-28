use std::path::PathBuf;

use harness_core::config::load_config_from_str;
use harness_core::perm::{
    permission_kind_for_capability, permission_kind_for_tool, PermissionDecision, PermissionKind,
    PermissionPolicy, PolicyDecision,
};
use harness_core::tool::ToolCapability;

const ASK_TIMEOUT_MS: u64 = 321;

fn deny_default_ask_decision() -> PolicyDecision {
    PolicyDecision::Ask {
        timeout_ms: ASK_TIMEOUT_MS,
        default_decision: PermissionDecision::Deny,
    }
}

#[test]
fn permission_policy_supports_native_tool_permission_kinds() {
    let config = load_config_from_str(
        r#"
        {
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
          agents: {
            deep: {
              description: "Deep work",
              model_ref: "default:gpt-4o-mini",
              permissions: {
                task: "deny",
                websearch: "allow",
                lsp: "ask",
              },
              tools: ["task", "websearch", "lsp", "batch"],
            },
          },
          permissions: {
            defaults: {
              edit: "ask",
              shell: "deny",
              network: "deny",
            },
          },
          runtime: {
            background_tasks: {
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000,
            },
            session_dir: ".agent-harness/sessions",
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp",
            },
          },
        }
        "#,
    )
    .expect("config should parse");

    assert_eq!(
        config.paths.session_dir,
        PathBuf::from(".agent-harness/sessions")
    );

    let policy = PermissionPolicy::from_config(&config).with_ask_timeout_ms(ASK_TIMEOUT_MS);

    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        deny_default_ask_decision()
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        deny_default_ask_decision()
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebFetch),
        deny_default_ask_decision()
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebSearch),
        deny_default_ask_decision()
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        deny_default_ask_decision()
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        deny_default_ask_decision()
    );

    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::Task),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::WebFetch),
        deny_default_ask_decision()
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::WebSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(Some("deep"), PermissionKind::Lsp),
        deny_default_ask_decision()
    );

    assert_eq!(
        permission_kind_for_tool("question"),
        Some(PermissionKind::Question)
    );
    assert_eq!(permission_kind_for_tool("task"), Some(PermissionKind::Task));
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
    assert_eq!(permission_kind_for_tool("agent.spawn"), None);
    assert_eq!(permission_kind_for_tool("search.web"), None);
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

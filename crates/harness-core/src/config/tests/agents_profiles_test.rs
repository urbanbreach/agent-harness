use super::*;
use crate::UnwrapOrAbort;

#[test]
fn public_agent_config_materializes_primary_and_named_subagents() {
    // Given
    let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "gpt-4o-mini": {
                  name: "GPT-4o mini",
                  variants: { high: { name: "High" } }
                }
              }
            }
          },
          model: "default/gpt-4o-mini",
          agent: {
            default: {
              system_prompt: "Do the work",
              variant: "high",
              permission: {
                edit: "deny",
                bash: "ask",
                task: "allow"
              },
              tools: ["read", "edit"]
            },
            librarian: {
              system_prompt: "Research the question"
            }
          },
          permission: "allow"
        }
        "#;

    // When
    let parsed = load_config_from_str(cfg)
        .unwrap_or_else(|error| panic!("agent config should load: {error}"));

    // Then
    assert_eq!(
        parsed.agents.keys().map(String::as_str).collect::<Vec<_>>(),
        ["default", "explore", "general", "librarian"]
    );
    let profile = &parsed.agents["default"];
    assert_eq!(profile.system_prompt.as_deref(), Some("Do the work"));
    assert_eq!(profile.model_ref, "default/gpt-4o-mini");
    assert_eq!(profile.variant.as_deref(), Some("high"));
    assert_eq!(
        profile.tool_failure_mode,
        ToolFailureMode::ContinueAsToolMessage
    );
    assert_eq!(profile.tools, ["read", "edit"]);
    let permissions = profile.permissions.as_ref().unwrap_or_abort();
    assert_eq!(permissions.edit, Some(PermissionMode::Deny));
    assert_eq!(permissions.shell, Some(PermissionMode::Ask));
    assert_eq!(permissions.task, Some(PermissionMode::Allow));
    assert_eq!(profile.mode, AgentMode::Primary);
    for name in ["explore", "general", "librarian"] {
        assert_eq!(parsed.agents[name].mode, AgentMode::Subagent);
    }
    assert_eq!(
        parsed.agents["librarian"].system_prompt.as_deref(),
        Some("Research the question")
    );
    let explore = &parsed.agents["explore"];
    assert!(!explore.tools.iter().any(|tool| tool == "codesearch"));
    assert_eq!(
        explore.permissions.as_ref().unwrap_or_abort().codesearch,
        Some(PermissionMode::Deny)
    );
    let librarian = &parsed.agents["librarian"];
    assert!(librarian.tools.iter().any(|tool| tool == "codesearch"));
    assert_eq!(
        librarian.permissions.as_ref().unwrap_or_abort().codesearch,
        Some(PermissionMode::Allow)
    );
}

#[test]
fn public_agent_config_rejects_role_shaped_config() {
    // Given
    let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" }
              }
            }
          },
          model: "default/gpt-4o-mini",
          agent: {
            build: { system_prompt: "Build work" }
          },
          permission: "allow"
        }
        "#;

    // When
    let error = load_config_from_str(cfg).expect_err("role-shaped agent config must fail");

    // Then
    assert!(error.to_string().contains("unknown field `build`"));
}

#[test]
fn public_agent_config_rejects_removed_selection_and_toggle_keys() {
    // Given
    for removed in [
        r#"default_agent: "default","#,
        r#"disabled_agents: ["explore"],"#,
    ] {
        let cfg = format!(
            r#"
            {{
              provider: {{
                default: {{
                  type: "openai_compatible",
                  options: {{
                    baseURL: "http://127.0.0.1:8317/v1",
                    apiKey: "test-key"
                  }},
                  models: {{ "gpt-4o-mini": {{ name: "GPT-4o mini" }} }}
                }}
              }},
              model: "default/gpt-4o-mini",
              {removed}
              permission: "allow"
            }}
            "#
        );

        // When
        let error = load_config_from_str(&cfg).expect_err("removed key must fail");

        // Then
        assert!(error.to_string().contains("unknown top-level config keys"));
    }
}

#[test]
fn shipped_primary_uses_generic_task_tools_without_plan_transitions() {
    // Given
    let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key"
              },
              models: { "gpt-4o-mini": { name: "GPT-4o mini" } }
            }
          },
          model: "default/gpt-4o-mini",
          permission: "allow"
        }
        "#;

    // When
    let parsed = load_config_from_str(cfg).unwrap_or_abort();

    // Then
    let tools = &parsed.agents["default"].tools;
    assert!(tools.iter().any(|tool| tool == "task"));
    assert!(!tools.iter().any(|tool| tool == "plan_enter"));
    assert!(!tools.iter().any(|tool| tool == "plan_exit"));
}

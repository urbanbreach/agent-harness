use super::*;
use crate::UnwrapOrAbort;

pub(super) fn merge_test_config(agent_overrides: &str) -> String {
    format!(
        r#"
        {{
          providers: {{
            default: {{
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: {{
                "gpt-4o-mini": {{
                  display_name: "GPT-4o mini",
                  variants: {{
                    low: {{}},
                    high: {{}},
                  }},
                }},
                "gpt-4o": {{
                  display_name: "GPT-4o",
                }},
              }},
            }},
          }},
          model: "default/gpt-4o-mini",
          agent: {{
            {agent_overrides}
          }},
          permissions: {{
            defaults: {{
              edit: "ask",
              shell: "ask",
              network: "deny",
            }},
          }},
          runtime: {{
            background_tasks: {{
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000,
            }},
            session_dir: ".agent-harness/sessions",
          }},
          integrations: {{
            remote_search: {{
              endpoint: "https://mcp.exa.ai/mcp",
            }},
          }},
        }}
        "#,
        agent_overrides = agent_overrides,
    )
}

pub(super) fn empty_general_override() -> String {
    r#"
    general: {
      tools: []
    },
    "#
    .to_string()
}

#[test]
fn merge_shipped_description_remains_canonical_when_markdown_differs() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_general_override())).unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "general",
        r#"---
{
  description: "From markdown",
  model_ref: "default/gpt-4o-mini"
}
---

Prompt body."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let general = parsed.agents.get("general").unwrap_or_abort();
    assert_eq!(
        general.description, "General-purpose implementation and research subagent.",
        "fixed profile descriptions must remain canonical"
    );
}

#[test]
fn merge_markdown_model_ref_takes_effect_for_shipped_agent() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config("")).unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "default",
        r#"---
{
  description: "Default from markdown",
  model_ref: "default/gpt-4o"
}
---

Default prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let default = parsed.agents.get("default").unwrap_or_abort();
    assert_eq!(
        default.model_ref, "default/gpt-4o",
        "markdown model_ref must take effect for shipped agent (model_ref_explicit=false)"
    );
    assert!(
        default.model_ref_explicit,
        "model_ref_explicit must be true when markdown provides model_ref"
    );
}

#[test]
fn merge_markdown_variant_takes_effect_when_config_has_none() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_general_override())).unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "general",
        r#"---
{
  description: "Custom",
  model_ref: "default/gpt-4o-mini",
  variant: "high"
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let general = parsed.agents.get("general").unwrap_or_abort();
    assert_eq!(
        general.variant.as_deref(),
        Some("high"),
        "markdown variant must take effect when config has no variant"
    );
}

#[test]
fn merge_markdown_temperature_takes_effect_when_config_has_none() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_general_override())).unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "general",
        r#"---
{
  description: "Custom",
  model_ref: "default/gpt-4o-mini",
  temperature: 0.3
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let general = parsed.agents.get("general").unwrap_or_abort();
    assert_eq!(
        general.temperature,
        Some(0.3),
        "markdown temperature must take effect when config has no temperature"
    );
}

#[test]
fn merge_shipped_permissions_remain_canonical_when_markdown_differs() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_general_override())).unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "general",
        r#"---
{
  description: "Custom",
  model_ref: "default/gpt-4o-mini",
  permissions: {
    shell: "ask",
    edit: "allow"
  }
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let general = parsed.agents.get("general").unwrap_or_abort();
    let permissions = general.permissions.as_ref().unwrap_or_abort();
    assert_eq!(
        permissions.shell,
        Some(PermissionMode::Allow),
        "shipped shell permission must remain canonical"
    );
    assert_eq!(
        permissions.edit,
        Some(PermissionMode::Allow),
        "markdown edit permission must take effect"
    );
}

#[test]
fn merge_markdown_max_iters_takes_effect_when_config_has_none() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_general_override())).unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "general",
        r#"---
{
  description: "Custom",
  model_ref: "default/gpt-4o-mini",
  max_iters: 5
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let general = parsed.agents.get("general").unwrap_or_abort();
    assert_eq!(
        general.max_iters,
        Some(5),
        "markdown max_iters must take effect when config has no max_iters"
    );
}

#[test]
fn merge_markdown_tool_failure_mode_takes_effect_when_config_has_serde_default() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_general_override())).unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "general",
        r#"---
{
  description: "Custom",
  model_ref: "default/gpt-4o-mini",
  tool_failure_mode: "fail_turn"
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let general = parsed.agents.get("general").unwrap_or_abort();
    assert_eq!(
        general.tool_failure_mode,
        ToolFailureMode::FailTurn,
        "markdown tool_failure_mode must take effect when config has serde default (ContinueAsToolMessage)"
    );
}

#[test]
fn merge_shipped_tools_remain_canonical_when_markdown_differs() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_general_override())).unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "general",
        r#"---
{
  description: "Custom",
  model_ref: "default/gpt-4o-mini",
  tools: ["read", "grep", "list"]
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let general = parsed.agents.get("general").unwrap_or_abort();
    assert!(general.tools.iter().any(|tool| tool == "edit"));
    assert!(general.tools.iter().any(|tool| tool == "skill"));
}

#[test]
fn merge_config_wins_when_both_present_for_all_fields() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        merge_test_config(
            r#"
            general: {
              model: "default/gpt-4o-mini",
              variant: "low",
              temperature: 0.1,
              permissions: {
                shell: "deny",
                edit: "deny"
              },
              max_iters: 10,
              tool_failure_mode: "fail_turn",
              tools: ["read", "edit"]
            },
            "#,
        ),
    )
    .unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "general",
        r#"---
{
  description: "From markdown",
  model_ref: "default/other-model",
  variant: "high",
  temperature: 0.9,
  permissions: {
    shell: "allow",
    edit: "allow"
  },
  max_iters: 99,
  tool_failure_mode: "continue_as_tool_message",
  tools: ["bash", "grep"]
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let general = parsed.agents.get("general").unwrap_or_abort();
    assert_eq!(
        general.description, "General-purpose implementation and research subagent.",
        "fixed profile descriptions must remain canonical"
    );
    assert_eq!(
        general.model_ref, "default/gpt-4o-mini",
        "config model_ref must win when both present (model_ref_explicit=true)"
    );
    assert_eq!(
        general.variant.as_deref(),
        Some("low"),
        "config variant must win when both present"
    );
    assert_eq!(
        general.temperature,
        Some(0.1),
        "config temperature must win when both present"
    );
    let permissions = general.permissions.as_ref().unwrap_or_abort();
    assert_eq!(
        permissions.shell,
        Some(PermissionMode::Deny),
        "config shell permission must win when both present"
    );
    assert_eq!(
        permissions.edit,
        Some(PermissionMode::Deny),
        "config edit permission must win when both present"
    );
    assert_eq!(
        general.max_iters,
        Some(10),
        "config max_iters must win when both present"
    );
    assert_eq!(
        general.tool_failure_mode,
        ToolFailureMode::FailTurn,
        "config tool_failure_mode must win when both present (non-default value)"
    );
    assert_eq!(
        general.tools,
        vec!["read", "edit"],
        "config tools must win when both present (non-empty)"
    );
}

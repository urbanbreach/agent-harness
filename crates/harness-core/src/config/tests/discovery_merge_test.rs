use super::*;

/// Minimal config with a model so shipped agents exist, plus an optional
/// extra agent block injected into the legacy `agents` map.
pub(super) fn merge_test_config(extra_agents: &str) -> String {
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
          agents: {{
            {extra_agents}
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
        extra_agents = extra_agents,
    )
}

/// Agent with empty description and empty tools so markdown fallback can fire
/// for those fields. `model_ref_explicit` is true because this goes through
/// the legacy `agents` path.
pub(super) fn empty_custom_agent() -> String {
    r#"
    custom: {
      description: "",
      model_ref: "default/gpt-4o-mini",
      tools: []
    },
    "#
    .to_string()
}

#[test]
fn merge_markdown_description_takes_effect_when_config_empty() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_custom_agent())).expect("write config");
    write_agent_markdown(
        &repo,
        "custom",
        r#"---
{
  description: "From markdown",
  model_ref: "default/gpt-4o-mini"
}
---

Prompt body."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).expect("merged config should parse");

    // assert
    let custom = parsed.agents.get("custom").expect("custom agent exists");
    assert_eq!(
        custom.description, "From markdown",
        "markdown description must take effect when config description is empty"
    );
}

#[test]
fn merge_markdown_model_ref_takes_effect_for_shipped_agent() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config("")).expect("write config");
    write_agent_markdown(
        &repo,
        "build",
        r#"---
{
  description: "Build from markdown",
  model_ref: "default/gpt-4o"
}
---

Build prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).expect("merged config should parse");

    // assert
    let build = parsed.agents.get("build").expect("build agent exists");
    assert_eq!(
        build.model_ref, "default/gpt-4o",
        "markdown model_ref must take effect for shipped agent (model_ref_explicit=false)"
    );
    assert!(
        build.model_ref_explicit,
        "model_ref_explicit must be true when markdown provides model_ref"
    );
}

#[test]
fn merge_markdown_variant_takes_effect_when_config_has_none() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_custom_agent())).expect("write config");
    write_agent_markdown(
        &repo,
        "custom",
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
    let parsed = load_config_from_file(&config_path).expect("merged config should parse");

    // assert
    let custom = parsed.agents.get("custom").expect("custom agent exists");
    assert_eq!(
        custom.variant.as_deref(),
        Some("high"),
        "markdown variant must take effect when config has no variant"
    );
}

#[test]
fn merge_markdown_temperature_takes_effect_when_config_has_none() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_custom_agent())).expect("write config");
    write_agent_markdown(
        &repo,
        "custom",
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
    let parsed = load_config_from_file(&config_path).expect("merged config should parse");

    // assert
    let custom = parsed.agents.get("custom").expect("custom agent exists");
    assert_eq!(
        custom.temperature,
        Some(0.3),
        "markdown temperature must take effect when config has no temperature"
    );
}

#[test]
fn merge_markdown_permissions_takes_effect_when_config_has_none() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_custom_agent())).expect("write config");
    write_agent_markdown(
        &repo,
        "custom",
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
    let parsed = load_config_from_file(&config_path).expect("merged config should parse");

    // assert
    let custom = parsed.agents.get("custom").expect("custom agent exists");
    let permissions = custom
        .permissions
        .as_ref()
        .expect("markdown permissions must take effect");
    assert_eq!(
        permissions.shell,
        Some(PermissionMode::Ask),
        "markdown shell permission must take effect"
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
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_custom_agent())).expect("write config");
    write_agent_markdown(
        &repo,
        "custom",
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
    let parsed = load_config_from_file(&config_path).expect("merged config should parse");

    // assert
    let custom = parsed.agents.get("custom").expect("custom agent exists");
    assert_eq!(
        custom.max_iters,
        Some(5),
        "markdown max_iters must take effect when config has no max_iters"
    );
}

#[test]
fn merge_markdown_tool_failure_mode_takes_effect_when_config_has_serde_default() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_custom_agent())).expect("write config");
    write_agent_markdown(
        &repo,
        "custom",
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
    let parsed = load_config_from_file(&config_path).expect("merged config should parse");

    // assert
    let custom = parsed.agents.get("custom").expect("custom agent exists");
    assert_eq!(
        custom.tool_failure_mode,
        ToolFailureMode::FailTurn,
        "markdown tool_failure_mode must take effect when config has serde default (ContinueAsToolMessage)"
    );
}

#[test]
fn merge_markdown_tools_take_effect_when_config_has_empty_tools() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_custom_agent())).expect("write config");
    write_agent_markdown(
        &repo,
        "custom",
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
    let parsed = load_config_from_file(&config_path).expect("merged config should parse");

    // assert
    let custom = parsed.agents.get("custom").expect("custom agent exists");
    assert_eq!(
        custom.tools,
        vec!["read", "grep", "list"],
        "markdown tools must take effect when config has empty tools"
    );
}

#[test]
fn merge_config_wins_when_both_present_for_all_fields() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        merge_test_config(
            r#"
            custom: {
              description: "From config",
              model_ref: "default/gpt-4o-mini",
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
    .expect("write config");
    write_agent_markdown(
        &repo,
        "custom",
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
    let parsed = load_config_from_file(&config_path).expect("merged config should parse");

    // assert
    let custom = parsed.agents.get("custom").expect("custom agent exists");
    assert_eq!(
        custom.description, "From config",
        "config description must win when both present"
    );
    assert_eq!(
        custom.model_ref, "default/gpt-4o-mini",
        "config model_ref must win when both present (model_ref_explicit=true)"
    );
    assert_eq!(
        custom.variant.as_deref(),
        Some("low"),
        "config variant must win when both present"
    );
    assert_eq!(
        custom.temperature,
        Some(0.1),
        "config temperature must win when both present"
    );
    let permissions = custom
        .permissions
        .as_ref()
        .expect("config permissions must be present");
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
        custom.max_iters,
        Some(10),
        "config max_iters must win when both present"
    );
    assert_eq!(
        custom.tool_failure_mode,
        ToolFailureMode::FailTurn,
        "config tool_failure_mode must win when both present (non-default value)"
    );
    assert_eq!(
        custom.tools,
        vec!["read", "edit"],
        "config tools must win when both present (non-empty)"
    );
}

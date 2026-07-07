use super::discovery_merge_test::{empty_custom_agent, merge_test_config};
use super::*;
use crate::UnwrapOrAbort;

#[test]
fn merge_preserves_existing_behavior_when_no_markdown_exists() {
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
            custom: {
              description: "Config only",
              model_ref: "default/gpt-4o-mini",
              variant: "low",
              tools: ["read"]
            },
            "#,
        ),
    )
    .unwrap_or_abort();
    // No markdown file for "custom"

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let custom = parsed.agents.get("custom").unwrap_or_abort();
    assert_eq!(custom.description, "Config only");
    assert_eq!(custom.variant.as_deref(), Some("low"));
    assert_eq!(custom.tools, vec!["read"]);
}

#[test]
fn markdown_frontmatter_permissions_bash_alias_maps_to_shell() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config(&empty_custom_agent())).unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "custom",
        r#"---
{
  description: "Custom",
  model_ref: "default/gpt-4o-mini",
  permissions: {
    bash: "allow",
    edit: "deny"
  }
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let custom = parsed.agents.get("custom").unwrap_or_abort();
    let permissions = custom.permissions.as_ref().unwrap_or_abort();
    assert_eq!(
        permissions.shell,
        Some(PermissionMode::Allow),
        "frontmatter `bash` must alias to ProfilePermissions.shell"
    );
    assert_eq!(
        permissions.edit,
        Some(PermissionMode::Deny),
        "frontmatter `edit` must parse correctly"
    );
}

#[test]
fn discovery_last_wins_project_level_overrides_git_root() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    let workspace = repo.join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = workspace.join("harness.jsonc");
    fs::write(&config_path, merge_test_config("")).unwrap_or_abort();

    write_agent_markdown_in(
        &repo,
        ".agent-harness",
        "build",
        r#"---
{
  description: "Build from git root",
  model_ref: "default/gpt-4o-mini"
}
---

Git root prompt."#,
    );
    write_agent_markdown_in(
        &workspace,
        ".agent-harness",
        "build",
        r#"---
{
  description: "Build from project workspace",
  model_ref: "default/gpt-4o"
}
---

Workspace prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let build = parsed.agents.get("build").unwrap_or_abort();
    assert_eq!(
        build.model_ref, "default/gpt-4o",
        "project-level (deeper) markdown must override git-root markdown with last-wins"
    );
    assert_eq!(
        build.system_prompt.as_deref(),
        Some("Workspace prompt."),
        "project-level prompt body must win"
    );
}

#[test]
fn discovery_shipped_agents_load_when_no_project_override() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, merge_test_config("")).unwrap_or_abort();

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    assert!(
        parsed.agents.contains_key("build"),
        "shipped build agent must load when no project override exists"
    );
    assert!(
        parsed.agents.contains_key("plan"),
        "shipped plan agent must load when no project override exists"
    );
}

#[test]
fn markdown_enable_false_disables_agent() {
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
        "custom-test",
        r#"---
{
  description: "Custom test agent",
  model_ref: "default/gpt-4o-mini",
  enable: false
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    assert!(
        !parsed.agents.contains_key("custom-test"),
        "agent with enable: false must not appear in the agent map"
    );
}

#[test]
fn markdown_disable_true_disables_agent() {
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
        "custom-test",
        r#"---
{
  description: "Custom test agent",
  model_ref: "default/gpt-4o-mini",
  disable: true
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    assert!(
        !parsed.agents.contains_key("custom-test"),
        "agent with disable: true must not appear in the agent map"
    );
}

#[test]
fn markdown_use_small_model_selects_small_model() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    let mut cfg = merge_test_config("");
    cfg = cfg.replace(
        "model: \"default/gpt-4o-mini\"",
        "model: \"default/gpt-4o-mini\",\n          small_model: \"default/gpt-4o\"",
    );
    fs::write(&config_path, cfg).unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "custom-test",
        r#"---
{
  description: "Custom test agent",
  use_small_model: true
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let custom = parsed.agents.get("custom-test").unwrap_or_abort();
    assert_eq!(
        custom.model_ref, "default/gpt-4o",
        "use_small_model: true must select the small model ref"
    );
}

#[test]
fn markdown_tools_map_shape_parses_correctly() {
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
        "custom-test",
        r#"---
{
  description: "Custom test agent",
  model_ref: "default/gpt-4o-mini",
  tools: { "read": true, "bash": false, "grep": true }
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let custom = parsed.agents.get("custom-test").unwrap_or_abort();
    assert!(
        custom.tools.contains(&"read".to_string()),
        "Map tools with read: true must include read"
    );
    assert!(
        custom.tools.contains(&"grep".to_string()),
        "Map tools with grep: true must include grep"
    );
    assert!(
        !custom.tools.contains(&"bash".to_string()),
        "Map tools with bash: false must NOT include bash"
    );
}

#[test]
fn markdown_tools_list_shape_still_works() {
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
        "custom-test",
        r#"---
{
  description: "Custom test agent",
  model_ref: "default/gpt-4o-mini",
  tools: ["read", "grep", "list"]
}
---

Prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    let custom = parsed.agents.get("custom-test").unwrap_or_abort();
    assert_eq!(
        custom.tools,
        vec!["read", "grep", "list"],
        "List-shaped tools must parse correctly"
    );
}

#[test]
fn json_config_enable_false_overrides_markdown_enable_true() {
    // arrange
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: {
                "gpt-4o-mini": {
                  display_name: "GPT-4o mini",
                  variants: { low: {}, high: {} },
                },
              },
            },
          },
          model: "default/gpt-4o-mini",
          agent: {
            "custom-test": {
              description: "JSON config disabled agent",
              model: "default/gpt-4o-mini",
              enable: false
            }
          },
          permission: "ask",
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
    .unwrap_or_abort();
    write_agent_markdown(
        &repo,
        "custom-test",
        r#"---
{
  description: "Markdown re-enabled agent",
  model_ref: "default/gpt-4o-mini",
  enable: true
}
---

Markdown prompt."#,
    );

    // act
    let parsed = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    assert!(
        !parsed.agents.contains_key("custom-test"),
        "JSON config enable:false must override markdown enable:true — agent must not appear"
    );
}

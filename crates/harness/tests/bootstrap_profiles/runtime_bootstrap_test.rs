use harness::UnwrapOrAbort;

fn public_runtime_config(agent_entries: &str, permission: &str) -> String {
    format!(
        r#"
        {{
          provider: {{
            default: {{
              type: "openai_compatible",
              options: {{
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
                apiMode: "responses",
                timeoutMs: 60000,
              }},
              models: {{
                "gpt-4o-mini": {{ name: "GPT-4o mini" }},
              }},
            }},
          }},
          model: "default/gpt-4o-mini",
          agent: {{ {agent_entries} }},
          permission: {permission},
        }}
        "#
    )
}

#[test]
fn interactive_bootstrap_builds_generic_runtime_state() {
    // arrange
    let raw = public_runtime_config(
        r#"
        default: { system_prompt: "Default prompt", tools: [] },
        general: {
          system_prompt: "General prompt",
          permission: { task: "deny" },
          tools: [],
        },
        "#,
        r#""allow""#,
    );
    let config = load_config_from_str(&raw).unwrap_or_abort();

    // act
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();

    // assert
    assert_eq!(
        coordinator_config
            .agent_profiles
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["default", "explore", "general", "librarian"]
    );
    let general_prompt = &coordinator_config.agent_profiles["general"].system_prompt;
    assert!(general_prompt.starts_with("General prompt"));
    assert!(general_prompt.contains("The exact model ID is default/gpt-4o-mini"));
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("general"), PermissionKind::Task),
        PolicyDecision::Deny
    );
}

#[test]
fn default_and_general_permissions_follow_configured_policy() {
    // arrange
    let raw = public_runtime_config(
        r#"
        default: {
          system_prompt: "Default prompt",
          permission: { edit: "allow", bash: "allow", task: "allow" },
        },
        general: {
          system_prompt: "General prompt",
          permission: { edit: "deny", bash: "deny", task: "deny" },
        },
        "#,
        r#""allow""#,
    );
    let config = load_config_from_str(&raw).unwrap_or_abort();
    // act
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();

    // assert
    assert_eq!(bootstrap::interactive_profile_name(&config), "default");
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("default"), PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("default"), PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("general"), PermissionKind::EditFs),
        PolicyDecision::Deny
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("general"), PermissionKind::Shell),
        PolicyDecision::Deny
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("general"), PermissionKind::Task),
        PolicyDecision::Deny
    );
}

#[test]
fn interactive_bootstrap_uses_discovered_default_markdown_prompt() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        public_runtime_config(
            r#"default: { model: "default/gpt-4o-mini", tools: [] }"#,
            r#""allow""#,
        ),
    )
    .unwrap_or_abort();
    write_agent_markdown(&repo, "default", "Markdown-backed generic prompt.");

    let config = load_config_from_repo_file(&config_path, &repo);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    // act
    let prompt = &coordinator_config.agent_profiles["default"].system_prompt;

    // assert
    assert!(prompt.starts_with("Markdown-backed generic prompt."));
    assert!(prompt.contains("The exact model ID is default/gpt-4o-mini"));
}

#[test]
fn interactive_bootstrap_prepends_project_agents_md_to_default_prompt() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        public_runtime_config(
            r#"default: { system_prompt: "Default prompt", tools: [] }"#,
            r#""allow""#,
        ),
    )
    .unwrap_or_abort();
    fs::write(repo.join("AGENTS.md"), "Project-wide instructions.").unwrap_or_abort();

    let config = load_config_from_repo_file(&config_path, &repo);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    // act
    let prompt = &coordinator_config.agent_profiles["default"].system_prompt;

    // assert
    assert!(prompt.contains("Instructions from:"));
    assert!(prompt.contains("Project-wide instructions."));
    assert!(prompt.starts_with("Default prompt"));
    assert!(prompt.contains("The exact model ID is default/gpt-4o-mini"));
}

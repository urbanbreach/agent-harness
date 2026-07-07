use harness::UnwrapOrAbort;
#[test]
fn interactive_bootstrap_builds_runtime_state_from_agents() {
    // arrange
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
              system_prompt: "Deep prompt",
              model_ref: "default:gpt-4o-mini",
              tools: [],
            },
            review: {
              description: "Review mode",
              system_prompt: "Review prompt",
              model_ref: "default:gpt-4o-mini",
              permissions: {
                task: "deny",
              },
              tools: [],
            },
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow",
            },
            shell_allowlist: {
              executables: ["git"],
              cwd_roots: ["."],
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
    .unwrap_or_abort();

    let coordinator_config =
        // act
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();

    // assert
    assert!(coordinator_config.agent_profiles.contains_key("deep"));
    assert!(coordinator_config.agent_profiles.contains_key("review"));
    let review_prompt = &coordinator_config.agent_profiles["review"].system_prompt;
    assert!(review_prompt.starts_with("Review prompt"));
    assert!(review_prompt.contains("The exact model ID is default/gpt-4o-mini"));

    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("review"), PermissionKind::Task),
        PolicyDecision::Deny
    );
}

#[test]
fn build_and_review_permissions_follow_configured_policy() {
    // arrange
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
            build: {
              description: "Implementation mode",
              system_prompt: "Build prompt",
              model_ref: "default:gpt-4o-mini",
              permissions: {
                edit: "allow",
                shell: "allow",
                task: "allow",
              },
              tools: [],
            },
            review: {
              description: "Review mode",
              system_prompt: "Review prompt",
              model_ref: "default:gpt-4o-mini",
              permissions: {
                edit: "deny",
                shell: "deny",
                task: "deny",
              },
              tools: [],
            },
          },
          default_agent: "build",
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow",
              task: "allow",
            },
            shell_allowlist: {
              executables: ["git"],
              cwd_roots: ["."],
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
    .unwrap_or_abort();

    let coordinator_config =
        // act
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();

    // assert
    assert_eq!(bootstrap::interactive_profile_name(&config), "build");
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("build"), PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("build"), PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("review"), PermissionKind::EditFs),
        PolicyDecision::Deny
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("review"), PermissionKind::Shell),
        PolicyDecision::Deny
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("review"), PermissionKind::Task),
        PolicyDecision::Deny
    );
}

#[test]
fn interactive_bootstrap_uses_discovered_markdown_prompt_when_inline_missing() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    fs::create_dir_all(&repo).unwrap_or_abort();

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
            build: {
              description: "Build work",
              model_ref: "default:gpt-4o-mini",
              tools: [],
            },
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow",
            },
            shell_allowlist: {
              executables: ["git"],
              cwd_roots: ["."],
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
    .unwrap_or_abort();
    write_agent_markdown(&repo, "build", "Markdown-backed build prompt.");

    let config = load_config_from_repo_file(&config_path, &repo);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    // act
    let prompt = &coordinator_config.agent_profiles["build"].system_prompt;
    // assert
    assert!(prompt.starts_with("Markdown-backed build prompt."));
    assert!(prompt.contains("The exact model ID is default/gpt-4o-mini"));
}

#[test]
fn interactive_bootstrap_prepends_project_agents_md_to_agent_prompt() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    fs::create_dir_all(&repo).unwrap_or_abort();

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
            build: {
              description: "Build work",
              system_prompt: "Build prompt",
              model_ref: "default:gpt-4o-mini",
              tools: [],
            },
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow",
            },
            shell_allowlist: {
              executables: ["git"],
              cwd_roots: ["."],
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
    .unwrap_or_abort();
    fs::write(repo.join("AGENTS.md"), "Project-wide instructions.")
        .unwrap_or_abort();

    let config = load_config_from_repo_file(&config_path, &repo);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    // act
    let prompt = &coordinator_config.agent_profiles["build"].system_prompt;
    // assert
    assert!(prompt.contains("Instructions from:"));
    assert!(prompt.contains("Project-wide instructions."));
    assert!(prompt.starts_with("Build prompt"));
    assert!(prompt.contains("The exact model ID is default/gpt-4o-mini"));
}

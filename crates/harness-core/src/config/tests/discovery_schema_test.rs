use super::*;

#[test]
fn relative_paths_remain_cwd_relative_when_loading_from_file() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let config_path = temp.path().join("nested/config.jsonc");
    fs::create_dir_all(config_path.parent().expect("config parent")).expect("create config parent");
    let cfg = r#"
            {
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  models: {
                    "gpt-4o-mini": {
                      display_name: "GPT-4o mini"
                    }
                  }
                }
              },
              agents: {
                deep: {
                  description: "Deep work",
                  model_ref: "default:gpt-4o-mini",
                  tools: ["fs.read"]
                }
              },
              permissions: {
                defaults: {
                  edit: "ask",
                  shell: "ask",
                  network: "deny"
                }
              },
              runtime: {
                background_tasks: {
                  default_concurrency: 2,
                  provider_concurrency: 2,
                  model_concurrency: 2,
                  stale_timeout_ms: 15000,
                  message_staleness_timeout_ms: 5000
                },
                session_dir: "relative-sessions",
                permissions: {
                  ask_timeout_ms: 45000
                },
                prompt: {
                  wait_timeout_ms: 15000
                },
                deterministic: {
                  enabled: false,
                  seed: 42
                }
              },
              integrations: {
                remote_search: {
                  endpoint: "https://mcp.exa.ai/mcp"
                }
              },
              logging: {
                file: "logs/harness.log"
              }
            }
            "#;
    fs::write(&config_path, cfg).expect("write config");

    let parsed = load_config_from_file(&config_path).expect("config should parse");
    assert_eq!(
        parsed.runtime.session_dir,
        PathBuf::from("relative-sessions")
    );
    assert_eq!(parsed.paths.session_dir, PathBuf::from("relative-sessions"));
    assert_eq!(parsed.logging.file, Some(PathBuf::from("logs/harness.log")));
}

#[test]
fn schema_uses_runtime_first_public_contract() {
    let schema = harness_schema_pretty_json().expect("schema generation should succeed");
    let parsed: serde_json::Value =
        serde_json::from_str(&schema).expect("schema output should be valid json");
    let properties = parsed
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .expect("schema should contain properties");

    assert!(properties.contains_key("$schema"));
    assert!(properties.contains_key("provider"));
    assert!(properties.contains_key("agent"));
    assert!(properties.contains_key("default_agent"));
    assert!(properties.contains_key("permission"));
    assert!(properties.contains_key("model"));
    assert!(properties.contains_key("small_model"));
    assert!(properties.contains_key("mcp"));
    assert!(properties.contains_key("skills"));
    assert!(properties.contains_key("instructions"));
    assert!(!properties.contains_key("categories"));
    assert!(!properties.contains_key("profiles"));
    let runtime = properties
        .get("runtime")
        .and_then(|value| value.get("allOf"))
        .and_then(serde_json::Value::as_array)
        .expect("public runtime schema should expose the narrow runtime settings surface");
    let runtime_ref = runtime
        .first()
        .and_then(|value| value.get("$ref"))
        .and_then(serde_json::Value::as_str);
    assert_eq!(
        runtime_ref,
        Some("#/definitions/PublicRuntimeSettingsConfig")
    );
    assert!(!properties.contains_key("integrations"));
}

#[test]
fn public_top_level_skills_translate_into_runtime_config() {
    let parsed = load_config_from_str(
        r#"
            {
              model: "default:gpt-4o-mini",
              provider: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  models: {
                    "gpt-4o-mini": {
                      display_name: "GPT-4o mini"
                    }
                  }
                }
              },
              skills: {
                disabled: ["skill:project:disabled-doctor"],
                walkToGitRoot: false,
                permissions: {
                  "internal-*": "deny"
                }
              }
            }
            "#,
    )
    .expect("runtime config with top-level skills should parse");

    assert!(!parsed.skills.walk_to_git_root);
    assert_eq!(
        parsed.skills.project_roots,
        vec![
            PathBuf::from(".agent-harness/skills"),
            PathBuf::from(".harness/skills")
        ]
    );
    assert_eq!(
        parsed.skills.global_roots,
        vec![PathBuf::from("~/.config/agent-harness/skills")]
    );
    assert_eq!(
        parsed.skills.disabled,
        vec!["skill:project:disabled-doctor".to_string()]
    );
    assert_eq!(
        parsed.skills.permissions.get("internal-*"),
        Some(&PermissionMode::Deny)
    );
}

#[test]
fn json5_comments_trailing_commas_and_schema_field_parse() {
    let cfg = r#"
        {
          // optional editor schema hint
            "$schema": "./config.json",
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
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
              tools: ["fs.read",],
            },
          },
          permissions: {
            defaults: {
              edit: "ask",
              shell: "ask",
              network: "deny",
            },
            shell_allowlist: {
              executables: ["git",],
              cwd_roots: [".",],
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
              permissions: {
                ask_timeout_ms: 45000,
              },
              prompt: {
                wait_timeout_ms: 15000,
              },
              deterministic: {
                enabled: false,
                seed: 42,
              },
            },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp",
            },
          },
        }
        "#;

    let parsed = load_config_from_str(cfg).expect("json5 flavored config should parse");
    assert_eq!(parsed.schema.as_deref(), Some("./config.json"));
    assert_eq!(parsed.agents["deep"].model_ref, "default:gpt-4o-mini");
}

#[test]
fn resolve_config_path_prefers_explicit_path_over_discovery() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");
    let explicit_config = temp.path().join("explicit.jsonc");

    fs::create_dir_all(xdg_config.parent().expect("xdg parent")).expect("create xdg config dir");
    fs::write(&xdg_config, "xdg").expect("write xdg config");
    fs::write(&cwd_config, "cwd").expect("write cwd config");
    fs::write(&explicit_config, "explicit").expect("write explicit config");

    let context = discovery_context(temp.path(), Some(&xdg_root));

    assert_eq!(
        resolve_config_path_with_context(Some(&explicit_config), &context.discovery),
        Some(explicit_config)
    );
}

#[test]
fn resolve_config_path_prefers_cwd_harness_jsonc_over_xdg_config() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");

    fs::create_dir_all(xdg_config.parent().expect("xdg parent")).expect("create xdg config dir");
    fs::write(&xdg_config, "xdg").expect("write xdg config");
    fs::write(&cwd_config, "cwd").expect("write cwd config");

    let context = discovery_context(temp.path(), Some(&xdg_root));

    assert_eq!(
        resolve_config_path_with_context(None, &context.discovery),
        Some(cwd_config)
    );
}

#[test]
fn resolve_config_layer_paths_orders_global_then_local() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");

    fs::create_dir_all(xdg_config.parent().expect("xdg parent")).expect("create xdg config dir");
    fs::write(
        &xdg_config,
        "{ providers: {}, permissions: {}, runtime: {}, integrations: {}, agents: {} }",
    )
    .expect("write xdg config");
    fs::write(&cwd_config, "{ agents: {} }").expect("write cwd config");

    let context = discovery_context(temp.path(), Some(&xdg_root));

    assert_eq!(
        discovery::resolve_config_layer_paths_with_context(None, &context.discovery),
        vec![xdg_config, cwd_config]
    );
}

#[test]
fn resolve_config_layer_paths_include_env_and_project_ancestor_layers() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let repo = temp.path().join("repo");
    let nested = repo.join("workspace/app");
    let env_config = temp.path().join("env/harness.env.jsonc");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let repo_config = repo.join("harness.jsonc");
    let repo_dot_config = repo.join(".agent-harness/harness.jsonc");
    let nested_config = nested.join("harness.json");
    let nested_dot_config = nested.join(".agent-harness/harness.json");

    fs::create_dir_all(xdg_config.parent().expect("xdg parent")).expect("create xdg dir");
    fs::create_dir_all(
        repo_dot_config
            .parent()
            .expect("repo .agent-harness parent"),
    )
    .expect("create repo .agent-harness dir");
    fs::create_dir_all(
        nested_dot_config
            .parent()
            .expect("nested .agent-harness parent"),
    )
    .expect("create nested .agent-harness dir");
    fs::create_dir_all(env_config.parent().expect("env parent")).expect("create env dir");
    fs::create_dir_all(repo.join(".git")).expect("create repo git dir");

    for path in [
        &xdg_config,
        &env_config,
        &repo_config,
        &repo_dot_config,
        &nested_config,
        &nested_dot_config,
    ] {
        fs::write(path, "{}").expect("write placeholder config");
    }

    let mut context = discovery_context(&nested, Some(&xdg_root));
    context.discovery.runtime_config_path = Some(env_config.clone());
    assert_eq!(
        discovery::resolve_config_layer_paths_with_context(None, &context.discovery),
        vec![
            xdg_config,
            env_config,
            repo_config,
            repo_dot_config,
            nested_config,
            nested_dot_config,
        ]
    );
}

#[test]
fn load_resolved_config_merges_global_then_local_and_prefers_local_values() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");

    fs::create_dir_all(xdg_config.parent().expect("xdg parent")).expect("create xdg config dir");
    fs::write(
        &xdg_config,
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
              permissions: {
                defaults: {
                  edit: "ask",
                  shell: "deny",
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
                deterministic: {
                  enabled: false,
                  seed: 42,
                },
              },
              integrations: {
                remote_search: {
                  endpoint: "https://mcp.exa.ai/mcp",
                },
              },
            }
            "#,
    )
    .expect("write xdg config");
    fs::write(
        &cwd_config,
        r#"
            {
              agents: {
                build: {
                  description: "Build profile",
                  model_ref: "default:gpt-4o-mini",
                  tools: ["read"],
                },
              },
              permissions: {
                defaults: {
                  shell: "allow",
                },
              },
              ui: {
                default_profile: "build",
              },
            }
            "#,
    )
    .expect("write cwd config");

    let context = discovery_context(temp.path(), Some(&xdg_root));

    let loaded = load_resolved_config_with_context(None, &context)
        .expect("load resolved config")
        .expect("merged config should resolve");

    assert_eq!(loaded.paths, vec![xdg_config.clone(), cwd_config.clone()]);
    assert_eq!(loaded.config.ui.default_profile.as_deref(), Some("build"));
    assert!(matches!(
        loaded.config.permissions.defaults.shell,
        PermissionMode::Allow
    ));
    assert!(loaded.config.agents.contains_key("build"));
    assert_eq!(loaded.primary_path(), Some(cwd_config.as_path()));
}

#[test]
fn load_resolved_config_applies_harness_config_content_last() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        serde_json::json!({
            "provider": {
                "default": {
                    "type": "openai_compatible",
                    "base_url": "http://127.0.0.1:1/v1",
                    "api_key": "DUMMY",
                    "models": {
                        "gpt-4o": { "name": "GPT-4o" },
                        "gpt-4o-mini": { "name": "GPT-4o mini" }
                    }
                }
            },
            "model": "default/gpt-4o",
            "small_model": "default/gpt-4o-mini",
            "default_agent": "build",
            "permission": {
                "bash": "deny",
                "edit": "allow"
            }
        })
        .to_string(),
    )
    .expect("write config");

    let mut context = discovery_context(temp.path(), None);
    context.runtime_content =
        Some("{ permission: { bash: \"allow\" }, default_agent: \"plan\" }".to_string());
    let loaded = load_resolved_config_with_context(None, &context)
        .expect("load config")
        .expect("config should resolve");
    assert!(matches!(
        loaded.config.permissions.defaults.shell,
        PermissionMode::Allow
    ));
    assert_eq!(loaded.config.default_agent.as_deref(), Some("plan"));
}

#[test]
fn load_resolved_config_explicit_path_bypasses_discovery_layers() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/config.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");
    let explicit_config = temp.path().join("explicit.jsonc");

    fs::create_dir_all(xdg_config.parent().expect("xdg parent")).expect("create xdg config dir");
    fs::write(
        &xdg_config,
        "{ providers: {}, permissions: {}, runtime: {}, integrations: {}, agents: {} }",
    )
    .expect("write xdg config");
    fs::write(&cwd_config, "{ agents: {} }").expect("write cwd config");
    fs::write(
        &explicit_config,
        config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
    )
    .expect("write explicit config");

    let context = discovery_context(temp.path(), Some(&xdg_root));

    let loaded = load_resolved_config_with_context(Some(&explicit_config), &context)
        .expect("load explicit config")
        .expect("explicit config should resolve");

    assert_eq!(loaded.paths, vec![explicit_config]);
}

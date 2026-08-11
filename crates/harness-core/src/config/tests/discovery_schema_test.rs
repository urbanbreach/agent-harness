use super::*;
use crate::UnwrapOrAbort;

#[test]
fn relative_paths_remain_cwd_relative_when_loading_from_file() {
    // arrange
    // act
    // assert
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config_path = temp.path().join("nested/config.jsonc");
    fs::create_dir_all(config_path.parent().unwrap_or_abort()).unwrap_or_abort();
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
              model: "default/gpt-4o-mini",
              agent: {
                default: {
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
    fs::write(&config_path, cfg).unwrap_or_abort();

    let parsed = load_config_from_file(&config_path).unwrap_or_abort();
    assert_eq!(
        parsed.runtime.session_dir,
        PathBuf::from("relative-sessions")
    );
    assert_eq!(parsed.paths.session_dir, PathBuf::from("relative-sessions"));
    assert_eq!(parsed.logging.file, Some(PathBuf::from("logs/harness.log")));
}

#[test]
fn schema_uses_runtime_first_public_contract() {
    // arrange
    // act
    // assert
    let schema = harness_schema_pretty_json().unwrap_or_abort();
    let parsed: serde_json::Value = serde_json::from_str(&schema).unwrap_or_abort();
    let properties = parsed
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .unwrap_or_abort();

    assert!(properties.contains_key("$schema"));
    assert!(properties.contains_key("provider"));
    assert!(properties.contains_key("agent"));
    assert!(!properties.contains_key("default_agent"));
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
        .unwrap_or_abort();
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
fn inert_compatibility_keys_absent_from_generated_schema() {
    // arrange
    // act
    let schema = harness_schema_pretty_json().unwrap_or_abort();
    let parsed: serde_json::Value = serde_json::from_str(&schema).unwrap_or_abort();
    let properties = parsed
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .unwrap_or_abort();

    // assert
    for key in [
        "compaction",
        "experimental",
        "layout",
        "logLevel",
        "shell",
        "snapshot",
        "tool_output",
        "tools",
        "username",
        "watcher",
    ] {
        assert!(
            !properties.contains_key(key),
            "InertCompatibility key `{key}` must not appear in generated JSON schema properties"
        );
    }
}

#[test]
fn public_top_level_skills_translate_into_runtime_config() {
    // arrange
    // act
    // assert
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
    .unwrap_or_abort();

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
    // arrange
    // act
    // assert
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
          model: "default/gpt-4o-mini",
          agent: {
            default: {
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

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    assert_eq!(parsed.schema.as_deref(), Some("./config.json"));
    assert_eq!(parsed.agents["default"].model_ref, "default/gpt-4o-mini");
}

#[test]
fn resolve_config_path_prefers_explicit_path_over_discovery() {
    // arrange
    // act
    // assert
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");
    let explicit_config = temp.path().join("explicit.jsonc");

    fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(&xdg_config, "xdg").unwrap_or_abort();
    fs::write(&cwd_config, "cwd").unwrap_or_abort();
    fs::write(&explicit_config, "explicit").unwrap_or_abort();

    let context = discovery_context(temp.path(), Some(&xdg_root));

    assert_eq!(
        resolve_config_path_with_context(Some(&explicit_config), &context.discovery),
        Some(explicit_config)
    );
}

#[test]
fn resolve_config_path_prefers_cwd_harness_jsonc_over_xdg_config() {
    // arrange
    // act
    // assert
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");

    fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(&xdg_config, "xdg").unwrap_or_abort();
    fs::write(&cwd_config, "cwd").unwrap_or_abort();

    let context = discovery_context(temp.path(), Some(&xdg_root));

    assert_eq!(
        resolve_config_path_with_context(None, &context.discovery),
        Some(cwd_config)
    );
}

#[test]
fn resolve_config_layer_paths_orders_global_then_local() {
    // arrange
    // act
    // assert
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");

    fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(&xdg_config, "{ model: \"default/model\", agent: {} }").unwrap_or_abort();
    fs::write(&cwd_config, "{ model: \"default/model\", agent: {} }").unwrap_or_abort();

    let context = discovery_context(temp.path(), Some(&xdg_root));

    assert_eq!(
        discovery::resolve_config_layer_paths_with_context(None, &context.discovery),
        vec![xdg_config, cwd_config]
    );
}

#[test]
fn resolve_config_layer_paths_include_env_and_project_ancestor_layers() {
    // arrange
    // act
    // assert
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let repo = temp.path().join("repo");
    let nested = repo.join("workspace/app");
    let env_config = temp.path().join("env/harness.env.jsonc");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let repo_config = repo.join("harness.jsonc");
    let repo_dot_config = repo.join(".agent-harness/harness.jsonc");
    let nested_config = nested.join("harness.json");
    let nested_dot_config = nested.join(".agent-harness/harness.json");

    fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::create_dir_all(repo_dot_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::create_dir_all(nested_dot_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::create_dir_all(env_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    for path in [
        &xdg_config,
        &env_config,
        &repo_config,
        &repo_dot_config,
        &nested_config,
        &nested_dot_config,
    ] {
        fs::write(path, "{}").unwrap_or_abort();
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
    // arrange
    // act
    // assert
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");

    fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
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
              model: "default/gpt-4o-mini",
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
    .unwrap_or_abort();
    fs::write(
        &cwd_config,
        r#"
            {
              provider: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  models: {
                    "gpt-4o-mini": { display_name: "GPT-4o mini" },
                  },
                },
              },
              model: "default/gpt-4o-mini",
              agent: {
                default: {
                  tools: ["read"],
                },
              },
              permissions: {
                defaults: {
                  shell: "allow",
                },
              },
            }
            "#,
    )
    .unwrap_or_abort();

    let context = discovery_context(temp.path(), Some(&xdg_root));

    let loaded = load_resolved_config_with_context(None, &context)
        .unwrap_or_else(|error| panic!("layered config should load: {error}"))
        .unwrap_or_abort();

    assert_eq!(loaded.paths, vec![xdg_config.clone(), cwd_config.clone()]);
    assert!(matches!(
        loaded.config.permissions.defaults.shell,
        PermissionMode::Allow
    ));
    assert!(loaded.config.agents.contains_key("default"));
    assert_eq!(loaded.primary_path(), Some(cwd_config.as_path()));
}

#[test]
fn load_resolved_config_applies_harness_config_content_last() {
    // arrange
    // act
    // assert
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        serde_json::json!({
            "provider": {
                "default": {
                    "type": "openai_compatible",
                    "options": {
                        "baseURL": "http://127.0.0.1:8317/v1",
                        "apiKey": "DUMMY"
                    },
                    "models": {
                        "gpt-4o": { "name": "GPT-4o" },
                        "gpt-4o-mini": { "name": "GPT-4o mini" }
                    }
                }
            },
            "model": "default/gpt-4o",
            "small_model": "default/gpt-4o-mini",
            "permission": {
                "bash": "deny",
                "edit": "allow"
            }
        })
        .to_string(),
    )
    .unwrap_or_abort();

    let mut context = discovery_context(temp.path(), None);
    context.runtime_content = Some(public_minimal_config_with_permission(
        r#"{ bash: "allow", edit: "allow" }"#,
    ));
    let loaded = load_resolved_config_with_context(None, &context)
        .unwrap_or_else(|error| panic!("content overlay should load: {error}"))
        .unwrap_or_abort();
    assert!(matches!(
        loaded.config.permissions.defaults.shell,
        PermissionMode::Allow
    ));
    assert_eq!(
        loaded
            .config
            .agents
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["default", "explore", "general", "librarian"]
    );
}

#[test]
fn load_resolved_config_explicit_path_bypasses_discovery_layers() {
    // arrange
    // act
    // assert
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/config.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");
    let explicit_config = temp.path().join("explicit.jsonc");

    fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(&xdg_config, "{ model: \"default/model\", agent: {} }").unwrap_or_abort();
    fs::write(&cwd_config, "{ model: \"default/model\", agent: {} }").unwrap_or_abort();
    fs::write(
        &explicit_config,
        config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
    )
    .unwrap_or_abort();

    let context = discovery_context(temp.path(), Some(&xdg_root));

    let loaded = load_resolved_config_with_context(Some(&explicit_config), &context)
        .unwrap_or_abort()
        .unwrap_or_abort();

    assert_eq!(loaded.paths, vec![explicit_config]);
}

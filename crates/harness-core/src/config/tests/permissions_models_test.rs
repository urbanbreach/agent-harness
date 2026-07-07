use super::*;
use crate::UnwrapOrAbort;

#[test]
fn permission_scalar_expands_to_public_kinds_and_network() {
    for (raw, mode) in [
        ("\"ask\"", PermissionMode::Ask),
        ("\"allow\"", PermissionMode::Allow),
        ("\"deny\"", PermissionMode::Deny),
    ] {
        let cfg = public_minimal_config_with_permission(raw);
        let parsed = load_config_from_str(&cfg).unwrap_or_abort();
        assert_eq!(parsed.permissions.defaults.edit, mode);
        assert_eq!(parsed.permissions.defaults.shell, mode);
        assert_eq!(parsed.permissions.defaults.network, mode);
        assert_eq!(parsed.permissions.defaults.question, Some(mode.clone()));
        assert_eq!(parsed.permissions.defaults.task, Some(mode.clone()));
        assert_eq!(parsed.permissions.defaults.webfetch, Some(mode.clone()));
        assert_eq!(parsed.permissions.defaults.websearch, Some(mode.clone()));
        assert_eq!(parsed.permissions.defaults.codesearch, Some(mode.clone()));
        assert_eq!(parsed.permissions.defaults.lsp, Some(mode));
    }
}

#[test]
fn permission_scalar_rejects_invalid_mode() {
    let cfg = public_minimal_config_with_permission("\"maybe\"");
    load_config_from_str(&cfg).expect_err("invalid permission scalar must fail");
}

#[test]
fn permission_object_accepts_per_tool_scalar_modes() {
    let cfg = public_minimal_config_with_permission(
        r#"{
                bash: "ask",
                edit: "deny",
                question: "allow",
                task: "ask",
                webfetch: "deny",
                websearch: "allow",
                codesearch: "deny",
                lsp: "allow"
            }"#,
    );
    let parsed = load_config_from_str(&cfg).unwrap_or_abort();

    assert_eq!(parsed.permissions.defaults.shell, PermissionMode::Ask);
    assert_eq!(parsed.permissions.defaults.edit, PermissionMode::Deny);
    assert_eq!(
        parsed.permissions.defaults.question,
        Some(PermissionMode::Allow)
    );
    assert_eq!(parsed.permissions.defaults.task, Some(PermissionMode::Ask));
    assert_eq!(
        parsed.permissions.defaults.webfetch,
        Some(PermissionMode::Deny)
    );
    assert_eq!(
        parsed.permissions.defaults.websearch,
        Some(PermissionMode::Allow)
    );
    assert_eq!(
        parsed.permissions.defaults.codesearch,
        Some(PermissionMode::Deny)
    );
    assert_eq!(parsed.permissions.defaults.lsp, Some(PermissionMode::Allow));
    assert!(parsed.permissions.rules.shell.is_empty());
    assert!(parsed.permissions.rules.edit.is_empty());
    assert!(parsed.permissions.rules.task.is_empty());
}

#[test]
fn permission_rule_object_preserves_shell_allowlist_and_rules() {
    let cfg = public_minimal_config_with_permission(
        r#"{
                "*": "deny",
                bash: {
                  "git status": "allow",
                  "cargo test*": "ask",
                  "*": "deny"
                },
                edit: {
                  "docs/**": "allow",
                  "crates/harness-core/src/config.rs": "ask",
                  "*": "deny"
                },
                task: {
                  "explore": "allow",
                  "review-*": "ask",
                  "*": "deny"
                },
                shell_allowlist: {
                  executables: ["git"],
                  cwd_roots: ["."]
                }
            }"#,
    );
    let parsed = load_config_from_str(&cfg).unwrap_or_abort();

    assert_eq!(
        parsed.permissions.defaults.question,
        Some(PermissionMode::Deny)
    );
    assert_eq!(parsed.permissions.shell_allowlist.executables, vec!["git"]);
    assert_eq!(parsed.permissions.shell_allowlist.cwd_roots, vec!["."]);
    assert_eq!(parsed.permissions.rules.shell.len(), 3);
    assert_eq!(parsed.permissions.rules.edit.len(), 3);
    assert_eq!(parsed.permissions.rules.task.len(), 3);
}

#[test]
fn shell_allowlist_loads_legacy_flat_shape_with_default_mode() {
    let cfg = public_minimal_config_with_permission(
        r#"{
                bash: "allow",
                shell_allowlist: {
                  executables: ["git"],
                  cwd_roots: ["."]
                }
            }"#,
    );

    let parsed = load_config_from_str(&cfg).unwrap_or_abort();

    assert_eq!(
        parsed.permissions.shell_allowlist.mode,
        ShellAllowlistMode::PermissionPatterns
    );
    assert_eq!(parsed.permissions.shell_allowlist.executables, vec!["git"]);
    assert_eq!(parsed.permissions.shell_allowlist.cwd_roots, vec!["."]);
}

#[test]
fn shell_allowlist_accepts_camel_case_cwd_roots_and_policy_mode() {
    let cfg = public_minimal_config_with_permission(
        r#"{
                bash: "allow",
                shellAllowlist: {
                  executables: ["cargo"],
                  cwdRoots: ["crates"],
                  policy_mode: "legacy_executables"
                }
            }"#,
    );

    let parsed = load_config_from_str(&cfg).unwrap_or_abort();

    assert_eq!(
        parsed.permissions.shell_allowlist.mode,
        ShellAllowlistMode::LegacyExecutables
    );
    assert_eq!(
        parsed.permissions.shell_allowlist.executables,
        vec!["cargo"]
    );
    assert_eq!(parsed.permissions.shell_allowlist.cwd_roots, vec!["crates"]);
}

#[test]
fn shell_allowlist_mode_round_trips_through_json() {
    let allowlist = ShellAllowlist {
        mode: ShellAllowlistMode::LegacyExecutables,
        executables: vec!["git".to_string()],
        cwd_roots: vec![".".to_string()],
    };

    let json = serde_json::to_value(&allowlist).unwrap_or_abort();
    assert_eq!(
        json.get("mode"),
        Some(&serde_json::json!("legacy_executables"))
    );

    let parsed: ShellAllowlist = serde_json::from_value(json).unwrap_or_abort();
    assert_eq!(parsed.mode, ShellAllowlistMode::LegacyExecutables);
    assert_eq!(parsed.executables, vec!["git"]);
    assert_eq!(parsed.cwd_roots, vec!["."]);

    let camel_case_alias: ShellAllowlist = serde_json::from_value(serde_json::json!({
        "policyMode": "legacy_executables",
    }))
    .unwrap_or_abort();
    assert_eq!(camel_case_alias.mode, ShellAllowlistMode::LegacyExecutables);
}

#[test]
fn permission_rule_rejects_invalid_selector_forms() {
    for permission in [
        r#"{ bash: { "/^git/": "allow" } }"#,
        r#"{ bash: { "cargo * test": "allow" } }"#,
        r#"{ edit: { "../secrets/**": "allow" } }"#,
        r#"{ edit: { "/tmp/file": "allow" } }"#,
        r#"{ edit: { "docs/*": "allow" } }"#,
        r#"{ bash: { "git status": "sometimes" } }"#,
        r#"{ bash: { "git status": { mode: "allow" } } }"#,
        r#"{ edit: { "docs/**": 1 } }"#,
        r#"{ task: { "/explore/": "allow" } }"#,
        r#"{ question: { "*": "allow" } }"#,
    ] {
        let cfg = public_minimal_config_with_permission(permission);
        load_config_from_str(&cfg).expect_err("invalid permission selector form must fail");
    }
}

#[test]
fn model_limit_modalities_and_options_normalize_to_catalog_metadata() {
    let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
                timeoutMs: 30000
              },
              models: {
                "gpt-4o-mini": {
                  name: "GPT-4o mini",
                  limit: { context: 272000, input: 200000, output: 128000 },
                  modalities: { input: ["text", "image"], output: ["text"] },
                  options: { reasoning: { effort: "high" } },
                  variants: {
                    fast: {
                      name: "Fast",
                      limit: { context: 128000, input: 64000, output: 32000 },
                      modalities: { input: ["text"], output: ["text"] },
                      options: { temperature: 0.2 }
                    }
                  }
                }
              }
            }
          },
          model: "default/gpt-4o-mini",
          agent: {
            build: {
              system_prompt: "Build work",
              variant: "fast"
            }
          },
          default_agent: "build",
          permission: "allow"
        }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
    assert_eq!(provider.timeout_ms, 30_000);
    let model = &provider.models["gpt-4o-mini"];
    assert_eq!(model.limit.context, Some(272_000));
    assert_eq!(model.modalities.input, vec!["text", "image"]);
    assert!(model.options.contains_key("reasoning"));
    assert_eq!(model.variants["fast"].limit.output, Some(32_000));

    let metadata = resolve_profile_model_metadata(&parsed, "build").unwrap_or_abort();
    assert_eq!(metadata.context_window_tokens, Some(128_000));
    assert_eq!(metadata.max_input_tokens, Some(64_000));
    assert_eq!(metadata.max_output_tokens, Some(32_000));
}

#[test]
fn model_limit_rejects_unknown_metadata_fields() {
    let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: {
                "gpt-4o-mini": {
                  name: "GPT-4o mini",
                  limit: { context: 272000, training: 1 }
                }
              }
            }
          },
          model: "default/gpt-4o-mini",
          permission: "allow"
        }
        "#;

    let err = load_config_from_str(cfg).expect_err("unknown limit field must fail");
    assert!(
        err.to_string().contains("unknown field `training`"),
        "unexpected error: {err}"
    );
}

#[test]
fn legacy_provider_name_and_options_normalize_to_runtime_shape() {
    let cfg = r#"
        {
          providers: {
            default: {
              type: "openai_compatible",
              name: "CLIProxyAPI",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "gpt-4o-mini": {
                  name: "GPT-4o mini"
                }
              }
            }
          },
          agents: {
            build: {
              description: "Build work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"]
            }
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
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
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          }
        }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
    assert_eq!(provider.name.as_deref(), Some("CLIProxyAPI"));
    assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
    assert_eq!(provider.api_key, "test-key");
    assert_eq!(provider.models["gpt-4o-mini"].display_name, "GPT-4o mini");

    let metadata = resolve_profile_model_metadata(&parsed, "build").unwrap_or_abort();
    assert_eq!(metadata.provider_display_label, "CLIProxyAPI");
}

#[test]
fn top_level_legacy_agent_key_is_translated() {
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
          agent: {
            plan: {
              description: "Planning work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"]
            }
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
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
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          }
        }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    assert!(parsed.agents.contains_key("plan"));
}

#[test]
fn invalid_explicit_default_profile_falls_back_to_build_when_available() {
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
            build: {
              description: "Build work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"]
            },
            plan: {
              description: "Planning work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"]
            }
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
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
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          },
          ui: {
            default_profile: "ops"
          }
        }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    assert_eq!(parsed.ui.default_profile.as_deref(), Some("build"));
    assert_eq!(parsed.default_agent.as_deref(), Some("build"));
}

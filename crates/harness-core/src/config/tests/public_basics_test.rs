use super::*;
use crate::UnwrapOrAbort;

#[test]
fn structured_summary_contract_defaults_on_and_serializes_alias() {
    let default_compaction: CompactionRuntimeConfig =
        serde_json::from_value(serde_json::json!({})).unwrap_or_abort();
    assert!(default_compaction.structured_summary_contract);
    assert!(default_compaction.estimated_token_triggers);
    assert_eq!(default_compaction.fallback_input_tokens, 32_768);

    let disabled_via_alias: CompactionRuntimeConfig = serde_json::from_value(serde_json::json!({
        "structuredSummaryContract": false,
        "estimatedTokenTriggers": false,
        "fallbackInputTokens": 65_536,
    }))
    .unwrap_or_abort();
    assert!(!disabled_via_alias.structured_summary_contract);
    assert!(!disabled_via_alias.estimated_token_triggers);
    assert_eq!(disabled_via_alias.fallback_input_tokens, 65_536);

    let serialized = serde_json::to_value(&disabled_via_alias).unwrap_or_abort();
    assert_eq!(
        serialized.get("structured_summary_contract"),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(
        serialized.get("estimated_token_triggers"),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(
        serialized.get("fallback_input_tokens"),
        Some(&serde_json::Value::from(65_536))
    );
}

#[test]
fn example_config_parses_public_agents_and_compaction() {
    let agents = r#"
            deep: {
              description: "Default deep execution profile",
              model_ref: "default:gpt-4o-mini",
              tools: ["read"],
            },
            review: {
              description: "Review profile",
              model_ref: "default:gpt-4o-mini",
              max_iters: 20,
              tool_failure_mode: "continue_as_tool_message",
              tools: ["read", "invalid"],
            },
        "#;

    let text = config_fixture(
        agents,
        "test-openai-api-key",
        Some(
            r#"
                ui: {
                  default_profile: "deep",
                },
                "#,
        ),
        Some("./config.json"),
    );
    let parsed = load_config_from_str(&text).unwrap_or_abort();

    assert_eq!(parsed.schema.as_deref(), Some("./config.json"));
    assert!(parsed.providers.contains_key("default"));
    assert!(parsed.agents.contains_key("deep"));
    assert_eq!(
        parsed.agents["review"].tool_failure_mode,
        ToolFailureMode::ContinueAsToolMessage
    );
    assert_eq!(parsed.agents["review"].max_iters, Some(20));
    assert_eq!(
        parsed.permissions.defaults.question,
        Some(PermissionMode::Ask)
    );
    assert_eq!(parsed.permissions.defaults.task, Some(PermissionMode::Ask));
    assert_eq!(
        parsed.permissions.defaults.webfetch,
        Some(PermissionMode::Deny)
    );
    assert_eq!(
        parsed.permissions.defaults.websearch,
        Some(PermissionMode::Deny)
    );
    assert_eq!(
        parsed.permissions.defaults.codesearch,
        Some(PermissionMode::Deny)
    );
    assert_eq!(parsed.permissions.defaults.lsp, Some(PermissionMode::Allow));
    assert_eq!(
        parsed.runtime.session_dir,
        PathBuf::from(".agent-harness/sessions")
    );
    assert_eq!(parsed.runtime.permissions.ask_timeout_ms, 45_000);
    assert_eq!(parsed.runtime.prompt.wait_timeout_ms, 15_000);
    assert_eq!(parsed.background_task.default_concurrency, 2);
    assert_eq!(
        parsed.paths.session_dir,
        PathBuf::from(".agent-harness/sessions")
    );
}

#[test]
fn built_in_lsp_presets_accept_override_only_entries() {
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
                session_dir: ".agent-harness/sessions",
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
              lsp: {
                servers: {
                  python: {
                    command: ["pyright-langserver", "--stdio"],
                    env: {
                      PYRIGHT_PYTHON_FORCE_VERSION: "latest"
                    }
                  },
                  go: {
                    command: ["gopls"]
                  },
                  yaml: {
                    initialization: {
                      yaml: {
                        keyOrdering: false
                      }
                    }
                  },
                  json: {
                    disabled: true
                  }
                }
              }
            }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    assert!(parsed.lsp.servers.contains_key("python"));
    assert!(parsed.lsp.servers.contains_key("go"));
    assert!(parsed.lsp.servers.contains_key("yaml"));
    assert!(parsed.lsp.servers.contains_key("json"));
}

#[test]
fn model_variant_metadata_accepts_max_reasoning_effort() {
    let parsed = load_config_from_str(
        r#"
            {
              provider: {
                "umans-ai-coding-plan": {
                  type: "openai_compatible",
                  options: {
                    baseURL: "https://api.code.umans.ai/v1",
                    apiKeyEnv: ["UMANS_API_KEY"]
                  },
                  models: {
                    "umans-glm-5.2": {
                      name: "GLM 5.2",
                      variants: {
                        high: {
                          name: "High",
                          metadata: { reasoningEffort: "high" }
                        },
                        max: {
                          name: "Max",
                          metadata: { reasoningEffort: "max" }
                        }
                      }
                    }
                  }
                }
              },
              model: "umans-ai-coding-plan/umans-glm-5.2",
              agent: {
                build: {
                  system_prompt: "Build work"
                }
              },
              default_agent: "build"
            }
        "#,
    )
    .unwrap_or_abort();
    let catalog = configured_model_catalog(&parsed);

    assert!(catalog.iter().any(|entry| {
        entry.provider == "umans-ai-coding-plan"
            && entry.model == "umans-glm-5.2"
            && entry.variant.as_deref() == Some("max")
            && entry.reasoning_effort.as_deref() == Some("max")
    }));
}

#[test]
fn missing_required_sections_are_deterministic() {
    let err = load_config_from_str("{}").expect_err("config without agents must fail");
    assert_eq!(err.to_string(), "missing required config sections: agents");
}

#[test]
fn provider_keys_reject_terminal_control_characters_without_echoing_them() {
    let err = load_config_from_str(
        r#"
            {
              provider: {
                "evil\u001b]52;c;SGFja2Vk\u0007": {
                  type: "openai_compatible",
                  options: {
                    baseURL: "http://127.0.0.1:8317/v1",
                    apiKey: "test-key"
                  },
                  models: {
                    "gpt-4o-mini": {
                      name: "GPT-4o mini"
                    }
                  }
                }
              },
              model: "evil\u001b]52;c;SGFja2Vk\u0007/gpt-4o-mini",
              agent: {
                build: {
                  system_prompt: "Build work"
                }
              },
              default_agent: "build",
              permission: "ask"
            }
            "#,
    )
    .expect_err("provider keys with terminal controls must fail");
    let message = err.to_string();

    assert!(message.contains("invalid provider id"));
    assert!(!message.contains('\u{1b}'), "message: {message:?}");
    assert!(!message.contains('\u{7}'), "message: {message:?}");
}

#[test]
fn retired_top_level_keys_fail_with_migration_guidance() {
    let parsed = load_config_from_str(
        r#"
            {
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
              model: "default/gpt-4o-mini",
              categories: {
                deep: {
                  description: "Deep work",
                  model_ref: "default:gpt-4o-mini",
                  tools: ["read"]
                }
              },
              backgroundTask: {
                defaultConcurrency: 2,
                providerConcurrency: 2,
                modelConcurrency: 2,
                staleTimeoutMs: 30000,
                messageStalenessTimeoutMs: 10000
              },
              paths: {
                session_dir: ".agent-harness/sessions"
              },
              deterministic: {
                enabled: false,
                seed: 42
              }
            }
            "#,
    )
    .unwrap_or_abort();

    assert!(parsed.agents.contains_key("deep"));
    assert_eq!(parsed.runtime.background_tasks.default_concurrency, 2);
    assert_eq!(
        parsed.paths.session_dir,
        PathBuf::from(".agent-harness/sessions")
    );
}

#[test]
fn runtime_background_tasks_camel_case_aliases_parse_without_duplicate_fields() {
    let parsed = load_config_from_str(
        r#"
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
                  tools: ["read"]
                }
              },
              runtime: {
                background_tasks: {
                  defaultConcurrency: 2,
                  providerConcurrency: 3,
                  modelConcurrency: 4,
                  staleTimeoutMs: 30000,
                  messageStalenessTimeoutMs: 10000
                },
                permissions: {
                  askTimeoutMs: 777
                },
                prompt: {
                  waitTimeoutMs: 999
                },
                sessionDir: ".agent-harness/custom-sessions"
              }
            }
            "#,
    )
    .unwrap_or_abort();

    assert_eq!(parsed.runtime.background_tasks.default_concurrency, 2);
    assert_eq!(parsed.runtime.background_tasks.provider_concurrency, 3);
    assert_eq!(parsed.runtime.background_tasks.model_concurrency, 4);
    assert_eq!(parsed.runtime.background_tasks.stale_timeout_ms, 30000);
    assert_eq!(
        parsed.runtime.background_tasks.message_staleness_timeout_ms,
        10000
    );
    assert_eq!(parsed.runtime.permissions.ask_timeout_ms, 777);
    assert_eq!(parsed.runtime.prompt.wait_timeout_ms, 999);
    assert_eq!(
        parsed.runtime.session_dir,
        PathBuf::from(".agent-harness/custom-sessions")
    );
}

#[test]
fn unknown_top_level_key_is_rejected_strictly() {
    let cfg = r#"
            {
              extraTopLevel: true,
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
                session_dir: ".agent-harness/sessions",
                deterministic: {
                  enabled: false,
                  seed: 42
                }
              },
              integrations: {
                remote_search: {
                  endpoint: "https://mcp.exa.ai/mcp"
                }
              }
            }
            "#;

    let err = load_config_from_str(cfg).expect_err("unknown top-level key must fail");
    assert!(err
        .to_string()
        .contains("unknown top-level config keys: `extraTopLevel`"));
    assert!(err.to_string().contains("`provider`"));
}

#[test]
fn top_level_hashline_edit_alias_and_default_are_accepted() {
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
              tools: ["read"]
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
            session_dir: ".agent-harness/sessions",
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
          hashlineEdit: false
        }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    assert!(!parsed.hashline_edit);

    let defaults_cfg = r#"
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
              tools: ["read"]
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
            session_dir: ".agent-harness/sessions",
            deterministic: {
              enabled: false,
              seed: 42
            }
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          }
        }
        "#;

    let parsed_defaults = load_config_from_str(defaults_cfg).unwrap_or_abort();
    assert!(parsed_defaults.hashline_edit);
}

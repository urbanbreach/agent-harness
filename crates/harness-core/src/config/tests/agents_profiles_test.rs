use super::*;
use crate::auth::ProviderId;
use crate::UnwrapOrAbort;

#[test]
fn top_level_default_agent_camel_case_alias_is_accepted() {
    // arrange
    // act
    // assert
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
            }
          },
          defaultAgent: "build",
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
    assert_eq!(parsed.default_agent.as_deref(), Some("build"));
    assert_eq!(parsed.ui.default_profile.as_deref(), Some("build"));
}

#[test]
fn unknown_nested_key_is_rejected_strictly() {
    // arrange
    // act
    // assert
    let cfg = config_fixture(
        &deep_profile(
            r#"
                tools: ["fs.read"],
                unexpected_profile_field: true,
                "#,
        ),
        "test-key",
        None,
        None,
    );

    let err = load_config_from_str(&cfg).expect_err("unknown nested key must fail");
    assert!(err
        .to_string()
        .contains("unknown field `unexpected_profile_field`"));
}

#[test]
fn profile_model_ref_provider_must_exist() {
    // arrange
    // act
    // assert
    let cfg = config_fixture(
        r#"
            deep: {
              description: "Deep work",
              model_ref: "missing:gpt-4o-mini",
              tools: ["fs.read"],
            },
            "#,
        "test-key",
        None,
        None,
    );

    let err = load_config_from_str(&cfg).expect_err("unknown provider must fail");
    assert_eq!(
            err.to_string(),
            "agent `deep` has invalid model selection `missing:gpt-4o-mini`: model selector: unknown provider `missing`; available providers: default"
        );
}

#[test]
fn profile_rejects_legacy_plan_mode_field() {
    // arrange
    // act
    // assert
    let cfg = config_fixture(
        &deep_profile(
            r#"
                plan_mode: true,
                tools: ["fs.read"],
                "#,
        ),
        "test-key",
        None,
        None,
    );

    let err = load_config_from_str(&cfg).expect_err("legacy plan_mode must fail");
    assert!(err.to_string().contains("unknown field `plan_mode`"));
}

#[test]
fn profile_rejects_legacy_exit_target_profile_field() {
    // arrange
    // act
    // assert
    let cfg = config_fixture(
        &deep_profile(
            r#"
                exit_target_profile: "build",
                tools: ["fs.read"],
                "#,
        ),
        "test-key",
        None,
        None,
    );

    let err = load_config_from_str(&cfg).expect_err("legacy exit_target_profile must fail");
    assert!(err
        .to_string()
        .contains("unknown field `exit_target_profile`"));
}

#[test]
fn ui_default_profile_must_exist() {
    // arrange
    // act
    // assert
    let cfg = config_fixture(
        &deep_profile(r#"tools: ["fs.read"],"#),
        "test-key",
        Some(
            r#"
                ui: {
                  default_profile: "ops",
                },
                "#,
        ),
        None,
    );

    let err = load_config_from_str(&cfg).expect_err("unknown ui default profile must fail");
    assert_eq!(
        err.to_string(),
        "ui.default_profile references unknown agent `ops`; available agents: deep"
    );
}

#[test]
fn default_agent_normalizes_to_runtime_shape() {
    // arrange
    // act
    // assert
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
              permissions: {
                edit: "deny",
                shell: "deny"
              },
              tools: ["fs.read"]
            }
          },
          default_agent: "build",
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
    assert!(parsed.agents.contains_key("build"));
    assert!(parsed.agents.contains_key("plan"));
    assert_eq!(parsed.ui.default_profile.as_deref(), Some("build"));
    assert_eq!(parsed.default_agent.as_deref(), Some("build"));
    assert_eq!(
        parsed.agents["plan"].permissions.as_ref().unwrap().edit,
        Some(PermissionMode::Deny)
    );
    assert_eq!(
        parsed.agents["plan"].permissions.as_ref().unwrap().shell,
        Some(PermissionMode::Deny)
    );
}

#[test]
fn public_default_agents_continue_after_tool_failures() {
    // arrange
    // act
    // assert
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
                  name: "GPT-4o mini"
                }
              }
            }
          },
          model: "default/gpt-4o-mini",
          small_model: "default/gpt-4o-mini",
          agent: {
            build: {
              system_prompt: "Build work"
            }
          },
          default_agent: "build",
          permission: {
            edit: "allow",
            bash: "allow",
            question: "allow",
            task: "allow",
            webfetch: "allow",
            websearch: "allow",
            codesearch: "allow",
            lsp: "allow"
          }
        }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    assert_eq!(
        parsed.agents["build"].tool_failure_mode,
        ToolFailureMode::ContinueAsToolMessage
    );
    assert!(parsed.agents.contains_key("plan"));
    assert_eq!(parsed.agents["plan"].mode, AgentMode::Primary);
    let plan = parsed.agents["plan"].permissions.as_ref().unwrap();
    assert_eq!(plan.shell, Some(PermissionMode::Ask));
    assert_eq!(plan.task, Some(PermissionMode::Allow));
    assert!(plan.rules.edit.iter().any(|rule| matches!(
        (&rule.selector, &rule.mode),
        (PermissionSelector::CatchAll, PermissionMode::Deny)
    )));
    assert!(plan.rules.edit.iter().any(|rule| matches!(
        (&rule.selector, &rule.mode),
        (PermissionSelector::Prefix(prefix), PermissionMode::Allow)
            if prefix == ".agent-harness/plans/"
    )));

    let tools = &parsed.agents["plan"].tools;
    for required_tool in [
        "read",
        "glob",
        "grep",
        "list",
        "lsp",
        "question",
        "task",
        "background_output",
        "edit",
        "bash",
        "plan_exit",
    ] {
        assert!(
            tools.contains(&required_tool.to_string()),
            "plan profile should expose required tool {required_tool}"
        );
    }
    assert!(tools.contains(&"bash".to_string()));
    assert!(parsed.agents["build"]
        .tools
        .contains(&"plan_enter".to_string()));
    assert!(!tools.contains(&"plan_enter".to_string()));
}

#[test]
fn public_agent_schema_accepts_prompt_model_tool_map_and_metadata() {
    // arrange
    // act
    // assert
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
            build: {
              name: "Builder",
              prompt: "Build work",
              model: "default/gpt-4o-mini",
              topP: 0.8,
              mode: "primary",
              hidden: false,
              color: "blue",
              options: { reasoningEffort: "low" },
              steps: 12,
              tools: {
                read: true,
                bash: false,
                task: true
              }
            },
            explore: {
              disable: true
            },
            reviewer: {
              name: "Reviewer",
              description: "Review work",
              model: "default/gpt-4o-mini",
              mode: "subagent",
              hidden: true,
              maxSteps: 4,
              tools: ["read"]
            }
          },
          default_agent: "build",
          permission: "allow"
        }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    let build = &parsed.agents["build"];
    assert_eq!(build.name.as_deref(), Some("Builder"));
    assert_eq!(build.system_prompt.as_deref(), Some("Build work"));
    assert_eq!(build.model_ref, "default/gpt-4o-mini");
    assert_eq!(build.top_p, Some(0.8));
    assert_eq!(build.mode, AgentMode::Primary);
    assert!(!build.hidden);
    assert_eq!(build.color.as_deref(), Some("blue"));
    assert!(build.options.contains_key("reasoningEffort"));
    assert_eq!(build.max_iters, Some(12));
    assert_eq!(build.tools, vec!["read".to_string(), "task".to_string()]);
    assert!(!parsed.agents.contains_key("explore"));
    let reviewer = &parsed.agents["reviewer"];
    assert_eq!(reviewer.name.as_deref(), Some("Reviewer"));
    assert_eq!(reviewer.mode, AgentMode::Subagent);
    assert!(reviewer.hidden);
    assert_eq!(reviewer.max_iters, Some(4));
}

#[test]
fn openai_compatible_cache_retention_normalizes_from_options_and_rejects_conflicts() {
    // arrange
    // act
    // assert
    let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
                cacheRetention: "long"
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" }
              }
            }
          },
          model: "default/gpt-4o-mini",
          default_agent: "build",
        }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    let ProviderConfig::OpenAiCompatible(provider) = &parsed.providers["default"] else {
        panic!("expected OpenAiCompatible");
    };
    assert_eq!(
        provider.cache_retention,
        harness_providers::CacheRetention::Long
    );

    let conflicting = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              cacheRetention: "none",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
                cacheRetention: "long"
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" }
              }
            }
          },
          model: "default/gpt-4o-mini",
          default_agent: "build",
        }
        "#;

    let err = load_config_from_str(conflicting).expect_err("conflicting cache retention must fail");
    assert!(
        err.to_string().contains("options.cacheRetention"),
        "unexpected error: {err}"
    );
}

#[test]
fn openai_compatible_auth_provider_normalizes_from_options_and_rejects_conflicts() {
    // arrange
    // act
    // assert
    let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                authProvider: "codex",
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key"
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" }
              }
            }
          },
          model: "default/gpt-4o-mini",
          default_agent: "build",
        }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    let ProviderConfig::OpenAiCompatible(provider) = &parsed.providers["default"] else {
        panic!("expected OpenAiCompatible");
    };
    assert_eq!(provider.auth_provider, Some(ProviderId::codex()));

    let conflicting = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              authProvider: "codex",
              options: {
                authProvider: "github-copilot",
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key"
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" }
              }
            }
          },
          model: "default/gpt-4o-mini",
          default_agent: "build",
        }
        "#;

    let err = load_config_from_str(conflicting).expect_err("conflicting auth provider fails");
    assert!(
        err.to_string().contains("options.authProvider"),
        "unexpected error: {err}"
    );
}

#[test]
fn default_agent_rejects_subagent_only_and_hidden_profiles() {
    // arrange
    // act
    // assert
    for (field, value, expected) in [
        (
            "mode",
            "\"subagent\"",
            "must not reference a subagent-only profile",
        ),
        ("hidden", "true", "must not reference a hidden profile"),
    ] {
        let cfg = format!(
            r#"
        {{
          provider: {{
            default: {{
              type: "openai_compatible",
              options: {{
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              }},
              models: {{
                "gpt-4o-mini": {{ name: "GPT-4o mini" }}
              }}
            }}
          }},
          model: "default/gpt-4o-mini",
          agent: {{
            build: {{
              prompt: "Build work",
              {field}: {value}
            }}
          }},
          default_agent: "build",
          permission: "allow"
        }}
        "#
        );
        let err = load_config_from_str(&cfg).expect_err("invalid default agent must fail");
        assert!(err.to_string().contains(expected));
    }
}

#[test]
fn default_agent_rejects_disabled_profile() {
    // arrange
    // act
    // assert
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
            explore: { disable: true }
          },
          default_agent: "explore",
          permission: "allow"
        }
        "#;

    let err = load_config_from_str(cfg).expect_err("disabled default agent must fail");
    assert!(err
        .to_string()
        .contains("default_agent `explore` references a disabled agent"));
}

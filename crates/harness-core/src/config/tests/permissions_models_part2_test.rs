use super::*;
use crate::UnwrapOrAbort;

#[test]
fn legacy_provider_name_and_options_normalize_to_runtime_shape() {
    // arrange
    // act
    // assert
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

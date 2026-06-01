use harness_core::config::{
    load_config_from_str, resolve_model_selection, resolve_profile_model_metadata, HarnessConfig,
};
use harness_core::model_resolution::{ModelFamily, ModelFamilySource, PromptFamily};

fn variant_test_config() -> HarnessConfig {
    json5::from_str(
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
                "gpt-5.4-mini": {
                  display_name: "GPT-5.4 Mini",
                  metadata: {
                    context_window_tokens: 128000,
                  },
                  max_input_tokens: 128000,
                  max_output_tokens: 8192,
                  variants: {
                    deterministic: {
                      display_name: "Deterministic",
                      max_output_tokens: 4096,
                      metadata: {
                        description: "Deterministic mode",
                        reasoning_effort: "minimal",
                        text_verbosity: "low",
                        recommended_for: "deep",
                      },
                    },
                  },
                },
              },
            },
          },
          agents: {
            deep: {
              description: "Deep work",
              model_ref: "default:gpt-5.4-mini",
              variant: "deterministic",
              tools: ["fs.read"],
            },
          },
          permissions: {
            defaults: {
              edit: "ask",
              shell: "ask",
              network: "deny",
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
    .expect("config shape should deserialize")
}

fn profile_test_config() -> HarnessConfig {
    load_config_from_str(
        r#"
        {
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: {
                "gpt-5.4": {
                  display_name: "GPT-5.4",
                },
                "gpt-5.4-mini": {
                  display_name: "GPT-5.4 Mini",
                  variants: {
                    low: {
                      metadata: { reasoning_effort: "low", text_verbosity: "low" },
                    },
                    disabled: {
                      disabled: true,
                    },
                  },
                },
              },
            },
          },
          model_profile: {
            fast: {
              model: "default:gpt-5.4-mini",
              variant: "low",
              fallback: [
                { model: "default:gpt-5.4" },
              ],
            },
          },
          agents: {
            build: {
              description: "Build",
              model_ref: "fast",
              tools: ["read"],
            },
          },
          permissions: {
            defaults: { edit: "ask", shell: "ask", network: "deny" },
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
            remote_search: { endpoint: "https://mcp.exa.ai/mcp" },
          },
        }
        "#,
    )
    .expect("profile config should load")
}

#[test]
fn model_variant_resolution_returns_variant_display_and_metadata() {
    let metadata = resolve_profile_model_metadata(&variant_test_config(), "deep")
        .expect("variant metadata should resolve");

    assert_eq!(metadata.profile, "deep");
    assert_eq!(metadata.provider, "default");
    assert_eq!(metadata.model, "gpt-5.4-mini");
    assert_eq!(metadata.variant.as_deref(), Some("deterministic"));
    assert_eq!(metadata.display_label, "GPT-5.4 Mini · Deterministic");
    assert_eq!(
        metadata.token_window_label.as_deref(),
        Some("128k ctx · 128k in · 4k out")
    );
    assert_eq!(metadata.context_window_tokens, Some(128000));
    assert_eq!(metadata.max_input_tokens, Some(128000));
    assert_eq!(metadata.max_output_tokens, Some(4096));
    assert_eq!(metadata.description.as_deref(), Some("Deterministic mode"));
    assert_eq!(metadata.reasoning_effort.as_deref(), Some("minimal"));
    assert_eq!(metadata.text_verbosity.as_deref(), Some("low"));
    assert_eq!(metadata.recommended_for.as_deref(), Some("deep"));
    assert_eq!(metadata.resolution.family, ModelFamily::Gpt5);
    assert_eq!(
        metadata.resolution.family_source,
        ModelFamilySource::Heuristic
    );
    assert_eq!(metadata.resolution.prompt_family, PromptFamily::Gpt);
    assert_eq!(
        metadata.resolution.capabilities.context_window_tokens,
        Some(128000)
    );
}

#[test]
fn model_variant_resolution_allows_variant_context_window_override() {
    let config = load_config_from_str(
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              apiKey: "test-key",
              models: {
                "gpt-5.4": {
                  name: "GPT 5.4",
                  metadata: { context_window_tokens: 272000, supports_reasoning_summaries: true },
                  max_input_tokens: 272000,
                  max_output_tokens: 128000,
                  variants: {
                    "1m-high": {
                      name: "1M High",
                      context_window_tokens: 922000,
                      max_input_tokens: 922000,
                      metadata: { reasoning_effort: "high" },
                    },
                  },
                },
              },
            },
          },
          model: "default/gpt-5.4",
          agent: { build: { model: "default/gpt-5.4", variant: "1m-high" } },
          default_agent: "build",
          permission: { edit: "ask", bash: "ask" },
        }
        "#,
    )
    .expect("config should load");

    let metadata =
        resolve_profile_model_metadata(&config, "build").expect("variant metadata should resolve");

    assert_eq!(metadata.model, "gpt-5.4");
    assert_eq!(metadata.variant.as_deref(), Some("1m-high"));
    assert_eq!(metadata.context_window_tokens, Some(922000));
    assert_eq!(metadata.max_input_tokens, Some(922000));
    assert_eq!(
        metadata.token_window_label.as_deref(),
        Some("922k ctx · 922k in · 128k out")
    );
}

#[test]
fn model_variant_resolution_rejects_unknown_variant() {
    let mut config = variant_test_config();
    config.agents.get_mut("deep").expect("deep profile").variant = Some("ghost".to_string());

    let err = resolve_profile_model_metadata(&config, "deep").expect_err("variant must fail");

    assert_eq!(
        err.to_string(),
        "agent `deep` has invalid model selection `default:gpt-5.4-mini`: model selector: unknown variant `ghost` for model `default:gpt-5.4-mini`; available variants: deterministic"
    );
}

#[test]
fn named_model_profile_resolves_primary_and_fallback_order() {
    let config = profile_test_config();

    let selection = resolve_model_selection(&config, "fast", None).expect("profile resolves");

    assert_eq!(selection.profile.as_deref(), Some("fast"));
    assert_eq!(selection.primary.model_ref, "default:gpt-5.4-mini");
    assert_eq!(selection.primary.variant.as_deref(), Some("low"));
    assert_eq!(selection.primary.reasoning_effort.as_deref(), Some("low"));
    assert_eq!(selection.primary.resolution.family, ModelFamily::Gpt5);
    assert_eq!(selection.fallback.len(), 1);
    assert_eq!(selection.fallback[0].model_ref, "default:gpt-5.4");
    assert_eq!(selection.fallback[0].resolution.family, ModelFamily::Gpt5);
}

#[test]
fn model_resolution_prefers_metadata_family_and_exposes_capabilities() {
    let config = load_config_from_str(
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              apiKey: "test-key",
              models: {
                "enterprise-alpha": {
                  name: "Enterprise Alpha",
                  metadata: {
                    family: "gemini",
                    contextWindowTokens: 1048576,
                    supportsToolCalls: false,
                    supportsReasoningSummaries: true
                  },
                  modalities: { input: ["text", "image"], output: ["text"] },
                  limit: { input: 900000, output: 64000 },
                },
              },
            },
          },
          model: "default/enterprise-alpha",
          agent: {
            build: { model: "default/enterprise-alpha" },
          },
          default_agent: "build",
          permission: { edit: "ask", bash: "ask" },
        }
        "#,
    )
    .expect("config should load");

    let selection = resolve_model_selection(&config, "default/enterprise-alpha", None)
        .expect("direct model resolves");
    let resolution = &selection.primary.resolution;

    assert_eq!(resolution.family, ModelFamily::Gemini);
    assert_eq!(resolution.family_source, ModelFamilySource::Metadata);
    assert_eq!(resolution.prompt_family, PromptFamily::Gemini);
    assert!(resolution.capabilities.supports_vision);
    assert!(!resolution.capabilities.supports_tool_calls);
    assert!(resolution.capabilities.supports_reasoning_summaries);
    assert_eq!(resolution.capabilities.context_window_tokens, Some(1048576));
    assert_eq!(resolution.capabilities.max_input_tokens, Some(900000));
    assert_eq!(resolution.capabilities.max_output_tokens, Some(64000));
}

#[test]
fn direct_model_refs_do_not_resolve_as_profile_names() {
    let config = profile_test_config();

    let selection = resolve_model_selection(&config, "default/gpt-5.4", None)
        .expect("direct slash ref resolves");

    assert_eq!(selection.profile, None);
    assert_eq!(selection.primary.model_ref, "default:gpt-5.4");
    assert!(selection.fallback.is_empty());
}

#[test]
fn unqualified_unknown_selector_is_profile_only() {
    let config = profile_test_config();

    let err = resolve_model_selection(&config, "gpt-5.4", None)
        .expect_err("bare unknown selector should not become default provider model");

    assert!(err.to_string().contains("unknown model profile `gpt-5.4`"));
    assert!(err.to_string().contains("available profiles: fast"));
}

#[test]
fn named_model_profile_rejects_disabled_variant() {
    let err = load_config_from_str(
        r#"
        {
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: {
                "gpt-5.4-mini": {
                  display_name: "GPT-5.4 Mini",
                  variants: { disabled: { disabled: true } },
                },
              },
            },
          },
          modelProfile: {
            fast: { model: "default:gpt-5.4-mini", variant: "disabled" },
          },
          agents: {
            build: { description: "Build", model_ref: "fast", tools: ["read"] },
          },
          permissions: { defaults: { edit: "ask", shell: "ask", network: "deny" } },
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
          integrations: { remote_search: { endpoint: "https://mcp.exa.ai/mcp" } },
        }
        "#,
    )
    .expect_err("disabled profile variant should fail validation");

    assert!(err.to_string().contains("variant `disabled`"));
    assert!(err.to_string().contains("is disabled"));
}

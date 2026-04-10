use harness_core::config::{resolve_profile_model_metadata, HarnessConfig};

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
}

#[test]
fn model_variant_resolution_rejects_unknown_variant() {
    let mut config = variant_test_config();
    config.agents.get_mut("deep").expect("deep profile").variant = Some("ghost".to_string());

    let err = resolve_profile_model_metadata(&config, "deep").expect_err("variant must fail");

    assert_eq!(
        err.to_string(),
        "agent `deep` references unknown variant `ghost` for model `default:gpt-5.4-mini`; available variants: deterministic"
    );
}

#[test]
fn profile_reasoning_effort_applies_when_model_selection_has_no_variant_reasoning() {
    let mut config = variant_test_config();
    let profile = config.agents.get_mut("deep").expect("deep profile");
    profile.variant = None;
    profile.reasoning_effort = Some(harness_core::config::ModelVariantReasoningEffort::High);

    let metadata = resolve_profile_model_metadata(&config, "deep")
        .expect("profile reasoning metadata should resolve");

    assert_eq!(metadata.variant, None);
    assert_eq!(metadata.display_label, "GPT-5.4 Mini");
    assert_eq!(metadata.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(metadata.text_verbosity, None);
}

#[test]
fn profile_reasoning_effort_overrides_variant_reasoning_after_model_selection() {
    let mut config = variant_test_config();
    config
        .agents
        .get_mut("deep")
        .expect("deep profile")
        .reasoning_effort = Some(harness_core::config::ModelVariantReasoningEffort::High);

    let metadata = resolve_profile_model_metadata(&config, "deep")
        .expect("profile reasoning metadata should resolve");

    assert_eq!(metadata.variant.as_deref(), Some("deterministic"));
    assert_eq!(metadata.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(metadata.text_verbosity.as_deref(), Some("low"));
}

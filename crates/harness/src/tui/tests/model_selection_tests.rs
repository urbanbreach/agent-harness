use super::*;

#[test]
fn interactive_launch_metadata_exposes_catalog_and_cross_profile_switch_options() {
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
                "gpt-5.4-mini": {
                  display_name: "GPT-5.4 Mini",
                  variants: {
                    low: {
                      display_name: "Low"
                    },
                    medium: {
                      display_name: "Medium"
                    },
                    high: {
                      display_name: "High"
                    },
                    xhigh: {
                      display_name: "XHigh"
                    }
                  }
                },
                "gpt-5.4": {
                  display_name: "GPT-5.4"
                }
              }
            }
          },
          agents: {
            build: {
              description: "Implementation",
              system_prompt: "Implement carefully.",
              model_ref: "default:gpt-5.4-mini",
              tools: []
            },
            plan: {
              description: "Planning",
              system_prompt: "Plan carefully.",
              model_ref: "default:gpt-5.4-mini",
              variant: "low",
              tools: []
            },
            ops: {
              description: "Operations",
              system_prompt: "Operate carefully.",
              model_ref: "default:gpt-5.4",
              tools: []
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
        "#,
    )
    .expect("config should parse");

    let agent_profiles = bootstrap::interactive_agent_profiles(&config)
        .expect("interactive agent profiles should build");
    let metadata = interactive_launch_metadata(Some(&config), &agent_profiles, "build")
        .expect("launch metadata should build");

    assert!(metadata
        .available_models()
        .iter()
        .any(|option| option.profile == "build"));
    assert!(metadata
        .available_models()
        .iter()
        .any(|option| option.profile == "ops" && option.model == "gpt-5.4"));
    assert!(metadata
        .available_models()
        .iter()
        .any(|option| option.profile == "build" && option.model == "gpt-5.4"));
    let mut mini_variants = metadata
        .available_models()
        .iter()
        .filter(|option| option.profile == "build" && option.model == "gpt-5.4-mini")
        .filter_map(|option| option.variant.as_deref())
        .collect::<Vec<_>>();
    mini_variants.sort_unstable();
    assert_eq!(mini_variants, vec!["high", "low", "medium", "xhigh"]);
}

#[test]
fn shipped_example_config_preserves_configured_model_variant() {
    let config_path = crate::cli_config::shipped_example_config_path();
    let config = harness_core::config::load_config_from_file(&config_path)
        .expect("shipped example config should parse with discovered prompts");

    let agent_profiles = bootstrap::interactive_agent_profiles(&config)
        .expect("interactive agent profiles should build");
    let metadata = interactive_launch_metadata(Some(&config), &agent_profiles, "build")
        .expect("launch metadata should build");

    assert_eq!(metadata.profile(), "build");
    assert_eq!(metadata.variant(), Some("high"));
}

#[test]
fn persisted_model_selection_restores_valid_variant_for_active_profile() {
    let base = LaunchMetadata::from_model_option(&ModelOption {
        profile: "build".to_string(),
        provider: "default".to_string(),
        provider_display_label: Some("Default".to_string()),
        provider_backend_label: None,
        model: "gpt-5.4-mini".to_string(),
        model_display_label: Some("GPT-5.4 Mini".to_string()),
        variant: None,
        variant_display_label: None,
        display_label: Some("GPT-5.4 Mini".to_string()),
        token_window_label: None,
        context_window_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: None,
        reasoning_effort: None,
        text_verbosity: None,
        recommended_for: None,
    })
    .with_available_models(vec![
        ModelOption {
            profile: "build".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("Default".to_string()),
            provider_backend_label: None,
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: None,
            variant_display_label: None,
            display_label: Some("GPT-5.4 Mini".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        },
        ModelOption {
            profile: "build".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("Default".to_string()),
            provider_backend_label: None,
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("high".to_string()),
            variant_display_label: Some("High".to_string()),
            display_label: Some("GPT-5.4 Mini High".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: None,
            reasoning_effort: Some("high".to_string()),
            text_verbosity: None,
            recommended_for: None,
        },
    ]);

    let restored = apply_model_selection_to_launch_metadata(
        base,
        &PersistedModelSelection {
            schema_version: 1,
            profile: "build".to_string(),
            provider: "default".to_string(),
            model: "gpt-5.4-mini".to_string(),
            variant: Some("high".to_string()),
        },
    );

    assert_eq!(restored.profile(), "build");
    assert_eq!(restored.provider(), "default");
    assert_eq!(restored.model(), Some("gpt-5.4-mini"));
    assert_eq!(restored.variant(), Some("high"));
    assert_eq!(restored.reasoning_effort(), Some("high"));
}

#[test]
fn persisted_model_selection_preserves_switchable_profiles() {
    let base = LaunchMetadata::from_model_ref("ops", "default:gpt-5.4")
        .with_available_models(vec![ModelOption::from_model_ref("ops", "default:gpt-5.4")])
        .with_switchable_profiles(vec![
            "ops".to_string(),
            "build".to_string(),
            "plan".to_string(),
        ]);

    let restored = apply_model_selection_to_launch_metadata(
        base,
        &PersistedModelSelection {
            schema_version: 1,
            profile: "ops".to_string(),
            provider: "default".to_string(),
            model: "gpt-5.4".to_string(),
            variant: None,
        },
    );

    assert_eq!(restored.profile(), "ops");
    assert_eq!(restored.switchable_profiles(), ["ops", "build", "plan"]);
}

#[test]
fn persisted_model_selection_ignores_unconfigured_variant() {
    let base =
        LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini").with_available_models(
            vec![ModelOption::from_model_ref("build", "default:gpt-5.4-mini")],
        );
    let restored = apply_model_selection_to_launch_metadata(
        base.clone(),
        &PersistedModelSelection {
            schema_version: 1,
            profile: "build".to_string(),
            provider: "default".to_string(),
            model: "gpt-5.4-mini".to_string(),
            variant: Some("stale".to_string()),
        },
    );

    assert_eq!(restored, base);
}

#[test]
fn persisted_model_selection_round_trips_model_json() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("model.json");
    let metadata = LaunchMetadata::from_model_option(&ModelOption {
        profile: "build".to_string(),
        provider: "default".to_string(),
        provider_display_label: None,
        provider_backend_label: None,
        model: "gpt-5.4-mini".to_string(),
        model_display_label: None,
        variant: Some("xhigh".to_string()),
        variant_display_label: None,
        display_label: None,
        token_window_label: None,
        context_window_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: None,
        reasoning_effort: None,
        text_verbosity: None,
        recommended_for: None,
    });

    save_persisted_model_selection_to_path(&path, &metadata).expect("persist model selection");
    let selection = load_persisted_model_selection_from_path(&path).expect("load model selection");

    assert_eq!(selection.schema_version, 1);
    assert_eq!(selection.profile, "build");
    assert_eq!(selection.provider, "default");
    assert_eq!(selection.model, "gpt-5.4-mini");
    assert_eq!(selection.variant.as_deref(), Some("xhigh"));
}

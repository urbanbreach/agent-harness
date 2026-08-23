use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::config::{configured_model_catalog, load_config_from_str};
use harness_tui::app::ModelOption;
use harness_tui::UnwrapOrAbort;

pub(crate) fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub(crate) fn rich_model_config() -> &'static str {
    r#"
    {
      model: "default:gpt-5.4-mini",
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
              max_output_tokens: 16384,
              variants: {
                deterministic: {
                  display_name: "Deterministic",
                  max_output_tokens: 4096,
                  metadata: {
                    description: "Stable low-variance coding",
                    reasoning_effort: "minimal",
                    text_verbosity: "low",
                    recommended_for: "deep debugging",
                  },
                },
                creative: {
                  display_name: "Creative",
                  metadata: {
                    description: "Higher-variance drafting",
                    reasoning_effort: "high",
                    text_verbosity: "high",
                    recommended_for: "novel drafting",
                  },
                },
              },
            },
          },
        },
      },
      agent: {
        default: {
          model: "default:gpt-5.4-mini",
          variant: "deterministic",
          tools: ["read"],
        },
        general: {
          model: "default:gpt-5.4-mini",
          variant: "creative",
          tools: ["read"],
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
    "#
}

pub(crate) fn available_models() -> Vec<ModelOption> {
    vec![
        ModelOption::from_model_ref("default", "default:gpt-5.4-mini"),
        ModelOption::from_model_ref("general", "default:gpt-5.4-mini"),
    ]
}

pub(crate) fn primary_subagent_models() -> Vec<ModelOption> {
    vec![
        ModelOption::from_model_ref("default", "default:gpt-5.4-mini"),
        ModelOption {
            profile: "general".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: Some("128k ctx · 128k in · 4k out".to_string()),
            model_limits: harness_core::config::ResolvedModelLimits::compatibility_mirror(
                Some(128000),
                Some(128000),
                Some(4096),
            ),
            description: Some("Stable low-variance coding".to_string()),
            profile_description: None,
            reasoning_effort: Some("minimal".to_string()),
            text_verbosity: Some("low".to_string()),
            thinking: None,
            recommended_for: Some("research".to_string()),
        },
    ]
}

pub(crate) fn multi_provider_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            profile: "default".to_string(),
            provider: "anthropic".to_string(),
            provider_display_label: Some("Anthropic".to_string()),
            provider_backend_label: None,
            model: "claude-sonnet-4-5".to_string(),
            model_display_label: Some("Claude Sonnet 4.5".to_string()),
            variant: None,
            variant_display_label: None,
            display_label: Some("Claude Sonnet 4.5".to_string()),
            token_window_label: None,
            model_limits: harness_core::config::ResolvedModelLimits::compatibility_mirror(
                None, None, None,
            ),
            description: None,
            profile_description: None,
            reasoning_effort: None,
            text_verbosity: None,
            thinking: None,
            recommended_for: None,
        },
        ModelOption {
            profile: "default".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("OpenAI".to_string()),
            provider_backend_label: None,
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: None,
            variant_display_label: None,
            display_label: Some("GPT-5.4 Mini".to_string()),
            token_window_label: None,
            model_limits: harness_core::config::ResolvedModelLimits::compatibility_mirror(
                None, None, None,
            ),
            description: None,
            profile_description: None,
            reasoning_effort: None,
            text_verbosity: None,
            thinking: None,
            recommended_for: None,
        },
    ]
}

pub(crate) fn duplicate_primary_subagent_models() -> Vec<ModelOption> {
    vec![
        ModelOption {
            profile: "default".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: Some("128k ctx · 128k in · 4k out".to_string()),
            model_limits: harness_core::config::ResolvedModelLimits::compatibility_mirror(
                Some(128000),
                Some(128000),
                Some(4096),
            ),
            description: Some("Stable low-variance coding".to_string()),
            profile_description: None,
            reasoning_effort: Some("minimal".to_string()),
            text_verbosity: Some("low".to_string()),
            thinking: None,
            recommended_for: Some("implementation".to_string()),
        },
        ModelOption {
            profile: "general".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: Some("128k ctx · 128k in · 4k out".to_string()),
            model_limits: harness_core::config::ResolvedModelLimits::compatibility_mirror(
                Some(128000),
                Some(128000),
                Some(4096),
            ),
            description: Some("Stable low-variance coding".to_string()),
            profile_description: None,
            reasoning_effort: Some("minimal".to_string()),
            text_verbosity: Some("low".to_string()),
            thinking: None,
            recommended_for: Some("research".to_string()),
        },
    ]
}

pub(crate) fn same_profile_variant_options() -> Vec<ModelOption> {
    vec![
        ModelOption {
            profile: "default".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: Some("128k ctx · 128k in · 4k out".to_string()),
            model_limits: harness_core::config::ResolvedModelLimits::compatibility_mirror(
                Some(128000),
                Some(128000),
                Some(4096),
            ),
            description: Some("Stable low-variance coding".to_string()),
            profile_description: None,
            reasoning_effort: Some("minimal".to_string()),
            text_verbosity: Some("low".to_string()),
            thinking: None,
            recommended_for: Some("debugging".to_string()),
        },
        ModelOption {
            profile: "default".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("creative".to_string()),
            variant_display_label: Some("Creative".to_string()),
            display_label: Some("GPT-5.4 Mini · Creative".to_string()),
            token_window_label: Some("128k ctx · 128k in · 16k out".to_string()),
            model_limits: harness_core::config::ResolvedModelLimits::compatibility_mirror(
                Some(128000),
                Some(128000),
                Some(16384),
            ),
            description: Some("Higher-variance drafting".to_string()),
            profile_description: None,
            reasoning_effort: Some("high".to_string()),
            text_verbosity: Some("high".to_string()),
            thinking: None,
            recommended_for: Some("novel drafting".to_string()),
        },
    ]
}

pub(crate) fn reasoning_order_variant_options() -> Vec<ModelOption> {
    [
        ("medium", "Medium", "medium"),
        ("high", "High", "high"),
        ("xhigh", "XHigh", "xhigh"),
    ]
    .into_iter()
    .map(|(variant, label, reasoning_effort)| ModelOption {
        profile: "default".to_string(),
        provider: "default".to_string(),
        provider_display_label: Some("default".to_string()),
        provider_backend_label: Some("OpenAI".to_string()),
        model: "gpt-5.4-mini".to_string(),
        model_display_label: Some("GPT-5.4 Mini".to_string()),
        variant: Some(variant.to_string()),
        variant_display_label: Some(label.to_string()),
        display_label: Some(format!("GPT-5.4 Mini · {label}")),
        token_window_label: Some("128k ctx · 128k in · 16k out".to_string()),
        model_limits: harness_core::config::ResolvedModelLimits::compatibility_mirror(
            Some(128000),
            Some(128000),
            Some(16384),
        ),
        description: None,
        profile_description: None,
        reasoning_effort: Some(reasoning_effort.to_string()),
        text_verbosity: Some("medium".to_string()),
        thinking: None,
        recommended_for: None,
    })
    .collect()
}

pub(crate) fn config_backed_profile_model_options(profile: &str) -> Vec<ModelOption> {
    let config = load_config_from_str(rich_model_config()).unwrap_or_abort();
    configured_model_catalog(&config)
        .into_iter()
        .map(|entry| ModelOption {
            profile: profile.to_string(),
            provider: entry.provider,
            provider_display_label: Some(entry.provider_display_label),
            provider_backend_label: entry.provider_backend_label,
            model: entry.model,
            model_display_label: Some(entry.model_display_label),
            variant: entry.variant,
            variant_display_label: entry.variant_display_label,
            display_label: Some(entry.display_label),
            token_window_label: entry.token_window_label,
            model_limits: entry.limits,
            description: entry.description,
            profile_description: None,
            reasoning_effort: entry.reasoning_effort,
            text_verbosity: entry.text_verbosity,
            thinking: entry.thinking,
            recommended_for: entry.recommended_for,
        })
        .collect()
}

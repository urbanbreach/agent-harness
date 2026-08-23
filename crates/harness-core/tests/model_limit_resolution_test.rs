use harness_core::config::{
    load_config_from_str, resolve_configured_model_metadata, MaxInputSemantics,
    ModelLimitProvenanceKind,
};
use harness_core::provider_catalog::ProviderCatalog;
use harness_core::UnwrapOrAbort;

fn model_config(models: &str, model: &str, variant: Option<&str>) -> String {
    let variant = variant
        .map(|value| format!(r#", variant: "{value}""#))
        .unwrap_or_default();
    format!(
        r#"{{
          provider: {{
            default: {{
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:1/v1",
              apiKey: "test-key",
              models: {{ {models} }},
            }},
          }},
          model: "default/{model}",
          agent: {{ default: {{ model: "default/{model}"{variant} }} }},
          permission: "deny",
        }}"#
    )
}

#[test]
fn explicit_limits_define_provider_visible_max_input_semantics() {
    // arrange
    let config = load_config_from_str(&model_config(
        r#"known: {
          name: "Known",
          limit: { context: 128000, input: 96000, output: 16000 },
        }"#,
        "known",
        None,
    ))
    .unwrap_or_abort();

    // act
    let resolved =
        resolve_configured_model_metadata(&config, "default", "known", None).unwrap_or_abort();

    // assert
    assert!(resolved.limits.is_exact());
    assert_eq!(resolved.limits.context_window_tokens(), Some(128_000));
    assert_eq!(resolved.limits.max_input_tokens(), Some(96_000));
    assert_eq!(resolved.limits.max_output_tokens(), Some(16_000));
    assert_eq!(
        resolved.limits.max_input_semantics,
        MaxInputSemantics::ProviderVisibleInputTokens
    );
    assert_eq!(
        resolved.limits.max_input.provenance.kind,
        ModelLimitProvenanceKind::ExplicitConfig
    );
}

#[test]
fn custom_model_without_limits_is_explicit_unknown() {
    // arrange
    let config = load_config_from_str(&model_config(
        r#"custom: { name: "Custom OpenAI-compatible" }"#,
        "custom",
        None,
    ))
    .unwrap_or_abort();

    // act
    let resolved =
        resolve_configured_model_metadata(&config, "default", "custom", None).unwrap_or_abort();

    // assert
    assert!(!resolved.limits.is_exact());
    assert_eq!(resolved.limits.context_window_tokens(), None);
    assert_eq!(
        resolved.limits.context_window.provenance.kind,
        ModelLimitProvenanceKind::Unknown
    );
}

#[test]
fn context_and_output_without_input_are_selectable_and_input_stays_unknown() {
    // arrange
    let config = load_config_from_str(&model_config(
        r#"known: { name: "Known", limit: { context: 128000, output: 16000 } }"#,
        "known",
        None,
    ))
    .unwrap_or_abort();

    // act
    let resolved =
        resolve_configured_model_metadata(&config, "default", "known", None).unwrap_or_abort();

    // assert
    assert!(resolved.limits.is_selectable_known());
    assert!(!resolved.limits.has_authoritative_input());
    assert_eq!(resolved.limits.max_input_tokens(), None);
    assert_eq!(
        resolved.limits.max_input.provenance.kind,
        ModelLimitProvenanceKind::Unknown
    );
}

#[test]
fn family_detection_never_invents_limits() {
    // arrange
    let config = load_config_from_str(&model_config(
        r#""claude-sonnet-custom": {
          name: "Claude compatible",
          metadata: { family: "claude" },
        }"#,
        "claude-sonnet-custom",
        None,
    ))
    .unwrap_or_abort();

    // act
    let resolved =
        resolve_configured_model_metadata(&config, "default", "claude-sonnet-custom", None)
            .unwrap_or_abort();

    // assert
    assert!(!resolved.limits.is_exact());
    assert_eq!(resolved.limits.context_window_tokens(), None);
    assert_eq!(
        resolved.limits.context_window.provenance.kind,
        ModelLimitProvenanceKind::Unknown
    );
}

#[test]
fn variant_override_changes_only_its_sourced_limit() {
    // arrange
    let config = load_config_from_str(&model_config(
        r#"known: {
          name: "Known",
          limit: { context: 128000, input: 96000, output: 16000 },
          variants: {
            compact: { name: "Compact", limit: { output: 4096 } },
          },
        }"#,
        "known",
        Some("compact"),
    ))
    .unwrap_or_abort();

    // act
    let base =
        resolve_configured_model_metadata(&config, "default", "known", None).unwrap_or_abort();
    let variant = resolve_configured_model_metadata(&config, "default", "known", Some("compact"))
        .unwrap_or_abort();

    // assert
    assert_eq!(variant.limits.context_window, base.limits.context_window);
    assert_eq!(variant.limits.max_input, base.limits.max_input);
    assert_eq!(variant.limits.max_output_tokens(), Some(4_096));
    assert_eq!(
        variant.limits.max_output.provenance.kind,
        ModelLimitProvenanceKind::ExplicitConfig
    );
    assert!(variant
        .limits
        .max_output
        .provenance
        .detail
        .contains("variant `compact`"));
}

#[test]
fn generated_catalog_representatives_preserve_exact_per_field_values_and_provenance() {
    // arrange
    let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();
    let missing_input = [
        (
            "anthropic",
            "claude-sonnet-4-5",
            200_000,
            64_000,
            "2025-09-29",
        ),
        ("google", "gemini-2.5-pro", 1_048_576, 65_536, "2025-06-05"),
        ("moonshotai", "kimi-k2.5", 262_144, 262_144, "2026-01"),
        ("zai", "glm-4.6", 204_800, 131_072, "2025-09-30"),
        (
            "deepseek",
            "deepseek-chat",
            1_000_000,
            384_000,
            "2026-02-28",
        ),
    ];

    // act
    for (provider, model, context, output, date) in missing_input {
        let limits = &catalog
            .validated_model(provider, model)
            .unwrap_or_abort()
            .limits;

        // assert
        assert_eq!(limits.context_window_tokens(), Some(context));
        assert_eq!(limits.max_input_tokens(), None);
        assert_eq!(limits.max_output_tokens(), Some(output));
        assert_eq!(
            limits.context_window.provenance.kind,
            ModelLimitProvenanceKind::GeneratedCatalog
        );
        assert_eq!(
            limits.max_output.provenance.kind,
            ModelLimitProvenanceKind::GeneratedCatalog
        );
        assert_eq!(
            limits.max_input.provenance.kind,
            ModelLimitProvenanceKind::Unknown
        );
        assert_eq!(
            limits.context_window.provenance.verified_at.as_deref(),
            Some(date)
        );
    }

    let openai = &catalog
        .validated_model("openai", "gpt-5.4")
        .unwrap_or_abort()
        .limits;
    assert_eq!(openai.context_window_tokens(), Some(1_050_000));
    assert_eq!(openai.max_input_tokens(), Some(922_000));
    assert_eq!(openai.max_output_tokens(), Some(128_000));
    assert!(openai.is_exact());
}

#[test]
fn generated_catalog_quarantines_only_the_invalid_partition() {
    // arrange
    let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();

    // act
    let models = catalog
        .providers()
        .into_iter()
        .flat_map(|provider| provider.models.values())
        .collect::<Vec<_>>();
    let complete = models
        .iter()
        .filter(|model| model.limits.has_authoritative_input())
        .count();
    let missing_input = models.len() - complete;

    // assert
    assert_eq!(catalog.providers().len(), 116);
    assert_eq!(models.len(), 3_394);
    assert_eq!(catalog.diagnostics().len(), 62);
    assert_eq!(complete, 306);
    assert_eq!(missing_input, 3_088);
    assert!(models
        .iter()
        .all(|model| model.limits.is_selectable_known()));
}

#[test]
fn zero_context_limit_is_rejected() {
    // arrange
    let raw = model_config(
        r#"broken: { name: "Broken", limit: { context: 0, input: 1, output: 1 } }"#,
        "broken",
        None,
    );

    // act
    let error = load_config_from_str(&raw).expect_err("zero context must fail");

    // assert
    assert!(error
        .to_string()
        .contains("context window must be greater than zero"));
}

#[test]
fn output_above_context_is_rejected() {
    // arrange
    let raw = model_config(
        r#"broken: {
          name: "Broken",
          limit: { context: 8192, input: 4096, output: 16384 },
        }"#,
        "broken",
        None,
    );

    // act
    let error = load_config_from_str(&raw).expect_err("oversized output must fail");

    // assert
    assert!(error
        .to_string()
        .contains("max output 16384 exceeds context window 8192"));
}

#[test]
fn output_without_context_is_rejected() {
    // arrange
    let raw = model_config(
        r#"broken: { name: "Broken", limit: { output: 2048 } }"#,
        "broken",
        None,
    );

    // act
    let error = load_config_from_str(&raw).expect_err("output without context must fail");

    // assert
    assert!(error.to_string().contains("context and output together"));
}

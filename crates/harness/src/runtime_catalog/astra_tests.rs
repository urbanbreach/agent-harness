use super::*;
use harness_core::config::configured_model_catalog;
use harness_core::context_budget::{compute_request_budget, RequestBudgetInput};
use harness_providers::{ProviderOutputCapDisposition, ProviderRequestCost};

#[test]
fn astra_config_keeps_272k_usable_context_after_output_and_safety_reserves() {
    for raw in [
        include_str!("../../../../harness.jsonc"),
        include_str!("../../../../configs/harness.example.jsonc"),
        include_str!("../../../../configs/provider-catalog.reference.jsonc"),
    ] {
        let config = load_config_from_str(raw).unwrap_or_abort();
        let astra = configured_model_catalog(&config)
            .into_iter()
            .find(|entry| entry.model == "gpt-6-astra" && entry.variant.as_deref() == Some("low"))
            .unwrap_or_abort();

        let budget = compute_request_budget(RequestBudgetInput {
            model_limits: &astra.limits,
            request_cost: ProviderRequestCost::default(),
            requested_output_tokens: None,
            safety_margin_tokens: 16_384,
            estimated_token_triggers: true,
            fallback_input_tokens: 32_768,
            output_cap_disposition: ProviderOutputCapDisposition::ProviderDefaulted(128_000),
        })
        .unwrap_or_abort();

        assert_eq!(budget.compaction_threshold_tokens, Some(272_000));
        assert_eq!(budget.reserved_output_tokens, Some(128_000));
        assert_eq!(astra.limits.context_window_tokens(), Some(1_050_000));
    }
}

#[test]
fn astra_catalog_normalization_preserves_limits_and_adds_supported_efforts() {
    let config = serde_json::from_value::<ModelConfig>(serde_json::json!({
        "name": "GPT 6 Astra",
        "limit": { "context": 1050000, "input": 922000, "output": 128000 }
    }))
    .unwrap_or_abort();

    let normalized = normalize_codex_model_variants("gpt-6-astra", config);

    assert_eq!(normalized.limit.context, Some(1_050_000));
    assert_eq!(normalized.limit.input, Some(922_000));
    assert_eq!(normalized.limit.output, Some(128_000));
    assert_eq!(
        normalized
            .variants
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["high", "low", "max", "medium", "xhigh"]
    );
}

#[test]
fn astra_is_selectable_in_the_offline_builtin_codex_catalog() {
    let mut config = shipped_builtin_base_config().unwrap_or_abort();
    config.providers.insert(
        BUILTIN_CODEX_PROVIDER_ID.to_string(),
        builtin_codex_provider().unwrap_or_abort(),
    );

    let entries = configured_model_catalog(&config);

    let astra = entries
        .iter()
        .find(|entry| entry.model == "gpt-6-astra" && entry.variant.as_deref() == Some("max"))
        .unwrap_or_abort();
    assert_eq!(astra.provider, BUILTIN_CODEX_PROVIDER_ID);
    assert_eq!(astra.limits.context_window_tokens(), Some(1_050_000));
    assert_eq!(astra.limits.max_input_tokens(), Some(922_000));
    assert!(!entries
        .iter()
        .any(|entry| { entry.model == "gpt-6-astra" && entry.variant.as_deref() == Some("none") }));
}

#[test]
fn astra_discovery_preserves_api_limits_and_explicit_codex_overrides() {
    let dir = tempfile::tempdir().unwrap_or_abort();
    let path = dir.path().join("models.json");
    std::fs::write(
        &path,
        r#"{"openai":{"name":"OpenAI","models":{"gpt-6-astra":{
            "id":"gpt-6-astra","name":"GPT 6 Astra","tool_call":true,
            "limit":{"context":1050000,"input":922000,"output":128000}
        }}}}"#,
    )
    .unwrap_or_abort();
    let catalog = ProviderCatalog::from_path(&path).unwrap_or_abort();
    let mut config = load_config_from_str(
        r#"{
            "provider":{"openai-codex":{
                "type":"openai_compatible",
                "options":{"authProvider":"codex","baseURL":"https://api.openai.com/v1"},
                "models":{"gpt-5.5":{"name":"GPT 5.5"}}
            }},
            "model":"openai-codex/gpt-5.5"
        }"#,
    )
    .unwrap_or_abort();

    merge_live_codex_models(&mut config, &catalog);

    let astra = configured_model_catalog(&config)
        .into_iter()
        .find(|entry| entry.model == "gpt-6-astra" && entry.variant.as_deref() == Some("max"))
        .unwrap_or_abort();
    assert_eq!(astra.limits.context_window_tokens(), Some(1_050_000));
    let api = catalog
        .validated_model("openai", "gpt-6-astra")
        .unwrap_or_abort();
    assert_eq!(api.limits.context_window_tokens(), Some(1_050_000));
    assert_eq!(api.limits.max_input_tokens(), Some(922_000));

    let codex = match config.providers.get_mut(BUILTIN_CODEX_PROVIDER_ID) {
        Some(ProviderConfig::OpenAiCompatible(codex)) => Some(codex),
        _ => None,
    }
    .unwrap_or_abort();
    codex.models.insert(
        "gpt-6-astra".to_string(),
        serde_json::from_value(serde_json::json!({
            "name": "Local Astra",
            "limit": {"context": 500000, "input": 400000, "output": 128000}
        }))
        .unwrap_or_abort(),
    );

    merge_live_codex_models(&mut config, &catalog);

    let explicit = configured_model_catalog(&config)
        .into_iter()
        .find(|entry| entry.model == "gpt-6-astra" && entry.variant.is_none())
        .unwrap_or_abort();
    assert_eq!(explicit.limits.context_window_tokens(), Some(500_000));
    assert_eq!(explicit.limits.max_input_tokens(), Some(400_000));
}

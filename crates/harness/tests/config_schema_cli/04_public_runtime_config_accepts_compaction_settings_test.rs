#[test]
fn public_runtime_config_accepts_compaction_settings() {
    let parsed: PublicRuntimeConfig = json5::from_str(
        r#"
        {
          runtime: {
            compaction: {
              modelBacked: true,
              model: "default/gpt-5.4-mini",
              splitOversizedTurns: true,
              autoRetryOverflow: false,
              structuredSummaryContract: false,
              estimatedTokenTriggers: false,
              fallbackInputTokens: 65536,
            }
          }
        }
        "#,
    )
    .expect("parse runtime compaction config");

    assert!(parsed.runtime.compaction.model_backed);
    assert_eq!(
        parsed.runtime.compaction.model_ref.as_deref(),
        Some("default/gpt-5.4-mini")
    );
    assert!(parsed.runtime.compaction.split_oversized_turns);
    assert!(!parsed.runtime.compaction.auto_retry_overflow);
    assert!(!parsed.runtime.compaction.structured_summary_contract);
    assert!(!parsed.runtime.compaction.estimated_token_triggers);
    assert_eq!(parsed.runtime.compaction.fallback_input_tokens, 65_536);
}
#[test]
fn public_runtime_config_accepts_new_compaction_settings() {
    let parsed: PublicRuntimeConfig = json5::from_str(
        r#"
        {
          runtime: {
            compaction: {
              structured_summary_contract: true,
              estimated_token_triggers: true,
              fallback_input_tokens: 32768,
            }
          }
        }
        "#,
    )
    .expect("parse runtime compaction config with new canonical keys");

    assert!(parsed.runtime.compaction.structured_summary_contract);
    assert!(parsed.runtime.compaction.estimated_token_triggers);
    assert_eq!(parsed.runtime.compaction.fallback_input_tokens, 32_768);
}

#[test]
fn public_runtime_config_accepts_provider_retry_settings() {
    let parsed: PublicRuntimeConfig = json5::from_str(
        r#"
        {
          runtime: {
            provider_retry: {
              max_retries: 3,
              base_delay_ms: 125,
              max_delay_ms: 4000,
            }
          }
        }
        "#,
    )
    .expect("parse runtime provider retry config");

    assert_eq!(parsed.runtime.provider_retry.max_retries, 3);
    assert_eq!(parsed.runtime.provider_retry.base_delay_ms, 125);
    assert_eq!(parsed.runtime.provider_retry.max_delay_ms, 4_000);
}
#[test]
fn root_runtime_example_uses_canonical_public_keys() {
    let root_example =
        fs::read_to_string(repo_root().join("harness.jsonc")).expect("read root runtime example");

    let parsed: PublicRuntimeConfig =
        json5::from_str(&root_example).expect("parse root runtime example");

    assert_eq!(parsed.default_agent.as_deref(), Some("build"));
    assert_eq!(parsed.model.as_deref(), Some("default/gpt-5.4"));
    assert_eq!(parsed.small_model.as_deref(), Some("default/gpt-5.4-mini"));
    assert!(parsed.agent.build.is_some());
    assert!(parsed.agent.plan.is_some());
    assert!(parsed.agent.general.is_some());
    assert!(parsed.agent.explore.is_some());
    assert!(parsed.agent.visual_engineering.is_some());
    assert!(parsed.agent.artistry.is_some());
    assert!(parsed.agent.ultrabrain.is_some());
    assert!(parsed.agent.deep.is_some());
    assert!(parsed.agent.quick.is_some());
    assert!(parsed.agent.unspecified_low.is_some());
    assert!(parsed.agent.unspecified_high.is_some());
    assert!(parsed.agent.writing.is_some());
    assert!(parsed.agent.title.is_some());
    assert!(parsed.agent.summary.is_some());
    assert!(parsed.agent.compaction.is_some());
    assert!(!root_example.contains("\"base_url\""));
    assert!(!root_example.contains("\"api_key\""));
    assert!(!root_example.contains("\"api_mode\""));
    assert!(!root_example.contains("\"timeout_ms\""));
    assert!(!root_example.contains("\"model_backed\""));

    let provider = parsed.provider.get("default").expect("default provider");
    let ProviderConfig::OpenAiCompatible(provider) = provider;
    let mini = provider
        .models
        .get("gpt-5.4-mini")
        .expect("mini model keeps variant cycle options");
    let mut variants = mini.variants.keys().map(String::as_str).collect::<Vec<_>>();
    variants.sort_unstable();
    assert_eq!(variants, vec!["high", "low", "medium", "xhigh"]);
    assert!(mini
        .variants
        .values()
        .all(|variant| variant.metadata.reasoning_effort.is_some()));
}

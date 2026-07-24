use harness::UnwrapOrAbort;
#[test]
fn public_runtime_config_accepts_compaction_settings() {
    // arrange
    // act
    // assert
    let parsed: PublicRuntimeConfig = json5::from_str(
        r#"
        {
          runtime: {
            compaction: {
              enabled: true,
              reserve_tokens: 8192,
              keep_recent_tokens: 4096,
              split_oversized_turns: true,
              auto_retry_overflow: false,
              structured_summary_contract: false,
              estimated_token_triggers: false,
              fallback_input_tokens: 65536,
            }
          }
        }
        "#,
    )
    .unwrap_or_abort();

    assert!(parsed.runtime.compaction.enabled);
    assert_eq!(parsed.runtime.compaction.reserve_tokens, 8_192);
    assert_eq!(parsed.runtime.compaction.keep_recent_tokens, 4_096);
    assert!(parsed.runtime.compaction.split_oversized_turns);
    assert!(!parsed.runtime.compaction.auto_retry_overflow);
    assert!(!parsed.runtime.compaction.structured_summary_contract);
    assert!(!parsed.runtime.compaction.estimated_token_triggers);
    assert_eq!(parsed.runtime.compaction.fallback_input_tokens, 65_536);
}
#[test]
fn public_runtime_config_accepts_new_compaction_settings() {
    // arrange
    // act
    // assert
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
    .unwrap_or_abort();

    assert!(parsed.runtime.compaction.structured_summary_contract);
    assert!(parsed.runtime.compaction.estimated_token_triggers);
    assert_eq!(parsed.runtime.compaction.fallback_input_tokens, 32_768);
}

#[test]
fn public_runtime_config_accepts_provider_retry_settings() {
    // arrange
    // act
    // assert
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
    .unwrap_or_abort();

    assert_eq!(parsed.runtime.provider_retry.max_retries, 3);
    assert_eq!(parsed.runtime.provider_retry.base_delay_ms, 125);
    assert_eq!(parsed.runtime.provider_retry.max_delay_ms, 4_000);
}
#[test]
fn root_runtime_example_uses_canonical_public_keys() {
    // arrange
    // act
    // assert
    let root_example =
        fs::read_to_string(repo_root().join("harness.jsonc")).unwrap_or_abort();

    let parsed: PublicRuntimeConfig =
        json5::from_str(&root_example).unwrap_or_abort();

    assert_eq!(parsed.default_agent.as_deref(), Some("build"));
    assert_eq!(parsed.model.as_deref(), Some("umans-ai-coding-plan/umans-kimi-k2.7"));
    assert_eq!(parsed.small_model.as_deref(), Some("umans-ai-coding-plan/umans-flash"));
    assert!(parsed.provider.contains_key("umans-ai-coding-plan"));
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

    let provider = parsed.provider.get("default").unwrap_or_abort();
    let ProviderConfig::OpenAiCompatible(provider) = provider else {
        panic!("expected default provider to be OpenAiCompatible")
    };
    let mini = provider
        .models
        .get("gpt-5.4-mini")
        .unwrap_or_abort();
    let mut variants = mini.variants.keys().map(String::as_str).collect::<Vec<_>>();
    variants.sort_unstable();
    assert_eq!(variants, vec!["high", "low", "medium", "xhigh"]);
    assert!(mini
        .variants
        .values()
        .all(|variant| variant.metadata.reasoning_effort.is_some()));
}

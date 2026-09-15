use super::*;
use harness_core::UnwrapOrAbort;

#[tokio::test]
async fn compaction_v2_long_session_preempts_overflow() {
    // Given: two large completed turns and one pressured pending turn.
    let (harness, provider) = CompactionV2Harness::scripted(
        vec![
            provider_text_events(&"A ".repeat(6_000)),
            provider_text_events(&"B ".repeat(6_000)),
            provider_text_events("pressure summary"),

            provider_text_events("bounded answer"),
        ],
        CompactionRuntimeConfig {
            reserve_tokens: 4_096,
            fallback_input_tokens: 12_000,
            keep_recent_tokens: 4_000,
            ..CompactionRuntimeConfig::default()
        },
    )
    .await;
    harness.turn("first pressure turn").await;
    harness.turn("second pressure turn").await;

    // When: the third turn requires proactive compaction.
    let request_id = harness.turn(&"C ".repeat(6_000)).await;
    harness.stop().await;

    // Then: one summary commit precedes the first pressured provider dispatch.
    let events = harness.events();
    let compacted = events
        .iter()
        .find(|event| matches!(event.payload, EventV1::SessionCompaction(_)))
        .unwrap_or_abort();
    let started = events
        .iter()
        .find(|event| {
            matches!(event.payload, EventV1::ProviderRequestStarted(_))
                && event.correlation_id.as_deref() == Some(request_id.as_str())
        })
        .unwrap_or_abort();
    assert!(
        compacted.seq < started.seq,
        "summary commit must precede pressured dispatch"
    );
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        4,
        "the pressured turn dispatch remains single-shot"
    );
    let compaction = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
    assert!(compaction.summary.contains("pressure summary"));
}

#[tokio::test]
async fn compaction_threshold_overrides_control_commit_boundary_and_preserve_hard_limit() {
    // Provider usage, rather than fixture text length, must control the commit threshold.
    for (window, config, trigger) in [
        (16_001, serde_json::json!({}), 8_001_u32),
        (32_000, serde_json::json!({}), 16_000),
        (32_000, serde_json::json!({"threshold_percent": 60}), 19_200),
        (32_000, serde_json::json!({"threshold_percent": 70, "model_thresholds": {"mock:model-1": 55}}), 17_600),
        (32_000, serde_json::json!({"threshold_percent": 70, "model_thresholds": {"mock:model-1": 60}, "agent_thresholds": {"alpha": 45}}), 14_400),
        (32_000, serde_json::json!({"threshold_percent": 100}), 27_904),
        (32_000, serde_json::json!({"threshold_percent": 70, "threshold_tokens": 12_345}), 12_345),
        (32_000, serde_json::json!({"threshold_tokens": 12_000, "model_thresholds": {"mock:model-1": {"tokens": 18_123}}}), 18_123),
        (32_000, serde_json::json!({"threshold_tokens": 12_000, "model_thresholds": {"mock:model-1": 60}, "agent_thresholds": {"alpha": {"tokens": 14_001}}}), 14_001),
        (32_000, serde_json::json!({"threshold_tokens": 12_000, "model_thresholds": {"mock:model-1": {"tokens": 18_123}}, "agent_thresholds": {"alpha": 50}}), 16_000),
        (32_000, serde_json::json!({"threshold_tokens": 4_294_967_295_u32}), 27_904),
    ] {
        for occupied in [trigger - 1, trigger, trigger + 1] {
            let mut settings: CompactionRuntimeConfig = serde_json::from_value(config.clone()).unwrap_or_abort();
            settings.reserve_tokens = 4_096;
            settings.keep_recent_tokens = 1_000;
            let known_model = config.get("model_thresholds").is_some();
            settings.fallback_input_tokens = if known_model { window * 2 } else { window };
            let (harness, _) = CompactionV2Harness::scripted(vec![
                vec![ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta("retained answer ".repeat(1_000)),
                    ProviderStreamEvent::Done { usage: Some(harness_providers::CompletionUsage {
                        prompt_tokens: occupied - 1_000, completion_tokens: 1_000, total_tokens: occupied,
                    }) }],
                provider_text_events("threshold summary"),
            ], settings).await;
            if known_model {
                harness.turn_with_target(&"original task ".repeat(1_000), compaction_v2_target("model-1", window - 4_096, 4_096)).await;
            } else {
                harness.turn(&"original task ".repeat(1_000)).await;
            }
            let outcome = harness.coordinator.compact_agent_context(harness.agent_id.clone(), None, "threshold").await.unwrap_or_abort();
            assert_eq!(matches!(outcome, harness_core::coord::ManualCompactionOutcome::Compacted { .. }), occupied >= trigger,
                "config={config}, occupied={occupied}");
            harness.stop().await;
            assert_eq!(session_compaction_values(&harness.events()).len(), usize::from(occupied >= trigger),
                "background preparation must not commit below the threshold: config={config}, occupied={occupied}");
        }
    }
}

#[tokio::test]
async fn compaction_adaptive_threshold_counts_only_replaced_history_as_savings() {
    for (prompt_repeats, lowers_threshold) in [(1_000, false), (4_000, true)] {
        let answer = |occupied| vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("retained answer ".repeat(1_000)),
            ProviderStreamEvent::Done { usage: Some(harness_providers::CompletionUsage {
                prompt_tokens: occupied - 1_000, completion_tokens: 1_000, total_tokens: occupied,
            }) },
        ];
        let (harness, _) = CompactionV2Harness::scripted(vec![
            answer(20_000), provider_text_events("first summary"),
            answer(15_500), provider_text_events("second summary"),
        ], CompactionRuntimeConfig {
            reserve_tokens: 4_096, keep_recent_tokens: 1_000, fallback_input_tokens: 32_000,
            ..CompactionRuntimeConfig::default()
        }).await;
        harness.turn(&"original task ".repeat(prompt_repeats)).await;
        let first = harness.coordinator.compact_agent_context(harness.agent_id.clone(), None, "manual").await.unwrap_or_abort();
        assert!(matches!(first, ManualCompactionOutcome::Compacted { .. }));
        assert_eq!(session_compaction_values(&harness.events())[0]["tokens_before"], 20_000,
            "the submitted prompt is already included in provider usage and must not be counted twice");
        harness.turn("continue").await;
        let next = harness.coordinator.compact_agent_context(harness.agent_id.clone(), None, "threshold").await.unwrap_or_abort();
        let accounting = session_compaction_values(&harness.events()).iter().map(|data| (
            data["tokens_before"].clone(), data["tokens_after"].clone(), data["first_kept_event_seq"].clone(), data["summary"].as_str().map(str::len),
        )).collect::<Vec<_>>();
        assert_eq!(matches!(next, ManualCompactionOutcome::Compacted { .. }), lowers_threshold,
            "only structural savings above 50% should lower the trigger from 16,000 to 14,400: {accounting:?}");
        harness.stop().await;
        assert_eq!(session_compaction_values(&harness.events()).len(), 1 + usize::from(lowers_threshold));
    }
}

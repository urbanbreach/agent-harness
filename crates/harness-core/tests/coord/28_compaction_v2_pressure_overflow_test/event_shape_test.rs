use super::*;
use harness_core::UnwrapOrAbort;

#[tokio::test]
async fn compaction_v2_manual_auto_share_event_shape() {
    // arrange
    // act
    // assert
    // Given: equivalent successful manual and pre-prompt trigger paths.
    let (manual, _) = CompactionV2Harness::scripted(
        vec![
            provider_text_events("one"),
            provider_text_events("two"),
            provider_text_events("summary"),
        ],
        CompactionRuntimeConfig::default(),
    )
    .await;
    manual.turn("shape one").await;
    manual.turn("shape two").await;
    manual
        .coordinator
        .compact_agent_context(manual.agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    manual.stop().await;
    let (automatic, _) = CompactionV2Harness::scripted(
        vec![
            provider_text_events(&"A".repeat(12_000)),
            provider_text_events(&"B".repeat(12_000)),
            provider_text_events("automatic summary"),
            provider_text_events("split prefix"),
            provider_text_events("automatic answer"),
        ],
        CompactionRuntimeConfig {
            reserve_tokens: 4_096,
            fallback_input_tokens: 12_000,
            ..CompactionRuntimeConfig::default()
        },
    )
    .await;
    automatic.turn("auto shape one").await;
    automatic.turn("auto shape two").await;

    // When: automatic pressure commits through its trigger path.
    automatic.turn(&"C".repeat(12_000)).await;
    automatic.stop().await;
    let manual_payload = session_compaction_values(&manual.events())
        .pop()
        .unwrap_or_abort();
    let automatic_payload = session_compaction_values(&automatic.events())
        .pop()
        .unwrap_or_abort();

    // Then: both triggers emit the same typed durable shape.
    let manual_keys = manual_payload
        .as_object()
        .unwrap_or_abort()
        .keys()
        .collect::<BTreeSet<_>>();
    let automatic_keys = automatic_payload
        .as_object()
        .unwrap_or_abort()
        .keys()
        .collect::<BTreeSet<_>>();
    assert_eq!(manual_keys, automatic_keys);
    assert_eq!(manual_payload["trigger_reason"], "manual");
    assert_eq!(automatic_payload["trigger_reason"], "pre_prompt");
    for field in [
        "agent_id",
        "summary_provider_id",
        "summary_model_id",
        "from_hook",
        "read_files",
        "modified_files",
    ] {
        assert_eq!(
            manual_payload[field], automatic_payload[field],
            "manual/automatic lifecycle contract differs at `{field}`"
        );
    }
    for field in [
        "first_kept_entry_id",
        "tokens_after",
        "summary_model_id",
        "summary_usage",
    ] {
        assert!(
            manual_payload.get(field).is_some(),
            "manual/auto shared payload is missing `{field}`"
        );
        assert_eq!(
            manual_payload[field].is_null(),
            automatic_payload[field].is_null(),
            "manual/automatic optional field presence differs at `{field}`"
        );
    }
    assert_eq!(manual_payload["from_hook"], false);
    assert_eq!(automatic_payload["from_hook"], false);
    assert!(manual_payload["tokens_before"]
        .as_u64()
        .is_some_and(|tokens| tokens > 0));
    assert!(automatic_payload["tokens_before"]
        .as_u64()
        .is_some_and(|tokens| tokens > 0));
}

use super::*;
use harness_core::UnwrapOrAbort;

#[tokio::test]
async fn compaction_v2_repeated_runs_keep_latest_rolling_summary() {
    // arrange
    // act
    // assert
    // Given: two successful rolling compactions with distinct summaries.
    let (harness, provider) = CompactionV2Harness::scripted(
        vec![
            provider_text_events("answer one"),
            provider_text_events("answer two"),
            provider_text_events("rolling summary one"),
            provider_text_events("answer three"),
            provider_text_events("rolling summary two"),
            provider_text_events("answer four"),
        ],
        CompactionRuntimeConfig::default(),
    )
    .await;
    harness.turn("rolling turn one").await;
    harness.turn("rolling turn two").await;
    harness
        .coordinator
        .compact_agent_context(harness.agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    harness.turn("rolling turn three").await;

    // When: the second compaction replaces the first active boundary.
    harness
        .coordinator
        .compact_agent_context(harness.agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    harness.turn("inspect rolling context").await;
    harness.stop().await;

    // Then: exactly one newest summary is visible and rolling boundaries have distinct EntryIds.
    let last = provider.requests().last().cloned().unwrap_or_abort();
    let active_summaries = last
        .messages
        .iter()
        .filter(|message| message.content.contains("rolling summary"))
        .collect::<Vec<_>>();
    assert_eq!(active_summaries.len(), 1);
    assert!(active_summaries[0].content.contains("rolling summary two"));
    let payloads = session_compaction_values(&harness.events());
    let entry_ids = payloads
        .iter()
        .filter_map(|payload| payload.get("first_kept_entry_id")?.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        entry_ids.len(),
        2,
        "each rolling boundary needs typed first-kept identity"
    );
    assert_ne!(
        entry_ids[0], entry_ids[1],
        "newest rolling boundary must advance"
    );
}

#[tokio::test]
async fn compaction_v2_atomic_tool_pair_compacts_only_at_message_boundaries() {
    let huge_output = "U".repeat(50_000);
    let typed_cut = plan_safe_cut(
        &[
            SafeCutCandidate::text("older reducible answer"),
            SafeCutCandidate::atomic(12_500, true, false),
        ],
        1_000,
        estimate_compaction_text_tokens,
    );
    let cut = typed_cut.unwrap_or_abort();
    assert_eq!(cut.first_kept_index, 0);
    assert_eq!(cut.summarized_tokens, 0);
    let (harness, provider) = CompactionV2Harness::scripted_with_tool(
        vec![
            provider_text_events("older reducible answer"),
            provider_tool_events("atomic-call", "atomic_tool", "{}"),
            provider_text_events("tool continuation"),
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::error("context overflow after atomic tool result"),
            ],
            provider_text_events("summary that must not commit"),
        ],
        CompactionRuntimeConfig::default(),
        huge_output,
    )
    .await;
    harness.turn("older reducible turn").await;
    harness.turn("atomic protocol turn").await;
    let boundary_before = active_compaction_boundary(&harness.events(), &harness.agent_id);

    let request_id = harness
        .turn("overflow after unsplittable protocol entry")
        .await;
    harness.stop().await;
    let events = harness.events();
    let boundary_after = active_compaction_boundary(&events, &harness.agent_id);

    assert!(events.iter().any(|event| {
        event.correlation_id.as_deref() == Some(request_id.as_str())
            && matches!(event.payload, EventV1::TaskCompleted(_))
    }));
    assert_eq!(boundary_after.count, boundary_before.count + 1);

    let requests = provider.requests();
    assert!(
        !requests.is_empty(),
        "protocol assertion requires at least one provider request"
    );
    let protocol_is_complete = requests.iter().all(|request| {
        let call_ids = request
            .messages
            .iter()
            .filter_map(|message| message.assistant_tool_calls.as_ref())
            .flatten()
            .map(|call| call.tool_call_id.as_str())
            .collect::<Vec<_>>();
        request
            .messages
            .iter()
            .filter_map(|message| message.tool_call_id.as_deref())
            .all(|result_id| call_ids.contains(&result_id))
    });
    assert!(
        protocol_is_complete,
        "provider context exposed a tool result without its originating call"
    );
}

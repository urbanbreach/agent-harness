#[tokio::test]
async fn compaction_v2_large_tool_result_preserves_protocol() {
    // arrange
    // act
    // assert
    let (_temp, coordinator, run, agent_id, provider, tool_calls) = large_tool_harness(
        vec![
            provider_text_events(&"A".repeat(12_000)),
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "call_large".to_string(),
                    function_name: "shell_run".to_string(),
                    arguments_json: "{}".to_string(),
                },
                ProviderStreamEvent::Done { usage: None },
            ],
            provider_text_events("tool turn completed"),
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::error("context overflow"),
            ],
            provider_text_events("bounded protocol summary"),
            provider_text_events("bounded protocol split prefix"),
            provider_text_events("protocol retry answer"),
        ],
        HookRuntimeConfig::default(),
    )
    .await;
    tool_turn(&coordinator, &agent_id, "old reducible history").await;
    tool_turn(&coordinator, &agent_id, "produce a large tool result").await;

    let request_id = tool_turn(&coordinator, &agent_id, "retry with valid protocol").await;
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.requests();
    let events = load_events(&run.events_path);
    assert_eq!(requests.len(), 7, "history, tool loop, overflow, split summaries, retry");
    let compaction = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
    assert!(compaction.summary.contains("bounded protocol summary"));
    assert!(compaction.summary.contains("bounded protocol split prefix"));
    let retry = requests.last().unwrap_or_abort();
    let retry_started = events
        .iter()
        .filter(|event| {
            event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(event.payload, EventV1::ProviderRequestStarted(_))
        })
        .nth(1)
        .unwrap_or_abort();
    let committed_at_retry = events
        .iter()
        .filter(|event| event.seq <= retry_started.seq)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        normalize_provider_messages(&retry.messages),
        normalize_committed_messages(&committed_at_retry),
        "overflow retry must equal the committed/reopen canonical provider context"
    );
    let normalized_retry = normalize_provider_messages(&retry.messages);
    let canonical_tool_call_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(result) => Some(result.tool_call_id.to_string()),
            _ => None,
        })
        .unwrap_or_abort();
    let durable_tool_call_id = provider_tool_call_id(&events, &canonical_tool_call_id);
    let call_indices = normalized_retry
        .iter()
        .enumerate()
        .filter(|(_, message)| message.tool_call_ids == [durable_tool_call_id.as_str()])
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let result_indices = normalized_retry
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            message.tool_result_id.as_deref() == Some(durable_tool_call_id.as_str())
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(call_indices.len(), 1);
    assert_eq!(result_indices.len(), 1);
    assert!(call_indices[0] < result_indices[0]);
    assert_eq!(
        normalized_retry
            .iter()
            .filter(|message| message.content == "tool turn completed")
            .count(),
        1,
        "canonical retry must not duplicate assistant narrative"
    );
    let semantics = harness_providers::generic_request_budget_semantics(
        retry,
        retry.messages.len().saturating_sub(1),
    )
    .unwrap_or_abort();
    let retry_budget = (match &retry_started.payload {
        EventV1::ProviderRequestStarted(started) => Some(
            started
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.context_budget),
        ),
        _ => None,
    })
    .flatten()
    .unwrap_or_abort();
    assert_eq!(
        retry_budget.components.history_tokens,
        semantics.request_cost.history_tokens
    );
    assert!(retry_budget.components.history_tokens >= 1_000);
    assert_eq!(
        retry_budget.occupied_input_tokens,
        semantics.request_cost.total_input_tokens().unwrap_or_abort()
    );
    assert!(
        retry_budget
            .compaction_threshold_tokens
            .is_some_and(|threshold| retry_budget.occupied_input_tokens < threshold)
    );
    assert!(requests.iter().any(|request| {
        request.max_tokens.is_some()
            && request
                .messages
                .iter()
                .any(|message| message.content.contains("<conversation>"))
    }));
    assert_eq!(session_compaction_values(&events).len(), 1);
    assert_eq!(tool_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::ToolCallFinished(_)))
            .count(),
        1,
        "overflow retry must reuse the durable tool result without replaying the tool"
    );
}

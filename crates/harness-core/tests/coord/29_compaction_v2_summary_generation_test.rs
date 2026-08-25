use harness_core::UnwrapOrAbort;
use tokio_util::sync::CancellationToken;

#[path = "../../src/coord/session_compaction/summary_reducer.rs"]
mod production_summary_reducer;

struct SummaryGenerationRun {
    result: Result<ManualCompactionOutcome, CoordinatorError>,
    requests: Vec<CompletionRequest>,
    values: Vec<serde_json::Value>,
    boundary_before: ActiveCompactionBoundary,
    boundary_after: ActiveCompactionBoundary,
}

async fn run_summary_generation(events: Vec<ProviderStreamEvent>) -> SummaryGenerationRun {
    let (harness, provider) = CompactionV2Harness::scripted(
        vec![
            provider_text_events("summary source one"),
            provider_text_events("summary source two"),
            events,
        ],
        CompactionRuntimeConfig::default(),
    )
    .await;
    harness.turn("summary turn one").await;
    harness.turn("summary turn two").await;
    let boundary_before = active_compaction_boundary(&harness.events(), &harness.agent_id);
    let result = harness
        .coordinator
        .compact_agent_context(harness.agent_id.clone(), None, "manual")
        .await;
    harness.stop().await;
    let boundary_after = active_compaction_boundary(&harness.events(), &harness.agent_id);
    SummaryGenerationRun {
        result,
        requests: provider.requests(),
        values: session_compaction_values(&harness.events()),
        boundary_before,
        boundary_after,
    }
}

#[tokio::test]
async fn compaction_v2_summary_generation_success_captures_usage_and_provenance() {
    // Given: a real file-tool turn and summary usage deliberately unrelated to stored text size.
    let file_path = "/workspace/accounting.rs".to_string();
    let summary_events = vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("generated summary".to_string()),
        ProviderStreamEvent::Done {
            usage: Some(CompletionUsage {
                prompt_tokens: 11,
                completion_tokens: 50_000,
                total_tokens: 50_011,
            }),
        },
    ];
    let (harness, provider) = CompactionV2Harness::scripted_with_named_tool(
        vec![
            provider_tool_events("call_read", "read", &serde_json::json!({"path": file_path}).to_string()),
            provider_text_events("file turn complete"),
            provider_text_events("second source answer"),
            summary_events,
        ],
        CompactionRuntimeConfig::default(),
        "read",
        "file contents".to_string(),
    )
    .await;
    harness.turn("inspect a file").await;
    harness.turn("second summary source").await;
    let result = harness
        .coordinator
        .compact_agent_context(harness.agent_id.clone(), None, "manual")
        .await;
    harness.stop().await;
    let values = session_compaction_values(&harness.events());

    // When: the successful event is inspected as a machine-consumed payload.
    let payload = values.last().unwrap_or_abort();
    let summary = payload["summary"].as_str().unwrap_or_abort();
    let tokens_after = match result.unwrap_or_abort() {
        ManualCompactionOutcome::Compacted { tokens_after, .. } => tokens_after,
        ManualCompactionOutcome::NoOp => panic!("summary generation should compact"),
    };
    let request = provider.requests().last().cloned().unwrap_or_abort();

    // Then: durable accounting includes final file-operation text and ignores generation usage.
    assert!(summary.contains("<read-files>"));
    assert!(summary.contains(&file_path));
    assert!(tokens_after >= harness_core::estimate_compaction_text_tokens(summary));
    assert!(tokens_after < 50_000, "generation usage is separate accounting");
    assert_eq!(request.provider_id.as_deref(), Some("mock"));
    assert_eq!(request.model_id, "model-1");
    assert_eq!(request_digest(&request).len(), 64);
    assert_eq!(
        payload.get("summary_usage"),
        Some(&serde_json::json!({
            "prompt_tokens": 11,
            "completion_tokens": 50_000,
            "total_tokens": 50_011,
        }))
    );
    assert_eq!(
        payload.get("summary_provider_id"),
        Some(&serde_json::json!("mock"))
    );
    assert_eq!(
        payload.get("summary_model_id"),
        Some(&serde_json::json!("model-1"))
    );
    assert!(payload
        .get("first_kept_entry_id")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|entry_id| !entry_id.is_empty()));
    assert!(payload
        .get("tokens_after")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|tokens| tokens > 0));
}

#[tokio::test]
async fn compaction_v2_summary_generation_empty_is_not_committable() {
    // Given: an empty terminal summary stream.
    let run = run_summary_generation(vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::Done { usage: None },
    ])
    .await;

    // When/Then: rejection preserves the exact durable boundary.
    assert!(run.result.is_err());
    assert_eq!(run.boundary_after, run.boundary_before);
}

#[tokio::test]
async fn compaction_v2_non_fitting_summary_preserves_previous_boundary() {
    // Given: one active boundary followed by a generated replacement larger than the current
    // model's complete request budget.
    let oversized_summary = "S".repeat(80_000);
    let (harness, _) = CompactionV2Harness::scripted(
        vec![
            provider_text_events("fit source one"),
            provider_text_events("fit source two"),
            provider_text_events("stable prior boundary"),
            provider_text_events("fit source three"),
            provider_text_events(&oversized_summary),
        ],
        CompactionRuntimeConfig::default(),
    )
    .await;
    harness.turn("fit turn one").await;
    harness.turn("fit turn two").await;
    harness
        .coordinator
        .compact_agent_context(harness.agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    harness.turn("fit turn three").await;
    let boundary_before = active_compaction_boundary(&harness.events(), &harness.agent_id);

    // When: validation recomputes the post-summary request before the success append.
    let result = harness
        .coordinator
        .compact_agent_context(harness.agent_id.clone(), None, "manual")
        .await;

    // Then: the oversized replacement is rejected without replacing the prior active boundary.
    assert!(result
        .as_ref()
        .is_err_and(|error| error.to_string().contains("does not fit current-model request")));
    assert_eq!(
        active_compaction_boundary(&harness.events(), &harness.agent_id),
        boundary_before,
    );
    harness.stop().await;
}

#[tokio::test]
async fn compaction_v2_summary_generation_provider_error_is_not_committable() {
    // Given: provider text followed by a provider error rather than completion.
    let run = run_summary_generation(vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("partial summary".to_string()),
        ProviderStreamEvent::error("summary provider failed"),
    ])
    .await;

    // When/Then: partial text cannot mutate the prior boundary.
    assert!(run.result.is_err());
    assert_eq!(run.boundary_after, run.boundary_before);
}

#[tokio::test]
async fn compaction_v2_summary_generation_duplicate_terminal_is_not_committable() {
    // Given: a stream containing two terminal completion events.
    let run = run_summary_generation(vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("ambiguous summary".to_string()),
        ProviderStreamEvent::Done { usage: None },
        ProviderStreamEvent::Done { usage: None },
    ])
    .await;

    // When/Then: duplicate terminal state is rejected before append.
    assert!(
        run.result.is_err(),
        "duplicate terminal summary must be non-committable"
    );
    assert_eq!(run.boundary_after, run.boundary_before);
}

#[tokio::test]
async fn compaction_v2_summary_generation_late_delta_is_not_committable() {
    // Given: text arriving after the terminal completion event.
    let run = run_summary_generation(vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("premature summary".to_string()),
        ProviderStreamEvent::Done { usage: None },
        ProviderStreamEvent::TextDelta("late text".to_string()),
    ])
    .await;

    // When/Then: post-terminal content invalidates the result atomically.
    assert!(
        run.result.is_err(),
        "post-terminal delta must be non-committable"
    );
    assert_eq!(
        run.boundary_after, run.boundary_before,
        "post-terminal delta must not replace the active compaction boundary",
    );
}

#[tokio::test]
async fn compaction_v2_summary_generation_cancelled_is_not_committable() {
    // Given: the production reducer owns a bounded, still-open event stream.
    let (events_tx, events_rx) = tokio::sync::mpsc::channel(1);
    let cancellation = CancellationToken::new();
    let reducer_cancellation = cancellation.clone();
    let reduction = tokio::spawn(async move {
        production_summary_reducer::reduce_summary_stream(
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(events_rx)),
            &reducer_cancellation,
        )
        .await
    });
    events_tx
        .send(ProviderStreamEvent::Start)
        .await
        .unwrap_or_abort();

    // When: cancellation is signalled while no terminal event exists.
    cancellation.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), reduction)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();

    // Then: no generated summary is committable.
    assert_eq!(
        result,
        Err(production_summary_reducer::SummaryGenerationError::Cancelled)
    );
}

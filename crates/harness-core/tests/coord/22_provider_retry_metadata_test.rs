use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn provider_retry_retries_retryable_empty_failures_and_records_attempt_metadata() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![ProviderStreamEvent::categorized_error(
            "temporary rate limit",
            ProviderErrorCategory::RateLimited,
        )],
        vec![ProviderStreamEvent::categorized_error(
            "socket reset",
            ProviderErrorCategory::TransportFailure,
        )],
        provider_text_events("retry recovered"),
    ]);
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles();
    config.provider_retry = harness_core::config::ProviderRetryRuntimeConfig {
        max_retries: 2,
        base_delay_ms: 0,
        max_delay_ms: 10,
    };
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("coord_provider_retry_recovers", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "please retry")
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "retry recovered"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert_eq!(provider.requests().len(), 3);
    let starts = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(started)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
            {
                Some(started)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 3);
    let attempts = starts
        .iter()
        .map(|started| {
            started
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.retry.as_ref())
                .unwrap_or_abort()
                .attempt
        })
        .collect::<Vec<_>>();
    assert_eq!(attempts, vec![0, 1, 2]);
    assert_eq!(
        starts[1]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.retry.as_ref())
            .and_then(|retry| retry.category),
        Some(ProviderErrorCategory::RateLimited)
    );
    assert_eq!(
        starts[2]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.retry.as_ref())
            .and_then(|retry| retry.category),
        Some(ProviderErrorCategory::TransportFailure)
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(&event.payload, EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.result_summary == "retry recovered"))
            .count(),
        1
    );
}

#[tokio::test]
async fn provider_retry_does_not_retry_partial_output_failures() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("partial answer".to_string()),
            ProviderStreamEvent::categorized_error(
                "temporary rate limit",
                ProviderErrorCategory::RateLimited,
            ),
        ],
        provider_text_events("should not run"),
    ]);
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_provider_retry_partial_output",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial fails")
        .await
        .unwrap_or_abort();

    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.reason.contains("temporary rate limit")
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert_eq!(provider.requests().len(), 1);
}

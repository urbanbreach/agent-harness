use harness_core::UnwrapOrAbort;
use harness_core::store::EventStore;

#[tokio::test]
async fn provider_retry_cancellation_wins_during_backoff() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![vec![ProviderStreamEvent::categorized_error(
        "temporary rate limit",
        ProviderErrorCategory::RateLimited,
    )]]);
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles();
    config.provider_retry = harness_core::config::ProviderRetryRuntimeConfig {
        max_retries: 2,
        base_delay_ms: 600_000,
        max_delay_ms: 600_000,
    };
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("coord_provider_retry_cancel_backoff", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    // act
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "please cancel me")
        .await
        .unwrap_or_abort();

    let task_id = wait_until(Duration::from_millis(700), || async {
        load_events(&run.events_path)
            .into_iter()
            .find_map(|event| match &event.payload {
                EventV1::TaskScheduled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
                {
                    Some(data.task_id.clone())
                }
                _ => None,
            })
    })
    .await
    .unwrap_or_abort();

    // assert
    let _: () = coordinator
        .cancel_task(task_id.clone(), "operator cancelled during retry backoff")
        .await
        .unwrap_or_abort();

    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if data.task_id == task_id
                        && data.reason.contains("operator cancelled during retry backoff")
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert_eq!(provider.requests().len(), 1, "cancellation should prevent retry attempts");
}

#[tokio::test]
async fn provider_retry_max_retries_zero_disables_headless_retries() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![ProviderStreamEvent::categorized_error(
            "temporary rate limit",
            ProviderErrorCategory::RateLimited,
        )],
        provider_text_events("should not run"),
    ]);
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles();
    config.provider_retry = harness_core::config::ProviderRetryRuntimeConfig {
        max_retries: 0,
        base_delay_ms: 0,
        max_delay_ms: 10,
    };
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("coord_provider_retry_max_retries_zero", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    // act
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "no retries please")
        .await
        .unwrap_or_abort();

    // assert
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

    assert_eq!(
        provider.requests().len(),
        1,
        "max_retries=0 should make the first provider failure terminal"
    );
}

#[tokio::test]
async fn provider_retry_uses_retry_after_header_for_backoff_delay() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![ProviderStreamEvent::categorized_error_with_retry_after_ms(
            "rate limited",
            ProviderErrorCategory::RateLimited,
            Some(2_500),
        )],
        provider_text_events("retry after recovered"),
    ]);
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles();
    config.provider_retry = harness_core::config::ProviderRetryRuntimeConfig {
        max_retries: 2,
        base_delay_ms: 600_000,
        max_delay_ms: 600_000,
    };
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("coord_provider_retry_after_header", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    // act
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "respect retry-after")
        .await
        .unwrap_or_abort();

    // assert
    let events = wait_for_events(&run.events_path, Duration::from_millis(7_000), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "retry after recovered"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert_eq!(provider.requests().len(), 2);
    let second_start = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(started)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
            {
                Some(started)
            }
            _ => None,
        })
        .nth(1)
        .unwrap_or_abort();
    let retry = second_start
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.retry.as_ref())
        .unwrap_or_abort();
    assert_eq!(retry.attempt, 1);
    assert_eq!(retry.delay_ms, Some(2_500), "should use Retry-After value");
    assert_eq!(retry.category, Some(ProviderErrorCategory::RateLimited));
}

#[tokio::test]
async fn old_replay_logs_without_provider_retry_metadata_replay_identically() {
    // arrange
    use harness_core::event::SCHEMA_VERSION;

    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "old_log_without_retry";
    let store = harness_core::store::JsonlFileEventStore::open(temp_dir.path(), run_id, true)
        .unwrap_or_abort();
    let actor = EventActor::new(ActorKind::Worker, Some("alpha".to_string()));

    store
        .append(harness_core::store::EventEnvelopeWithoutSeqV1 {
            schema_version: SCHEMA_VERSION,
            event_id: "evt-run-started".to_string(),
            run_id: run_id.to_string().into(),
            mono_ms: 1,
            ts: None,
            actor: EventActor::new(ActorKind::Supervisor, None),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: EventV1::RunStarted(harness_core::event::RunStartedEvent {
                run_name: run_id.to_string().into(),
                workspace_root: "/workspace/project".to_string(),
            }),
        })
        .unwrap_or_abort();

    store
        .append(harness_core::store::EventEnvelopeWithoutSeqV1 {
            schema_version: SCHEMA_VERSION,
            event_id: "evt-provider-started".to_string(),
            run_id: run_id.to_string().into(),
            mono_ms: 2,
            ts: None,
            actor: actor.clone(),
            correlation_id: Some("turn-one".to_string()),
            causation_id: None,
            stream_key: None,
            payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider-call-1".into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "old request".to_string(),
                request_digest: "digest".to_string(),
                metadata: Some(ProviderRequestStartedMetadata::default()),
            }),
        })
        .unwrap_or_abort();

    // act
    let log_body = std::fs::read_to_string(temp_dir.path().join(run_id).join("events.jsonl"))
        .unwrap_or_abort();

    // assert
    assert!(
        !log_body.contains("\"retry\""),
        "old-style event should serialize without a retry field: {log_body}"
    );

    let replayed: Vec<_> = store
        .replay(0)
        .unwrap_or_abort()
        .filter_map(|result| result.ok())
        .collect()
        .await;

    let started_event = replayed
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data) => Some(data),
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(
        started_event.metadata.as_ref().and_then(|m| m.retry.as_ref()),
        None,
        "old logs without retry metadata should replay with retry absent"
    );
}

async fn wait_until<F, Fut, T>(timeout: Duration, mut poll: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let start = std::time::Instant::now();
    loop {
        if let Some(value) = poll().await {
            return Some(value);
        }
        if start.elapsed() >= timeout {
            return None;
        }
        tokio::task::yield_now().await;
    }
}

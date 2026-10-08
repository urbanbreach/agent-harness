use super::*;
use crate::{clock::FakeClock, redact::DefaultRedactor};
use harness_providers::{
    mock::MockProvider, CompletionRequest, Provider, ProviderErrorCategory, ProviderEventStream,
    ProviderStreamEvent as Stream,
};
use tokio::sync::Notify;
use tokio_stream::StreamExt;

struct GatedProvider {
    inner: MockProvider,
    entered: Notify,
    release: Notify,
}
#[async_trait::async_trait]
impl Provider for GatedProvider {
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        if self.inner.call_count() == 0 {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.inner.stream_completion(request).await
    }
}
fn target(model: &str) -> ResolvedModelTarget {
    ResolvedModelTarget {
        model_ref: format!("mock:{model}"),
        provider: "mock".into(),
        model: model.into(),
        variant: Some("selected".into()),
        reasoning_effort: Some("high".into()),
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        limits: Default::default(),
        resolution: Default::default(),
        catalog_entry: None,
    }
}

#[tokio::test]
async fn provider_fallback_advances_once_and_persists_for_queued_and_resumed_turns(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let failed = || {
        vec![Stream::categorized_error(
            "provider unavailable",
            ProviderErrorCategory::TransportFailure,
        )]
    };
    let done = || {
        vec![
            Stream::TextDelta("finished".into()),
            Stream::Done { usage: None },
        ]
    };
    let provider = Arc::new(GatedProvider {
        inner: MockProvider::script([failed(), failed(), done(), done(), done(), failed(), done()]),
        entered: Notify::new(),
        release: Notify::new(),
    });
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn Provider>;
    config.provider_retry.max_retries = 0;
    config.agent_prompt_sources.insert(
        "default".into(),
        Arc::new(crate::system_prompt::PromptSource {
            configured: None,
            suffix: "\n\nKeep instructions.".into(),
            ..Default::default()
        }),
    );
    let third = target("third");
    config
        .agent_model_fallbacks
        .insert("default".into(), vec![target("second"), third]);
    let actor = || EventActor::new(ActorKind::User, None);
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("fallback", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(actor(), "default", None)
        .await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    coordinator
        .request_agent_turn(actor(), agent.clone(), "first")
        .await?;
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        provider.entered.notified(),
    )
    .await?;
    let queued = coordinator
        .request_agent_turn(actor(), agent.clone(), "queued")
        .await?;
    provider.release.notify_one();
    await_turn(&mut events, &queued).await?;
    assert_eq!(
        coordinator.agent_runtime_info(&agent).await?.model_ref,
        "mock:third"
    );
    coordinator.stop_run().await?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "resumed")
        .await?;
    let mut events = resumed.event_store().await?.subscribe(1)?;
    let request = resumed
        .request_agent_turn(actor(), agent.clone(), "resumed")
        .await?;
    await_turn(&mut events, &request).await?;
    let last = resumed
        .request_agent_turn(actor(), agent.clone(), "exhausted")
        .await?;
    let failure = await_turn(&mut events, &last)
        .await
        .err()
        .ok_or("exhausted fallback unexpectedly completed")?;
    assert_eq!(failure.to_string(), "provider unavailable");
    let mut selected = target("zai-glm-5-3");
    selected.resolution =
        crate::model_resolution::resolve_model(crate::model_resolution::ModelResolutionInput {
            provider: "mock",
            model: &selected.model,
            metadata_family: None,
            input_modalities: &[],
            supports_tool_calls: None,
            supports_reasoning_summaries: None,
        });
    let selected = resumed
        .request_agent_turn_with_model_target(actor(), agent, "switch", selected)
        .await?;
    await_turn(&mut events, &selected).await?;
    resumed.stop_run().await?;
    let requests = provider.inner.captured_requests().await;
    assert_eq!(
        requests
            .iter()
            .map(|r| r.model_id.as_str())
            .collect::<Vec<_>>(),
        [
            "default",
            "second",
            "third",
            "third",
            "third",
            "third",
            "zai-glm-5-3"
        ]
    );
    assert!(requests[2..]
        .iter()
        .all(|r| r.variant.as_deref() == Some("selected")
            && r.reasoning_effort.as_deref() == Some("high")));
    assert_eq!(requests[0].messages[1..], requests[1].messages[1..]);
    assert_eq!(requests[0].messages[1..], requests[2].messages[1..]);
    for request in &requests {
        assert!(request.messages[0]
            .content
            .contains(&format!("Active model: mock:{}", request.model_id)));
        assert!(request.messages[0].content.ends_with("Keep instructions."));
    }
    let [.., previous, switched] = &requests[..] else {
        return Err("missing provider requests".into());
    };
    let body = |content: &str, model: &str| content.replace(&format!("mock:{model}"), "");
    assert_ne!(
        body(&previous.messages[0].content, &previous.model_id),
        body(&switched.messages[0].content, &switched.model_id),
        "model selection must replace the previous prompt"
    );
    let history = crate::store::read_events(&run.events_path)?;
    assert_eq!(
        history
            .iter()
            .filter(|e| matches!(e.payload, EventV1::AssistantMessageFinished(_)))
            .count(),
        4
    );
    Ok(())
}
#[tokio::test]
async fn retries_are_bounded_honor_backoff_and_stop_after_visible_output(
) -> Result<(), Box<dyn std::error::Error>> {
    for (partial, exhausted, count, category, message, failure) in [
        (
            false,
            false,
            2,
            ProviderErrorCategory::RateLimited,
            "first failure",
            ProviderRetryFailure::RateLimited,
        ),
        (
            true,
            false,
            1,
            ProviderErrorCategory::RateLimited,
            "first failure",
            ProviderRetryFailure::RateLimited,
        ),
        (
            false,
            true,
            2,
            ProviderErrorCategory::RateLimited,
            "first failure",
            ProviderRetryFailure::RateLimited,
        ),
        (
            false,
            false,
            2,
            ProviderErrorCategory::TransportFailure,
            "provider returned HTTP 503 Service Unavailable",
            ProviderRetryFailure::HttpStatus(503),
        ),
    ] {
        let failed =
            || Stream::categorized_error_with_retry_after_ms(message, category, Some(u64::MAX));
        let temp = tempfile::tempdir()?;
        let first = if partial {
            vec![Stream::TextDelta("partial reply".into()), failed()]
        } else {
            vec![failed()]
        };
        let last = if exhausted {
            vec![Stream::categorized_error(
                "last failure",
                ProviderErrorCategory::TransportFailure,
            )]
        } else {
            vec![Stream::Done { usage: None }]
        };
        let provider = Arc::new(MockProvider::script([first, last]));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn Provider>;
        config.provider_retry = crate::config::ProviderRetryRuntimeConfig {
            max_retries: 1,
            base_delay_ms: 0,
            max_delay_ms: 7,
        };
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run("retry", temp.path()).await?;
        let mut live = coordinator.event_store().await?.subscribe_runtime(1)?;
        let actor = || EventActor::new(ActorKind::User, None);
        let agent = coordinator
            .spawn_agent_idle(actor(), "default", None)
            .await?;
        let mut events = coordinator.event_store().await?.subscribe(1)?;
        let request = coordinator
            .request_agent_turn(actor(), agent, "prompt")
            .await?;
        let result = await_turn(&mut events, &request).await;
        if partial || exhausted {
            assert_eq!(
                result.err().ok_or("expected provider failure")?.to_string(),
                message
            );
        } else {
            result?;
        }
        coordinator.stop_run().await?;
        let mut retry_notice = None;
        while let Some(event) = live.next().await {
            match event? {
                RuntimeEvent::Live(event) => {
                    retry_notice = match event.payload {
                        LiveEventV1::ProviderRetrying { retry, .. } => Some(retry),
                        _ => retry_notice,
                    };
                }
                RuntimeEvent::Durable(event) => match &event.payload {
                    EventV1::ProviderRequestStarted(data)
                        if data
                            .metadata
                            .as_ref()
                            .and_then(|metadata| metadata.retry)
                            .is_some_and(|retry| retry.attempt > 0) =>
                    {
                        assert_eq!(
                            retry_notice.and_then(|retry| retry.failure),
                            Some(failure),
                            "retry must be announced before redispatch"
                        );
                    }
                    EventV1::RunFinished(_) => break,
                    _ => {}
                },
            }
        }
        assert_eq!(retry_notice.is_some(), count == 2);
        assert_eq!(provider.call_count(), count);
        let history = crate::store::read_events(&run.events_path)?;
        let attempts: Vec<_> = history
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::ProviderRequestStarted(e) => e.metadata.as_ref().and_then(|m| m.retry),
                _ => None,
            })
            .collect();
        assert_eq!(attempts.len(), count);
        assert_eq!(attempts[0].attempt, 0);
        if count == 2 {
            assert_eq!(attempts[1].failure, Some(failure));
            assert_eq!(
                (
                    attempts[1].attempt,
                    attempts[1].max_attempts,
                    attempts[1].delay_ms
                ),
                (1, 2, Some(7))
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn cancellation_interrupts_retry_backoff_without_another_provider_call(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([vec![
        Stream::categorized_error_with_retry_after_ms(
            "retry later",
            ProviderErrorCategory::RateLimited,
            Some(60_000),
        ),
    ]]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn Provider>;
    config.provider_retry = crate::config::ProviderRetryRuntimeConfig {
        max_retries: 1,
        base_delay_ms: 60_000,
        max_delay_ms: 60_000,
    };
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("cancel backoff", temp.path()).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    let agent = coordinator
        .spawn_agent_idle(actor(), "default", None)
        .await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    let request = coordinator
        .request_agent_turn(actor(), agent, "prompt")
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::ProviderRequestFinished(_)) {
                return Ok(());
            }
        }
        Err(EventStoreError::Invalid("provider request did not finish"))
    })
    .await??;
    coordinator
        .cancel_task(request.clone(), "stop retries")
        .await?;
    let error = await_turn(&mut events, &request)
        .await
        .err()
        .ok_or("expected cancellation")?;
    assert!(error.to_string().contains("stop retries"));
    coordinator.stop_run().await?;
    assert_eq!(provider.call_count(), 1);
    Ok(())
}

async fn await_turn(
    events: &mut crate::store::EventStream,
    request: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(e) if e.task_id.as_str() == request => return Ok(()),
                EventV1::TaskCancelled(e) => return Err(e.reason.into()),
                _ => {}
            }
        }
        Err::<(), Box<dyn std::error::Error>>("turn did not complete".into())
    })
    .await?
}

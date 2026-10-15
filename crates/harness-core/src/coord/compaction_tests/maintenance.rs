use super::*;

pub(super) struct RoutedSummaries {
    pub(super) turns: MockProvider,
    pub(super) summaries: MockProvider,
}
#[async_trait::async_trait]
impl harness_providers::Provider for RoutedSummaries {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        if request.messages.first().is_some_and(|m| {
            m.content
                .ends_with(harness_compaction::prompts::summarization_system_prompt())
        }) {
            self.summaries.stream_completion(request).await
        } else {
            self.turns.stream_completion(request).await
        }
    }
}

#[tokio::test]
async fn automatic_summary_retries_transient_errors_and_warns_without_failing_the_turn(
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio_stream::StreamExt;
    for succeeds in [false, true] {
        let temp = tempfile::tempdir()?;
        let rate_limit = || {
            vec![Stream::categorized_error(
                "summary rate limited",
                harness_providers::ProviderErrorCategory::RateLimited,
            )]
        };
        let provider = Arc::new(RoutedSummaries {
            turns: MockProvider::script([
                answer("old answer ".repeat(1000)),
                answer("latest answer"),
                answer("completed despite summary failure"),
            ]),
            summaries: MockProvider::script(if succeeds {
                vec![
                    rate_limit(),
                    answer("A free-form checkpoint, without required headings."),
                    answer("Rewrite checkpoint"),
                ]
            } else {
                vec![rate_limit(); 8]
            }),
        });
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.compaction.method_order = vec![CompactionMethod::Soft];
        config.compaction.keep_recent_tokens = 100;
        config.compaction.reserve_tokens = Some(0);
        config.compaction.fallback_input_tokens = 32_000;
        config.compaction.threshold_tokens = std::num::NonZeroU32::new(5_000);
        config.compaction.enabled = false;
        config.compaction.async_enabled = false;
        config.compaction.retry.max_retries = 1;
        config.compaction.retry.base_delay_ms = 1;
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator
            .start_run("summary retries", temp.path())
            .await?;
        let agent = coordinator
            .spawn_agent_idle(handle::system(), "default", None)
            .await?;
        for text in ["old request ".repeat(1000), "latest request".into()] {
            let turn = coordinator
                .request_agent_turn(handle::system(), &agent, text)
                .await?;
            settled(&coordinator, &turn).await?;
        }
        let store = coordinator.event_store().await?;
        let mut live = store.subscribe_runtime(1)?;
        coordinator
            .call(|s| {
                s.config.compaction.enabled = true;
                Ok(())
            })
            .await?;
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, "continue")
            .await?;
        settled(&coordinator, &turn).await?;
        if !succeeds {
            let warned = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                live.any(|event| {
                    matches!(event, Ok(RuntimeEvent::Live(e)) if matches!(&e.payload,
                        LiveEventV1::RuntimeNotice { message, .. }
                            if message.contains("Auto-compaction failed")
                                && message.contains("summary rate limited")))
                }),
            )
            .await?;
            assert!(warned, "missing automatic warning");
        }
        coordinator.stop_run().await?;
        let events = crate::store::read_events(&run.events_path)?;
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e.payload, EventV1::SessionCompaction(_)))
                .count(),
            usize::from(succeeds)
        );
        let requests = provider.summaries.captured_requests().await;
        assert!(requests.len() >= 2);
        assert_eq!(
            requests[0].messages, requests[1].messages,
            "transient retry must preserve its input"
        );
        assert_eq!(provider.turns.call_count(), 3);
        let continuation = provider.turns.captured_requests().await;
        assert_eq!(
            continuation[2]
                .messages
                .iter()
                .any(|m| m.content.contains("A free-form checkpoint")),
            succeeds
        );
    }
    Ok(())
}

#[tokio::test]
async fn manual_handoff_uses_live_history_and_replaces_it_with_working_memory(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let document = "My working memory.\nNext Steps: finish the queue migration.";
    let provider = Arc::new(MockProvider::script([
        answer("old answer"),
        answer(document),
        answer("continued"),
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.compaction.enabled = false;
    config.compaction.keep_recent_tokens = 1;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("handoff", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    let turn = coordinator
        .request_agent_turn(handle::system(), &agent, "original request")
        .await?;
    settled(&coordinator, &turn).await?;
    assert!(matches!(
        coordinator
            .handoff_agent_context(&agent, Some("Keep queue decisions.".into()))
            .await?,
        ManualCompactionOutcome::Compacted {
            method: CompactionMethod::Handoff,
            ..
        }
    ));
    let turn = coordinator
        .request_agent_turn(handle::system(), &agent, "continue")
        .await?;
    settled(&coordinator, &turn).await?;
    let requests = provider.captured_requests().await;
    assert!(requests[1]
        .messages
        .iter()
        .any(|m| m.content == "original request"));
    assert!(requests[1]
        .messages
        .iter()
        .any(|m| m.content == "old answer"));
    assert!(requests[1]
        .messages
        .iter()
        .any(|m| m.content.contains("Keep queue decisions.")));
    assert_eq!(
        requests[2].messages[0].content,
        summary_message(Some(CompactionMethod::Handoff), document)
    );
    assert!(requests[2]
        .messages
        .iter()
        .any(|m| m.content == "old answer"));
    assert!(!requests[2]
        .messages
        .iter()
        .any(|m| m.content == "original request"));
    coordinator.stop_run().await?;
    let events = crate::store::read_events(&run.events_path)?;
    assert!(events.iter().any(|e| matches!(&e.payload, EventV1::SessionCompaction(c)
        if c.method == Some(CompactionMethod::Handoff) && c.summary == document && c.first_kept_request_id.is_some())));
    Ok(())
}

#[tokio::test]
async fn length_stops_retry_with_reminders_and_stop_after_four_empty_responses(
) -> Result<(), Box<dyn std::error::Error>> {
    for recovers in [true, false] {
        let temp = tempfile::tempdir()?;
        let length = vec![Stream::DoneWithMetadata {
            usage: Some(harness_providers::CompletionUsage {
                completion_tokens: 100,
                ..Default::default()
            }),
            metadata: Some(harness_providers::ProviderStreamFinishedMetadata {
                output_limit_reached: true,
                ..Default::default()
            }),
        }];
        let script = if recovers {
            vec![length, answer("recovered")]
        } else {
            vec![length; 4]
        };
        let provider = Arc::new(MockProvider::script(script));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        // Length recovery needs automatic maintenance; the window is far from exhausted.
        config.compaction.enabled = true;
        config.compaction.async_enabled = false;
        config.compaction.method_order = vec![CompactionMethod::Soft];
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator
            .start_run("length recovery", temp.path())
            .await?;
        let agent = coordinator
            .spawn_agent_idle(handle::system(), "default", None)
            .await?;
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, "work")
            .await?;
        let result = settled(&coordinator, &turn).await;
        assert_eq!(result.is_ok(), recovers);
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), if recovers { 2 } else { 4 });
        assert!(requests[1].messages.iter().any(|m| m.content
            == super::super::reminders::wrap(&harness_compaction::prompts::length_stop_reminder(
                100, 1, 3
            ))));
        coordinator.stop_run().await?;
        let events = crate::store::read_events(&run.events_path)?;
        if !recovers {
            let model = requests[0].model_id.as_str();
            let provider_id = requests[0]
                .provider_id
                .as_deref()
                .ok_or("missing provider")?;
            let expected = format!("Length-stop recovery gave up after 4 consecutive `length` responses from {provider_id}/{model} with no text or tool call. Try switching models or raising the model's max output tokens.");
            let expected = CoordinatorError::Invalid(expected).to_string();
            assert!(events.iter().any(|e| matches!(&e.payload, EventV1::TaskCancelled(t) if t.failure && t.reason == expected)));
        }
    }
    Ok(())
}

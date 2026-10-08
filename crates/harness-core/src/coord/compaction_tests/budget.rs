use super::*;

struct BudgetedSummaryProvider {
    turns: MockProvider,
    summaries: tokio::sync::Mutex<Vec<harness_providers::CompletionRequest>>,
}
#[async_trait::async_trait]
impl harness_providers::Provider for BudgetedSummaryProvider {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        if request
            .messages
            .first()
            .is_some_and(|message| message.content.starts_with("Summarize this conversation"))
        {
            let final_chunk = request.messages[1].content.contains("END_OLD");
            let text = if final_chunk {
                SUMMARY.to_owned()
            } else {
                let padding = "abc "
                    .repeat(request.max_tokens.unwrap_or_default().saturating_sub(256) as usize);
                SUMMARY.replace("## Progress\n", &format!("## Progress\n{padding}\n"))
            };
            self.summaries.lock().await.push(request);
            Box::pin(tokio_stream::iter(answer(text)))
        } else {
            self.turns.stream_completion(request).await
        }
    }
    fn manages_context(&self, _: &harness_providers::CompletionRequest) -> bool {
        true
    }
}

#[tokio::test]
async fn compaction_bounds_intermediate_summaries_to_the_next_input_budget(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(BudgetedSummaryProvider {
        turns: MockProvider::script([
            answer(format!("{} END_OLD", "abc ".repeat(9000))),
            answer("latest answer"),
            answer("continued"),
        ]),
        summaries: Default::default(),
    });
    let settings = crate::config::load_config_from_str(
        r#"{
            model: 'local/small', provider: { local: {
                type: 'openai_compatible', baseUrl: 'https://example.invalid',
                models: { small: { limit: { context: 12288, output: 8192 } } }
            } }
        }"#,
    )?;
    let target = crate::config::resolve_model_selection(&settings, "local/small", None)?.primary;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.agent_model_targets.insert("default".into(), target);
    config.compaction.keep_recent_tokens = 1;
    config.compaction.reserve_tokens = 0;
    config.compaction.suppress_auto_compaction = true;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("chunk budget", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    for text in ["first request", "recent request"] {
        let turn = coordinator
            .request_agent_turn(EventActor::new(ActorKind::User, None), agent.clone(), text)
            .await?;
        super::history_tests::settled(&coordinator, &turn).await?;
    }
    assert!(matches!(
        coordinator
            .compact_agent_context(agent.clone(), None, "manual")
            .await?,
        ManualCompactionOutcome::Compacted { .. }
    ));
    let summaries = provider.summaries.lock().await;
    assert!(summaries.len() > 1);
    for request in &summaries[..summaries.len() - 1] {
        assert!(request.max_tokens.is_some_and(|cap| cap < 4096));
    }
    assert_eq!(
        summaries.last().ok_or("missing final summary")?.max_tokens,
        Some(8192)
    );
    drop(summaries);
    let turn = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "continue")
        .await?;
    super::history_tests::settled(&coordinator, &turn).await?;
    assert!(provider
        .turns
        .captured_requests()
        .await
        .last()
        .ok_or("missing continuation")?
        .messages
        .iter()
        .any(|message| message.content.contains(SUMMARY)));
    coordinator.stop_run().await?;
    Ok(())
}

use super::*;

struct DelegatingProvider {
    child_release: Arc<tokio::sync::Notify>,
    parent_calls: AtomicUsize,
}
#[async_trait::async_trait]
impl harness_providers::Provider for DelegatingProvider {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        if request
            .messages
            .last()
            .is_some_and(|m| m.content == "held child")
        {
            let (tx, rx) = mpsc::channel(2);
            let release = Arc::clone(&self.child_release);
            tokio::spawn(async move {
                release.notified().await;
                let _ = tx
                    .send(Stream::TextDelta("surviving child answer".into()))
                    .await;
                let _ = tx.send(settled_metadata("survivor")).await;
            });
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
        } else if self.parent_calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Box::pin(tokio_stream::iter(vec![
                Stream::ToolCallComplete {
                    tool_call_id: "foreground".into(),
                    function_name: "spawn_subagent".into(),
                    arguments_json: json!({
                        "prompt":"held child","description":"foreground child",
                        "subagent_type":"native-fixture","background":false,
                    })
                    .to_string(),
                },
                settled_metadata("parent call"),
            ]))
        } else {
            Box::pin(tokio_stream::iter(vec![
                Stream::TextDelta("parent released".into()),
                settled_metadata("parent release"),
            ]))
        }
    }
}

#[tokio::test]
async fn native_foreground_reply_cancels_attached_child_on_waiter_and_prompt_cancel(
) -> Result<(), Box<dyn std::error::Error>> {
    for cancel_prompt in [false, true] {
        let temp = tempfile::tempdir()?;
        let release = Arc::new(tokio::sync::Notify::new());
        let provider = Arc::new(DelegatingProvider {
            child_release: Arc::clone(&release),
            parent_calls: AtomicUsize::new(0),
        });
        let mut config = super::super::children_tests::configuration(temp.path(), provider);
        config.provider_model_concurrency = 2;
        let mut root = AgentProfile::fallback("default");
        root.toolset = vec!["spawn_subagent".into()];
        config.agent_profiles.insert("default".into(), root);
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator.start_run("real waiter", temp.path()).await?;
        let parent = coordinator
            .spawn_agent(system_actor(), "default", None)
            .await?;
        let mut events = coordinator.subscribe_new_events().await?;
        let prompt = coordinator
            .request_agent_turn(system_actor(), parent.clone(), "native parent")
            .await?;
        let (child, child_attempt, waiter) =
            wait_delegated_child_started(&mut events, &parent).await?;
        let mut events = coordinator.subscribe_new_events().await?;
        coordinator
            .request_subagent_cancel(
                system_actor(),
                if cancel_prompt {
                    SubagentCommandRequest::ParentPromptCancel {
                        prompt_id: prompt.clone(),
                    }
                } else {
                    SubagentCommandRequest::WaiterCancel {
                        waiter_id: waiter.clone(),
                    }
                },
            )
            .await?;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut cancelled = std::collections::BTreeSet::new();
            let mut root_terminal = false;
            while !(cancelled.contains(&waiter)
                && cancelled.contains(&child_attempt)
                && root_terminal
                && (!cancel_prompt || cancelled.contains(&prompt)))
            {
                let event = events.next().await.ok_or(EventStoreError::Invalid(
                    "native cancellation terminals missing",
                ))??;
                let (task_id, was_cancelled) = match event.payload {
                    EventV1::TaskCancelled(event) => (event.task_id, true),
                    EventV1::TaskCompleted(event) => (event.task_id, false),
                    _ => continue,
                };
                root_terminal |= task_id.as_str() == prompt;
                cancelled.extend(was_cancelled.then(|| task_id.to_string()));
            }
            Ok::<(), EventStoreError>(())
        })
        .await??;
        let finalized = coordinator.raw_finalized_state(child.clone()).await?;
        assert!(
            matches!(finalized, FinalizedStateResult::Unavailable { .. }),
            "cancelled child finalized-state classification: {}",
            match finalized {
                FinalizedStateResult::Available { .. } => "available",
                FinalizedStateResult::Unavailable { .. } => "unavailable",
            }
        );
        assert_eq!(
            coordinator.subagent_history().await?.records[&child].outcome,
            Some(SubagentTerminalOutcome::Cancelled)
        );
        release.notify_one();
        coordinator.stop_run().await?;
    }
    Ok(())
}

async fn wait_delegated_child_started(
    events: &mut crate::store::EventStream,
    parent: &str,
) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    let started = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut waiter = None;
        while let Some(event) = events.next().await {
            let event = event?;
            if let Some(e) = match &event.payload {
                EventV1::ToolCallRequested(e) if e.tool_id == "spawn_subagent" => Some(e),
                _ => None,
            } {
                waiter = Some(e.tool_call_id.to_string());
            }
            if matches!(event.payload, EventV1::ProviderRequestStarted(_))
                && event
                    .actor
                    .agent_id
                    .as_deref()
                    .is_some_and(|id| id != parent)
            {
                return Ok::<_, EventStoreError>((
                    event
                        .actor
                        .agent_id
                        .ok_or(EventStoreError::Invalid("child missing"))?,
                    event
                        .correlation_id
                        .ok_or(EventStoreError::Invalid("attempt missing"))?,
                    waiter.ok_or(EventStoreError::Invalid("waiter missing"))?,
                ));
            }
        }
        Err(EventStoreError::Invalid("delegation not started"))
    })
    .await??;
    Ok(started)
}

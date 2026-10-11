use super::*;

struct PendingProvider {
    entered: Arc<tokio::sync::Notify>,
    dropped: Arc<AtomicUsize>,
    sessions: Arc<std::sync::Mutex<Vec<harness_providers::ProviderSessionEvent>>>,
}
const ABORTED_USAGE: harness_providers::CompletionUsage = harness_providers::CompletionUsage {
    prompt_tokens: 7,
    completion_tokens: 3,
    total_tokens: 10,
};
struct PendingStream(Arc<AtomicUsize>);
impl tokio_stream::Stream for PendingStream {
    type Item = Stream;
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Stream>> {
        std::task::Poll::Pending
    }
}
impl Drop for PendingStream {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[async_trait::async_trait]
impl harness_providers::Provider for PendingProvider {
    async fn stream_completion(
        &self,
        _: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        self.entered.notify_one();
        Box::pin(PendingStream(Arc::clone(&self.dropped)))
    }
    /// Settles an abort the way a stateful backend does: after it fires, with billed usage.
    async fn stream_completion_abortable(
        &self,
        request: harness_providers::CompletionRequest,
        abort: tokio_util::sync::CancellationToken,
    ) -> harness_providers::ProviderEventStream {
        let settled = tokio_stream::iter([abort]).then(|abort| async move {
            abort.cancelled().await;
            Stream::Aborted {
                usage: Some(ABORTED_USAGE),
            }
        });
        Box::pin(self.stream_completion(request).await.merge(settled))
    }
    fn session_event(&self, event: &harness_providers::ProviderSessionEvent) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.push(event.clone());
        }
    }
}
#[tokio::test]
async fn cancellation_removes_queued_turns_and_drops_the_active_provider_stream(
) -> Result<(), Box<dyn std::error::Error>> {
    for disconnect in [false, true] {
        let temp = tempfile::tempdir()?;
        let entered = Arc::new(tokio::sync::Notify::new());
        let dropped = Arc::new(AtomicUsize::new(0));
        let sessions = Arc::default();
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::new(PendingProvider {
            entered: Arc::clone(&entered),
            dropped: Arc::clone(&dropped),
            sessions: Arc::clone(&sessions),
        });
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run("cancel", temp.path()).await?;
        let store = coordinator.event_store().await?;
        let agent = coordinator
            .spawn_agent_idle(
                EventActor::new(ActorKind::Supervisor, None),
                "default",
                None,
            )
            .await?;
        let first = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                agent.clone(),
                "first",
            )
            .await?;
        tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified()).await?;
        let second = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                agent.clone(),
                "second",
            )
            .await?;
        let third = coordinator
            .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "third")
            .await?;
        coordinator
            .cancel_task(&second, "queued cancellation")
            .await?;
        if disconnect {
            let terminal = coordinator.subscribe_new_events().await?;
            drop(coordinator);
            let mut terminal = terminal.skip_while(|event| {
                matches!(event, Ok(event) if !matches!(event.payload, EventV1::RunFailed(_)))
            });
            tokio::time::timeout(std::time::Duration::from_secs(3), terminal.next())
                .await?
                .ok_or("owner disconnect did not close the run")??;
        } else {
            tokio::time::timeout(std::time::Duration::from_secs(3), coordinator.stop_run())
                .await??;
        }
        let reopened = crate::store::Journal::open_existing(
            &temp.path().join("sessions"),
            run.run_id.as_str(),
            false,
        )?;
        reopened.close_writer()?;
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        let events = store
            .replay(1)?
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let cancelled: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.payload {
                EventV1::TaskCancelled(e) => Some(e.task_id.as_str()),
                _ => None,
            })
            .collect();
        for id in [&first, &second, &third] {
            assert!(
                cancelled.contains(&id.as_str()),
                "missing cancellation for {id}"
            );
        }
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e.payload, EventV1::ProviderRequestStarted(_)))
                .count(),
            1
        );
        assert!(if disconnect {
            matches!(
                events.last().map(|e| &e.payload),
                Some(EventV1::RunFailed(_))
            )
        } else {
            matches!(
                events.last().map(|e| &e.payload),
                Some(EventV1::RunFinished(_))
            )
        });
        // The abort settled before the stream dropped, so the cancelled request keeps its billed usage.
        assert!(events.iter().any(|e| matches!(
            &e.payload,
            EventV1::ProviderRequestFinished(f)
                if f.finish_reason == "cancelled" && f.usage == Some(ABORTED_USAGE)
        )));
        let sessions = sessions.lock().map(|s| s.clone()).unwrap_or_default();
        assert!(
            matches!(
                &sessions[..],
                [
                    harness_providers::ProviderSessionEvent::Routed { session_id: routed, .. },
                    harness_providers::ProviderSessionEvent::Closed { session_id: closed, .. },
                ] if routed == closed
            ),
            "{sessions:?}"
        );
    }
    Ok(())
}

/// Senpi's `model_select` and `thinking_level_select`: each pick reaches providers once, at the pick.
#[tokio::test]
async fn model_and_reasoning_picks_are_announced_to_providers(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_providers::ProviderSessionEvent as Session;
    let temp = tempfile::tempdir()?;
    let sessions = Arc::default();
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(PendingProvider {
        entered: Arc::default(),
        dropped: Arc::default(),
        sessions: Arc::clone(&sessions),
    });
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("select", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let initial = coordinator
        .agent_runtime_info(agent.clone())
        .await?
        .model_ref;
    let high = crate::agent::AgentModelSettings {
        reasoning_effort: Some("high".into()),
        ..Default::default()
    };
    for (model, settings) in [
        ("other:model", Default::default()),
        ("other:model", high.clone()),
        ("other:model", high),
        (initial.as_str(), Default::default()),
    ] {
        coordinator
            .announce_model_selection(agent.clone(), model.into(), settings)
            .await?;
    }
    let session = run.run_id.to_string();
    let initial = crate::agent::AgentModelRef::parse(&initial);
    assert_eq!(
        sessions.lock().map(|s| s.clone()).unwrap_or_default(),
        [
            Session::ModelSelected {
                session_id: session.clone(),
                provider_id: "other".into(),
                model_id: "model".into(),
            },
            Session::ReasoningSelected {
                session_id: session.clone(),
            },
            Session::ModelSelected {
                session_id: session,
                provider_id: initial.provider_id,
                model_id: initial.model_id,
            },
        ]
    );
    Ok(())
}

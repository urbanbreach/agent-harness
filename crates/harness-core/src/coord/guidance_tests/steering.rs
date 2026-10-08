//! User steering and the queued input an interrupt returns to the editor.
use super::*;

/// Holds the turn inside a tool call until the test releases it.
struct Gate {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
#[async_trait::async_trait]
impl Tool for Gate {
    fn id(&self) -> &'static str {
        "gate"
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    async fn call(&self, _: ToolContext, _: Value) -> Result<ToolResult, ToolError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(ToolResult::text("gate opened"))
    }
}

/// Holds the second provider request until released.
struct HoldSecond {
    provider: MockProvider,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
#[async_trait::async_trait]
impl harness_providers::Provider for HoldSecond {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        let stream = self.provider.stream_completion(request).await;
        if self.provider.call_count() == 2 {
            self.entered.notify_one();
            self.release.notified().await;
        }
        stream
    }
}

#[tokio::test]
async fn steering_joins_the_running_turn_or_becomes_a_turn_when_it_cannot(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (entered, release) = (
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(tokio::sync::Notify::new()),
    );
    let provider = Arc::new(HoldSecond {
        provider: MockProvider::script([
            call("hold", "gate", json!({})),
            answer("Adjusted to the steering."),
            answer("Handled the late message."),
            call("hold-again", "gate", json!({})),
            answer("Handled the requeued steering."),
        ]),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(Gate {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["gate".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("steering", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let user = || EventActor::new(ActorKind::User, None);
    let turn = coordinator
        .request_agent_turn(user(), agent.clone(), "start")
        .await?;
    entered.notified().await;
    let SteerOutcome::Steering { turn_id, .. } = coordinator
        .steer_agent_turn(user(), agent.clone(), "Use the second approach.")
        .await?
    else {
        return Err("a running turn did not accept steering".into());
    };
    assert_eq!(turn_id, turn);
    release.notify_one();
    // A message sent while the final request streams still joins the same turn.
    provider.entered.notified().await;
    let SteerOutcome::Steering { turn_id, .. } = coordinator
        .steer_agent_turn(user(), agent.clone(), "Late message.")
        .await?
    else {
        return Err("the running turn did not accept the late message".into());
    };
    assert_eq!(turn_id, turn);
    provider.release.notify_one();
    assert_eq!(terminal(&coordinator, &turn).await?, None);
    let requests = provider.provider.captured_requests().await;
    // The steering arrives after the tool result, before the next request of the same turn.
    let steered = &requests[1].messages;
    let last = steered.last().ok_or("empty request")?;
    assert_eq!(
        (last.role, last.content.as_str()),
        (MessageRole::User, "Use the second approach.")
    );
    assert!(steered.iter().any(|m| m.content == "gate opened"));
    assert_eq!(
        requests[2].messages.last().map(|m| m.content.as_str()),
        Some("Late message.")
    );

    // An idle agent has no turn to steer; the caller submits a normal prompt instead.
    assert_eq!(
        coordinator
            .steer_agent_turn(user(), agent.clone(), "Idle follow-up.")
            .await?,
        SteerOutcome::Idle
    );

    // Steering for a turn that is cancelled before taking it becomes its own turn.
    let cancelled = coordinator
        .request_agent_turn(user(), agent.clone(), "second")
        .await?;
    entered.notified().await;
    let SteerOutcome::Steering {
        message_id: requeued,
        ..
    } = coordinator
        .steer_agent_turn(user(), agent.clone(), "Steering for a cancelled turn.")
        .await?
    else {
        return Err("the running turn did not accept steering".into());
    };
    coordinator
        .cancel_task(&cancelled, "user cancelled")
        .await?;
    release.notify_one();
    assert!(terminal(&coordinator, &cancelled).await?.is_some());
    assert_eq!(terminal(&coordinator, &requeued).await?, None);
    let requests = provider.provider.captured_requests().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests[4].messages.last().map(|m| m.content.as_str()),
        Some("Steering for a cancelled turn.")
    );
    coordinator.stop_run().await?;

    // Every steering message was journaled on acceptance, and the stopped journal has
    // nothing in flight, so recovery invents no cancellations for delivered steering.
    let events = crate::store::read_events(&run.events_path)?;
    let accepted = events
        .iter()
        .filter(|e| matches!(e.payload, EventV1::SteeringAccepted(_)))
        .count();
    assert_eq!(accepted, 3);
    let mut in_flight = crate::proj::InFlight::default();
    for event in &events {
        in_flight.apply(event);
    }
    assert!(in_flight.stable());
    // Rebuilt history keeps both steering messages inside their turn.
    let rebuilt = super::history::messages(&events, &agent, true, "", &run.run_dir)?;
    let contents: Vec<_> = rebuilt
        .entries
        .iter()
        .map(|e| e.message.content.as_str())
        .collect();
    let steering = contents
        .iter()
        .position(|c| *c == "Use the second approach.")
        .ok_or("steering missing from history")?;
    assert_eq!(
        &contents[steering - 1..steering + 4],
        &[
            "gate opened",
            "Use the second approach.",
            "Adjusted to the steering.",
            "Late message.",
            "Handled the late message.",
        ]
    );
    Ok(())
}

#[tokio::test]
async fn interrupt_returns_steering_and_plain_follow_ups_in_send_order(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (entered, release) = (
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(tokio::sync::Notify::new()),
    );
    let provider = Arc::new(MockProvider::script([call("hold", "gate", json!({}))]));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(Gate {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["gate".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("interrupt", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let user = || EventActor::new(ActorKind::User, None);
    let turn = coordinator
        .request_agent_turn(user(), agent.clone(), "start")
        .await?;
    entered.notified().await;
    // A follow-up queued first, then steering for the turn's next request.
    let follow_up = coordinator
        .request_agent_turn(user(), agent.clone(), "Follow-up after the turn.")
        .await?;
    let SteerOutcome::Steering {
        message_id: steering,
        ..
    } = coordinator
        .steer_agent_turn(user(), agent.clone(), "Steer this turn.")
        .await?
    else {
        return Err("the running turn did not accept steering".into());
    };

    let returned = coordinator.withdraw_queued_input(agent.clone()).await?;
    assert_eq!(returned, ["Follow-up after the turn.", "Steer this turn."]);
    coordinator.cancel_task(&turn, "interrupted").await?;
    release.notify_one();
    assert!(terminal(&coordinator, &turn).await?.is_some());
    for id in [&follow_up, &steering] {
        assert_eq!(
            terminal(&coordinator, id).await?.as_deref(),
            Some(RETURNED_TO_EDITOR_REASON)
        );
    }
    // Nothing returned to the editor reaches the model or starts another turn.
    assert!(coordinator.withdraw_queued_input(agent).await?.is_empty());
    assert_eq!(provider.call_count(), 1);
    coordinator.stop_run().await?;
    let events = crate::store::read_events(&run.events_path)?;
    let mut in_flight = crate::proj::InFlight::default();
    for event in &events {
        in_flight.apply(event);
    }
    assert!(in_flight.stable());
    // Transcripts leave the returned input out, whether built at once or incrementally.
    let mentions_follow_up = |transcript: &crate::transcript_projection::TranscriptProjection| {
        transcript.messages.iter().any(|message| {
            message.parts.iter().any(|part| {
                matches!(part, crate::transcript_projection::ProjectedPart::Text(text)
                    if text.text == "Follow-up after the turn.")
            })
        })
    };
    assert!(!mentions_follow_up(
        &crate::transcript_projection::project_transcript(&events)?
    ));
    let split = events
        .iter()
        .position(|event| matches!(&event.payload, EventV1::TaskCancelled(data) if data.reason == RETURNED_TO_EDITOR_REASON))
        .ok_or("withdrawal not journaled")?;
    let mut canonical =
        crate::session::CanonicalSessionProjection::from_event_history(&events[..split])?;
    assert!(mentions_follow_up(&canonical.transcript));
    canonical.apply_events(&events[split..])?;
    assert!(!mentions_follow_up(&canonical.transcript));
    Ok(())
}

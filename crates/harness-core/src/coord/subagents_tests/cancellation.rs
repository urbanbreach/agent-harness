use super::*;

struct Waiter;
#[async_trait::async_trait]
impl Tool for Waiter {
    fn id(&self) -> &str {
        "waiter"
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    async fn call(&self, context: ToolContext, _: Value) -> Result<ToolResult, ToolError> {
        context.cancellation.cancelled().await;
        Err(ToolError::Cancelled)
    }
}

#[tokio::test]
async fn cancellation_intents_keep_child_prompt_waiter_and_shutdown_scopes_distinct(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(HeldProvider);
    config.provider_model_concurrency = 5;
    config.permission_policy = PermissionPolicy::allow_all();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(Waiter));
    config.tool_registry = Arc::new(registry);
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec!["waiter".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("cancel scopes", temp.path()).await?;
    let parent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let child = coordinator
        .spawn_agent(system_actor(), "default", Some(parent.clone()))
        .await?;
    let descendant = coordinator
        .spawn_agent(system_actor(), "default", Some(child.clone()))
        .await?;
    let owned_descendant = coordinator
        .spawn_agent(system_actor(), "default", Some(child.clone()))
        .await?;
    let other = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let mut attempts = BTreeMap::new();
    for id in [&parent, &child, &descendant, &owned_descendant, &other] {
        attempts.insert(
            id.clone(),
            coordinator
                .request_agent_turn(system_actor(), id.clone(), "held")
                .await?,
        );
    }
    let waiter = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(parent.clone())),
            None,
            "waiter",
            json!({}),
        )
        .await?;
    let remaining = attempts.clone();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut started = std::collections::BTreeSet::new();
        while let Some(event) = events.next().await {
            let event = event?;
            let request = matches!(event.payload, EventV1::ProviderRequestStarted(_))
                .then_some(event.correlation_id)
                .flatten();
            started.extend(request);
            if matches!(event.payload, EventV1::ToolCallStarted(_)) {
                started.insert(waiter.clone());
            }
            if remaining.values().all(|id| started.contains(id)) && started.contains(&waiter) {
                return Ok::<_, EventStoreError>(());
            }
        }
        Err(EventStoreError::Invalid("held work did not start"))
    })
    .await??;
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::WaiterCancel {
                waiter_id: waiter.clone(),
            },
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskCancelled(e) if e.task_id.as_str() == waiter) {
                return Ok::<_, EventStoreError>(());
            }
        }
        Err(EventStoreError::Invalid("waiter did not cancel"))
    })
    .await??;
    assert!(matches!(
        coordinator.raw_finalized_state(descendant.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Active
        }
    ));
    let original_ancestry = coordinator.subagent_history().await?.records[&descendant]
        .metadata
        .as_ref()
        .ok_or("descendant metadata missing")?
        .ancestry
        .clone();
    coordinator
        .reparent_subagent_to_root(system_actor(), descendant.clone(), InjectedSubagentDepth(0))
        .await?;
    let routed = coordinator.subagent_history().await?;
    let metadata = routed.records[&descendant]
        .metadata
        .as_ref()
        .ok_or("routed metadata missing")?;
    assert_eq!(metadata.ancestry, original_ancestry);
    assert_eq!(metadata.injected_depth, InjectedSubagentDepth(0));
    assert!(matches!(
        &metadata.execution_owner,
        SubagentExecutionOwner::RootSession { .. }
    ));
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ExplicitChildKill {
                child_id: SubagentId(child.clone()),
            },
        )
        .await?;
    wait_cancelled(&mut events, &attempts[&child]).await?;
    assert!(matches!(
        coordinator.raw_finalized_state(descendant.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Active
        }
    ));
    assert!(coordinator
        .request_agent_turn(system_actor(), child.clone(), "cannot wake")
        .await
        .is_err());
    assert!(coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ChildSessionCancel {
                session_id: descendant.clone(),
                descendants: vec![SubagentId(other.clone())]
            }
        )
        .await
        .is_err());
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ParentPromptCancel {
                prompt_id: attempts[&parent].clone(),
            },
        )
        .await?;
    wait_cancelled(&mut events, &attempts[&parent]).await?;
    assert!(matches!(
        coordinator.raw_finalized_state(descendant.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Active
        }
    ));
    let mut events = coordinator.subscribe_new_events().await?;
    let cancelled = coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ChildSessionCancel {
                session_id: child.clone(),
                descendants: vec![SubagentId(owned_descendant.clone())],
            },
        )
        .await?;
    assert!(cancelled.contains(&SubagentId(owned_descendant.clone())));
    assert!(
        !cancelled.contains(&SubagentId(descendant.clone())),
        "root-reparented execution is not an ancestry cancellation edge"
    );
    wait_cancelled(&mut events, &attempts[&owned_descendant]).await?;
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ChildSessionCancel {
                session_id: descendant.clone(),
                descendants: Vec::new(),
            },
        )
        .await?;
    wait_cancelled(&mut events, &attempts[&descendant]).await?;
    coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ParentSessionStop {
                session_id: parent.clone(),
            },
        )
        .await?;
    assert!(coordinator
        .request_agent_turn(system_actor(), parent, "admission stopped")
        .await
        .is_err());
    assert!(matches!(
        coordinator.raw_finalized_state(other.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Active
        }
    ));
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_subagent_cancel(system_actor(), SubagentCommandRequest::RootShutdown)
        .await?;
    wait_cancelled(&mut events, &attempts[&other]).await?;
    assert!(coordinator.run_info().await.is_err());
    let history = crate::store::read_events(&run.events_path)?;
    assert_eq!(
        history
            .iter()
            .filter(|e| matches!(e.payload, EventV1::SubagentCancelRequested(_)))
            .count(),
        7
    );
    assert!(history
        .iter()
        .all(|e| !matches!(e.payload, EventV1::ConversationRewound(_))));
    Ok(())
}

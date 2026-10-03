use super::*;

#[tokio::test]
async fn finalized_reference_append_failure_never_publishes_available_state(
) -> Result<(), Box<dyn std::error::Error>> {
    for artifact_failure in [true, false] {
        assert_finalized_storage_failure(artifact_failure).await?;
    }
    Ok(())
}
async fn assert_finalized_storage_failure(
    artifact_failure: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(MockProvider::script([vec![
        Stream::TextDelta("complete".into()),
        settled_metadata("settled"),
    ]]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("append failure", temp.path()).await?;
    let agent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    coordinator
        .call(move |s| {
            let inner = s.store.clone().ok_or(CoordinatorError::RunNotStarted)?;
            let artifacts = s.info()?.artifacts_dir.clone();
            s.store = Some(Arc::new(super::super::tests::InterceptStore {
                inner,
                before_append: Box::new(move |event| {
                    if artifact_failure
                        && matches!(event.payload, EventV1::ProviderRequestStarted(_))
                    {
                        obstruct_artifact_directory(&artifacts)?;
                    }
                    if !artifact_failure && matches!(event.payload, EventV1::FinalizedAgentState(_))
                    {
                        Err(EventStoreError::Io(std::io::Error::other(
                            "finalized append failure",
                        )))
                    } else {
                        Ok(())
                    }
                }),
            }));
            Ok(())
        })
        .await?;
    let mut events = coordinator.event_store().await?.subscribe_runtime(1)?;
    coordinator
        .request_agent_turn(system_actor(), agent.clone(), "complete")
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if matches!(event?, RuntimeEvent::Live(e) if matches!(e.payload, LiveEventV1::RuntimeWarning { .. })) { return Ok::<_, EventStoreError>(()); }
        }
        Err(EventStoreError::Invalid("failure not published"))
    }).await??;
    assert!(matches!(
        coordinator.raw_finalized_state(agent.clone()).await?,
        FinalizedStateResult::Unavailable { .. }
    ));
    assert!(coordinator.subagent_history().await?.finalized.is_empty());
    assert!(coordinator
        .request_agent_turn(system_actor(), agent.clone(), "must reject")
        .await
        .is_err());
    coordinator
        .call(move |s| {
            assert!(
                !s.agents[&agent].busy,
                "removed completion cannot strand a busy actor"
            );
            assert!(s.running.is_empty());
            assert!(s.fault.is_some());
            Ok(())
        })
        .await?;
    let journal = crate::store::read_events(&run.events_path)?;
    assert!(journal
        .iter()
        .all(|e| !matches!(e.payload, EventV1::FinalizedAgentState(_))));
    assert_eq!(
        run.artifacts_dir.is_dir(),
        !artifact_failure,
        "an orphan blob is not authority and failed artifact IO is fail-closed"
    );
    assert!(coordinator.stop_run().await.is_err());
    Ok(())
}
fn obstruct_artifact_directory(path: &std::path::Path) -> Result<(), EventStoreError> {
    if path.is_dir() {
        std::fs::remove_dir(path)?;
    }
    std::fs::write(path, b"not a directory")?;
    Ok(())
}

#[tokio::test]
async fn missing_normalized_fields_and_policy_modified_state_are_not_exact(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(MockProvider::script([
        vec![
            Stream::TextDelta("answer".into()),
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("sk-policy-rejected-secret-value".into()),
            settled_metadata("safe"),
        ],
    ]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("fidelity", temp.path()).await?;
    let first = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let second = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    for (agent, reason) in [
        (first, FinalizedStateUnavailable::UnsupportedReasoning),
        (second, FinalizedStateUnavailable::PolicyModified),
    ] {
        let mut events = coordinator.subscribe_new_events().await?;
        let id = coordinator
            .request_agent_turn(system_actor(), agent.clone(), "prompt")
            .await?;
        wait_terminal(&mut events, &id).await?;
        assert_eq!(
            coordinator.raw_finalized_state(agent).await?,
            FinalizedStateResult::Unavailable { reason }
        );
    }
    assert!(!std::fs::read_to_string(&run.events_path)?.contains("provider_reasoning_delta"));
    for entry in std::fs::read_dir(&run.artifacts_dir)? {
        assert!(
            !std::fs::read_to_string(entry?.path())?.contains("sk-policy-rejected-secret-value")
        );
    }
    coordinator.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn accepted_context_is_applied_before_projection_failure(
) -> Result<(), Box<dyn std::error::Error>> {
    for initialize in [false, true] {
        assert_context_projection_failure(initialize).await?;
    }
    Ok(())
}
async fn assert_context_projection_failure(
    initialize: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let cwd = temp.path().join("effective");
    std::fs::create_dir(&cwd)?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.permission_policy = PermissionPolicy::allow_all();
    config.provider = Arc::new(MockProvider::script([vec![
        Stream::TextDelta("source answer".into()),
        settled_metadata("source"),
    ]]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("apply context", temp.path()).await?;
    let source = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    coordinator
        .set_agent_execution_cwd(system_actor(), source.clone(), cwd.clone())
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let turn = coordinator
        .request_agent_turn(system_actor(), source.clone(), "source")
        .await?;
    wait_terminal(&mut events, &turn).await?;
    let target = coordinator
        .spawn_agent(system_actor(), "default", Some(source.clone()))
        .await?;
    let child_events = temp
        .path()
        .join("sessions")
        .join(&target)
        .join("events.jsonl");
    std::fs::remove_file(&child_events)?;
    std::fs::create_dir(&child_events)?;
    let mut events = coordinator.subscribe_new_events().await?;
    let result = if initialize {
        coordinator
            .initialize_agent_from_finalized(
                system_actor(),
                target.clone(),
                source,
                FinalizedContextCopy::Fork,
            )
            .await
    } else {
        coordinator
            .set_agent_execution_cwd(system_actor(), target.clone(), cwd.clone())
            .await
    };
    assert!(
        result.is_err(),
        "projection IO must fail closed after authoritative append"
    );
    let recorded = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            let event = event?;
            if matches!(&event.payload, EventV1::AgentContextInitialized(e) if e.agent_id.0 == target)
                || matches!(&event.payload, EventV1::AgentExecutionContextChanged(e) if e.agent_id.0 == target)
            {
                return Ok::<_, EventStoreError>(event);
            }
        }
        Err(EventStoreError::Invalid("accepted event missing"))
    })
    .await??;
    assert!(crate::store::read_events(&run.events_path)?.contains(&recorded));
    coordinator
        .call(move |s| {
            let state = &s.agents[&target];
            assert!(s.fault.is_some());
            if initialize {
                assert!(state
                    .messages
                    .entries
                    .iter()
                    .any(|e| e.message.content == "source answer"));
                assert!(state.source_reference.is_some());
            } else {
                assert_eq!(state.cwd, cwd);
                assert_eq!(state.execution.effective_cwd, cwd.to_string_lossy());
            }
            Ok(())
        })
        .await?;
    assert!(coordinator.stop_run().await.is_err());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn changed_canonical_cwd_is_rejected_before_provider_or_tool_dispatch(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let accepted = temp.path().join("accepted");
    let moved = temp.path().join("moved");
    let denied = temp.path().join("denied");
    std::fs::create_dir(&accepted)?;
    std::fs::create_dir(&denied)?;
    let provider = Arc::new(MockProvider::script([]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.permission_policy = PermissionPolicy::from_rules(vec![
        crate::perm::PermissionRule {
            permission: "*".into(),
            pattern: crate::config::PermissionSelector::CatchAll,
            action: crate::perm::PermissionAction::Allow,
        },
        crate::perm::PermissionRule {
            permission: "read".into(),
            pattern: crate::config::PermissionSelector::Exact("denied".into()),
            action: crate::perm::PermissionAction::Deny,
        },
    ])?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(LargeTool(Arc::clone(&calls))));
    config.tool_registry = Arc::new(registry);
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec!["large".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("changed cwd", temp.path()).await?;
    let agent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    assert!(coordinator
        .set_agent_execution_cwd(system_actor(), agent.clone(), denied.clone())
        .await
        .is_err());
    coordinator
        .set_agent_execution_cwd(system_actor(), agent.clone(), accepted.clone())
        .await?;
    std::fs::rename(&accepted, &moved)?;
    std::os::unix::fs::symlink(&denied, &accepted)?;
    let actor = EventActor::new(ActorKind::Worker, Some(agent.clone()));
    assert!(coordinator
        .request_tool_call(actor, None, "large", json!({}))
        .await
        .is_err());
    assert!(coordinator
        .request_agent_turn(system_actor(), agent.clone(), "must not dispatch")
        .await
        .is_err());
    assert_eq!(provider.call_count(), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    coordinator
        .call(move |s| {
            assert_eq!(s.agents[&agent].cwd, accepted);
            assert_eq!(s.agents[&agent].generation, 0);
            Ok(())
        })
        .await?;
    coordinator.stop_run().await?;
    Ok(())
}

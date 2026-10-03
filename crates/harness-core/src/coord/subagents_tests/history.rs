use super::*;

#[tokio::test]
async fn versioned_fold_backs_deferred_finishes_expires_and_seals_usage(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(MockProvider::script([vec![
        Stream::TextDelta("answer".into()),
        settled_metadata("settled"),
    ]]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("fold", temp.path()).await?;
    let parent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let child = coordinator
        .spawn_agent(system_actor(), "default", Some(parent))
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let id = coordinator
        .request_agent_turn(system_actor(), child.clone(), "prompt")
        .await?;
    wait_terminal(&mut events, &id).await?;
    let journal = crate::store::read_events(&run.events_path)?;
    let transitions: Vec<_> = journal
        .iter()
        .filter_map(|e| match &e.payload {
            EventV1::SubagentTransition(e) => Some(e.as_ref().clone()),
            _ => None,
        })
        .collect();
    let spawn = transitions.first().ok_or("spawn missing")?;
    let finish = transitions.last().ok_or("finish missing")?;
    let mut fold = SubagentHistory::default();
    assert!(fold.apply_transition(finish, 0));
    assert_eq!(fold.deferred_count(), 1);
    assert!(fold.apply_transition(spawn, 1));
    assert!(fold.records[&child].lifecycle.is_finished());
    assert_eq!(fold.records[&child].accounting, finish.accounting);
    assert!(!fold.apply_transition(spawn, 2));
    assert!(!fold.apply_transition(finish, 2));
    for attemptless in [false, true] {
        let mut future_finish = finish.clone();
        future_finish.generation = spawn.generation + 1;
        future_finish.notification_seq = Some(5);
        if attemptless {
            future_finish.attempt_id = None;
        }
        let mut mismatched = SubagentHistory::default();
        assert!(mismatched.apply_transition(&future_finish, 0));
        assert!(mismatched.apply_transition(spawn, 1));
        assert!(!mismatched.records[&child].lifecycle.is_finished());
        assert_eq!(mismatched.records[&child].accounting, None);
        assert!(!mismatched.finalized.contains_key(&child));
        if attemptless {
            let mut next_spawn = spawn.clone();
            next_spawn.generation += 1;
            next_spawn.attempt_id = Some("later-native-attempt".into());
            next_spawn.notification_seq = Some(4);
            assert!(mismatched.apply_transition(&next_spawn, 2));
            assert!(mismatched.records[&child].lifecycle.is_finished());
            assert_eq!(mismatched.records[&child].accounting, finish.accounting);
            assert!(
                !mismatched.finalized.contains_key(&child),
                "a prior attempt/generation reference cannot become the later attempt's state"
            );
        }
    }
    let mut wrong_reference = finish.clone();
    wrong_reference
        .finalized_state
        .as_mut()
        .ok_or("finish reference missing")?
        .generation += 1;
    let mut reference_fold = SubagentHistory::default();
    assert!(reference_fold.apply_transition(&wrong_reference, 0));
    assert!(reference_fold.apply_transition(spawn, 1));
    assert!(!reference_fold.finalized.contains_key(&child));
    let mut stale = finish.clone();
    stale.generation = 0;
    assert!(!fold.apply_transition(&stale, 2));
    let mut retired = finish.clone();
    retired.transition = SubagentTransitionKind::Retired;
    assert!(fold.apply_transition(&retired, 3));
    assert!(!fold.apply_transition(finish, 4));
    let mut deferred = SubagentHistory::default();
    for n in 0..257 {
        let mut event = finish.clone();
        event.child_id = SubagentId(format!("deferred-{n}"));
        event.finalized_state = None;
        assert!(deferred.apply_transition(&event, 0));
    }
    assert_eq!(deferred.deferred_count(), 256);
    deferred.expire(60_000);
    assert_eq!(deferred.deferred_count(), 0);
    assert!(deferred.records.values().all(|r| !r
        .lifecycle
        .retains_attempt(&SubagentAttemptKey::Id(id.clone()))));
    let legacy: Vec<_> = journal
        .iter()
        .filter(|e| {
            !matches!(
                e.payload,
                EventV1::SubagentTransition(_) | EventV1::FinalizedAgentState(_)
            )
        })
        .cloned()
        .collect();
    let legacy_bytes = serde_json::to_vec(&legacy)?;
    let decoded: Vec<EventEnvelopeV1> = serde_json::from_slice(&legacy_bytes)?;
    let legacy_fold = SubagentHistory::from_events(&decoded);
    assert!(legacy_fold.records[&child].legacy);
    assert!(legacy_fold.records[&child].lifecycle.is_finished());
    assert!(legacy_fold.finalized.is_empty());
    assert_eq!(serde_json::to_vec(&decoded)?, legacy_bytes);
    coordinator.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn direct_child_projection_validates_original_finalized_and_initialized_owners(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::TextDelta("root source".into()),
            settled_metadata("root"),
        ],
        vec![
            Stream::TextDelta("child source".into()),
            settled_metadata("child"),
        ],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.permission_policy = PermissionPolicy::allow_all();
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("projection owners", temp.path())
        .await?;
    let parent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let child = coordinator
        .spawn_agent(system_actor(), "default", Some(parent.clone()))
        .await?;
    for agent in [&parent, &child] {
        let mut events = coordinator.subscribe_new_events().await?;
        let turn = coordinator
            .request_agent_turn(system_actor(), agent.clone(), "source")
            .await?;
        wait_terminal(&mut events, &turn).await?;
    }
    let initialized = coordinator
        .spawn_agent(system_actor(), "default", Some(parent.clone()))
        .await?;
    coordinator
        .initialize_agent_from_finalized(
            system_actor(),
            initialized.clone(),
            parent,
            FinalizedContextCopy::Fork,
        )
        .await?;
    coordinator.stop_run().await?;
    let original = std::fs::read(&run.events_path)?;
    for (id, copied) in [(&child, false), (&initialized, true)] {
        let resumed = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        resumed.resume_run(id.clone(), "direct child").await?;
        if copied {
            let id = id.clone();
            resumed
                .call(move |s| {
                    assert!(s.agents[&id]
                        .messages
                        .entries
                        .iter()
                        .any(|e| e.message.content == "root source"));
                    Ok(())
                })
                .await?;
        } else {
            let FinalizedStateResult::Available { state } =
                resumed.raw_finalized_state(id.clone()).await?
            else {
                return Err("original-owner child state unavailable".into());
            };
            assert_eq!(state.owner_run_id, run.run_id.to_string());
        }
        assert_eq!(provider.call_count(), 2);
        resumed.stop_run().await?;
        assert_eq!(std::fs::read(&run.events_path)?, original);
    }
    let metadata = temp
        .path()
        .join("sessions")
        .join(&child)
        .join(crate::proj::META_FILE_NAME);
    let value: Value = serde_json::from_slice(&std::fs::read(&metadata)?)?;
    for field in ["parent_session_id", "parent_run_id"] {
        let mut changed = value.clone();
        changed["harness_lineage"][field] = "foreign".into();
        std::fs::write(&metadata, serde_json::to_vec(&changed)?)?;
        let denied = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        assert!(denied
            .resume_run(child.clone(), "reject foreign owner")
            .await
            .is_err());
        assert!(denied.run_info().await.is_err());
    }
    Ok(())
}

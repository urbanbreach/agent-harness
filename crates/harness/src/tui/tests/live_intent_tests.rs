use super::*;
use harness::UnwrapOrAbort;

#[test]
fn plan_handoff_updates_live_agent_target_to_spawned_build_agent() {
    // arrange
    // act
    // assert
    let target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some("agent_plan".to_string()),
        profile: "plan".to_string(),
        last_request_id: Some("req_plan".to_string()),
    }));
    let event = lineage_test_event(
        1,
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: "agent_build".to_string(),
            profile: "build".to_string(),
            parent_agent_id: Some("agent_plan".to_string()),
        }),
    );

    maybe_update_live_agent_target_for_plan_handoff(&event, Some(&target));

    let target = target.lock().unwrap_or_abort();
    assert_eq!(target.agent_id.as_deref(), Some("agent_build"));
    assert_eq!(target.profile, "build");
    assert_eq!(target.last_request_id, None);
}

#[tokio::test]
async fn compact_intent_reports_noop_status_for_idle_live_agent() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.agent_profiles = golden_path_profiles();

    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("compact_status", temp_dir.path())
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "planner", None)
        .await
        .unwrap_or_abort();

    let live_agent_target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some(agent_id),
        profile: "planner".to_string(),
        last_request_id: None,
    }));
    let (intent_tx, intent_rx) = mpsc::unbounded_channel();
    let (status_tx, status_rx) = std_mpsc::channel();

    let handle = tokio::spawn(handle_ui_intents(
        coordinator.clone(),
        intent_rx,
        user_actor(),
        Some(live_agent_target),
        status_tx,
        TuiAuthBackendContext {
            config_path: None,
            session_dir: Some(temp_dir.path().to_path_buf()),
            workspace_root: temp_dir.path().to_path_buf(),
            config_digest: "test-digest".to_string(),
        },
    ));

    intent_tx.send(UiIntent::CompactSession).unwrap_or_abort();
    drop(intent_tx);

    handle.await.unwrap_or_abort().unwrap_or_abort();
    let status = status_rx.recv().unwrap_or_abort();
    assert!(matches!(
        status,
        LiveUpdate::OperatorNotice {
            message,
            level: OperatorNoticeLevel::Info,
        } if message == "manual compaction skipped: need at least two completed turns"
    ));

    coordinator.stop_run().await.unwrap_or_abort();
}

#[test]
fn manual_compaction_success_message_reports_active_context_delta() {
    // arrange
    // act
    // assert
    assert_eq!(
        manual_compaction_success_message("summary preview", 18_200, 4_100),
        "manual compaction applied · ctx 18.2K → 4.1K est · summary preview"
    );
    assert_eq!(
        manual_compaction_success_message("summary preview", 4_100, 4_100),
        "manual compaction applied · ctx estimate unchanged · summary preview"
    );
}

#[test]
fn foreground_background_success_message_reports_single_and_multiple_counts() {
    // arrange
    // act
    // assert
    assert_eq!(
        foreground_background_success_message(1),
        "foreground subagent moved to background"
    );
    assert_eq!(
        foreground_background_success_message(2),
        "2 foreground subagents moved to background"
    );
}

#[tokio::test]
async fn event_forwarder_stops_after_terminal_event_when_requested() {
    // arrange
    let store = Arc::new(InMemoryEventStore::new());
    store
        .append(forwarder_event_draft(
            "run_forwarder_terminal",
            "started",
            EventV1::RunStarted(RunStartedEvent {
                run_name: "forwarder terminal".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ))
        .unwrap_or_abort();
    store
        .append(forwarder_event_draft(
            "run_forwarder_terminal",
            "finished",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ))
        .unwrap_or_abort();
    let (tx, rx) = std_mpsc::channel();

    // act
    tokio::time::timeout(
        Duration::from_millis(500),
        forward_events_to_tui(store, tx, 1, None, true),
    )
    .await
    .unwrap_or_abort()
    .unwrap_or_abort();

    // assert
    let updates = rx.try_iter().collect::<Vec<_>>();
    assert_eq!(updates.len(), 2);
    assert!(matches!(updates[0], LiveUpdate::Event(_)));
    assert!(
        matches!(updates[1], LiveUpdate::Event(ref event) if is_terminal_event(&event.payload))
    );
}

#[tokio::test]
async fn event_forwarder_updates_live_agent_target_on_plan_handoff() {
    // arrange
    // act
    // assert
    let store = Arc::new(InMemoryEventStore::new());
    store
        .append(forwarder_event_draft(
            "run_forwarder_plan_handoff",
            "agent_spawned",
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_build".to_string(),
                profile: harness_core::plan::BUILD_AGENT_NAME.to_string(),
                parent_agent_id: Some("agent_plan".to_string()),
            }),
        ))
        .unwrap_or_abort();
    store
        .append(forwarder_event_draft(
            "run_forwarder_plan_handoff",
            "finished",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ))
        .unwrap_or_abort();
    let (tx, rx) = std_mpsc::channel();
    let target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some("agent_plan".to_string()),
        profile: harness_core::plan::PLAN_AGENT_NAME.to_string(),
        last_request_id: Some("req_plan".to_string()),
    }));

    tokio::time::timeout(
        Duration::from_millis(500),
        forward_events_to_tui(store, tx, 1, Some(Arc::clone(&target)), true),
    )
    .await
    .unwrap_or_abort()
    .unwrap_or_abort();

    let updates = rx.try_iter().collect::<Vec<_>>();
    assert_eq!(updates.len(), 2);
    let target = target.lock().unwrap_or_abort();
    assert_eq!(target.agent_id.as_deref(), Some("agent_build"));
    assert_eq!(target.profile, harness_core::plan::BUILD_AGENT_NAME);
    assert_eq!(target.last_request_id, None);
}

#[tokio::test]
async fn compact_intent_reports_unavailable_when_no_live_agent_target_exists() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.agent_profiles = golden_path_profiles();

    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("compact_status", temp_dir.path())
        .await
        .unwrap_or_abort();

    let (intent_tx, intent_rx) = mpsc::unbounded_channel();
    let (status_tx, status_rx) = std_mpsc::channel();

    let handle = tokio::spawn(handle_ui_intents(
        coordinator.clone(),
        intent_rx,
        user_actor(),
        None,
        status_tx,
        TuiAuthBackendContext {
            config_path: None,
            session_dir: Some(temp_dir.path().to_path_buf()),
            workspace_root: temp_dir.path().to_path_buf(),
            config_digest: "test-digest".to_string(),
        },
    ));

    intent_tx.send(UiIntent::CompactSession).unwrap_or_abort();
    drop(intent_tx);

    handle.await.unwrap_or_abort().unwrap_or_abort();
    let status = status_rx.recv().unwrap_or_abort();
    assert!(matches!(
        status,
        LiveUpdate::OperatorNotice {
            message,
            level: OperatorNoticeLevel::Error,
        } if message == "manual compaction unavailable: no live agent target"
    ));

    coordinator.stop_run().await.unwrap_or_abort();
}

#[test]
fn live_ui_router_forwards_compact_intent_without_switching_workflow() {
    // arrange
    // act
    // assert
    let (intent_tx, mut intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let launch_selection = Arc::new(Mutex::new(LaunchMetadata::default()));
    let (selected_workflow, sink) = build_live_ui_intent_router(
        intent_tx,
        Arc::clone(&launch_selection),
        false,
        "test-digest".to_string(),
    );

    sink(UiIntent::CompactSession);

    assert!(recover_mutex_lock(&selected_workflow).is_none());
    assert_eq!(intent_rx.try_recv().ok(), Some(UiIntent::CompactSession));
}

#[test]
fn live_ui_router_forwards_interrupt_intent_without_switching_workflow() {
    // arrange
    // act
    // assert
    let (intent_tx, mut intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let launch_selection = Arc::new(Mutex::new(LaunchMetadata::default()));
    let (selected_workflow, sink) = build_live_ui_intent_router(
        intent_tx,
        Arc::clone(&launch_selection),
        false,
        "test-digest".to_string(),
    );

    sink(UiIntent::InterruptSession {
        task_ids: vec!["task_active".to_string()],
    });

    assert!(recover_mutex_lock(&selected_workflow).is_none());
    assert_eq!(
        intent_rx.try_recv().ok(),
        Some(UiIntent::InterruptSession {
            task_ids: vec!["task_active".to_string()],
        })
    );
}

#[test]
fn live_ui_router_forwards_foreground_background_intent_without_switching_workflow() {
    // arrange
    // act
    // assert
    let (intent_tx, mut intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let launch_selection = Arc::new(Mutex::new(LaunchMetadata::default()));
    let (selected_workflow, sink) = build_live_ui_intent_router(
        intent_tx,
        Arc::clone(&launch_selection),
        false,
        "test-digest".to_string(),
    );

    sink(UiIntent::BackgroundForegroundSubagents);

    assert!(recover_mutex_lock(&selected_workflow).is_none());
    assert_eq!(
        intent_rx.try_recv().ok(),
        Some(UiIntent::BackgroundForegroundSubagents)
    );
}

#[test]
fn live_ui_router_forwards_single_handle_demote_intent_without_switching_workflow() {
    // arrange
    // act
    // assert
    let (intent_tx, mut intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let launch_selection = Arc::new(Mutex::new(LaunchMetadata::default()));
    let (selected_workflow, sink) = build_live_ui_intent_router(
        intent_tx,
        Arc::clone(&launch_selection),
        false,
        "test-digest".to_string(),
    );

    sink(UiIntent::DemoteForegroundChildTask {
        handle_id: "req_child_demote".to_string(),
    });

    assert!(recover_mutex_lock(&selected_workflow).is_none());
    assert_eq!(
        intent_rx.try_recv().ok(),
        Some(UiIntent::DemoteForegroundChildTask {
            handle_id: "req_child_demote".to_string(),
        })
    );
}

#[test]
fn live_ui_router_records_model_switch_without_switching_workflow() {
    // arrange
    // act
    // assert
    let (intent_tx, mut intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let launch_selection = Arc::new(Mutex::new(LaunchMetadata::default()));
    let (selected_workflow, sink) = build_live_ui_intent_router(
        intent_tx,
        Arc::clone(&launch_selection),
        false,
        "test-digest".to_string(),
    );
    let launch_metadata =
        LaunchMetadata::from_model_ref("ops", "anthropic:claude-3.7").with_mode_label("Live");

    sink(UiIntent::SwitchModel {
        profile: "ops".to_string(),
        launch_metadata: launch_metadata.clone(),
    });

    assert!(recover_mutex_lock(&selected_workflow).is_none());
    assert_eq!(
        intent_rx.try_recv().ok(),
        Some(UiIntent::SwitchModel {
            profile: "ops".to_string(),
            launch_metadata,
        })
    );
    let recorded = recover_mutex_lock(&launch_selection).clone();
    assert_eq!(recorded.profile(), "ops");
    assert_eq!(recorded.provider(), "anthropic");
    assert_eq!(recorded.model(), Some("claude-3.7"));
    assert_eq!(recorded.mode_label(), None);
}

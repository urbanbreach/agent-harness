use super::*;
use harness::UnwrapOrAbort;

#[tokio::test]
async fn configured_opaque_tui_target_preserves_reasoning_summary_capability_in_provider_request() {
    // arrange
    use harness_core::config::{resolve_model_selection, HarnessConfig};
    use harness_providers::mock::MockProvider;

    let config: HarnessConfig = load_config_from_str(
        r#"
        {
          provider: {
            custom: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
                apiMode: "responses"
              },
              models: {
                "opaque-model-id": {
                  name: "Opaque Gemini",
                  metadata: {
                    family: "gemini",
                    supportsReasoningSummaries: true
                  },
                  variants: {
                    high: {
                      name: "High",
                      metadata: { reasoningEffort: "high" }
                    }
                  }
                }
              }
            }
          },
          model: "custom/opaque-model-id",
          agent: {
            default: {
              system_prompt: "Answer carefully.",
              model: "custom/opaque-model-id",
              tools: []
            }
          },
          permission: "allow"
        }
        "#,
    )
    .unwrap_or_abort();
    let mut agent_profiles = bootstrap::interactive_agent_profiles(&config).unwrap_or_abort();
    agent_profiles
        .get_mut("default")
        .unwrap_or_abort()
        .toolset
        .clear();
    let launch_metadata =
        interactive_launch_metadata(Some(&config), &agent_profiles, "default").unwrap_or_abort();
    let launch_metadata = apply_model_selection_to_launch_metadata(
        launch_metadata,
        &PersistedModelSelection {
            schema_version: 2,
            config_digest: "test-digest".to_string(),
            profile: "default".to_string(),
            provider: "custom".to_string(),
            model: "opaque-model-id".to_string(),
            variant: Some("high".to_string()),
        },
    );
    let provider = Arc::new(MockProvider::default());
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut coordinator_config = CoordinatorConfig::new(temp_dir.path().join("sessions"));
    coordinator_config.deterministic_store = true;
    coordinator_config.provider_model_concurrency = 1;
    coordinator_config.agent_profiles = agent_profiles;
    let coordinator_provider = Arc::clone(&provider);
    coordinator_config.provider = coordinator_provider;
    coordinator_config.agent_model_targets.insert(
        "default".to_string(),
        resolve_model_selection(&config, "custom:opaque-model-id", None)
            .unwrap_or_abort()
            .primary,
    );
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("opaque-tui-target", temp_dir.path())
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();
    let live_agent_target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some(agent_id),
        profile: "default".to_string(),
        last_request_id: None,
    }));
    let (intent_tx, intent_rx) = mpsc::unbounded_channel();
    let (status_tx, _status_rx) = live_update_channel();
    let handle = tokio::spawn(handle_ui_intents(
        coordinator.clone(),
        intent_rx,
        user_actor(),
        Some(live_agent_target),
        status_tx,
        TuiAuthBackendContext {
            config_path: None,
            session_dir: Some(temp_dir.path().join("sessions")),
            workspace_root: temp_dir.path().to_path_buf(),
            config_digest: "test-digest".to_string(),
        },
    ));

    // act
    intent_tx
        .send(UiIntent::SubmitPrompt {
            text: "preserve configured capabilities".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            attachments: Vec::new(),
            launch_metadata,
        })
        .unwrap_or_abort();
    drop(intent_tx);
    handle.await.unwrap_or_abort().unwrap_or_abort();
    let request = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(request) = provider.captured_requests().await.into_iter().next() {
                break request;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_abort();

    // assert
    assert_eq!(request.model_id, "opaque-model-id");
    assert_eq!(request.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(request.reasoning_summary.as_deref(), Some("auto"));
    coordinator.stop_run().await.unwrap_or_abort();
}

#[tokio::test]
async fn selected_tui_variant_target_reaches_provider_start_runtime_context() {
    // arrange
    use harness_core::config::{ResolvedModelLimits, ResolvedModelTarget};
    use harness_core::model_resolution::{resolve_model, ModelResolutionInput};
    use harness_core::proj::RunMetadata;

    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = CoordinatorConfig::new(temp_dir.path().join("sessions"));
    config.deterministic_store = true;
    config.agent_profiles = golden_path_profiles();
    config.provider = Arc::new(golden_path_provider());
    config.agent_model_targets.insert(
        "default".to_string(),
        ResolvedModelTarget {
            model_ref: "mock:model-1".to_string(),
            provider: "mock".to_string(),
            model: "model-1".to_string(),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            limits: ResolvedModelLimits::compatibility_mirror(
                Some(8_192),
                Some(4_096),
                Some(1_024),
            ),
            resolution: resolve_model(ModelResolutionInput {
                provider: "mock",
                model: "model-1",
                metadata_family: None,
                input_modalities: &[],
                supports_tool_calls: Some(true),
                supports_reasoning_summaries: Some(false),
            }),
            catalog_entry: None,
        },
    );
    let selected_limits =
        ResolvedModelLimits::compatibility_mirror(Some(64_000), Some(48_000), Some(8_000));
    let selected = ModelOption {
        profile: "default".to_string(),
        provider: "mock".to_string(),
        provider_display_label: Some("Mock".to_string()),
        provider_backend_label: Some("Test backend".to_string()),
        model: "model-1".to_string(),
        model_display_label: Some("Variant Model".to_string()),
        variant: Some("high".to_string()),
        variant_display_label: Some("High".to_string()),
        display_label: Some("Variant Model · High".to_string()),
        token_window_label: Some("64k ctx · 48k in · 8k out".to_string()),
        model_limits: selected_limits.clone(),
        description: Some("selected target".to_string()),
        profile_description: Some("Default profile".to_string()),
        reasoning_effort: Some("high".to_string()),
        text_verbosity: Some("low".to_string()),
        thinking: None,
        recommended_for: Some("review".to_string()),
    };
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("tui-target", temp_dir.path())
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();
    let live_agent_target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some(agent_id),
        profile: "default".to_string(),
        last_request_id: None,
    }));
    let (intent_tx, intent_rx) = mpsc::unbounded_channel();
    let (status_tx, _status_rx) = live_update_channel();
    let handle = tokio::spawn(handle_ui_intents(
        coordinator.clone(),
        intent_rx,
        user_actor(),
        Some(live_agent_target),
        status_tx,
        TuiAuthBackendContext {
            config_path: None,
            session_dir: Some(temp_dir.path().join("sessions")),
            workspace_root: temp_dir.path().to_path_buf(),
            config_digest: "test-digest".to_string(),
        },
    ));

    // act
    intent_tx
        .send(UiIntent::SubmitPrompt {
            text: "use selected target".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            attachments: Vec::new(),
            launch_metadata: LaunchMetadata::from_model_option(&selected),
        })
        .unwrap_or_abort();
    drop(intent_tx);
    handle.await.unwrap_or_abort().unwrap_or_abort();
    let metadata: RunMetadata = serde_json::from_str(
        &std::fs::read_to_string(run.run_dir.join("meta.json")).unwrap_or_abort(),
    )
    .unwrap_or_abort();
    let recorded = metadata.recorded_runtime_context.unwrap_or_abort();

    // assert
    assert_eq!(recorded.provider, "mock");
    assert_eq!(recorded.model, "model-1");
    assert_eq!(recorded.variant.as_deref(), Some("high"));
    assert_eq!(recorded.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(recorded.model_limits, selected_limits);
    coordinator.stop_run().await.unwrap_or_abort();
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
        .spawn_agent_idle(supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();

    let live_agent_target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some(agent_id),
        profile: "default".to_string(),
        last_request_id: None,
    }));
    let (intent_tx, intent_rx) = mpsc::unbounded_channel();
    let (status_tx, status_rx) = live_update_channel();

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
    let (tx, rx) = live_update_channel();

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
    assert!(matches!(
        updates[1],
        LiveUpdate::Event(ref event)
            if matches!(
                event.as_ref(),
                harness_core::event::RuntimeEvent::Durable(durable)
                    if is_terminal_event(&durable.payload)
            )
    ));
}

#[tokio::test]
async fn event_forwarder_delivers_live_fragments_without_advancing_durable_sequence() {
    // Given: the forwarder has consumed a durable start barrier.
    let store = Arc::new(InMemoryEventStore::new());
    store
        .append(forwarder_event_draft(
            "run_forwarder_live",
            "started",
            EventV1::RunStarted(RunStartedEvent {
                run_name: "forwarder live".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ))
        .unwrap_or_abort();
    let (tx, rx) = live_update_channel();
    let forward_store = Arc::clone(&store);
    let forwarder =
        tokio::spawn(async move { forward_events_to_tui(forward_store, tx, 1, None, true).await });
    let first_rx = rx.receiver().clone();
    let first =
        tokio::task::spawn_blocking(move || first_rx.recv_timeout(Duration::from_millis(500)))
            .await
            .unwrap_or_abort()
            .unwrap_or_abort();
    assert!(matches!(
        first,
        LiveUpdate::Event(event)
            if matches!(event.as_ref(), harness_core::event::RuntimeEvent::Durable(durable) if durable.seq == 1)
    ));

    // When: a sequence-free live fragment is followed by durable sequence 2.
    store.publish_live(harness_core::event::LiveEventEnvelope {
        event_id: "live-text".to_string(),
        run_id: "run_forwarder_live".into(),
        mono_ms: 2,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent-1".to_string()),
        ),
        correlation_id: Some("turn-1".to_string()),
        causation_id: None,
        stream_key: Some("agent:agent-1".to_string()),
        payload: harness_core::event::LiveEventV1::ProviderTextDelta {
            request_id: "provider-1".into(),
            delta: "hello".to_string(),
        },
    });
    store
        .append(forwarder_event_draft(
            "run_forwarder_live",
            "finished",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ))
        .unwrap_or_abort();
    tokio::time::timeout(Duration::from_millis(500), forwarder)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort()
        .unwrap_or_abort();

    // Then: the live variant is preserved and the next durable event remains sequence 2.
    let remaining_rx = rx.receiver().clone();
    let remaining = tokio::task::spawn_blocking(move || {
        [
            remaining_rx.recv_timeout(Duration::from_millis(500)),
            remaining_rx.recv_timeout(Duration::from_millis(500)),
        ]
    })
    .await
    .unwrap_or_abort();
    assert!(matches!(
        &remaining[0],
        Ok(LiveUpdate::Event(event))
            if matches!(event.as_ref(), harness_core::event::RuntimeEvent::Live(live) if matches!(&live.payload, harness_core::event::LiveEventV1::ProviderTextDelta { delta, .. } if delta == "hello"))
    ));
    assert!(matches!(
        &remaining[1],
        Ok(LiveUpdate::Event(event))
            if matches!(event.as_ref(), harness_core::event::RuntimeEvent::Durable(durable) if durable.seq == 2)
    ));
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
    let (status_tx, status_rx) = live_update_channel();

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
        reason: harness_tui::app::InterruptReason::User,
    });

    assert!(recover_mutex_lock(&selected_workflow).is_none());
    assert_eq!(
        intent_rx.try_recv().ok(),
        Some(UiIntent::InterruptSession {
            task_ids: vec!["task_active".to_string()],
            reason: harness_tui::app::InterruptReason::User,
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

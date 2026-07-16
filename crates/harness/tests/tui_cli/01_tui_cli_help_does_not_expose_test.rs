use harness::UnwrapOrAbort;
#[test]
fn tui_cli_help_does_not_expose_headless_output_flags() {
    let output = run_harness(["tui", "--help"]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("--output"));
    assert!(!stdout.contains("--quiet"));
}
#[test]
fn tui_startup_new_session_bootstraps_live_after_intent() {
    let _guard = startup_draft_test_lock()
        .lock()
        .unwrap_or_abort();
    set_pending_live_prompt_draft(None);

    set_pending_live_prompt_draft(Some("launcher draft".to_string()));
    let app = AppState::new_live(None, false, None);

    assert_eq!(app.composer.prompt_buffer, "launcher draft");
    assert!(
        app.composer.prompt_history.is_empty(),
        "draft carry-over must not auto-submit"
    );
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Ready);
    assert!(
        !app.runtime_state().summary.contains("startup launcher"),
        "startup-only status must not leak into live runtime state"
    );

    set_pending_live_prompt_draft(None);
}
#[test]
fn tui_startup_replay_session_uses_replay_mode() {
    let app = AppState::new_replay(std::path::PathBuf::from("/tmp/run"), Vec::new());
    assert!(app.replay_mode, "replay launch should enter replay mode");
}
#[test]
fn tui_startup_carries_unsent_draft_into_new_live_session() {
    let _guard = startup_draft_test_lock()
        .lock()
        .unwrap_or_abort();
    set_pending_live_prompt_draft(None);

    set_pending_live_prompt_draft(Some("unsent startup draft".to_string()));
    let app = AppState::new_live(None, false, None);

    assert_eq!(app.composer.prompt_buffer, "unsent startup draft");
    assert_eq!(app.composer.prompt_cursor, "unsent startup draft".chars().count());
    assert!(!app.startup_mode, "live handoff must clear startup mode");
    assert!(
        app.composer.prompt_history.is_empty(),
        "live handoff must not create pending turn"
    );
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Ready);

    set_pending_live_prompt_draft(None);
}
#[tokio::test]
async fn tui_new_live_bootstrap_stays_idle_until_first_user_prompt() {
    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();

    let mut coordinator_config = CoordinatorConfig::new(&session_dir);
    coordinator_config.agent_profiles.insert(
        "deep".to_string(),
        AgentProfile {
            name: "deep".to_string(),
            category: "deep".to_string(),
            model_ref: "default:default".to_string(),
            model_ref_explicit: true,
            system_prompt: "deep agent mode intro".to_string(),
            cache_retention: Default::default(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            toolset: Vec::new(),
            permission_ruleset: Vec::new(),
        },
    );

    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("interactive", &workspace)
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            "deep",
            None,
        )
        .await
        .unwrap_or_abort();

    let before = load_events_from_run_dir(&run.run_dir).unwrap_or_abort();
    assert!(before
        .iter()
        .any(|event| matches!(&event.payload, EventV1::AgentSpawned(_))));
    assert!(
        !before.iter().any(|event| matches!(
            &event.payload,
            EventV1::UserMessageSubmitted(_) | EventV1::ProviderRequestStarted(_)
        )),
        "idle live bootstrap must not auto-submit a synthetic first turn"
    );

    let request_id = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, Some("interactive-user".to_string())),
            agent_id,
            "first real prompt",
        )
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let after = load_events_from_run_dir(&run.run_dir).unwrap_or_abort();
    let first_started = after
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(payload) => Some((event, payload)),
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(
        first_started.0.correlation_id.as_deref(),
        Some(request_id.as_str())
    );
    assert_eq!(
        first_started
            .1
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.turn_id.as_deref()),
        Some(request_id.as_str())
    );
    assert_eq!(first_started.1.prompt_summary, "first real prompt");
    assert_eq!(
        after.iter()
            .filter(|event| matches!(&event.payload, EventV1::ProviderRequestStarted(_)))
            .count(),
        1,
        "interactive bootstrap should only create one provider request after the user's first prompt"
    );
}
#[tokio::test]
async fn interactive_runtime_routes_non_default_profile_to_matching_provider() {
    let (default_provider, ops_provider, provider_router) = capturing_interactive_provider_router();

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();

    let config_path = temp.path().join("harness.multi-provider.jsonc");
    std::fs::write(
        &config_path,
        multi_provider_interactive_config(
            "https://default.fixture/v1",
            "https://ops.fixture/v1",
            &session_dir,
        ),
    )
    .unwrap_or_abort();

    let config = load_config_from_file(&config_path).unwrap_or_abort();
    let mut coordinator_config = bootstrap::build_interactive_coordinator_config(&config)
        .unwrap_or_abort();
    coordinator_config.provider = provider_router;
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("interactive", &workspace)
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            "ops",
            None,
        )
        .await
        .unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, Some("interactive-user".to_string())),
            agent_id,
            "Hello from ops",
        )
        .await
        .unwrap_or_abort();

    let provider_started = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let events = load_events_from_run_dir(&run.run_dir).unwrap_or_abort();
            if let Some(provider_started) = events.iter().find_map(|event| match &event.payload {
                EventV1::ProviderRequestStarted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
                {
                    Some(data.clone())
                }
                _ => None,
            }) {
                break provider_started;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_abort();

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if !ops_provider.requests().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_abort();

    coordinator.stop_run().await.unwrap_or_abort();

    let default_requests = default_provider.requests();
    let ops_requests = ops_provider.requests();
    assert!(
        default_requests.is_empty(),
        "interactive runtime should not hit providers.default for ops profile"
    );
    assert_eq!(
        ops_requests.len(),
        1,
        "interactive runtime should hit the selected provider exactly once"
    );
    assert_eq!(ops_requests[0].provider_id.as_deref(), Some("anthropic"));
    assert_eq!(ops_requests[0].model_id, "claude-3.7");

    assert_eq!(provider_started.provider_id, "anthropic");
    assert_eq!(provider_started.model_id, "claude-3.7");
}
#[tokio::test]
async fn new_live_session_persists_selected_runtime_context_into_run_metadata() {
    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();

    let mut coordinator_config = CoordinatorConfig::new(&session_dir);
    coordinator_config.agent_profiles.insert(
        "deep".to_string(),
        AgentProfile {
            name: "deep".to_string(),
            category: "deep".to_string(),
            model_ref: "default:gpt-5.4-mini".to_string(),
            model_ref_explicit: true,
            system_prompt: "deep agent mode intro".to_string(),
            cache_retention: Default::default(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: ToolFailureMode::FailTurn,
            toolset: Vec::new(),
            permission_ruleset: Vec::new(),
        },
    );
    coordinator_config.agent_profiles.insert(
        "ops".to_string(),
        AgentProfile {
            name: "ops".to_string(),
            category: "ops".to_string(),
            model_ref: "anthropic:claude-3.7".to_string(),
            model_ref_explicit: true,
            system_prompt: "ops agent mode intro".to_string(),
            cache_retention: Default::default(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: ToolFailureMode::FailTurn,
            toolset: Vec::new(),
            permission_ruleset: Vec::new(),
        },
    );

    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("interactive", &workspace)
        .await
        .unwrap_or_abort();
    coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            "ops",
            None,
        )
        .await
        .unwrap_or_abort();

    let meta_body = std::fs::read_to_string(run.run_dir.join("meta.json")).unwrap_or_abort();
    let metadata: RunMetadata = serde_json::from_str(&meta_body).unwrap_or_abort();
    let recorded_runtime_context = metadata
        .recorded_runtime_context
        .unwrap_or_abort();

    assert_eq!(recorded_runtime_context.profile, "ops");
    assert_eq!(recorded_runtime_context.provider, "anthropic");
    assert_eq!(recorded_runtime_context.model, "claude-3.7");

    let bootstrap_events = load_events_from_run_dir(&run.run_dir).unwrap_or_abort();
    assert!(bootstrap_events
        .iter()
        .any(|event| matches!(&event.payload, EventV1::AgentSpawned(_))));
    assert!(
        !bootstrap_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::UserMessageSubmitted(_) | EventV1::ProviderRequestStarted(_)
        )),
        "selected runtime context must persist before the first user turn starts"
    );

    coordinator.stop_run().await.unwrap_or_abort();
}
#[test]
fn tui_continue_session_bootstraps_live_with_preloaded_history() {
    let _guard = startup_draft_test_lock()
        .lock()
        .unwrap_or_abort();
    set_pending_live_prompt_draft(None);

    set_pending_live_prompt_draft(Some("preserved continue draft".to_string()));
    set_pending_live_launch_metadata(
        LaunchMetadata::new("alpha", "mock", Some("model-1".to_string()))
            .with_mode_label("Continued"),
    );

    let mut app = AppState::new_live(
        Some(std::path::PathBuf::from("/tmp/sessions/run_continue")),
        false,
        None,
    );
    for event in [
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope_with_correlation(
            2,
            Some("req_000001"),
            EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_000001".into(),
                text: "first question".to_string(),
            }),
        ),
        envelope_with_correlation(
            3,
            Some("req_000001"),
            EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_000001".into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "first question".to_string(),
                request_digest: "digest-1".to_string(),
                metadata: None,
            }),
        ),
        envelope_with_correlation(
            4,
            Some("req_000001"),
            EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_000001".into(),
                delta: "first answer".to_string(),
            }),
        ),
        envelope_with_correlation(
            5,
            Some("req_000001"),
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000001".to_string().into(),
                result_summary: "first answer".to_string(),
                result_digest: "digest-out".to_string(),
                metadata: None,
            }),
        ),
    ] {
        app.ingest_event(event);
    }

    assert_eq!(app.active_provider(), "mock");
    assert_eq!(app.current_model_label(), "model-1");
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);
    assert_eq!(app.composer.prompt_buffer, "preserved continue draft");
}
#[test]
fn tui_continue_session_restores_launch_metadata_from_history() {
    let _guard = startup_draft_test_lock()
        .lock()
        .unwrap_or_abort();

    set_pending_live_launch_metadata(
        LaunchMetadata::new(
            "history-profile",
            "history-provider",
            Some("history-model".to_string()),
        )
        .with_mode_label("Continued"),
    );

    let app = AppState::new_live(
        Some(std::path::PathBuf::from("/tmp/sessions/run_continue")),
        false,
        None,
    );

    assert_eq!(app.launch_mode_label(), Some("Continued"));
    assert_eq!(app.active_profile(), "history-profile");
    assert_eq!(app.active_provider(), "history-provider");
    assert_eq!(app.current_model_label(), "history-model");
}

use harness_core::config::{
    ModelLimitProvenance, ResolvedModelLimits, ResolvedModelTarget,
};
use harness_core::model_resolution::ModelResolution;
use harness_core::UnwrapOrAbort;

fn known_limits(context: u32, input: u32, output: u32) -> ResolvedModelLimits {
    ResolvedModelLimits::from_values(
        Some(context),
        Some(input),
        Some(output),
        ModelLimitProvenance::explicit("task 4 test"),
    )
}

fn model_target(model_id: &str, limits: ResolvedModelLimits) -> ResolvedModelTarget {
    ResolvedModelTarget {
        model_ref: format!("mock:{model_id}"),
        provider: "mock".to_string(),
        model: model_id.to_string(),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        limits,
        resolution: ModelResolution::default(),
        catalog_entry: None,
    }
}

#[tokio::test]
async fn unified_context_budget_recosts_retry_and_tool_loop_requests() {
    // arrange: a known-limit model, one retryable failure, and one tool continuation.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = BudgetObservingProvider::new(vec![
        vec![ProviderStreamEvent::categorized_error(
            "retry once",
            ProviderErrorCategory::RateLimited,
        )],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call_shell".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: r#"{"command":"printf done"}"#.to_string(),
            },
            ProviderStreamEvent::Done { usage: None },
        ],
        provider_text_events("budgeted answer"),
    ]);
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider = Arc::new(provider.clone());
    config.tool_registry = test_tool_registry();
    config.permission_policy = allow_all_permission_policy();
    config.agent_profiles = agent_profiles();
    config.agent_profiles.get_mut("alpha").unwrap_or_abort().toolset =
        vec!["shell.run".to_string()];
    config.provider_retry = harness_core::config::ProviderRetryRuntimeConfig {
        max_retries: 1,
        base_delay_ms: 0,
        max_delay_ms: 0,
    };
    config.compaction.reserve_tokens = 1_000;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let target = model_target("model-1", known_limits(50_000, 45_000, 4_000));
    let run = coordinator
        .start_run("budget_retry_tool_loop", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();

    // act: the turn retries once and then continues after a tool result.
    let request_id = coordinator
        .request_agent_turn_with_model_target(
            supervisor_actor(),
            agent_id,
            "run the shell tool",
            target.clone(),
        )
        .await
        .unwrap_or_abort();
    let events = wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "budgeted answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    // assert: every current request is capped and every start carries current budget evidence.
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.max_tokens == Some(4_000)));
    let snapshots = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(started)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
            {
                started.metadata.as_ref()?.context_budget
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(snapshots.len(), 3);
    assert!(snapshots.iter().all(|snapshot| {
        snapshot.reserved_output_tokens == Some(4_000)
            && snapshot.occupied_input_tokens > 0
    }));
    assert_eq!(snapshots[0].occupied_input_tokens, snapshots[1].occupied_input_tokens);
    assert!(snapshots[2].occupied_input_tokens > snapshots[1].occupied_input_tokens);

    let meta_body = fs::read_to_string(run.run_dir.join("meta.json")).unwrap_or_abort();
    let metadata: harness_core::proj::RunMetadata =
        serde_json::from_str(&meta_body).unwrap_or_abort();
    let recorded = metadata.recorded_runtime_context.unwrap_or_abort();
    assert_eq!(recorded.model_limits, target.limits);
    assert_eq!(recorded.last_request_budget, snapshots.last().copied());
    let meta_value: serde_json::Value = serde_json::from_str(&meta_body).unwrap_or_abort();
    let runtime_value = &meta_value["recorded_runtime_context"];
    for mirror in [
        "context_window_tokens",
        "max_input_tokens",
        "max_output_tokens",
    ] {
        assert!(runtime_value.get(mirror).is_none(), "scalar mirror `{mirror}` was persisted");
    }

    let observations = provider.observations();
    let distinct_message_digests = observations
        .iter()
        .map(|observation| observation.message_digest.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(distinct_message_digests.len(), 2);
    assert!(observations.len() >= 8, "each preparation must recost");
}

#[tokio::test]
async fn last_request_budget_root_metadata_ignores_child_requests() {
    // arrange: root and child requests with different canonical reservations.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = BudgetObservingProvider::new(vec![
        provider_text_events("root answer"),
        provider_text_events("child answer"),
    ]);
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles();
    config.compaction.reserve_tokens = 1_000;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let root_target = model_target("model-root", known_limits(50_000, 45_000, 4_000));
    let child_target = model_target("model-child", known_limits(20_000, 18_000, 1_000));
    let run = coordinator
        .start_run("root_budget_only", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let root_agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let root_request_id = coordinator
        .request_agent_turn_with_model_target(
            supervisor_actor(),
            root_agent_id.clone(),
            "root turn",
            root_target,
        )
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(payload)
                    if event.correlation_id.as_deref() == Some(root_request_id.as_str())
                        && payload.result_summary == "root answer"
            )
        })
    })
    .await;
    let child_agent_id = coordinator
        .spawn_agent_idle(
            supervisor_actor(),
            "alpha",
            Some(root_agent_id),
        )
        .await
        .unwrap_or_abort();

    // act: the child dispatches with its own event-scoped budget.
    let child_request_id = coordinator
        .request_agent_turn_with_model_target(
            supervisor_actor(),
            child_agent_id,
            "child turn",
            child_target,
        )
        .await
        .unwrap_or_abort();
    let events = wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(payload)
                    if event.correlation_id.as_deref() == Some(child_request_id.as_str())
                        && payload.result_summary == "child answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    // assert: child evidence remains in its event and cannot overwrite root metadata.
    let child_snapshot = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(started)
                if event.correlation_id.as_deref() == Some(child_request_id.as_str()) =>
            {
                started.metadata.as_ref()?.context_budget
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(child_snapshot.reserved_output_tokens, Some(1_000));
    let metadata: harness_core::proj::RunMetadata = serde_json::from_str(
        &fs::read_to_string(run.run_dir.join("meta.json")).unwrap_or_abort(),
    )
    .unwrap_or_abort();
    let root_snapshot = metadata
        .recorded_runtime_context
        .and_then(|context| context.last_request_budget)
        .unwrap_or_abort();
    assert_eq!(root_snapshot.reserved_output_tokens, Some(4_000));
}

#[tokio::test]
async fn fallback_recosts_budget_for_the_current_model() {
    // arrange: a primary model and fallback with different canonical output reservations.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = BudgetObservingProvider::new(vec![
        vec![ProviderStreamEvent::categorized_error(
            "primary unavailable",
            ProviderErrorCategory::MissingCredentials,
        )],
        provider_text_events("fallback answer"),
    ]);
    let primary = model_target("model-1", known_limits(50_000, 45_000, 4_000));
    let fallback = model_target("model-2", known_limits(20_000, 18_000, 1_000));
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles();
    config.compaction.reserve_tokens = 1_000;
    config
        .agent_model_targets
        .insert("alpha".to_string(), primary);
    config
        .agent_model_fallbacks
        .insert("alpha".to_string(), vec![fallback]);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("budget_model_fallback", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();

    // act: the primary provider call fails and the coordinator switches model.
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "use fallback")
        .await
        .unwrap_or_abort();
    let events = wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "fallback answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    // assert: transmitted caps and snapshots follow the model active for each attempt.
    let requests = provider.requests();
    assert_eq!(requests.iter().map(|request| request.model_id.as_str()).collect::<Vec<_>>(), vec!["model-1", "model-2"]);
    assert_eq!(requests.iter().map(|request| request.max_tokens).collect::<Vec<_>>(), vec![Some(4_000), Some(1_000)]);
    let reservations = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(started)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
            {
                started.metadata.as_ref()?.context_budget?.reserved_output_tokens
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(reservations, vec![4_000, 1_000]);
    assert!(provider
        .observations()
        .iter()
        .any(|observation| observation.model_id == "model-2" && observation.max_tokens == Some(1_000)));
}

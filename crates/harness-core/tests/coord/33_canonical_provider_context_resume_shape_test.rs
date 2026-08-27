#[test]
fn resume_rejects_profile_shape_drift_before_provider_dispatch() {
    // arrange
    // act
    // assert
    // Given: a canonical view persisted against the original profile/tool shape.
    let provider = CapturingProvider::new(vec!["must not dispatch"]);
    let mut original_profile = agent_profiles().remove("default").unwrap_or_abort();
    original_profile.toolset = vec!["shell.run".to_string()];
    let registry = test_tool_registry();
    let tools = harness_core::agent::build_provider_tool_defs_for_model(
        &original_profile,
        registry.as_ref(),
        "mock:model-1",
    )
    .unwrap_or_abort();
    let target = drift_target(
        "target-a-variant",
        "high",
        "low",
        "auto",
        serde_json::json!({"budget": 1024}),
    );
    let model = harness_core::agent::AgentModelRef {
        provider_id: target.provider.clone(),
        model_id: target.model.clone(),
    };
    let runtime_selection = harness_core::agent::canonical_runtime_selection(
        harness_core::agent::CanonicalRuntimeSelectionInput {
            profile: &original_profile,
            model: &model,
            settings: harness_core::agent::AgentModelSettings::from(&target),
            resolved_limits: target.limits,
            tools: &tools,
        },
    )
    .unwrap_or_abort();
    let entry_id = harness_core::ids::EntryId::new("shape-leaf");
    let view = harness_core::session::CanonicalProviderView {
        owner: harness_core::session::ProviderViewOwner::root(
            "agent_000001",
            harness_core::ids::SessionId::new("run_000001"),
        ),
        selected_leaf: entry_id.clone(),
        active_entry_ids: vec![entry_id.clone()],
        entries: vec![harness_core::session::SessionEntry {
            id: entry_id,
            parent_id: None,
            turn_id: Some(harness_core::ids::TurnId::new("turn-shape")),
            run_id: harness_core::ids::RunId::new("run_000001"),
            payload: harness_core::session::SessionEntryPayload::UserMessage {
                text: "persisted history".to_string(),
                attachments: Vec::new(),
            },
        }],
        pending_prompt: Some(harness_core::session::CanonicalPendingPrompt {
            turn_id: harness_core::ids::TurnId::new("turn-pending"),
            text: "continue".to_string(),
            attachments: Vec::new(),
        }),
        latest_compaction_summary: None,
        tool_pairs: Vec::new(),
        attachments: Vec::new(),
        usage_boundaries: Vec::new(),
        watermark: Some(harness_core::session::RecordSequence::new(3)),
        runtime_selection,
    };
    let mut drifted_profile = original_profile;
    drifted_profile.system_prompt = "drifted system prompt".to_string();

    // When: lowering checks the current profile/tool shape before any provider call.
    let error = harness_core::agent::lower_provider_continuation(
        harness_core::agent::LowerProviderContinuationInput {
            view: &view,
            transient_operational_turns: &[],
            profile: &drifted_profile,
            tools: Some(tools),
            tool_choice: Some(harness_providers::ToolChoice::Auto),
            fresh_request_id: "request-fresh",
        },
    )
    .unwrap_err();

    // Then: the mismatch is typed and the provider has observed no request.
    assert!(matches!(
        error,
        harness_core::agent::ProviderContinuationLoweringError::ProfileToolShapeMismatch { .. }
    ));
    assert!(provider.requests().is_empty());
}

#[tokio::test]
async fn provider_request_started_persists_canonical_runtime_selection() {
    // arrange
    // act
    // assert
    // Given: a configured target whose optional settings are all distinct.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let target = drift_target(
        "persisted-variant",
        "high",
        "low",
        "auto",
        serde_json::json!({"budget": 2048, "mode": "persisted"}),
    );
    let provider = CapturingProvider::new(vec!["persisted response"]);
    let coordinator = coordinator_for_target(
        temp_dir.path(),
        std::sync::Arc::new(provider),
        target.clone(),
    );
    let run = coordinator
        .start_run(
            "runtime-selection",
            std::path::PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();

    // When: one provider-backed turn completes through the coordinator boundary.
    let request_id = coordinator
        .request_agent_turn_with_model_target(
            supervisor_actor(),
            agent_id,
            "persist selection",
            target,
        )
        .await
        .unwrap_or_abort();
    await_turn_terminal(&mut events, &request_id).await;
    coordinator.stop_run().await.unwrap_or_abort();

    // Then: request metadata and assistant provenance carry the same redacted selection.
    let persisted = load_events(&run.events_path);
    let started = persisted
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(started) => started
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.runtime_selection.as_deref()),
            _ => None,
        })
        .unwrap_or_abort();
    let provenance = persisted
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::AssistantMessageFinished(finished) => finished.provenance.as_ref(),
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(
        (
            started.provider_id.as_str(),
            started.model_id.as_str(),
            started.variant.as_deref(),
            started.reasoning_effort.as_deref(),
            started.text_verbosity.as_deref(),
            started.reasoning_summary.as_deref(),
            started.thinking.as_ref(),
        ),
        (
            "mock",
            "model-1",
            Some("persisted-variant"),
            Some("high"),
            Some("low"),
            Some("auto"),
            Some(&serde_json::json!({"budget": 2048, "mode": "persisted"})),
        )
    );
    assert_eq!(started.profile_tool_shape_digest.len(), 64);
    assert_eq!(provenance.runtime_selection.as_deref(), Some(started));
}

#[path = "33_canonical_provider_context_resume_media_test.rs"]
mod media_test;

#[path = "33_canonical_provider_context_resume_overlay_test.rs"]
mod overlay_test;

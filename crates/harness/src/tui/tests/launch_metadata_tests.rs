use super::*;

#[test]
fn continue_launch_metadata_preserves_cross_profile_switch_options() {
    // arrange
    // act
    // assert
    let continue_metadata = LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
        .with_available_models(vec![ModelOption::from_model_ref(
            "build",
            "default:gpt-5.4-mini",
        )])
        .with_mode_label("Continued");
    let continue_profile = continue_metadata.profile().to_string();

    assert_eq!(continue_profile, "build");
    assert!(continue_metadata
        .available_models()
        .iter()
        .any(|option| option.profile == "build"));
}

#[test]
fn continue_metadata_prefers_recorded_runtime_context_before_event_inference() {
    // arrange
    // act
    // assert
    let historical_events = vec![EventEnvelopeV1 {
        schema_version: 1,
        event_id: "evt-0001".to_string(),
        seq: 1,
        run_id: "run_fixture".into(),
        mono_ms: 1,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        correlation_id: Some("req_000001".to_string()),
        causation_id: None,
        stream_key: Some("agent:agent_000001".to_string()),
        payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_000001".into(),
            provider_id: "heuristic-provider".to_string(),
            model_id: "heuristic-model".to_string(),
            prompt_summary: "turn".to_string(),
            request_digest: "digest".to_string(),
            metadata: None,
        }),
    }];
    let recorded_runtime_context = RecordedRuntimeContext {
        profile: "recorded-profile".to_string(),
        profile_description: Some("Recorded agent".to_string()),
        provider: "recorded-provider".to_string(),
        provider_display_label: Some("Recorded Provider".to_string()),
        provider_backend_label: Some("OpenAI".to_string()),
        model: "recorded-model".to_string(),
        variant: Some("recorded-variant".to_string()),
        display_label: "Recorded Model".to_string(),
        model_display_label: Some("Recorded Model".to_string()),
        variant_display_label: Some("Recorded Variant".to_string()),
        token_window_label: Some("128k ctx".to_string()),
        model_limits: harness_core::config::ResolvedModelLimits::compatibility_mirror(
            Some(128_000),
            Some(64_000),
            Some(8_000),
        ),
        context_window_tokens: Some(128_000),
        max_input_tokens: Some(64_000),
        max_output_tokens: Some(8_000),
        description: Some("recorded description".to_string()),
        recommended_for: Some("deep work".to_string()),
        reasoning_effort: Some("high".to_string()),
        text_verbosity: Some("medium".to_string()),
        thinking: None,
    };

    let metadata = continue_launch_metadata(
        "run_fixture",
        Some(&recorded_runtime_context),
        &historical_events,
        "agent_000001",
        Some("heuristic-profile"),
    );

    assert_eq!(metadata.profile(), "recorded-profile");
    assert_eq!(metadata.provider(), "recorded-provider");
    assert_eq!(metadata.model(), Some("recorded-model"));
    assert_eq!(metadata.variant(), Some("recorded-variant"));
    assert_eq!(metadata.display_label(), Some("Recorded Model"));
    assert_eq!(metadata.mode_label(), Some("Continued"));
}

#[test]
fn replay_launch_metadata_prefers_recorded_runtime_context_before_event_inference() {
    // arrange
    // act
    // assert
    let historical_events = vec![EventEnvelopeV1 {
        schema_version: 1,
        event_id: "evt-0001".to_string(),
        seq: 1,
        run_id: "run_fixture".into(),
        mono_ms: 1,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        correlation_id: Some("req_000001".to_string()),
        causation_id: None,
        stream_key: Some("agent:agent_000001".to_string()),
        payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_000001".into(),
            provider_id: "heuristic-provider".to_string(),
            model_id: "heuristic-model".to_string(),
            prompt_summary: "turn".to_string(),
            request_digest: "digest".to_string(),
            metadata: None,
        }),
    }];
    let recorded_runtime_context = RecordedRuntimeContext {
        profile: "recorded-profile".to_string(),
        profile_description: Some("Recorded agent".to_string()),
        provider: "recorded-provider".to_string(),
        provider_display_label: Some("Recorded Provider".to_string()),
        provider_backend_label: Some("OpenAI".to_string()),
        model: "recorded-model".to_string(),
        variant: None,
        display_label: "Recorded Replay Model".to_string(),
        model_display_label: Some("Recorded Replay Model".to_string()),
        variant_display_label: None,
        token_window_label: None,
        model_limits: harness_core::config::ResolvedModelLimits::compatibility_mirror(
            None, None, None,
        ),
        context_window_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        recommended_for: None,
        reasoning_effort: None,
        text_verbosity: None,
        thinking: None,
    };

    let metadata = replay_launch_metadata(Some(&recorded_runtime_context), &historical_events);

    assert_eq!(metadata.profile(), "recorded-profile");
    assert_eq!(metadata.provider(), "recorded-provider");
    assert_eq!(metadata.model(), Some("recorded-model"));
    assert_eq!(metadata.display_label(), Some("Recorded Replay Model"));
    assert_eq!(metadata.mode_label(), Some("Replay"));
}

#[test]
fn replay_bootstrap_falls_back_when_recorded_runtime_context_missing() {
    // arrange
    // act
    // assert
    let historical_events = vec![
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0001".to_string(),
            seq: 1,
            run_id: "run_fixture".into(),
            mono_ms: 1,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_fixture".to_string()),
            payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "legacy-profile".to_string(),
                parent_agent_id: None,
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0002".to_string(),
            seq: 2,
            run_id: "run_fixture".into(),
            mono_ms: 2,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            correlation_id: Some("req_000001".to_string()),
            causation_id: None,
            stream_key: Some("agent:agent_000001".to_string()),
            payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".into(),
                provider_id: "legacy-provider".to_string(),
                model_id: "legacy-model".to_string(),
                prompt_summary: "hello".to_string(),
                request_digest: "digest-1".to_string(),
                metadata: None,
            }),
        },
    ];

    let metadata = replay_launch_metadata(None, &historical_events);

    assert_eq!(metadata.profile(), "legacy-profile");
    assert_eq!(metadata.provider(), "legacy-provider");
    assert_eq!(metadata.model(), Some("legacy-model"));
    assert_eq!(metadata.mode_label(), Some("Replay"));
}

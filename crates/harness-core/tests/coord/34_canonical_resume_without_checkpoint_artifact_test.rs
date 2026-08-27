use harness_core::session::{AssistantPart, ProviderProvenance};

#[allow(
    deprecated,
    reason = "the fixture proves deprecated checkpoint events remain read-only migration input"
)]
#[tokio::test]
async fn resume_rebuilds_canonical_provider_view_without_checkpoint_artifact() {
    // arrange
    // act
    // assert
    // Given: a complete provider turn plus an applied legacy checkpoint whose file is absent.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_canonical_resume_without_checkpoint_artifact";
    let worker = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
    let correlated = |seq, payload| {
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            seq,
            worker.clone(),
            Some("req_000001"),
            payload,
        )
    };
    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "canonical-artifact-free".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            correlated(
                3,
                EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "req_000001".into(),
                    text: "durable prompt".to_string(),
                }),
            ),
            correlated(
                4,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "durable prompt".to_string(),
                    request_digest: "digest-request".to_string(),
                    metadata: None,
                }),
            ),
            correlated(
                5,
                EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "req_000001".into(),
                    delta: "durable answer".to_string(),
                }),
            ),
            correlated(
                6,
                EventV1::ProviderRequestFinished(
                    harness_core::event::ProviderRequestFinishedEvent {
                        request_id: "req_000001".into(),
                        finish_reason: "stop".to_string(),
                        output_digest: Some("digest-output".to_string()),
                        usage: None,
                        metadata: None,
                    },
                ),
            ),
            correlated(
                7,
                EventV1::AssistantMessageFinished(
                    harness_core::event::AssistantMessageFinishedEvent {
                        request_id: "req_000001".into(),
                        tool_call_count: 0,
                        parts: vec![AssistantPart::Text {
                            text: "durable answer".to_string(),
                        }],
                        provenance: Some(ProviderProvenance {
                            provider_id: "mock".to_string(),
                            model_id: "model-1".to_string(),
                            request_id: "req_000001".into(),
                            response_id: None,
                            stop_reason: Some("stop".to_string()),
                            usage: None,
                            runtime_selection: None,
                        }),
                        assistant_message: None,
                    },
                ),
            ),
            resume_fixture_event(
                run_id,
                8,
                EventV1::CompactionWritten(harness_core::event::CompactionWrittenEvent {
                    checkpoint_id: "legacy-missing".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: "artifacts/compactions/missing.json".to_string(),
                    artifact_digest: Some("0".repeat(64)),
                    artifact_bytes: 1,
                    trigger_reason: "legacy".to_string(),
                    through_seq: 7,
                    through_request_id: Some("req_000001".to_string()),
                    provider_id: Some("mock".to_string()),
                    model_id: Some("model-1".to_string()),
                    tokens_before: None,
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                    summary_source: None,
                    preserved_turns: 0,
                }),
            ),
            resume_fixture_event(
                run_id,
                9,
                EventV1::CompactionApplied(harness_core::event::CompactionAppliedEvent {
                    checkpoint_id: "legacy-missing".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 7,
                    through_request_id: Some("req_000001".to_string()),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    preserved_turns: Some(0),
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                10,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );
    let provider = CapturingProvider::new(Vec::new());
    let coordinator =
        test_resume_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()));

    // When: the coordinator restores the run without a checkpoint artifact on disk.
    coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_else(|error| panic!("canonical artifact-free resume failed: {error}"));

    // Then: canonical journal recovery succeeds without provider execution or artifact loading.
    assert!(provider.requests().is_empty());
    assert!(!temp_dir
        .path()
        .join(run_id)
        .join("artifacts/compactions/missing.json")
        .exists());
    coordinator.stop_run().await.unwrap_or_abort();
}

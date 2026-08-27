#[tokio::test]
async fn completion_replay_failure_finishes_and_cancels_pending_compaction() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let config = test_config(temp_dir.path());
    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let (_command_tx, command_rx) = mpsc::channel(1);
    let (job_tx, job_rx) = mpsc::channel(1);
    let mut coordinator = Coordinator::new(config, clock, redactor, command_rx, job_tx, job_rx);
    coordinator
        .start_run_internal_async(
            "completion_replay_failure".to_string(),
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();

    let agent_id = "agent_000001".to_string();
    let (respond_to, response) = oneshot::channel();
    let (generation, cancellation_token, events_path) = {
        let run_state = coordinator.run_state.as_mut().unwrap_or_abort();
        let generation = run_state.next_compaction_generation();
        let cancellation_token = run_state.shutdown_token.child_token();
        let base = super::super::CompactionGenerationBase::capture(run_state, None);
        run_state.pending_compactions.insert(
            agent_id.clone(),
            super::super::PendingCompactionState {
                agent_id: agent_id.clone(),
                task_id: None,
                generation,
                base,
                cancellation_token: cancellation_token.clone(),
                trigger: ProviderCompactionTrigger {
                    agent_id: agent_id.clone(),
                    profile_name: "alpha".to_string(),
                    model_ref: "mock:model-1".to_string(),
                    provider_id: None,
                    model_id: None,
                    through_request_id: None,
                    trigger_reason: "manual".to_string(),
                    tokens_before: None,
                    estimate_source: None,
                },
                response: super::super::PendingCompactionResponse::Manual(respond_to),
            },
        );
        (
            generation,
            cancellation_token,
            run_state.info.events_path.clone(),
        )
    };
    OpenOptions::new()
        .append(true)
        .open(events_path)
        .unwrap_or_abort()
        .write_all(b"not-json\n")
        .unwrap_or_abort();

    // act
    coordinator
        .compaction_generated_internal(
            agent_id.clone(),
            generation,
            Box::new(Err(CoordinatorError::CompactionFailed(
                "summary generation failed".to_string(),
            ))),
        )
        .await;
    let result = timeout(Duration::from_secs(1), response)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();

    // assert
    assert!(matches!(
        result,
        Err(CoordinatorError::EventStore(
            crate::store::EventStoreError::InvalidJsonLine { .. }
        ))
    ));
    assert!(cancellation_token.is_cancelled());
    assert!(coordinator
        .run_state
        .as_ref()
        .unwrap_or_abort()
        .pending_compactions
        .is_empty());
}

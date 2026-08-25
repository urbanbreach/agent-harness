use harness::UnwrapOrAbort;

#[test]
fn export_uses_committed_assistant_content_instead_of_delta_replay() {
    // Given: a session whose transient compatibility delta differs from its final commit.
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_committed_export");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_committed_export",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "committed export".into(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_committed_export",
                2,
                EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "turn-1".into(),
                    text: "question".to_string(),
                }),
            ),
            envelope(
                "run_committed_export",
                3,
                EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "provider-1".into(),
                    delta: "transient draft".to_string(),
                }),
            ),
            envelope(
                "run_committed_export",
                4,
                EventV1::AssistantMessageFinished(
                    harness_core::event::AssistantMessageFinishedEvent {
                        request_id: "provider-1".into(),
                        tool_call_count: 0,
                        parts: vec![harness_core::session::AssistantPart::Text {
                            text: "canonical answer".to_string(),
                        }],
                        provenance: None,
                        assistant_message: None,
                    },
                ),
            ),
        ],
    );

    // When: the product markdown export is rendered.
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "export",
        "run_committed_export",
    ]);

    // Then: only the durable assistant commit is exported.
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let markdown = String::from_utf8(output.stdout).unwrap_or_abort();
    assert!(markdown.contains("canonical answer"), "{markdown}");
    assert!(!markdown.contains("transient draft"), "{markdown}");
}

#[test]
fn interactive_mock_reopen_hint_preserves_offline_resume_mode() {
    // Given: a resumable interactive session whose persisted provider is mock.
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_mock_resume");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    let mut completed = envelope_with_actor(
        "run_mock_resume",
        5,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000001".to_string().into(),
            result_summary: "Hello world".to_string(),
            result_digest: "digest-result".to_string(),
            metadata: None,
        }),
    );
    completed.correlation_id = Some("req_000001".to_string());
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_mock_resume",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_mock_resume",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_mock_resume",
                3,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000001".into(),
                    text: "hello".to_string(),
                }),
            ),
            envelope_with_actor(
                "run_mock_resume",
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-request".to_string(),
                    metadata: None,
                }),
            ),
            completed,
            envelope(
                "run_mock_resume",
                6,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    // When: the operator asks for the supported continuation command.
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "reopen",
        "--session",
        "run_mock_resume",
        "--json",
    ]);
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_abort();

    // Then: the command preserves the offline provider selection.
    assert!(output.status.success());
    assert_eq!(response["summary"]["mode"], "interactive_mock");
    assert_eq!(response["summary"]["resumable"], true);
    assert_eq!(
        response["summary"]["continue_hint"],
        "harness prompt --mock --resume run_mock_resume --text \"<next prompt>\""
    );
}

#[test]
fn prompt_cli_accepts_mock_resume_for_offline_continuation() {
    // Given: the documented offline continuation argument combination.
    // When: clap parses the command before session lookup.
    let output = run_harness([
        "prompt",
        "--mock",
        "--resume",
        "missing-run",
        "--text",
        "hello",
    ]);

    // Then: argument parsing succeeds and reaches the expected missing-session boundary.
    assert_ne!(output.status.code(), 2);
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("cannot be used with"),
        "mock resume must be an accepted CLI combination"
    );
}

#[test]
fn interactive_mock_session_continues_offline_from_semantic_commit() {
    // Given: one persisted mock turn with only a final semantic assistant commit.
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_mock_continue");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    std::fs::write(
        run_dir.join("meta.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "run_id": "run_mock_continue",
            "run_name": "interactive",
            "workspace_root": "/tmp/workspace",
            "profile_preset": "default",
            "provider": "mock",
            "model": "model-1",
            "created_at": "1710000000000",
            "config_digest": "test-digest",
            "harness_version": "test",
            "mode_source": "interactive_mock"
        }))
        .unwrap_or_abort(),
    )
    .unwrap_or_abort();
    let worker = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
    let mut provider_started = envelope_with_actor(
        "run_mock_continue",
        4,
        worker.clone(),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_000002".into(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "hello".to_string(),
            request_digest: "digest-request".to_string(),
            metadata: None,
        }),
    );
    provider_started.correlation_id = Some("req_000001".to_string());
    let mut provider_finished = envelope_with_actor(
        "run_mock_continue",
        5,
        worker.clone(),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_000002".into(),
            finish_reason: "done".to_string(),
            output_digest: Some("digest-output".to_string()),
            usage: None,
            metadata: None,
        }),
    );
    provider_finished.correlation_id = Some("req_000001".to_string());
    let mut assistant_finished = envelope_with_actor(
        "run_mock_continue",
        6,
        worker.clone(),
        EventV1::AssistantMessageFinished(
            harness_core::event::AssistantMessageFinishedEvent {
                request_id: "req_000002".into(),
                tool_call_count: 0,
                parts: vec![harness_core::session::AssistantPart::Text {
                    text: "Hello world".to_string(),
                }],
                provenance: None,
                assistant_message: None,
            },
        ),
    );
    assistant_finished.correlation_id = Some("req_000001".to_string());
    let mut completed = envelope_with_actor(
        "run_mock_continue",
        7,
        worker,
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000001".to_string().into(),
            result_summary: "Hello world".to_string(),
            result_digest: "digest-output".to_string(),
            metadata: None,
        }),
    );
    completed.correlation_id = Some("req_000001".to_string());
    let second_worker = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
    let mut second_provider_started = envelope_with_actor(
        "run_mock_continue",
        9,
        second_worker.clone(),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_000004".into(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "hello".to_string(),
            request_digest: "digest-request-second".to_string(),
            metadata: None,
        }),
    );
    second_provider_started.correlation_id = Some("req_000003".to_string());
    let mut second_provider_finished = envelope_with_actor(
        "run_mock_continue",
        10,
        second_worker.clone(),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_000004".into(),
            finish_reason: "done".to_string(),
            output_digest: Some("digest-output-second".to_string()),
            usage: None,
            metadata: None,
        }),
    );
    second_provider_finished.correlation_id = Some("req_000003".to_string());
    let mut second_assistant_finished = envelope_with_actor(
        "run_mock_continue",
        11,
        second_worker.clone(),
        EventV1::AssistantMessageFinished(
            harness_core::event::AssistantMessageFinishedEvent {
                request_id: "req_000004".into(),
                tool_call_count: 0,
                parts: vec![harness_core::session::AssistantPart::Text {
                    text: "Hello again".to_string(),
                }],
                provenance: None,
                assistant_message: None,
            },
        ),
    );
    second_assistant_finished.correlation_id = Some("req_000003".to_string());
    let mut second_completed = envelope_with_actor(
        "run_mock_continue",
        12,
        second_worker,
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000002".to_string().into(),
            result_summary: "Hello again".to_string(),
            result_digest: "digest-output-second".to_string(),
            metadata: None,
        }),
    );
    second_completed.correlation_id = Some("req_000003".to_string());
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_mock_continue",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_mock_continue",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_mock_continue",
                3,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000001".into(),
                    text: "hello".to_string(),
                }),
            ),
            provider_started,
            provider_finished,
            assistant_finished,
            completed,
            envelope(
                "run_mock_continue",
                8,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000003".into(),
                    text: "hello".to_string(),
                }),
            ),
            second_provider_started,
            second_provider_finished,
            second_assistant_finished,
            second_completed,
            envelope(
                "run_mock_continue",
                13,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    // When: the next prompt uses the persisted mock provider and restored semantic history.
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "prompt",
        "--mock",
        "--resume",
        "run_mock_continue",
        "--text",
        "hello",
        "--system-prompt-override",
        "default-prompt",
        "--tools",
        "edit",
    ]);

    // Then: continuation succeeds offline and appends another final commit without deltas.
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = std::fs::read_to_string(run_dir.join("events.jsonl")).unwrap_or_abort();
    let events = body
        .lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).unwrap_or_abort())
        .collect::<Vec<_>>();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::AssistantMessageFinished(_)))
            .count(),
        3
    );
    assert!(
        !events.iter().any(|event| matches!(
            event.payload,
            EventV1::ProviderStreamDelta(_) | EventV1::ProviderReasoningDelta(_)
        )),
        "offline continuation must not rebuild history from transport fragments"
    );

    let reopened = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "reopen",
        "--session",
        "run_mock_continue",
        "--json",
    ]);
    assert!(
        reopened.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&reopened.stderr)
    );
    let reopened: serde_json::Value =
        serde_json::from_slice(&reopened.stdout).unwrap_or_abort();
    assert_eq!(reopened["summary"]["resumable"], true);
    assert_eq!(
        reopened["summary"]["continue_hint"],
        "harness prompt --mock --resume run_mock_continue --text \"<next prompt>\""
    );
}

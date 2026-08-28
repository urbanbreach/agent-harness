use harness::UnwrapOrAbort;
use harness_core::event::{
    AssistantMessageFinishedEvent, SessionCompactionEvent, TaskTerminalScope,
};
use harness_core::session::{AssistantPart, AssistantToolCall};

fn mixed_canonical_events(run_id: &str) -> Vec<EventEnvelopeV1> {
    let lineage = TaskLineageMetadata {
        parent_tool_call_id: Some("toolcall_1".to_string()),
        parent_task_id: Some("task_1".to_string()),
        parent_request_id: Some("req_1".to_string()),
        parent_session_id: Some(run_id.to_string()),
        child_session_id: Some("child-run-1".to_string()),
        child_request_id: Some("child-req-1".to_string()),
        child_provider_id: Some("mock".to_string()),
        child_model_id: Some("model-1".to_string()),
    };
    let metadata = ToolCallMetadata {
        canonical_tool_id: Some("agent.spawn".to_string()),
        alias_source_tool_id: Some("task".to_string()),
        lineage: Some(lineage.clone()),
        artifact_refs: Vec::new(),
        timing: None,
        hook_executions: Vec::new(),
    };

    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "mixed canonical".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_1".to_string(),
                profile: "build".to_string(),
                parent_agent_id: None,
            }),
        ),
        agent_envelope(
            run_id,
            3,
            "agent_1",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_1".into(),
                text: "delegate safely".to_string(),
            }),
        ),
        agent_envelope(
            run_id,
            4,
            "agent_1",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_2".into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "delegate safely".to_string(),
                request_digest: "request-digest".to_string(),
                metadata: None,
            }),
        ),
        agent_envelope(
            run_id,
            5,
            "agent_1",
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: "req_2".into(),
                tool_call_count: 1,
                parts: vec![
                    AssistantPart::Text {
                        text: "canonical answer".to_string(),
                    },
                    AssistantPart::ToolCall(AssistantToolCall {
                        tool_call_id: "toolcall_1".into(),
                        provider_tool_call_id: None,
                        tool_id: "task".to_string(),
                        args_summary: "delegate".to_string(),
                        args_digest: "args-digest".to_string(),
                        provider_call_id: None,
                    }),
                ],
                provenance: None,
                assistant_message: None,
            }),
        ),
        envelope(
            run_id,
            6,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_1".into(),
                tool_id: "task".to_string(),
                args_summary: "delegate".to_string(),
                args_digest: "args-digest".to_string(),
                metadata: Some(metadata.clone()),
            }),
        ),
        envelope(
            run_id,
            7,
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_1".to_string(),
                kind: "task".to_string(),
                tool_call_id: Some("toolcall_1".into()),
                summary: "spawn child".to_string(),
                request_digest: "permission-digest".to_string(),
                timeout_ms: 30_000,
                default_decision: PermissionDecision::Deny,
            }),
        ),
        envelope(
            run_id,
            8,
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: "perm_1".to_string(),
                decision: PermissionDecision::Allow,
                reason: Some("approved".to_string()),
            }),
        ),
        envelope(
            run_id,
            9,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_1".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("child complete".to_string()),
                output_digest: Some("output-digest".to_string()),
                output_json: Some(serde_json::json!({
                    "child_session_id": "child-run-1",
                    "child_request_id": "child-req-1",
                    "route": {"profile_id": "general", "status": "completed"}
                })),
                metadata: Some(metadata),
            }),
        ),
        envelope(
            run_id,
            10,
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_1".into(),
                result_summary: "child complete".to_string(),
                result_digest: "result-digest".to_string(),
                metadata: Some(TaskCompletionMetadata {
                    lineage: Some(lineage),
                    task_scope: Some(TaskTerminalScope::ToolCall),
                    timing: None,
                    hook_executions: Vec::new(),
                }),
            }),
        ),
        envelope(
            run_id,
            11,
            EventV1::SessionCompaction(SessionCompactionEvent {
                agent_id: "agent_1".to_string(),
                summary: "durable compacted context".to_string(),
                first_kept_event_seq: 3,
                first_kept_request_id: Some("req_1".to_string()),
                first_kept_entry_id: None,
                tokens_before: 2_000,
                tokens_after: Some(800),
                summary_usage: None,
                summary_provider_id: Some("mock".to_string()),
                summary_model_id: Some("model-1".to_string()),
                read_files: Vec::new(),
                modified_files: Vec::new(),
                current_intent: None,
                trigger_reason: "threshold".to_string(),
                from_hook: false,
            }),
        ),
        envelope(
            run_id,
            12,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}

#[test]
fn mixed_canonical_journal_keeps_identity_order_and_status_across_cli_views() {
    // arrange
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_mixed_canonical");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(&run_dir, &mixed_canonical_events("run_mixed_canonical"));
    let export_path = session_dir.path().join("mixed-export.json");

    // act
    let replay_output = run_harness([
        "replay",
        "--session",
        run_dir.to_str().unwrap_or_abort(),
        "--json",
    ]);
    let reopen_output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "reopen",
        "--session",
        "run_mixed_canonical",
        "--json",
    ]);
    let export_output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "export",
        "run_mixed_canonical",
        "--output",
        export_path.to_str().unwrap_or_abort(),
    ]);

    // assert
    assert!(
        replay_output.status.success(),
        "replay stderr:\n{}",
        String::from_utf8_lossy(&replay_output.stderr)
    );
    assert!(
        reopen_output.status.success(),
        "reopen stderr:\n{}",
        String::from_utf8_lossy(&reopen_output.stderr)
    );
    assert!(
        export_output.status.success(),
        "export stderr:\n{}",
        String::from_utf8_lossy(&export_output.stderr)
    );

    let replay: serde_json::Value =
        serde_json::from_slice(&replay_output.stdout).unwrap_or_abort();
    let reopen: serde_json::Value =
        serde_json::from_slice(&reopen_output.stdout).unwrap_or_abort();
    let export: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&export_path).unwrap_or_abort(),
    )
    .unwrap_or_abort();
    for view in [&replay, &export["replay"]] {
        assert_eq!(view["run_id"], "run_mixed_canonical");
        assert_eq!(view["status"], "finished");
        assert_eq!(view["child_sessions"][0]["child_session_id"], "child-run-1");
    }
    assert_eq!(reopen["summary"]["run_id"], "run_mixed_canonical");
    assert_eq!(reopen["summary"]["status"], "finished");
    assert!(reopen["summary"]["child_sessions"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .any(|child| child["agent_id"] == "child-run-1"));
    assert_eq!(export["catalog"]["run_id"], "run_mixed_canonical");
    assert_eq!(export["catalog"]["status"], "finished");

    let routes = export["support"]["route_metadata"]
        .as_array()
        .unwrap_or_abort();
    let session_route = routes
        .iter()
        .find(|route| route["source"] == "session_replay")
        .unwrap_or_abort();
    assert_eq!(session_route["route"]["profiles"], serde_json::json!(["build"]));
    assert_eq!(
        session_route["route"]["provider_models"],
        serde_json::json!(["mock/model-1"])
    );
    assert_eq!(session_route["route"]["tools"][0]["seq"], 6);
    assert_eq!(session_route["route"]["permissions"][0]["seq"], 7);
    assert_eq!(session_route["route"]["permissions"][1]["seq"], 8);
    assert_eq!(routes[1]["source"], "task_output");
    assert_eq!(routes[1]["child_session_id"], "child-run-1");
}

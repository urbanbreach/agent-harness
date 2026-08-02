use harness_core::UnwrapOrAbort;
#[test]
fn projects_task_lineage_and_child_session_metadata() {
    // arrange
    // act
    // assert
    let lineage = TaskLineageMetadata {
        parent_tool_call_id: Some("toolcall_000777".to_string()),
        parent_task_id: Some("task_000777".to_string()),
        parent_request_id: Some("req_000001".to_string()),
        parent_session_id: Some("agent_000001".to_string()),
        child_session_id: Some("agent_000777".to_string()),
        child_request_id: Some("req_000777".to_string()),
        child_provider_id: Some("default".to_string()),
        child_model_id: Some("gpt-5".to_string()),
    };
    let metadata = Some(ToolCallMetadata {
        canonical_tool_id: Some("agent.spawn".to_string()),
        alias_source_tool_id: Some("task".to_string()),
        lineage: Some(lineage.clone()),
        artifact_refs: Vec::new(),
        timing: None,
        hook_executions: Vec::new(),
    });
    let events = vec![
        envelope(
            1,
            supervisor(),
            None,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "default".to_string(),
                parent_agent_id: None,
            }),
        ),
        tool_requested(
            2,
            "req_000001",
            "toolcall_000777",
            "task",
            "delegate",
            metadata,
        ),
        envelope(
            3,
            system(),
            Some("req_000001"),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000777".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:default:gpt-5".to_string()),
            }),
        ),
        envelope(
            4,
            worker(),
            Some("req_000001"),
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000777".to_string().into(),
                result_summary: "child completed".to_string(),
                result_digest: "digest-task".to_string(),
                metadata: Some(TaskCompletionMetadata {
                    lineage: Some(lineage),
                    task_scope: None,
                    timing: None,
                    hook_executions: Vec::new(),
                }),
            }),
        ),
        envelope(
            5,
            supervisor(),
            None,
            EventV1::AgentStopped(AgentStoppedEvent {
                agent_id: "agent_000001".to_string(),
                reason: "done".to_string(),
            }),
        ),
    ];

    let projection = project_transcript(&events).unwrap_or_abort();

    assert_eq!(
        projection
            .session
            .agent_profiles
            .get("agent_000001")
            .map(String::as_str),
        Some("default")
    );
    assert_eq!(projection.session_lineage.len(), 1);
    assert_eq!(
        projection.session_lineage[0].child_session_id.as_deref(),
        Some("agent_000777")
    );
    assert_eq!(
        projection.session_lineage[0].parent_tool_call_id.as_deref(),
        Some("toolcall_000777")
    );

    let assistant = assistant_message(&projection, "req_000001");
    let ProjectedPart::ToolCall(tool) = &assistant.parts[0] else {
        panic!("expected tool part")
    };
    assert_eq!(
        tool.lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some("agent_000777")
    );

    let completed_task = projection
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .find_map(|part| match part {
            ProjectedPart::Task(task) if task.state == ProjectedTaskState::Completed => Some(task),
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(
        completed_task.result_summary.as_deref(),
        Some("child completed")
    );
    assert_eq!(
        completed_task
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some("req_000777")
    );
}
#[test]
fn tolerates_old_minimal_metadata_and_projects_incomplete_or_failed_states() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            1,
            worker(),
            Some("req_legacy"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_legacy".into(),
                delta: "legacy text".to_string(),
            }),
        ),
        envelope(
            2,
            worker(),
            Some("req_legacy"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_legacy".into(),
                finish_reason: "error".to_string(),
                output_digest: None,
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            3,
            system(),
            None,
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: "perm_legacy".to_string(),
                decision: PermissionDecision::Deny,
                reason: Some("default".to_string()),
            }),
        ),
        envelope(
            4,
            system(),
            None,
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_000001".to_string(),
                kind: "bash".to_string(),
                tool_call_id: None,
                summary: "run command".to_string(),
                request_digest: "digest-permission".to_string(),
                timeout_ms: 1000,
                default_decision: PermissionDecision::Deny,
            }),
        ),
        envelope(
            5,
            system(),
            None,
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: "perm_000001".to_string(),
                decision: PermissionDecision::Allow,
                reason: Some("approved".to_string()),
            }),
        ),
        envelope(
            6,
            system(),
            None,
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "toolcall_legacy".into(),
            }),
        ),
        envelope(
            7,
            system(),
            None,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_legacy".into(),
                status: ToolCallStatus::Failed,
                output_summary: Some("failed".to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            8,
            system(),
            None,
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: "task_legacy".to_string().into(),
                reason: "cancelled".to_string(),
                task_scope: None,
            }),
        ),
        envelope(
            9,
            system(),
            None,
            EventV1::TaskResultLate(TaskResultLateEvent {
                task_id: "task_legacy".to_string().into(),
                result_digest: "digest-late".to_string(),
            }),
        ),
        envelope(
            10,
            system(),
            None,
            EventV1::PolicyViolationDetected(PolicyViolationDetectedEvent {
                policy: "worker_spawn".to_string(),
                detail: "workers cannot spawn directly".to_string(),
            }),
        ),
        envelope(
            11,
            user(),
            None,
            EventV1::UiIntentReceived(UiIntentReceivedEvent {
                intent: "resume_picker".to_string(),
                params: BTreeMap::from([("filter".to_string(), "recent".to_string())]),
            }),
        ),
        envelope(
            12,
            supervisor(),
            None,
            EventV1::RunFailed(RunFailedEvent {
                error: "fatal".to_string(),
            }),
        ),
    ];

    let projection = project_transcript(&events).unwrap_or_abort();

    assert_eq!(projection.session.status, TranscriptRunStatus::Failed);
    let assistant = assistant_message(&projection, "req_legacy");
    assert_eq!(
        assistant.state,
        harness_core::transcript_projection::ProjectedMessageState::Failed
    );

    let resolved_permission = projection
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .find_map(|part| match part {
            ProjectedPart::Permission(permission) if permission.permission_id == "perm_000001" => {
                Some(permission)
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(
        resolved_permission.state,
        ProjectedPermissionState::Resolved
    );
    assert_eq!(
        resolved_permission.decision,
        Some(PermissionDecision::Allow)
    );

    let legacy_tool = projection
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .find_map(|part| match part {
            ProjectedPart::ToolCall(tool) if tool.tool_call_id.as_str() == "toolcall_legacy" => {
                Some(tool.as_ref())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(legacy_tool.state, ProjectedToolCallState::Failed);
    assert_eq!(legacy_tool.started_seq, Some(6));
    assert_eq!(legacy_tool.finished_seq, Some(7));

    assert!(projection
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .any(|part| matches!(part, ProjectedPart::PolicyViolation(_))));
    assert!(projection
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .any(|part| matches!(part, ProjectedPart::UiIntent(_))));
}
#[test]
fn rejects_out_of_order_seq_without_panic() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            2,
            supervisor(),
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        envelope(
            1,
            supervisor(),
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];

    let err = project_transcript(&events).expect_err("out-of-order seq should fail");

    assert!(matches!(
        err,
        TranscriptProjectionError::EventsOutOfOrder {
            previous_seq: 2,
            seq: 1
        }
    ));
}

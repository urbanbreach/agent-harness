use std::collections::BTreeMap;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, AgentStoppedEvent, ArtifactWrittenEvent,
    AssistantMessageFinishedEvent, CompactionAppliedEvent, CompactionFailedEvent,
    CompactionRequestedEvent, CompactionWrittenEvent, EventActor, EventArtifactRef,
    EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    PermissionResolvedEvent, PolicyViolationDetectedEvent, ProviderReasoningDeltaEvent,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFailedEvent, RunFinishedEvent, RunStartedEvent, TaskCancelledEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, TaskResultLateEvent, TaskScheduleState,
    TaskScheduledEvent, ToolCallFinishedEvent, ToolCallMetadata, ToolCallRequestedEvent,
    ToolCallStartedEvent, ToolCallStatus, UiIntentReceivedEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_core::transcript_projection::{
    project_transcript, ArtifactProjectionSource, CompactionCheckpointStatus, ProjectedMessageRole,
    ProjectedPart, ProjectedPermissionState, ProjectedTaskState, ProjectedToolCallState,
    TranscriptProjectionError, TranscriptRunStatus,
};

#[test]
fn projects_user_assistant_text_and_reasoning_parts() {
    let events = vec![
        envelope(
            1,
            supervisor(),
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        envelope(
            2,
            user(),
            None,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_000001".to_string(),
                text: "Explain the plan.".to_string(),
            }),
        ),
        envelope(
            3,
            worker(),
            Some("req_000001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider_req_1".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5".to_string(),
                prompt_summary: "Explain the plan.".to_string(),
                request_digest: "digest-request".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            worker(),
            Some("req_000001"),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: "provider_req_1".to_string(),
                delta: "think ".to_string(),
            }),
        ),
        envelope(
            5,
            worker(),
            Some("req_000001"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "provider_req_1".to_string(),
                delta: "The ".to_string(),
            }),
        ),
        envelope(
            6,
            worker(),
            Some("req_000001"),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: "provider_req_1".to_string(),
                delta: "first".to_string(),
            }),
        ),
        envelope(
            7,
            worker(),
            Some("req_000001"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "provider_req_1".to_string(),
                delta: "plan.".to_string(),
            }),
        ),
        envelope(
            8,
            worker(),
            Some("req_000001"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "provider_req_1".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            9,
            worker(),
            Some("req_000001"),
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: "provider_req_1".to_string(),
                tool_call_count: 0,
                assistant_message: None,
            }),
        ),
        envelope(
            10,
            supervisor(),
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];

    let projection = project_transcript(&events).expect("project transcript");

    assert_eq!(projection.session.status, TranscriptRunStatus::Finished);
    assert_eq!(projection.session.run_name.as_deref(), Some("interactive"));
    serde_json::to_value(&projection).expect("projection is serializable");

    let user = projection
        .messages
        .iter()
        .find(|message| message.role == ProjectedMessageRole::User)
        .expect("user message");
    assert_eq!(user.request_id.as_deref(), Some("req_000001"));
    let ProjectedPart::Text(user_text) = &user.parts[0] else {
        panic!("expected user text part")
    };
    assert_eq!(user_text.text, "Explain the plan.");

    let assistant = assistant_message(&projection, "req_000001");
    let provider = assistant.provider.as_ref().expect("provider metadata");
    assert_eq!(
        provider.provider_request_id.as_deref(),
        Some("provider_req_1")
    );
    assert_eq!(provider.provider_id.as_deref(), Some("default"));
    assert_eq!(provider.model_id.as_deref(), Some("gpt-5"));
    assert_eq!(provider.finish_reason.as_deref(), Some("stop"));
    assert_eq!(provider.output_digest.as_deref(), Some("digest-output"));
    assert_eq!(assistant.parts.len(), 2);
    let ProjectedPart::Reasoning(reasoning) = &assistant.parts[0] else {
        panic!("expected reasoning part first")
    };
    assert_eq!(reasoning.text, "think first");
    assert_eq!(reasoning.provenance.first_seq, 4);
    assert_eq!(reasoning.provenance.last_seq, 6);
    let ProjectedPart::Text(text) = &assistant.parts[1] else {
        panic!("expected assistant text part second")
    };
    assert_eq!(text.text, "The plan.");
    assert_eq!(text.provenance.first_seq, 5);
    assert_eq!(text.provenance.last_seq, 7);
}

#[test]
fn keeps_tool_results_on_source_ordered_tool_parts_when_finishes_arrive_out_of_order() {
    let events = vec![
        envelope(
            1,
            worker(),
            Some("req_000001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5".to_string(),
                prompt_summary: "use tools".to_string(),
                request_digest: "digest-request".to_string(),
                metadata: None,
            }),
        ),
        tool_requested(
            2,
            "req_000001",
            "toolcall_000001",
            "read",
            "README.md",
            None,
        ),
        tool_requested(
            3,
            "req_000001",
            "toolcall_000002",
            "bash",
            "git status",
            None,
        ),
        envelope(
            4,
            system(),
            Some("req_000001"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_000002".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("clean".to_string()),
                output_digest: Some("digest-bash".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            5,
            system(),
            Some("req_000001"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_000001".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("readme contents".to_string()),
                output_digest: Some("digest-read".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
    ];

    let projection = project_transcript(&events).expect("project transcript");
    let assistant = assistant_message(&projection, "req_000001");
    let tool_parts = assistant
        .parts
        .iter()
        .filter_map(|part| match part {
            ProjectedPart::ToolCall(tool) => Some(tool.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(tool_parts.len(), 2);
    assert_eq!(tool_parts[0].tool_call_id, "toolcall_000001");
    assert_eq!(tool_parts[0].tool_id, "read");
    assert_eq!(tool_parts[0].state, ProjectedToolCallState::Succeeded);
    assert_eq!(
        tool_parts[0].output_summary.as_deref(),
        Some("readme contents")
    );
    assert_eq!(tool_parts[0].finished_seq, Some(5));
    assert_eq!(tool_parts[1].tool_call_id, "toolcall_000002");
    assert_eq!(tool_parts[1].tool_id, "bash");
    assert_eq!(tool_parts[1].state, ProjectedToolCallState::Succeeded);
    assert_eq!(tool_parts[1].output_summary.as_deref(), Some("clean"));
    assert_eq!(tool_parts[1].finished_seq, Some(4));
}

#[test]
fn projects_compaction_checkpoint_requested_written_applied_and_failed_state() {
    let events = vec![
        envelope(
            1,
            system(),
            None,
            EventV1::CompactionRequested(CompactionRequestedEvent {
                checkpoint_id: "checkpoint_000001".to_string(),
                agent_id: "agent_000001".to_string(),
                trigger_reason: "manual".to_string(),
                through_seq: 10,
                through_request_id: Some("req_000001".to_string()),
                provider_id: Some("default".to_string()),
                model_id: Some("gpt-5".to_string()),
                tokens_before: Some(1000),
                tokens_before_estimate: Some(980),
                estimate_source: Some("provider_usage".to_string()),
            }),
        ),
        envelope(
            2,
            system(),
            None,
            EventV1::CompactionWritten(CompactionWrittenEvent {
                checkpoint_id: "checkpoint_000001".to_string(),
                agent_id: "agent_000001".to_string(),
                artifact_path: "artifacts/compactions/agent_000001/checkpoint_000001.json"
                    .to_string(),
                artifact_digest: Some("digest-checkpoint".to_string()),
                artifact_bytes: 123,
                trigger_reason: "manual".to_string(),
                through_seq: 10,
                through_request_id: Some("req_000001".to_string()),
                provider_id: Some("default".to_string()),
                model_id: Some("gpt-5".to_string()),
                tokens_before: Some(1000),
                tokens_before_estimate: Some(980),
                tokens_after_estimate: Some(400),
                summary_tokens_estimate: Some(80),
                compacted_turns: Some(3),
                reduction_tokens_estimate: Some(580),
                reduction_percent_estimate: Some(59),
                estimate_source: Some("provider_usage".to_string()),
                preserved_turns: 1,
            }),
        ),
        envelope(
            3,
            system(),
            None,
            EventV1::CompactionApplied(CompactionAppliedEvent {
                checkpoint_id: "checkpoint_000001".to_string(),
                agent_id: "agent_000001".to_string(),
                through_seq: 10,
                through_request_id: Some("req_000001".to_string()),
                tokens_before_estimate: Some(980),
                tokens_after_estimate: Some(400),
                summary_tokens_estimate: Some(80),
                compacted_turns: Some(3),
                preserved_turns: Some(1),
                reduction_tokens_estimate: Some(580),
                reduction_percent_estimate: Some(59),
                estimate_source: Some("provider_usage".to_string()),
            }),
        ),
        envelope(
            4,
            system(),
            None,
            EventV1::CompactionFailed(CompactionFailedEvent {
                agent_id: "agent_000001".to_string(),
                trigger_reason: "overflow_retry".to_string(),
                reason: "checkpoint did not reduce context".to_string(),
                checkpoint_id: Some("checkpoint_000002".to_string()),
                through_seq: Some(20),
                through_request_id: Some("req_000002".to_string()),
            }),
        ),
    ];

    let projection = project_transcript(&events).expect("project transcript");

    assert_eq!(projection.compaction_checkpoints.len(), 2);
    let applied = projection
        .compaction_checkpoints
        .iter()
        .find(|checkpoint| checkpoint.checkpoint_id.as_deref() == Some("checkpoint_000001"))
        .expect("applied checkpoint");
    assert_eq!(applied.status, CompactionCheckpointStatus::Applied);
    assert_eq!(applied.trigger_reason.as_deref(), Some("manual"));
    assert_eq!(
        applied.artifact.as_ref().map(|artifact| artifact.bytes),
        Some(Some(123))
    );
    assert_eq!(applied.provenance.first_seq, 1);
    assert_eq!(applied.provenance.last_seq, 3);

    let failed = projection
        .compaction_checkpoints
        .iter()
        .find(|checkpoint| checkpoint.checkpoint_id.as_deref() == Some("checkpoint_000002"))
        .expect("failed checkpoint");
    assert_eq!(failed.status, CompactionCheckpointStatus::Failed);
    assert_eq!(
        failed.reason.as_deref(),
        Some("checkpoint did not reduce context")
    );

    let compaction_parts = projection
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter(|part| matches!(part, ProjectedPart::Compaction(_)))
        .count();
    assert_eq!(compaction_parts, 4);
}

#[test]
fn projects_artifact_metadata_without_reading_artifact_contents() {
    let metadata = Some(ToolCallMetadata {
        canonical_tool_id: Some("read".to_string()),
        alias_source_tool_id: None,
        lineage: None,
        artifact_refs: vec![EventArtifactRef {
            path: "artifacts/toolcalls/toolcall_000001/request.json".to_string(),
            digest: Some("digest-request-artifact".to_string()),
        }],
        timing: None,
        hook_executions: Vec::new(),
    });
    let finish_metadata = Some(ToolCallMetadata {
        canonical_tool_id: Some("read".to_string()),
        alias_source_tool_id: None,
        lineage: None,
        artifact_refs: vec![EventArtifactRef {
            path: "artifacts/toolcalls/toolcall_000001/result.json".to_string(),
            digest: Some("digest-result-artifact".to_string()),
        }],
        timing: None,
        hook_executions: Vec::new(),
    });
    let events = vec![
        tool_requested(
            1,
            "req_000001",
            "toolcall_000001",
            "read",
            "README.md",
            metadata,
        ),
        envelope(
            2,
            system(),
            Some("req_000001"),
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_000001".to_string(),
                kind: "edit".to_string(),
                tool_call_id: Some("toolcall_000001".to_string()),
                summary: "read file".to_string(),
                request_digest: "digest-permission".to_string(),
                timeout_ms: 1000,
                default_decision: PermissionDecision::Deny,
            }),
        ),
        envelope(
            3,
            system(),
            Some("req_000001"),
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: "perm_000001".to_string(),
                decision: PermissionDecision::Allow,
                reason: Some("approved".to_string()),
            }),
        ),
        envelope(
            4,
            system(),
            Some("req_000001"),
            EventV1::ArtifactWritten(ArtifactWrittenEvent {
                path: "artifacts/toolcalls/toolcall_000001/stdout.txt".to_string(),
                digest: "digest-stdout".to_string(),
                bytes: 42,
                tool_call_id: Some("toolcall_000001".to_string()),
                tool_metadata: None,
                metadata: BTreeMap::from([
                    ("artifact_kind".to_string(), "tool_output".to_string()),
                    ("path".to_string(), "src/lib.rs".to_string()),
                ]),
            }),
        ),
        envelope(
            5,
            system(),
            Some("req_000001"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_000001".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("summary only".to_string()),
                output_digest: Some("digest-output".to_string()),
                output_json: None,
                metadata: finish_metadata,
            }),
        ),
    ];

    let projection = project_transcript(&events).expect("project transcript");

    assert_eq!(projection.artifacts.len(), 3);
    assert!(projection.artifacts.iter().any(|artifact| {
        artifact.path.ends_with("request.json")
            && artifact.source == ArtifactProjectionSource::ToolCallMetadata
            && artifact.bytes.is_none()
    }));
    assert!(projection.artifacts.iter().any(|artifact| {
        artifact.path.ends_with("stdout.txt")
            && artifact.source == ArtifactProjectionSource::ArtifactWritten
            && artifact.bytes == Some(42)
            && artifact.metadata.get("artifact_kind").map(String::as_str) == Some("tool_output")
            && artifact.metadata.get("path").map(String::as_str) == Some("src/lib.rs")
    }));

    let assistant = assistant_message(&projection, "req_000001");
    let ProjectedPart::ToolCall(tool) = &assistant.parts[0] else {
        panic!("expected tool part")
    };
    assert_eq!(tool.artifacts.len(), 3);
    assert_eq!(tool.permissions.len(), 1);
    assert_eq!(
        tool.permissions[0].state,
        ProjectedPermissionState::Resolved
    );
    assert_eq!(
        tool.permissions[0].decision,
        Some(PermissionDecision::Allow)
    );
    assert_eq!(tool.output_summary.as_deref(), Some("summary only"));
}

#[test]
fn projects_task_lineage_and_child_session_metadata() {
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
                task_id: "task_000777".to_string(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:default:gpt-5".to_string()),
            }),
        ),
        envelope(
            4,
            worker(),
            Some("req_000001"),
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000777".to_string(),
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

    let projection = project_transcript(&events).expect("project transcript");

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
        .expect("completed task part");
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
    let events = vec![
        envelope(
            1,
            worker(),
            Some("req_legacy"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_legacy".to_string(),
                delta: "legacy text".to_string(),
            }),
        ),
        envelope(
            2,
            worker(),
            Some("req_legacy"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_legacy".to_string(),
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
                tool_call_id: "toolcall_legacy".to_string(),
            }),
        ),
        envelope(
            7,
            system(),
            None,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_legacy".to_string(),
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
                task_id: "task_legacy".to_string(),
                reason: "cancelled".to_string(),
                task_scope: None,
            }),
        ),
        envelope(
            9,
            system(),
            None,
            EventV1::TaskResultLate(TaskResultLateEvent {
                task_id: "task_legacy".to_string(),
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

    let projection = project_transcript(&events).expect("project transcript");

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
        .expect("permission part");
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
            ProjectedPart::ToolCall(tool) if tool.tool_call_id == "toolcall_legacy" => {
                Some(tool.as_ref())
            }
            _ => None,
        })
        .expect("legacy tool part");
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
    let events = vec![
        envelope(
            2,
            supervisor(),
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
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

fn assistant_message<'a>(
    projection: &'a harness_core::transcript_projection::TranscriptProjection,
    request_id: &str,
) -> &'a harness_core::transcript_projection::ProjectedMessage {
    projection
        .messages
        .iter()
        .find(|message| {
            message.role == ProjectedMessageRole::Assistant
                && message.request_id.as_deref() == Some(request_id)
        })
        .expect("assistant message")
}

fn tool_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    tool_id: &str,
    args_summary: &str,
    metadata: Option<ToolCallMetadata>,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        worker(),
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata,
        }),
    )
}

fn supervisor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("coordinator".to_string()))
}

fn system() -> EventActor {
    EventActor::new(ActorKind::System, Some("coordinator".to_string()))
}

fn worker() -> EventActor {
    EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()))
}

fn user() -> EventActor {
    EventActor::new(ActorKind::User, None)
}

fn envelope(
    seq: u64,
    actor: EventActor,
    correlation_id: Option<&str>,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:020}"),
        seq,
        run_id: "run_transcript_projection".to_string(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

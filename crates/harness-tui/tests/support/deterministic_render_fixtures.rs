use harness_core::event::{
    ActorKind, EditAppliedEvent, EditProposedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    TaskLineageMetadata, ToolCallFinishedEvent, ToolCallMetadata, ToolCallRequestedEvent,
    ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};

pub(crate) fn tool_lifecycle_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_tool_lifecycle";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "Inspect tool activity".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Inspect tool activity".to_string(),
                request_digest: "digest-tool-lifecycle-request".to_string(),
                metadata: None,
            }),
        ),
        lifecycle_tool_requested(
            3,
            request_id,
            "tc_read",
            "read",
            r#"{"path":"src/ui.rs","start_line":1,"limit":24}"#,
            Some(ToolCallMetadata {
                canonical_tool_id: Some("fs.read".to_string()),
                alias_source_tool_id: Some("read".to_string()),
                ..ToolCallMetadata::default()
            }),
        ),
        lifecycle_tool_started(4, request_id, "tc_read"),
        lifecycle_tool_finished(
            5,
            request_id,
            "tc_read",
            ToolCallStatus::Succeeded,
            Some("24 lines read from src/ui.rs"),
            None,
            Some(ToolCallMetadata {
                canonical_tool_id: Some("fs.read".to_string()),
                alias_source_tool_id: Some("read".to_string()),
                ..ToolCallMetadata::default()
            }),
        ),
        lifecycle_tool_requested(
            6,
            request_id,
            "tc_edit",
            "edit.hashline_apply",
            r#"{"path":"crates/harness-tui/src/ui.rs"}"#,
            None,
        ),
        lifecycle_tool_started(7, request_id, "tc_edit"),
        envelope(
            8,
            Some("tc_edit"),
            EventV1::EditProposed(EditProposedEvent {
                edit_id: "edit_tool_lifecycle".to_string(),
                path: "crates/harness-tui/src/ui.rs".to_string(),
                summary: "Remove diff review surface".to_string(),
                patch_digest: "digest-tool-lifecycle-edit-patch".to_string(),
            }),
        ),
        envelope(
            9,
            Some("tc_edit"),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_tool_lifecycle".to_string(),
                path: "crates/harness-tui/src/ui.rs".to_string(),
                new_file_digest: "digest-tool-lifecycle-edit-file".to_string(),
                diff_rel_path: Some("artifacts/tool-lifecycle-inline.diff".to_string()),
                diff_digest: Some("digest-tool-lifecycle-edit-diff".to_string()),
            }),
        ),
        lifecycle_tool_finished(
            10,
            request_id,
            "tc_edit",
            ToolCallStatus::Succeeded,
            Some("Patched crates/harness-tui/src/ui.rs"),
            None,
            None,
        ),
        lifecycle_tool_requested(
            11,
            request_id,
            "tc_task",
            "task",
            r#"{"description":"audit tool lifecycle parity","subagent_type":"researcher"}"#,
            Some(ToolCallMetadata {
                canonical_tool_id: Some("agent.spawn".to_string()),
                alias_source_tool_id: Some("task".to_string()),
                lineage: Some(tool_lifecycle_lineage(request_id)),
                ..ToolCallMetadata::default()
            }),
        ),
        lifecycle_tool_started(12, request_id, "tc_task"),
        lifecycle_tool_finished(
            13,
            request_id,
            "tc_task",
            ToolCallStatus::Succeeded,
            Some("Found the whole-tool parity path."),
            Some(serde_json::json!({
                "description": "audit tool lifecycle parity",
                "profile": "researcher",
                "mode": "foreground",
                "status": "completed",
                "result_summary": "Found the whole-tool parity path.",
                "child_tool_call_count": 2,
                "child_session_id": "agent_worker",
                "child_request_id": "req_child"
            })),
            Some(ToolCallMetadata {
                canonical_tool_id: Some("agent.spawn".to_string()),
                alias_source_tool_id: Some("task".to_string()),
                lineage: Some(tool_lifecycle_lineage(request_id)),
                ..ToolCallMetadata::default()
            }),
        ),
        lifecycle_tool_requested(
            14,
            request_id,
            "tc_shell",
            "shell.run",
            r#"{"cmd":"cargo test -p harness-tui","cwd":"/workspace"}"#,
            None,
        ),
        lifecycle_tool_started(15, request_id, "tc_shell"),
        lifecycle_tool_finished(
            16,
            request_id,
            "tc_shell",
            ToolCallStatus::Failed,
            Some("exit code: 1\nstderr: snapshot mismatch"),
            None,
            None,
        ),
        envelope(
            17,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "Tool summaries are now easier to scan, and edits stay inline.".to_string(),
            }),
        ),
        envelope(
            18,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-tool-lifecycle-response".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn lifecycle_tool_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    tool_id: &str,
    args_summary: &str,
    metadata: Option<ToolCallMetadata>,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: format!("digest-{tool_call_id}-args"),
            metadata,
        }),
    )
}

fn lifecycle_tool_started(seq: u64, request_id: &str, tool_call_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: tool_call_id.into(),
        }),
    )
}

fn lifecycle_tool_finished(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    status: ToolCallStatus,
    output_summary: Option<&str>,
    output_json: Option<serde_json::Value>,
    metadata: Option<ToolCallMetadata>,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.into(),
            status,
            output_summary: output_summary.map(str::to_string),
            output_digest: Some(format!("digest-{tool_call_id}-output")),
            output_json,
            metadata,
        }),
    )
}

fn tool_lifecycle_lineage(request_id: &str) -> TaskLineageMetadata {
    TaskLineageMetadata {
        parent_tool_call_id: Some("tc_task".to_string()),
        parent_request_id: Some(request_id.to_string()),
        child_session_id: Some("agent_worker".to_string()),
        child_request_id: Some("req_child".to_string()),
        ..TaskLineageMetadata::default()
    }
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-tool-lifecycle-{seq:04}"),
        seq,
        run_id: "run_fixture".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("deterministic-render".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

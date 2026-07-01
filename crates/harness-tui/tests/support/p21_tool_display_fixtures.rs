use harness_core::event::{
    ActorKind, EventActor, EventArtifactRef, EventEnvelopeV1, EventV1, PermissionDecision,
    PermissionRequestedEvent, PermissionResolvedEvent, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, ToolCallFinishedEvent, ToolCallMetadata,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};

/// Build events that exercise the P2.1 tool display descriptor families:
/// running, completed, failed, denied, truncated, cancelled, late-result,
/// plus Harness-only tool families (background_cancel, plan_enter, session_list,
/// ast_grep_search, lsp, skill).
pub(crate) fn p21_tool_display_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_p21_display";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "Inspect P2.1 tool display descriptors".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Inspect P2.1 tool display descriptors".to_string(),
                request_digest: "digest-p21-request".to_string(),
                metadata: None,
            }),
        ),
        // S1: completed (succeeded) — session_list Harness-only tool
        p21_tool_requested(
            3,
            request_id,
            "tc_session_list",
            "session_list",
            r#"{"limit":5}"#,
            None,
        ),
        p21_tool_started(4, request_id, "tc_session_list"),
        p21_tool_finished(
            5,
            request_id,
            "tc_session_list",
            ToolCallStatus::Succeeded,
            Some("3 sessions found"),
            None,
            None,
        ),
        // S2: running — lsp tool (still running, no finish event)
        p21_tool_requested(
            6,
            request_id,
            "tc_lsp_running",
            "lsp",
            r#"{"operation":"diagnostics","path":"src/main.rs"}"#,
            None,
        ),
        p21_tool_started(7, request_id, "tc_lsp_running"),
        // S3: failed — ast_grep_search
        p21_tool_requested(
            8,
            request_id,
            "tc_ast_grep_failed",
            "ast_grep_search",
            r#"{"pattern":"foo"}"#,
            None,
        ),
        p21_tool_started(9, request_id, "tc_ast_grep_failed"),
        p21_tool_finished(
            10,
            request_id,
            "tc_ast_grep_failed",
            ToolCallStatus::Failed,
            Some("ast-grep binary not found"),
            None,
            None,
        ),
        // S4: denied — skill tool with permission deny
        p21_tool_requested(
            11,
            request_id,
            "tc_skill_denied",
            "skill",
            r#"{"name":"denied-skill"}"#,
            None,
        ),
        envelope(
            12,
            Some("tc_skill_denied"),
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_skill_denied".to_string(),
                kind: "skill".to_string(),
                tool_call_id: Some("tc_skill_denied".to_string()),
                summary: "Load skill denied-skill".to_string(),
                request_digest: "digest-skill-denied".to_string(),
                timeout_ms: 30_000,
                default_decision: PermissionDecision::Deny,
            }),
        ),
        envelope(
            13,
            Some("tc_skill_denied"),
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: "perm_skill_denied".to_string(),
                decision: PermissionDecision::Deny,
                reason: Some("Operator denied skill load".to_string()),
            }),
        ),
        p21_tool_finished(
            14,
            request_id,
            "tc_skill_denied",
            ToolCallStatus::Failed,
            Some("Operator denied skill load"),
            None,
            None,
        ),
        // S5: truncated — session_read with artifact ref and truncated output
        p21_tool_requested(
            15,
            request_id,
            "tc_session_read_truncated",
            "session_read",
            r#"{"session_id":"run-123"}"#,
            Some(ToolCallMetadata {
                artifact_refs: vec![EventArtifactRef {
                    path: "artifacts/session-read-full-output.txt".to_string(),
                    digest: Some("digest-truncated-output".to_string()),
                }],
                ..Default::default()
            }),
        ),
        p21_tool_started(16, request_id, "tc_session_read_truncated"),
        p21_tool_finished(
            17,
            request_id,
            "tc_session_read_truncated",
            ToolCallStatus::Succeeded,
            Some("session read output truncated"),
            None,
            Some(ToolCallMetadata {
                artifact_refs: vec![EventArtifactRef {
                    path: "artifacts/session-read-full-output.txt".to_string(),
                    digest: Some("digest-truncated-output".to_string()),
                }],
                ..Default::default()
            }),
        ),
        // S6: cancelled — background_cancel succeeded
        p21_tool_requested(
            18,
            request_id,
            "tc_bg_cancel",
            "background_cancel",
            r#"{"request_id":"req-bg-child"}"#,
            None,
        ),
        p21_tool_started(19, request_id, "tc_bg_cancel"),
        p21_tool_finished(
            20,
            request_id,
            "tc_bg_cancel",
            ToolCallStatus::Succeeded,
            Some("Cancelled background task req-bg-child"),
            Some(serde_json::json!({
                "request_id": "req-bg-child",
                "status": "cancelled"
            })),
            None,
        ),
        // S7: late-result — background_output with late_result status
        p21_tool_requested(
            21,
            request_id,
            "tc_bg_output_late",
            "background_output",
            r#"{"request_id":"req-bg-late"}"#,
            None,
        ),
        p21_tool_started(22, request_id, "tc_bg_output_late"),
        p21_tool_finished(
            23,
            request_id,
            "tc_bg_output_late",
            ToolCallStatus::Succeeded,
            Some("Late result arrived after cancellation"),
            Some(serde_json::json!({
                "request_id": "req-bg-late",
                "status": "late_result",
                "late_result": true,
                "child_tool_call_count": 3
            })),
            None,
        ),
        // S8: plan_enter — Harness-only control plane tool
        p21_tool_requested(
            24,
            request_id,
            "tc_plan_enter",
            "plan_enter",
            r#"{"goal":"Plan the refactor"}"#,
            None,
        ),
        p21_tool_started(25, request_id, "tc_plan_enter"),
        p21_tool_finished(
            26,
            request_id,
            "tc_plan_enter",
            ToolCallStatus::Succeeded,
            Some("Entered plan mode"),
            None,
            None,
        ),
        // Final assistant text
        envelope(
            27,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "P2.1 tool display descriptors cover all state families.".to_string(),
            }),
        ),
        envelope(
            28,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-p21-response".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn p21_tool_requested(
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
            tool_call_id: tool_call_id.to_string(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: format!("digest-{tool_call_id}-args"),
            metadata,
        }),
    )
}

fn p21_tool_started(seq: u64, request_id: &str, tool_call_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: tool_call_id.to_string(),
        }),
    )
}

fn p21_tool_finished(
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
            tool_call_id: tool_call_id.to_string(),
            status,
            output_summary: output_summary.map(str::to_string),
            output_digest: Some(format!("digest-{tool_call_id}-output")),
            output_json,
            metadata,
        }),
    )
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-p21-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("p21-display".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

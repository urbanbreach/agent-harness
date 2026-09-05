use super::*;

#[test]
fn tool_call_entries_prefer_resolved_identity_and_lifecycle_contract() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_contract",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_contract".into(),
            text: "Check tool contract".to_string(),
        }),
    ));
    app.ingest_event(provider_started(2, "req_contract", "default", "model-1"));
    app.ingest_event(envelope(
        3,
        "req_contract",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_contract".into(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"check tool contract","subagent_type":"researcher"}"#
                .to_string(),
            args_digest: "digest-contract".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("agent.spawn".to_string()),
                alias_source_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));

    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(tool_call.invoked_tool_id(), "task");
    assert_eq!(tool_call.effective_tool_id(), "agent.spawn");
    assert_eq!(tool_call.resolved_canonical_tool_id(), Some("agent.spawn"));
    assert_eq!(tool_call.resolved_alias_source_tool_id(), Some("task"));
    assert_eq!(tool_call.lifecycle_state(), ToolCallLifecycleState::Pending);
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Queued);

    app.ingest_event(envelope(
        4,
        "req_contract",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_contract".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tc_contract".into()),
            summary: "Need confirmation".to_string(),
            request_digest: "digest-perm-contract".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(tool_call.lifecycle_state(), ToolCallLifecycleState::Pending);
    assert_eq!(tool_call.status, ToolCallDisplayStatus::PendingPermission);
    assert_eq!(tool_call.permissions.len(), 1);
    assert_eq!(tool_call.permissions[0].resolved_decision, None);

    app.ingest_event(envelope(
        5,
        "req_contract",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_contract".to_string(),
            decision: harness_core::event::PermissionDecision::Allow,
            reason: None,
        }),
    ));

    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(tool_call.lifecycle_state(), ToolCallLifecycleState::Pending);
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Queued);
    assert_eq!(
        tool_call.permissions[0].resolved_decision,
        Some(harness_core::event::PermissionDecision::Allow)
    );
    assert_eq!(tool_call.permissions[0].last_seq, 5);

    app.ingest_event(envelope(
        6,
        "req_contract",
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_contract".into(),
        }),
    ));

    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(tool_call.lifecycle_state(), ToolCallLifecycleState::Running);
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Running);

    app.ingest_event(envelope(
        7,
        "req_contract",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_contract".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("child completed".to_string()),
            output_digest: Some("digest-contract-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(
        tool_call.lifecycle_state(),
        ToolCallLifecycleState::Completed
    );
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Succeeded);
}

#[test]
fn activity_permission_resolution_updates_activity_level_entry() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_activity_permission",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_activity_permission".into(),
            text: "Check activity permission".to_string(),
        }),
    ));
    app.ingest_event(provider_started(
        2,
        "req_activity_permission",
        "default",
        "model-1",
    ));
    app.ingest_event(envelope(
        3,
        "req_activity_permission",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_activity".to_string(),
            kind: "bash".to_string(),
            tool_call_id: None,
            summary: "Run shell command".to_string(),
            request_digest: "digest-perm-activity".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    assert_eq!(app.activities[0].permissions.len(), 1);
    assert_eq!(app.activities[0].permissions[0].resolved_decision, None);

    app.ingest_event(envelope(
        4,
        "req_activity_permission",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_activity".to_string(),
            decision: harness_core::event::PermissionDecision::Allow,
            reason: Some("approved once".to_string()),
        }),
    ));

    let permission = &app.activities[0].permissions[0];
    assert_eq!(
        permission.resolved_decision,
        Some(harness_core::event::PermissionDecision::Allow)
    );
    assert_eq!(
        permission.resolution_reason.as_deref(),
        Some("approved once")
    );
    assert_eq!(permission.last_seq, 4);
    assert_eq!(app.activities[0].last_seq, 4);
}

#[test]
fn orphan_question_permission_becomes_pending_ask_tool_row() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_orphan_pre",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_orphan_question".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("toolcall_question_orphan".into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}],
                }]
            })
            .to_string(),
            request_digest: "digest-orphan-question".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    assert!(app.activities.is_empty());

    app.ingest_event(provider_started(
        2,
        "req_orphan_question",
        "worker",
        "model-1",
    ));

    assert_eq!(app.activities.len(), 1);
    assert_eq!(app.activities[0].tool_calls.len(), 1);
    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(tool_call.tool_id, "user.question");
    assert_eq!(tool_call.tool_call_id, "toolcall_question_orphan");
    assert_eq!(tool_call.status, ToolCallDisplayStatus::PendingPermission);
    assert_eq!(tool_call.permissions.len(), 1);

    let rendered = render_debug(&app, 100, 28);
    assert!(
        rendered.contains("Ask Pick one"),
        "orphan question should project as Ask tool row\n{rendered}"
    );
    assert!(
        rendered.contains("Waiting on answers for Pick one"),
        "orphan question should project Waiting on answers footer\n{rendered}"
    );
    assert!(app.has_active_animations());
}

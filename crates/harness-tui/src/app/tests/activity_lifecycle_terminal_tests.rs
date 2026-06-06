use super::*;

pub(crate) fn replay_terminal_only_turn_completion_scope_marks_activity_done_without_task_row() {
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-terminal-only-done"),
        vec![
            envelope(
                1,
                "req_replay_terminal_only_done",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_replay_terminal_only_done".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "Explain the fix".to_string(),
                    request_digest: "digest-replay-terminal-only-done".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                2,
                "req_replay_terminal_only_done",
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_replay_terminal_only_done".to_string(),
                    result_summary: "Final answer".to_string(),
                    result_digest: "digest-replay-terminal-only-result".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: None,
                        task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
        ],
    );

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Done);
    assert_eq!(activity.transcript_text, "Final answer");
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);
}

pub(crate) fn replay_terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row() {
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-terminal-only-cancel"),
        vec![
            envelope(
                1,
                "req_replay_terminal_only_cancel",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_replay_terminal_only_cancel".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "Explain the fix".to_string(),
                    request_digest: "digest-replay-terminal-only-cancel".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                2,
                "req_replay_terminal_only_cancel",
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_replay_terminal_only_cancel".to_string(),
                    reason: "agent turn exceeded profile max_iters=24".to_string(),
                    task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                }),
            ),
        ],
    );

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Error);
    assert_eq!(
        activity.error_message.as_deref(),
        Some("agent turn exceeded profile max_iters=24")
    );
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Cancelled);
}

pub(crate) fn replay_terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state()
{
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-terminal-only-tool-cancel"),
        vec![
            envelope(
                1,
                "req_replay_terminal_only_tool_cancel",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_replay_terminal_only_tool_cancel".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "Read the file".to_string(),
                    request_digest: "digest-replay-terminal-only-tool-cancel".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                2,
                "req_replay_terminal_only_tool_cancel",
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_replay_terminal_only_tool_cancel".to_string(),
                    reason: "tool request timed out".to_string(),
                    task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                }),
            ),
        ],
    );

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Streaming);
    assert!(activity.error_message.is_none());
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Sending);
}

pub(crate) fn queued_turn_schedule_keeps_activity_queued_until_provider_starts() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_active".to_string(),
            text: "active".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_active",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_active".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "active".to_string(),
            request_digest: "digest-active".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_queued",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_queued".to_string(),
            text: "queued".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_queued",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_queued".to_string(),
            state: TaskScheduleState::Queued,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));

    let queued = app
        .activities
        .iter()
        .find(|activity| activity.request_id == "req_queued")
        .expect("queued activity");
    assert_eq!(queued.status, ActivityStatus::Queued);
    assert!(app.active_turn_in_progress());

    app.ingest_event(envelope(
        5,
        "req_queued",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_queued".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "queued".to_string(),
            request_digest: "digest-queued".to_string(),
            metadata: None,
        }),
    ));

    let queued = app
        .activities
        .iter()
        .find(|activity| activity.request_id == "req_queued")
        .expect("queued activity");
    assert_eq!(queued.status, ActivityStatus::Streaming);
}

pub(crate) fn tool_task_completion_does_not_copy_tool_output_into_activity_transcript() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_tool_completion_transcript",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_tool_completion_transcript".to_string(),
            text: "Inspect tokio docs".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_tool_completion_transcript",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_tool_completion_transcript".to_string(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "Inspect tokio docs".to_string(),
            request_digest: "digest-tool-completion-transcript".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_tool_completion_transcript",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_docs_tokio".to_string(),
            tool_id: "mcp.docs-rs.search_in_crate".to_string(),
            args_summary: r#"{"crate_name":"tokio","query":"spawn"}"#.to_string(),
            args_digest: "digest-docs-tokio-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_tool_completion_transcript",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_docs_tokio".to_string(),
            result_summary: "fn spawn\nstruct JoinHandle".to_string(),
            result_digest: "digest-task-docs-tokio".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_docs_tokio".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let activity = app.activities.front().expect("activity exists");
    assert!(
        activity.transcript_text.is_empty(),
        "tool task completion should not become assistant transcript text"
    );
    assert_eq!(activity.tool_calls.len(), 1);
    assert_eq!(activity.tool_calls[0].tool_call_id, "tc_docs_tokio");
}

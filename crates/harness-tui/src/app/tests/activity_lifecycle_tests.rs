use super::*;

#[path = "activity_lifecycle_terminal_tests.rs"]
mod activity_lifecycle_terminal_tests;
pub(super) use activity_lifecycle_terminal_tests::{
    queued_turn_schedule_keeps_activity_queued_until_provider_starts as terminal_queued_turn_schedule_keeps_activity_queued_until_provider_starts,
    replay_terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state as terminal_replay_terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state,
    replay_terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row as terminal_replay_terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row,
    replay_terminal_only_turn_completion_scope_marks_activity_done_without_task_row as terminal_replay_terminal_only_turn_completion_scope_marks_activity_done_without_task_row,
    tool_task_completion_does_not_copy_tool_output_into_activity_transcript as terminal_tool_task_completion_does_not_copy_tool_output_into_activity_transcript,
};

pub(super) fn provider_reasoning_delta_populates_thinking_stream_without_overwriting_answer_text() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(provider_started(
        1,
        "req_reasoning",
        "default",
        "gpt-4o-mini",
    ));
    app.ingest_event(envelope(
        2,
        "req_reasoning",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_reasoning".to_string(),
            delta: "Drafting a careful answer.".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_reasoning",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_reasoning".to_string(),
            delta: "Hello world".to_string(),
        }),
    ));

    let activity = app.activities.back().expect("streaming activity");
    assert_eq!(activity.thinking_text, "Drafting a careful answer.");
    assert_eq!(activity.transcript_text, "Hello world");
}

pub(super) fn provider_request_finished_keeps_activity_streaming_until_turn_task_completes() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_turn_task",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_turn_task".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_turn_task",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "provider_req_turn_task".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Investigate the harness".to_string(),
            request_digest: "digest-turn-task".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_turn_task",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "provider_req_turn_task".to_string(),
            delta: "Looking into the turn loop".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_turn_task",
        EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
            request_id: "provider_req_turn_task".to_string(),
            finish_reason: "done".to_string(),
            output_digest: Some("digest-turn-task-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Streaming);
    assert!(app.active_turn_in_progress());

    app.ingest_event(envelope(
        5,
        "req_turn_task",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_turn_task".to_string(),
            result_summary: "Final answer".to_string(),
            result_digest: "digest-turn-task-result".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: None,
                task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let activity = app.activities.back().expect("completed activity exists");
    assert_eq!(activity.status, ActivityStatus::Done);
    assert!(!app.active_turn_in_progress());
}

pub(super) fn cache_read_write_tokens_render_as_separate_status_labels() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_cache",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "provider_req_cache".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Use cached prompt".to_string(),
            request_digest: "digest-cache-start".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_cache",
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "provider_req_cache".to_string(),
            finish_reason: "done".to_string(),
            output_digest: Some("digest-cache-finish".to_string()),
            usage: None,
            metadata: Some(ProviderRequestFinishedMetadata {
                cache_read_tokens: Some(41),
                cache_write_tokens: Some(17),
                ..ProviderRequestFinishedMetadata::default()
            }),
        }),
    ));

    let segment = app
        .control_dock_view_model()
        .summary_segment
        .expect("cache summary status segment");
    assert_eq!(segment.text, "cache read 41 · write 17");
}

pub(super) fn task_cancelled_marks_matching_activity_as_error() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_cancelled_turn",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_cancelled_turn".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_cancelled_turn",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_cancelled_turn".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Edit the docs".to_string(),
            request_digest: "digest-cancelled-turn".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_cancelled_turn",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_cancelled_turn".to_string(),
            delta: "Still thinking".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_cancelled_turn",
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_cancelled_turn".to_string(),
            reason: "agent turn exceeded profile max_iters=24".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
        }),
    ));

    let activity = app.activities.back().expect("cancelled activity exists");
    assert_eq!(activity.status, ActivityStatus::Error);
    assert_eq!(
        activity.error_message.as_deref(),
        Some("agent turn exceeded profile max_iters=24")
    );
    assert!(!app.active_turn_in_progress());
}

pub(super) fn provider_error_categories_surface_in_tui_activity_and_runtime_state() {
    // arrange
    for category in [
        ProviderErrorCategory::MissingCredentials,
        ProviderErrorCategory::RateLimited,
        ProviderErrorCategory::ContextWindowExceeded,
    ] {
        let mut app = AppState::new_live(None, false, None);
        let request_id = format!("req_provider_{}", category.as_str());
        app.ingest_event(envelope(
            1,
            &request_id,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.clone(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Trigger provider category".to_string(),
                request_digest: "digest-provider-category".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            2,
            &request_id,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.clone(),
                finish_reason: "error".to_string(),
                output_digest: None,
                usage: None,
                metadata: Some(ProviderRequestFinishedMetadata {
                    provider_error_category: Some(category),
                    provider_error_remediation: Some(category.remediation().to_string()),
                    ..ProviderRequestFinishedMetadata::default()
                }),
            }),
        ));
        app.ingest_event(envelope(
            3,
            &request_id,
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: "task_provider_category".to_string(),
                reason: format!("{}: fixture provider failure", category.as_str()),
                task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
            }),
        ));

        let activity = app.activities.back().expect("provider error activity");
        // act
        let error_message = activity.error_message.as_deref().expect("error detail");
        // assert
        assert_eq!(activity.status, ActivityStatus::Error);
        assert!(error_message.contains(category.as_str()), "{error_message}");
        assert!(
            error_message.contains("fixture provider failure"),
            "{error_message}"
        );
        assert!(
            error_message.contains(category.remediation()),
            "{error_message}"
        );
        let runtime = app.runtime_state();
        assert_eq!(runtime.kind, RuntimeStateKind::Cancelled);
        assert!(
            runtime
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains(category.as_str())),
            "runtime detail should include category: {:?}",
            runtime.detail
        );
    }
}

pub(super) fn child_tool_task_completed_does_not_finish_parent_turn_activity() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_child_task_completed",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_parent_turn".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_child_task_completed",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child_task_completed".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Inspect a file".to_string(),
            request_digest: "digest-child-task-completed".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_child_task_completed",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_child_tool".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("tool:read".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_child_task_completed",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_tool".to_string(),
            result_summary: "24 lines read".to_string(),
            result_digest: "digest-child-tool-result".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_child_read".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Streaming);
    assert!(activity.transcript_text.is_empty());
    assert!(app.active_turn_in_progress());
}

pub(super) fn child_tool_task_cancelled_does_not_mark_parent_turn_activity_error() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_child_task_cancelled",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_parent_turn".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_child_task_cancelled",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child_task_cancelled".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Inspect a file".to_string(),
            request_digest: "digest-child-task-cancelled".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_child_task_cancelled",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_child_tool".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("tool:read".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_child_task_cancelled",
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_child_tool".to_string(),
            reason: "tool request timed out".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Streaming);
    assert!(activity.error_message.is_none());
    assert!(app.active_turn_in_progress());
}

pub(super) fn terminal_only_turn_completion_scope_marks_activity_done_without_task_row() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_terminal_only_done",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_terminal_only_done".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Explain the fix".to_string(),
            request_digest: "digest-terminal-only-done".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_terminal_only_done",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_terminal_only_done".to_string(),
            result_summary: "Final answer".to_string(),
            result_digest: "digest-terminal-only-result".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: None,
                task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Done);
    assert_eq!(activity.transcript_text, "Final answer");
    assert!(!app.active_turn_in_progress());
}

pub(super) fn terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_terminal_only_cancel",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_terminal_only_cancel".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Explain the fix".to_string(),
            request_digest: "digest-terminal-only-cancel".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_terminal_only_cancel",
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_terminal_only_cancel".to_string(),
            reason: "agent turn exceeded profile max_iters=24".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Error);
    assert_eq!(
        activity.error_message.as_deref(),
        Some("agent turn exceeded profile max_iters=24")
    );
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Cancelled);
    assert!(!app.active_turn_in_progress());
}

pub(super) fn terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_terminal_only_tool_cancel",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_terminal_only_tool_cancel".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Read the file".to_string(),
            request_digest: "digest-terminal-only-tool-cancel".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_terminal_only_tool_cancel",
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_terminal_only_tool_cancel".to_string(),
            reason: "tool request timed out".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Streaming);
    assert!(activity.error_message.is_none());
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Sending);
}

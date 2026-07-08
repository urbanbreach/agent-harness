use super::*;
use harness_core::event::ProviderRequestStartedMetadata;

mod event_helpers {
    include!("opencode_subagent_parity_event_helpers.rs");
}

#[derive(Clone, Copy)]
pub(super) enum TaskFixtureState {
    Running,
    Retrying,
    Completed,
    BackgroundCompleted,
}

pub(super) fn subagent_events(state: TaskFixtureState) -> Vec<EventEnvelopeV1> {
    let mut events = vec![
        event_helpers::run_started(1),
        event_helpers::agent_spawned_with_parent(2, "parent", "build", None),
        envelope(
            3,
            "req_parent",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_parent".into(),
                text: "Audit transcript parity".to_string(),
            }),
        ),
        provider_started(4, "req_parent", "default", "model-parent"),
        event_helpers::child_task_requested_for_evidence(
            5,
            "tc_task",
            "agent_worker",
            "req_child",
            false,
        ),
        envelope(
            6,
            "req_parent",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_task".into(),
            }),
        ),
        event_helpers::agent_spawned_with_parent(7, "agent_worker", "researcher", Some("parent")),
        envelope_with_actor(
            8,
            "req_child",
            EventActor::new(ActorKind::Worker, Some("agent_worker".to_string())),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_child".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("agent:running:researcher".to_string()),
            }),
        ),
        envelope_with_actor(
            9,
            "req_child",
            EventActor::new(ActorKind::Worker, Some("agent_worker".to_string())),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_child".into(),
                provider_id: "default".to_string(),
                model_id: "model-child".to_string(),
                prompt_summary: "Audit transcript parity".to_string(),
                request_digest: "digest-child".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            10,
            "req_child",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_child_read".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
                args_digest: "digest-child-read".to_string(),
                metadata: None,
            }),
        ),
    ];

    match state {
        TaskFixtureState::Running => {}
        TaskFixtureState::Retrying => events.push(retry_event(11)),
        TaskFixtureState::Completed | TaskFixtureState::BackgroundCompleted => {
            events.extend(completion_events(matches!(
                state,
                TaskFixtureState::BackgroundCompleted
            )));
        }
    }

    events
}

pub(super) fn detached_foreground_tool_call_event(seq: u64) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_parent",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_task".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("foreground subagent moved to background".to_string()),
            output_digest: Some("digest-task-backgrounded".to_string()),
            output_json: Some(serde_json::json!({
                "background": true,
                "child_session_id": "agent_worker",
                "child_request_id": "req_child",
                "mode": "background",
                "status": "scheduled",
            })),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                lineage: Some(event_helpers::lineage()),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

pub(super) fn sibling_events() -> Vec<EventEnvelopeV1> {
    vec![
        event_helpers::run_started(1),
        event_helpers::agent_spawned_with_parent(2, "parent", "planner", None),
        provider_started(3, "req_parent", "mock", "model-parent"),
        event_helpers::child_task_requested_for_evidence(
            4,
            "tc_child_a",
            "child_a",
            "req_child_a",
            false,
        ),
        event_helpers::child_task_requested_for_evidence(
            5,
            "tc_child_b",
            "child_b",
            "req_child_b",
            false,
        ),
        event_helpers::agent_spawned_with_parent(6, "child_a", "worker-a", Some("parent")),
        event_helpers::agent_spawned_with_parent(7, "child_b", "worker-b", Some("parent")),
        provider_started(8, "req_child_a", "mock", "model-child-a"),
        provider_started(9, "req_child_b", "mock", "model-child-b"),
    ]
}

pub(super) fn run_started(seq: u64) -> EventEnvelopeV1 {
    event_helpers::run_started(seq)
}

pub(super) fn agent_spawned_with_parent(
    seq: u64,
    agent_id: &str,
    profile: &str,
    parent_agent_id: Option<&str>,
) -> EventEnvelopeV1 {
    event_helpers::agent_spawned_with_parent(seq, agent_id, profile, parent_agent_id)
}

fn retry_event(seq: u64) -> EventEnvelopeV1 {
    envelope_with_actor(
        seq,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_worker".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".into(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "Audit transcript parity".to_string(),
            request_digest: "digest-child-retry".to_string(),
            metadata: Some(ProviderRequestStartedMetadata {
                retry: Some(harness_core::event::ProviderRequestRetryMetadata {
                    attempt: 1,
                    max_attempts: 3,
                    delay_ms: Some(1_000),
                    category: Some(ProviderErrorCategory::RateLimited),
                }),
                ..ProviderRequestStartedMetadata::default()
            }),
        }),
    )
}

fn completion_events(background: bool) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            11,
            "req_child",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_child_grep".into(),
                tool_id: "fs.grep".to_string(),
                args_summary: r#"{"pattern":"task row"}"#.to_string(),
                args_digest: "digest-child-grep".to_string(),
                metadata: None,
            }),
        ),
        envelope_with_actor(
            12,
            "req_child",
            EventActor::new(ActorKind::Worker, Some("agent_worker".to_string())),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_child".into(),
                delta: "Child details stay out of the parent task row.".to_string(),
            }),
        ),
        envelope_with_actor(
            13,
            "req_child",
            EventActor::new(ActorKind::Worker, Some("agent_worker".to_string())),
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_child".to_string().into(),
                result_summary: "Found the inline transcript path.".to_string(),
                result_digest: "digest-task-child".to_string(),
                metadata: Some(TaskCompletionMetadata {
                    lineage: Some(event_helpers::lineage()),
                    task_scope: Some(TaskTerminalScope::ToolCall),
                    timing: Some(ExecutionTimingMetadata {
                        started_mono_ms: Some(800),
                        finished_mono_ms: Some(2_400),
                        elapsed_ms: Some(1_600),
                    }),
                    hook_executions: Vec::new(),
                }),
            }),
        ),
        envelope(
            14,
            "req_parent",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_task".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("child session finished".to_string()),
                output_digest: Some("digest-task-finished".to_string()),
                output_json: Some(serde_json::json!({
                    "background": background,
                    "child_session_id": "agent_worker",
                    "child_request_id": "req_child",
                })),
                metadata: Some(ToolCallMetadata {
                    canonical_tool_id: Some("task".to_string()),
                    lineage: Some(event_helpers::lineage()),
                    ..ToolCallMetadata::default()
                }),
            }),
        ),
    ]
}

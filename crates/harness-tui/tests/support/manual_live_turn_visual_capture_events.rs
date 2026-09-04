use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    ProviderStreamDeltaEvent, RunFailedEvent, TaskCancelledEvent, TaskLineageMetadata,
    TaskScheduleMetadata, TaskScheduleState, TaskScheduledEvent, TaskTerminalScope,
    ToolCallRequestedEvent, ToolCallStartedEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
};

pub(crate) const ACTIVE_REQUEST_ID: &str = "req_manual_active";
pub(crate) const ACTIVE_TASK_ID: &str = "task_manual_active";
pub(crate) const QUEUED_REQUEST_ID: &str = "req_manual_queued";
pub(crate) const QUEUED_TASK_ID: &str = "task_manual_queued";

const CJK_PROMPT: &str = "验证中文终端双宽字符和右侧换行边界。当前构建正在验证中文双宽字符、活动命令、排队输入和右侧换行边界。当前构建正在验证中文双宽字符、活动命令、排队输入和右侧换行边界。";
const CJK_RESPONSE: &str = "当前构建正在验证中文双宽字符、活动子任务、排队输入和右侧换行边界。当前构建正在验证中文双宽字符、活动子任务、排队输入和右侧换行边界。当前构建正在验证中文双宽字符、活动子任务、排队输入和右侧换行边界。";
const CJK_QUEUED: &str = "排队消息：请继续验证中文输入、双宽字符与右侧换行边界。";

pub(crate) struct CaptureScenario {
    pub(crate) events: Vec<EventEnvelopeV1>,
    pub(crate) events_are_live: bool,
    pub(crate) status: Option<&'static str>,
    pub(crate) send_now_transition: bool,
}

impl CaptureScenario {
    fn plain(events: Vec<EventEnvelopeV1>) -> Self {
        Self {
            events,
            events_are_live: false,
            status: None,
            send_now_transition: false,
        }
    }

    fn live(events: Vec<EventEnvelopeV1>) -> Self {
        Self {
            events,
            events_are_live: true,
            status: None,
            send_now_transition: false,
        }
    }

    fn with_status(events: Vec<EventEnvelopeV1>, status: &'static str) -> Self {
        Self {
            events,
            events_are_live: false,
            status: Some(status),
            send_now_transition: false,
        }
    }

    fn send_now(events: Vec<EventEnvelopeV1>) -> Self {
        Self {
            events,
            events_are_live: false,
            status: None,
            send_now_transition: true,
        }
    }
}

pub(crate) fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-manual-live-turn-{seq:04}"),
        seq,
        run_id: "run_manual_live_turn".into(),
        mono_ms: seq.saturating_mul(1_000),
        ts: Some("2026-08-15T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("manual-live-turn".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some("run:run_manual_live_turn".to_string()),
        payload,
    }
}

fn active_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            ACTIVE_REQUEST_ID,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: ACTIVE_REQUEST_ID.into(),
                text: CJK_PROMPT.to_string(),
            }),
        ),
        envelope(
            2,
            ACTIVE_REQUEST_ID,
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: ACTIVE_TASK_ID.into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-manual".to_string()),
                metadata: None,
            }),
        ),
        envelope(
            3,
            ACTIVE_REQUEST_ID,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: ACTIVE_REQUEST_ID.into(),
                provider_id: "mock".to_string(),
                model_id: "model-manual".to_string(),
                prompt_summary: CJK_PROMPT.to_string(),
                request_digest: "digest-manual-active".to_string(),
                metadata: None,
            }),
        ),
    ]
}

fn waiting_model_events() -> Vec<EventEnvelopeV1> {
    let mut events = active_events();
    for event in &mut events {
        event.mono_ms = 1_000;
    }
    events
}

fn responding_events() -> Vec<EventEnvelopeV1> {
    let mut events = active_events();
    events.push(envelope(
        4,
        ACTIVE_REQUEST_ID,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: ACTIVE_REQUEST_ID.into(),
            delta: CJK_RESPONSE.to_string(),
        }),
    ));
    events
}

fn active_child_event() -> EventEnvelopeV1 {
    let mut event = envelope(
        6,
        ACTIVE_REQUEST_ID,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_manual_child".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:mock:model-manual-child".to_string()),
            metadata: Some(TaskScheduleMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tool_manual_wait".to_string()),
                    parent_task_id: Some(ACTIVE_TASK_ID.into()),
                    parent_request_id: Some(ACTIVE_REQUEST_ID.to_string()),
                    parent_session_id: Some("session_manual_parent".to_string()),
                    child_session_id: Some("session_manual_child".to_string()),
                    child_request_id: Some("req_manual_child".to_string()),
                    child_provider_id: Some("mock".to_string()),
                    child_model_id: Some("model-manual-child".to_string()),
                }),
            }),
        }),
    );
    event.actor = EventActor::new(ActorKind::Worker, Some("explore".to_string()));
    event
}

fn command_watcher_event(seq: u64, request_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_manual_command_watcher".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("tool:bash".to_string()),
            metadata: None,
        }),
    )
}

fn running_tool_events(tool_id: &str, args_summary: &str) -> Vec<EventEnvelopeV1> {
    let mut events = active_events();
    events.push(envelope(
        4,
        ACTIVE_REQUEST_ID,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool_manual_wait".into(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: "digest-manual-tool".to_string(),
            metadata: None,
        }),
    ));
    events.push(envelope(
        5,
        ACTIVE_REQUEST_ID,
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tool_manual_wait".into(),
        }),
    ));
    events
}

fn push_queued_prompt(events: &mut Vec<EventEnvelopeV1>) {
    events.push(envelope(
        7,
        QUEUED_REQUEST_ID,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: QUEUED_REQUEST_ID.into(),
            text: CJK_QUEUED.to_string(),
        }),
    ));
    events.push(envelope(
        8,
        QUEUED_REQUEST_ID,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: QUEUED_TASK_ID.into(),
            state: TaskScheduleState::Queued,
            queue_key: Some("provider_model:mock:model-manual".to_string()),
            metadata: None,
        }),
    ));
}

fn failed_events() -> Vec<EventEnvelopeV1> {
    let mut events = responding_events();
    events.push(envelope(
        5,
        ACTIVE_REQUEST_ID,
        EventV1::RunFailed(RunFailedEvent {
            error: "provider failed after partial response".to_string(),
        }),
    ));
    events
}

fn cancelled_events() -> Vec<EventEnvelopeV1> {
    let mut events = responding_events();
    events.push(envelope(
        5,
        ACTIVE_REQUEST_ID,
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: ACTIVE_TASK_ID.into(),
            reason: "interrupted".to_string(),
            task_scope: Some(TaskTerminalScope::AgentTurn),
        }),
    ));
    events
}

fn watcher_events() -> Vec<EventEnvelopeV1> {
    vec![command_watcher_event(1, "req_manual_watcher")]
}

pub(crate) fn scenario(name: &str) -> Result<CaptureScenario, std::io::Error> {
    Ok(match name {
        "waiting_model" => CaptureScenario::live(waiting_model_events()),
        "responding" => CaptureScenario::plain(responding_events()),
        "waiting_answers" => {
            CaptureScenario::plain(running_tool_events("question", r#"{"questions":[]}"#))
        }
        "waiting_subagent" => {
            let mut events = running_tool_events(
                "task",
                r#"{"description":"inspect workspace","run_in_background":false}"#,
            );
            events.push(active_child_event());
            CaptureScenario::plain(events)
        }
        "waiting_task_output" => {
            let mut events = running_tool_events(
                "background_output",
                r#"{"task_id":"bg_manual","block":false}"#,
            );
            events.push(command_watcher_event(6, ACTIVE_REQUEST_ID));
            CaptureScenario::plain(events)
        }
        "parked" | "parked_command" => {
            let mut events = running_tool_events(
                "background_output",
                r#"{"task_id":"bg_manual","block":true}"#,
            );
            events.push(command_watcher_event(6, ACTIVE_REQUEST_ID));
            CaptureScenario::plain(events)
        }
        "parked_queued" | "parked_command_queued" | "send_now" | "send_now_command" => {
            let mut events = running_tool_events(
                "background_output",
                r#"{"task_id":"bg_manual","block":true}"#,
            );
            events.push(command_watcher_event(6, ACTIVE_REQUEST_ID));
            push_queued_prompt(&mut events);
            if matches!(name, "send_now" | "send_now_command") {
                CaptureScenario::send_now(events)
            } else {
                CaptureScenario::plain(events)
            }
        }
        "watcher" | "command_watcher" => CaptureScenario::plain(watcher_events()),
        "recovering" => CaptureScenario::with_status(
            active_events(),
            "live stream lagged by 2; replaying from seq 1",
        ),
        "reconnecting" => {
            CaptureScenario::with_status(active_events(), "live event stream disconnected")
        }
        "failed" => CaptureScenario::plain(failed_events()),
        "cancelled" => CaptureScenario::plain(cancelled_events()),
        other => {
            return Err(std::io::Error::other(format!(
                "unknown manual capture scenario: {other}"
            )))
        }
    })
}

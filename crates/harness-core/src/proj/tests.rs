use super::*;
fn event(seq: u64, actor: &str, correlation: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("e{seq}"),
        run_id: "run".into(),
        seq,
        mono_ms: seq * 10,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some(actor.into())),
        correlation_id: Some(correlation.into()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}
#[test]
fn background_projection_preserves_cancellation_and_checks_task_ownership(
) -> Result<(), Box<dyn std::error::Error>> {
    let lineage = TaskLineageMetadata {
        parent_tool_call_id: Some("spawn".into()),
        child_session_id: Some("child".into()),
        child_request_id: Some("request".into()),
        ..Default::default()
    };
    let mut events = vec![
        event(
            1,
            "parent",
            "parent-turn",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "spawn".into(),
                tool_id: "task".into(),
                args_summary: "{}".into(),
                args_digest: "digest".into(),
                metadata: Some(ToolCallMetadata {
                    lineage: Some(lineage.clone()),
                    ..Default::default()
                }),
            }),
        ),
        event(
            2,
            "child",
            "request",
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "child-task".into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:default".into()),
                metadata: Some(TaskScheduleMetadata {
                    lineage: Some(lineage),
                }),
            }),
        ),
        event(
            3,
            "child",
            "request",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "read-1".into(),
                tool_id: "read".into(),
                args_summary: "{}".into(),
                args_digest: "digest".into(),
                metadata: None,
            }),
        ),
        event(
            4,
            "child",
            "read-1",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "read-1".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("read".into()),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ),
        event(
            5,
            "child",
            "request",
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: "child-task".into(),
                reason: "user cancelled".into(),
                failure: false,
                task_scope: Some(TaskTerminalScope::AgentTurn),
            }),
        ),
    ];
    let owner = EventActor::new(ActorKind::Worker, Some("parent".into()));
    let other = EventActor::new(ActorKind::Worker, Some("other".into()));
    let reference = resolve_background_request_ref(&events, &owner, None, Some("child"))?;
    assert!(matches!(
        resolve_background_request_ref(&events, &other, Some("request"), None),
        Err(BackgroundRequestProjectionError::Unauthorized)
    ));
    let projection = project_background_request(&events, &reference)?;
    assert_eq!(projection.status, "cancelled");
    assert!(projection.terminal);
    assert_eq!(projection.duration_ms, Some(30));
    assert_eq!(
        (
            projection.tool_calls.requested,
            projection.tool_calls.succeeded,
            projection.tool_calls.failed
        ),
        (1, 1, 0)
    );
    events.push(event(
        6,
        "child",
        "request",
        EventV1::TaskResultLate(TaskResultLateEvent {
            task_id: "child-task".into(),
            result_digest: "late".into(),
        }),
    ));
    let late = project_background_request(&events, &reference)?;
    assert!(late.late_result && late.terminal);
    assert_eq!(late.status, "late_result");
    assert_eq!(late.cancel_reason.as_deref(), Some("user cancelled"));
    assert_eq!(late.duration_ms, Some(30));
    assert!(resolve_all_background_request_refs(&events, &other).is_empty());
    assert_eq!(
        resolve_all_background_request_refs(&events, &owner).len(),
        1
    );
    events[1].event_id = events[0].event_id.clone();
    assert!(resolve_background_request_ref(&events, &owner, Some("request"), None).is_err());
    assert!(resolve_all_background_request_refs(&events, &owner).is_empty());
    Ok(())
}

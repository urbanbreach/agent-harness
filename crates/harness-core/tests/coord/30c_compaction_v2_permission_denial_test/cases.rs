#[tokio::test]
async fn compaction_v2_permission_denial_reopens_with_failed_tool_pair() {
    let scenario = capture_denial_scenario().await;

    let (requested, tool_call_id) = scenario
        .events
        .iter()
        .find_map(|event| {
            (event.correlation_id.as_deref() == Some(scenario.denied_request_id.as_str()))
                .then_some(event)
                .and_then(|event| match &event.payload {
                    EventV1::ToolCallRequested(payload) => {
                        Some((event, payload.tool_call_id.to_string()))
                    }
                    _ => None,
                })
        })
        .unwrap_or_abort();
    assert_eq!(
        scenario
            .events
            .iter()
            .filter(|event| {
                event.correlation_id.as_deref() == Some(scenario.denied_request_id.as_str())
                    && matches!(
                        &event.payload,
                        EventV1::ToolCallRequested(payload)
                            if payload.tool_call_id.as_str() == tool_call_id
                    )
            })
            .count(),
        1,
        "denied lifecycle must have one uniquely correlated request"
    );
    let finished = scenario
        .events
        .iter()
        .find(|event| {
            event.correlation_id.as_deref() == Some(scenario.denied_request_id.as_str())
                && matches!(
                    &event.payload,
                    EventV1::ToolCallFinished(payload)
                        if payload.tool_call_id.as_str() == tool_call_id
                            && payload.status == ToolCallStatus::Failed
                )
        })
        .unwrap_or_abort();
    assert_eq!(
        scenario
            .events
            .iter()
            .filter(|event| {
                event.correlation_id.as_deref() == Some(scenario.denied_request_id.as_str())
                    && matches!(
                        &event.payload,
                        EventV1::ToolCallFinished(payload)
                            if payload.tool_call_id.as_str() == tool_call_id
                                && payload.status == ToolCallStatus::Failed
                    )
            })
            .count(),
        1,
        "denied lifecycle must have one failed terminal"
    );
    assert_eq!(
        finished.causation_id.as_deref(),
        Some(requested.event_id.as_str())
    );
    assert!(!scenario.events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(payload) if payload.tool_call_id.as_str() == tool_call_id
        )
    }));
    let permission_id = scenario
        .events
        .iter()
        .find_map(|event| {
            (event.correlation_id.as_deref() == Some(scenario.denied_request_id.as_str()))
                .then_some(event)
                .and_then(|event| match &event.payload {
                    EventV1::PermissionRequested(payload)
                        if payload.tool_call_id.as_ref().map(|id| id.as_str())
                            == Some(tool_call_id.as_str()) =>
                    {
                        Some(payload.permission_id.as_str())
                    }
                    _ => None,
                })
        })
        .unwrap_or_abort();
    assert_eq!(
        scenario
            .events
            .iter()
            .filter(|event| {
                event.correlation_id.as_deref() == Some(scenario.denied_request_id.as_str())
                    && matches!(
                        &event.payload,
                        EventV1::PermissionRequested(payload)
                            if payload.tool_call_id.as_ref().map(|id| id.as_str())
                                == Some(tool_call_id.as_str())
                    )
            })
            .count(),
        1,
        "denied lifecycle must have one correlated permission request"
    );
    assert_eq!(
        scenario
            .events
            .iter()
            .filter(|event| {
                matches!(
                    &event.payload,
                    EventV1::PermissionResolved(payload)
                        if payload.permission_id == permission_id
                            && payload.decision == EventPermissionDecision::Deny
                            && payload.reason.as_deref() == Some("runtime owner denial")
                )
            })
            .count(),
        1,
        "denied permission must have one exact resolution"
    );
    assert_eq!(scenario.tool_calls, 0);
    let live_tool_call_id = only_assistant_tool_call_id(&scenario.continuation);
    assert_one_denied_pair(
        &scenario.continuation,
        &live_tool_call_id,
        "live continuation",
    );
    let durable_tool_call_id = only_assistant_tool_call_id(&scenario.followup);
    assert_eq!(durable_tool_call_id, tool_call_id);
    assert_one_denied_pair(
        &scenario.followup,
        &durable_tool_call_id,
        "reopened canonical followup",
    );
    let durable_result = scenario
        .followup
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some(tool_call_id.as_str()))
        .unwrap_or_abort();
    let finished_summary = match &finished.payload {
        EventV1::ToolCallFinished(payload) => payload.output_summary.as_deref(),
        _ => None,
    };
    assert_eq!(
        Some(durable_result.content.as_str()),
        finished_summary,
        "reopened provider result must be byte-equal to the canonical terminal event"
    );
}

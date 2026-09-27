use super::*;
use harness_core::event::{EventEnvelopeV1, EventV1};

pub(super) fn attach(
    ctx: &ToolContext,
    result: &mut ToolResult,
    events: &[EventEnvelopeV1],
    args: &Background,
) -> Result<(), ToolError> {
    let Some(value) = result.structured_json.as_mut() else {
        return Err(ToolError::Execution("child result has no metadata".into()));
    };
    if args.include_thinking {
        value["thinking"] = json!({"unavailable":true,"reason":"Reasoning is not retained in session history.","requested_max_chars":args.thinking_max_chars});
    }
    if !args.full_session {
        return Ok(());
    }
    let is_message = |event: &EventEnvelopeV1| {
        matches!(
            event.payload,
            EventV1::UserMessageSubmitted(_) | EventV1::AssistantMessageFinished(_)
        )
    };
    let after = args
        .since_message_id
        .as_deref()
        .map(|id| {
            events
                .iter()
                .find(|e| e.event_id == id && is_message(e))
                .map(|e| e.seq)
                .ok_or_else(|| {
                    ToolError::InvalidArguments(
                        "since_message_id is not a message in this child session".into(),
                    )
                })
        })
        .transpose()?;
    let mut selected: Vec<_> = events
        .iter()
        .filter(|e| after.is_none_or(|seq| e.seq > seq))
        .collect();
    if args.from_end {
        selected.reverse();
    }
    let total_events = selected.len();
    let total_messages = selected.iter().filter(|e| is_message(e)).count();
    let limit = args.message_limit.unwrap_or(200).min(200);
    let (mut output, mut messages, mut tools) = (Vec::new(), Vec::new(), Vec::new());
    let mut bytes = 0;
    for event in selected {
        if ctx.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let summary = crate::sessions::history::summary(ctx, event);
        bytes += summary.to_string().len();
        // Messages and tool results are disjoint subsets, so all three arrays stay below 4 MiB.
        if bytes > 2 * 1024 * 1024 || output.len() == 1000 {
            break;
        }
        if is_message(event) && messages.len() < limit {
            messages.push(summary.clone());
        }
        if args.include_tool_results && matches!(event.payload, EventV1::ToolCallFinished(_)) {
            tools.push(summary.clone());
        }
        output.push(summary);
    }
    let mut history = json!({"events":output,"messages":messages,"event_count":output.len(),"message_count":messages.len(),
        "total_event_count":total_events,"total_message_count":total_messages,"truncated":output.len()<total_events,
        "message_truncated":messages.len()<total_messages,"from_end":args.from_end});
    if args.include_tool_results {
        history["tool_results"] = tools.into();
    }
    value["full_session"] = history;
    Ok(())
}

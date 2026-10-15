//! Markdown layout of a branch's journal records. Raw means uncompacted and entry
//! preserving: nothing is summarized, and context-window boundaries stay in place.
use harness_core::{
    event::{EventEnvelopeV1, EventV1, ToolCallStatus},
    session::AssistantPart,
};
use std::collections::HashMap;

const HEADER: &str = "# Current branch — full history\n\nSource: caller-bound live session branch. Context-window boundaries are retained below; this view never uses a registry or on-disk fallback.\n\n";

/// The tool a call id names and the id the model saw for it.
type Calls<'a> = HashMap<&'a str, (&'a str, &'a str)>;

/// Renders the active-branch events as entries chained by their predecessor.
pub(crate) fn render_full_history(events: &[EventEnvelopeV1]) -> String {
    let calls = calls(events);
    let mut out = String::from(HEADER);
    let mut parent: Option<u64> = None;
    for event in events {
        let Some((kind, body)) = entry(event, &calls) else {
            continue;
        };
        let parent_id = parent.map_or_else(|| "root".to_owned(), |id| id.to_string());
        out.push_str(&format!(
            "## Entry {} · {kind}\n\nParent: {parent_id}\nTimestamp: {}\n\n{body}\n\n",
            event.seq,
            event.ts.as_deref().unwrap_or_default(),
        ));
        parent = Some(event.seq);
    }
    let mut out = out.trim().to_owned();
    out.push('\n');
    out
}

fn calls(events: &[EventEnvelopeV1]) -> Calls<'_> {
    let mut calls = HashMap::new();
    for event in events {
        if let EventV1::AssistantMessageFinished(finished) = &event.payload {
            for part in &finished.parts {
                if let AssistantPart::ToolCall(call) = part {
                    let shown = call
                        .provider_tool_call_id
                        .as_deref()
                        .unwrap_or_else(|| call.tool_call_id.as_str());
                    calls.insert(call.tool_call_id.as_str(), (call.tool_id.as_str(), shown));
                }
            }
        }
    }
    calls
}

/// The entry type and body of one record, or `None` for records that are not
/// conversation entries.
fn entry(event: &EventEnvelopeV1, calls: &Calls<'_>) -> Option<(&'static str, String)> {
    match &event.payload {
        EventV1::UserMessageSubmitted(message) => {
            Some(("message", format!("### user\n\n{}", message.text)))
        }
        EventV1::RuntimeReminder(reminder) => Some((
            "custom_message",
            format!("### user ({})\n\n{}", reminder.kind.as_str(), reminder.text),
        )),
        EventV1::AssistantMessageFinished(finished) => {
            Some(("message", assistant(&finished.parts)))
        }
        EventV1::ToolCallFinished(finished) => {
            let (name, shown) = calls.get(finished.tool_call_id.as_str())?;
            let failed = finished.status == ToolCallStatus::Failed;
            let text = finished.output_summary.clone().unwrap_or_else(|| {
                if failed { "Tool execution failed." } else { "Tool completed." }.to_owned()
            });
            let images = finished.metadata.as_ref().map_or(0, |metadata| {
                metadata
                    .attachments
                    .iter()
                    .filter(|a| a.mime.starts_with("image/"))
                    .count()
            });
            let mut body = format!(
                "### tool result: {name} ({shown}){}\n\n{text}",
                if failed { " [error]" } else { "" }
            );
            for _ in 0..images {
                body.push_str("\n\n[image]");
            }
            Some(("message", body))
        }
        EventV1::SessionCompaction(compaction) => Some((
            "compaction",
            format!(
                "> Context window boundary: compaction\n\nFirst kept entry: {}\nTokens before: {}\n\n{}",
                compaction.first_kept_event_seq, compaction.tokens_before, compaction.summary
            ),
        )),
        EventV1::BranchSummary(branch) => Some((
            "branch_summary",
            format!("From entry: {}\n\n{}", branch.from_event_seq, branch.summary),
        )),
        EventV1::ContextNotes(notes) => Some((
            "custom",
            fenced(
                "json",
                &serde_json::to_string_pretty(&serde_json::json!({
                    "customType": "context_notes",
                    "data": {"version": notes.version, "text": notes.text},
                }))
                .unwrap_or_default(),
            ),
        )),
        _ => None,
    }
}

fn assistant(parts: &[AssistantPart]) -> String {
    let mut blocks = vec!["### assistant".to_owned()];
    for part in parts {
        match part {
            AssistantPart::Text { text } if !text.is_empty() => blocks.push(text.clone()),
            AssistantPart::Text { .. } => {}
            AssistantPart::Reasoning { text } => {
                blocks.push(format!("<thinking>\n{text}\n</thinking>"));
            }
            AssistantPart::ToolCall(call) => {
                let shown = call
                    .provider_tool_call_id
                    .as_deref()
                    .unwrap_or_else(|| call.tool_call_id.as_str());
                let arguments = serde_json::from_str::<serde_json::Value>(&call.args_summary)
                    .ok()
                    .and_then(|value| serde_json::to_string_pretty(&value).ok());
                blocks.push(format!(
                    "#### tool call: {} ({shown})\n\n{}",
                    call.tool_id,
                    fenced(
                        if arguments.is_some() { "json" } else { "text" },
                        arguments.as_deref().unwrap_or(&call.args_summary),
                    )
                ));
            }
        }
    }
    blocks.join("\n\n")
}

/// A code fence long enough that nothing inside can close it.
fn fenced(language: &str, body: &str) -> String {
    let longest = body
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}{language}\n{body}\n{fence}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::{
        event::{
            ActorKind, AssistantMessageFinishedEvent, BranchSummaryEvent, ContextNotesEvent,
            EventActor, SessionCompactionEvent, ToolCallFinishedEvent, UserMessageSubmittedEvent,
            SCHEMA_VERSION,
        },
        session::AssistantToolCall,
    };

    fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("e{seq}"),
            seq,
            run_id: "run".into(),
            mono_ms: seq,
            ts: Some(format!("2026-10-14T12:00:{seq:02}.000Z")),
            actor: EventActor::new(ActorKind::Worker, Some("agent".into())),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    fn branch() -> Vec<EventEnvelopeV1> {
        vec![
            envelope(
                1,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req".into(),
                    text: "Implement the task".into(),
                }),
            ),
            envelope(
                2,
                EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                    request_id: "req".into(),
                    tool_call_count: 1,
                    parts: vec![
                        AssistantPart::Reasoning {
                            text: "plan".into(),
                        },
                        AssistantPart::Text {
                            text: "Reading.".into(),
                        },
                        AssistantPart::ToolCall(AssistantToolCall {
                            tool_call_id: "call-1".into(),
                            provider_tool_call_id: Some("provider-1".into()),
                            tool_id: "read".into(),
                            args_summary: r#"{"filePath":"a```b"}"#.into(),
                            args_digest: String::new(),
                            provider_call_id: None,
                        }),
                    ],
                    provenance: None,
                    assistant_message: None,
                }),
            ),
            envelope(
                3,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "call-1".into(),
                    status: ToolCallStatus::Failed,
                    output_summary: Some("no such file".into()),
                    output_digest: None,
                    output_json: None,
                    metadata: None,
                }),
            ),
            // A nested call no assistant message made is not a conversation entry.
            envelope(
                4,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "nested".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("hidden".into()),
                    output_digest: None,
                    output_json: None,
                    metadata: None,
                }),
            ),
            envelope(
                5,
                EventV1::SessionCompaction(SessionCompactionEvent {
                    agent_id: "agent".into(),
                    summary: "Fixed notice.".into(),
                    first_kept_event_seq: 2,
                    tokens_before: 1234,
                    ..Default::default()
                }),
            ),
            envelope(
                6,
                EventV1::ContextNotes(ContextNotesEvent {
                    agent_id: "agent".into(),
                    version: 1,
                    text: "Notes".into(),
                }),
            ),
            envelope(
                7,
                EventV1::BranchSummary(BranchSummaryEvent {
                    agent_id: "agent".into(),
                    summary: "Elsewhere.".into(),
                    from_event_seq: 3,
                    ..Default::default()
                }),
            ),
        ]
    }

    #[test]
    fn the_branch_renders_every_entry_with_boundaries_and_chained_parents() {
        let expected = r#"# Current branch — full history

Source: caller-bound live session branch. Context-window boundaries are retained below; this view never uses a registry or on-disk fallback.

## Entry 1 · message

Parent: root
Timestamp: 2026-10-14T12:00:01.000Z

### user

Implement the task

## Entry 2 · message

Parent: 1
Timestamp: 2026-10-14T12:00:02.000Z

### assistant

<thinking>
plan
</thinking>

Reading.

#### tool call: read (provider-1)

````json
{
  "filePath": "a```b"
}
````

## Entry 3 · message

Parent: 2
Timestamp: 2026-10-14T12:00:03.000Z

### tool result: read (provider-1) [error]

no such file

## Entry 5 · compaction

Parent: 3
Timestamp: 2026-10-14T12:00:05.000Z

> Context window boundary: compaction

First kept entry: 2
Tokens before: 1234

Fixed notice.

## Entry 6 · custom

Parent: 5
Timestamp: 2026-10-14T12:00:06.000Z

```json
{
  "customType": "context_notes",
  "data": {
    "text": "Notes",
    "version": 1
  }
}
```

## Entry 7 · branch_summary

Parent: 6
Timestamp: 2026-10-14T12:00:07.000Z

From entry: 3

Elsewhere.
"#;
        assert_eq!(render_full_history(&branch()), expected);
    }

    #[test]
    fn an_empty_branch_renders_only_the_header() {
        let text = render_full_history(&[]);
        assert!(text.starts_with("# Current branch — full history\n\nSource:"));
        assert!(text.ends_with("fallback.\n"));
    }
}

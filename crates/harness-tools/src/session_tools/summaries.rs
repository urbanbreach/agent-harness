// allow: SIZE_OK — session management (lineage + projection + inspection)
use std::collections::BTreeMap;

use harness_core::event::{AssistantMessageFinishedEvent, EventEnvelopeV1, EventV1};
use harness_core::session::AssistantPart;
use serde_json::{json, Value};

use super::SessionEntry;

pub(super) fn safe_event_summary(event: &EventEnvelopeV1) -> Value {
    match &event.payload {
        EventV1::RunStarted(data) => json!({
            "seq": event.seq,
            "event_id": event.event_id,
            "event_type": "run_started",
            "run_id": event.run_id,
            "mono_ms": event.mono_ms,
            "summary": data.run_name,
            "workspace_root": data.workspace_root,
        }),
        EventV1::SessionTitleUpdated(data) => basic_summary(event, "session_title", &data.title),
        EventV1::RunFinished(data) => basic_summary(event, "run_finished", &data.summary),
        EventV1::RunFailed(data) => basic_summary(event, "run_failed", &data.error),
        EventV1::AgentSpawned(data) => json!({
            "seq": event.seq,
            "event_id": event.event_id,
            "event_type": "agent_spawned",
            "agent_id": data.agent_id,
            "profile": data.profile,
            "parent_agent_id": data.parent_agent_id,
        }),
        EventV1::UserMessageSubmitted(data) => basic_summary(event, "user_message", &data.text),
        EventV1::ProviderRequestStarted(data) => json!({
            "seq": event.seq,
            "event_id": event.event_id,
            "event_type": "provider_request_started",
            "request_id": data.request_id,
            "provider_id": data.provider_id,
            "model_id": data.model_id,
            "summary": data.prompt_summary,
        }),
        EventV1::AssistantMessageFinished(data) => json!({
            "seq": event.seq,
            "event_id": event.event_id,
            "event_type": "assistant_message_finished",
            "request_id": data.request_id,
            "tool_call_count": data.tool_call_count,
            "text": committed_assistant_text(data),
            "assistant_message_present": data.assistant_message.is_some(),
            "assistant_message_metadata": data.assistant_message,
        }),
        EventV1::ToolCallRequested(data) => json!({
            "seq": event.seq,
            "event_id": event.event_id,
            "event_type": "tool_call_requested",
            "tool_call_id": data.tool_call_id,
            "tool_id": data.tool_id,
            "summary": data.args_summary,
            "metadata": data.metadata,
        }),
        EventV1::ToolCallFinished(data) => json!({
            "seq": event.seq,
            "event_id": event.event_id,
            "event_type": "tool_call_finished",
            "tool_call_id": data.tool_call_id,
            "status": data.status,
            "summary": data.output_summary,
            "output_digest": data.output_digest,
            "output_json_present": data.output_json.is_some(),
            "metadata": data.metadata,
        }),
        EventV1::TaskCompleted(data) => {
            basic_summary(event, "task_completed", &data.result_summary)
        }
        EventV1::TaskCancelled(data) => basic_summary(event, "task_cancelled", &data.reason),
        EventV1::TaskResultLate(data) => json!({
            "seq": event.seq,
            "event_id": event.event_id,
            "event_type": "task_result_late",
            "task_id": data.task_id,
            "result_digest": data.result_digest,
        }),
        EventV1::ArtifactWritten(data) => json!({
            "seq": event.seq,
            "event_id": event.event_id,
            "event_type": "artifact_written",
            "path": data.path,
            "digest": data.digest,
            "bytes": data.bytes,
            "tool_call_id": data.tool_call_id,
        }),
        other => json!({
            "seq": event.seq,
            "event_id": event.event_id,
            "event_type": event_type_label(other),
            "run_id": event.run_id,
            "mono_ms": event.mono_ms,
        }),
    }
}

pub(super) fn safe_message_summaries(entry: &SessionEntry) -> Vec<Value> {
    entry
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.payload,
                EventV1::UserMessageSubmitted(_) | EventV1::AssistantMessageFinished(_)
            )
        })
        .map(safe_event_summary)
        .collect()
}

fn committed_assistant_text(data: &AssistantMessageFinishedEvent) -> String {
    data.parts
        .iter()
        .filter_map(|part| match part {
            AssistantPart::Text { text } => Some(text.as_str()),
            AssistantPart::Reasoning { .. } | AssistantPart::ToolCall(_) => None,
        })
        .collect()
}

fn basic_summary(event: &EventEnvelopeV1, event_type: &str, summary: &str) -> Value {
    json!({
        "seq": event.seq,
        "event_id": event.event_id,
        "event_type": event_type,
        "run_id": event.run_id,
        "mono_ms": event.mono_ms,
        "summary": summary,
    })
}

#[derive(Debug)]
pub(super) struct SearchDocument {
    pub(super) seq: u64,
    pub(super) event_id: String,
    pub(super) field: &'static str,
    pub(super) text: String,
}

pub(super) fn safe_search_documents(entry: &SessionEntry) -> Vec<SearchDocument> {
    let mut documents = Vec::new();
    if let Some(title) = entry.catalog.run_name.as_ref() {
        documents.push(SearchDocument {
            seq: 0,
            event_id: "catalog".to_string(),
            field: "title",
            text: title.clone(),
        });
    }
    for event in &entry.events {
        match &event.payload {
            EventV1::UserMessageSubmitted(data) => {
                documents.push(search_doc(event, "user_message", &data.text))
            }
            EventV1::SessionTitleUpdated(data) => {
                documents.push(search_doc(event, "title", &data.title))
            }
            EventV1::ProviderRequestStarted(data) => {
                documents.push(search_doc(
                    event,
                    "provider_prompt_summary",
                    &data.prompt_summary,
                ));
            }
            EventV1::AssistantMessageFinished(data) => {
                let text = committed_assistant_text(data);
                if !text.is_empty() {
                    documents.push(search_doc(event, "assistant_message", &text));
                }
            }
            EventV1::ToolCallRequested(data) => {
                documents.push(search_doc(event, "tool_args_summary", &data.args_summary))
            }
            EventV1::ToolCallFinished(data) => {
                if let Some(summary) = data.output_summary.as_deref() {
                    documents.push(search_doc(event, "tool_output_summary", summary));
                }
            }
            EventV1::RunFinished(data) => {
                documents.push(search_doc(event, "run_summary", &data.summary))
            }
            EventV1::RunFailed(data) => documents.push(search_doc(event, "run_error", &data.error)),
            _ => {}
        }
    }
    documents
}

fn search_doc(event: &EventEnvelopeV1, field: &'static str, text: &str) -> SearchDocument {
    SearchDocument {
        seq: event.seq,
        event_id: event.event_id.clone(),
        field,
        text: text.to_string(),
    }
}

pub(super) fn search_excerpt(
    haystack: &str,
    needle: &str,
    case_sensitive: bool,
    context_limit: usize,
) -> Option<String> {
    let haystack_search = if case_sensitive {
        haystack.to_string()
    } else {
        haystack.to_ascii_lowercase()
    };
    let needle_search = if case_sensitive {
        needle.to_string()
    } else {
        needle.to_ascii_lowercase()
    };
    let index = haystack_search.find(&needle_search)?;
    let match_end = index.saturating_add(needle_search.len());
    let start = floor_char_boundary(haystack, index.saturating_sub(context_limit));
    let end = ceil_char_boundary(haystack, match_end.saturating_add(context_limit));
    Some(haystack[start..end].to_string())
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

pub(super) fn event_counts_by_type(events: &[EventEnvelopeV1]) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for event in events {
        *counts
            .entry(event_type_label(&event.payload).to_string())
            .or_insert(0) += 1;
    }
    counts
}

#[allow(
    deprecated,
    reason = "deprecated event variants kept for backward compatibility with existing session logs"
)]
fn event_type_label(event: &EventV1) -> &'static str {
    match event {
        EventV1::RunStarted(_) => "run_started",
        EventV1::SessionTitleUpdated(_) => "session_title_updated",
        EventV1::RunFinished(_) => "run_finished",
        EventV1::RunFailed(_) => "run_failed",
        EventV1::AgentSpawned(_) => "agent_spawned",
        EventV1::AgentStopped(_) => "agent_stopped",
        EventV1::TaskScheduled(_) => "task_scheduled",
        EventV1::TaskCancelled(_) => "task_cancelled",
        EventV1::TaskCompleted(_) => "task_completed",
        EventV1::TaskResultLate(_) => "task_result_late",
        EventV1::BackgroundTaskNotification(_) => "background_task_notification",
        EventV1::StaleDetected(_) => "stale_detected",
        EventV1::UserMessageSubmitted(_) => "user_message_submitted",
        EventV1::PromptAttachmentsSubmitted(_) => "prompt_attachments_submitted",
        EventV1::ProviderRequestStarted(_) => "provider_request_started",
        EventV1::ProviderStreamDelta(_) => "provider_stream_delta",
        EventV1::ProviderReasoningDelta(_) => "provider_reasoning_delta",
        EventV1::ProviderRequestFinished(_) => "provider_request_finished",
        EventV1::AssistantMessageFinished(_) => "assistant_message_finished",
        EventV1::CompactionRequested(_) => "compaction_requested",
        EventV1::CompactionWritten(_) => "compaction_written",
        EventV1::CompactionApplied(_) => "compaction_applied",
        EventV1::CompactionFailed(_) => "compaction_failed",
        EventV1::ToolCallRequested(_) => "tool_call_requested",
        EventV1::ToolCallStarted(_) => "tool_call_started",
        EventV1::ToolCallFinished(_) => "tool_call_finished",
        EventV1::PermissionRequested(_) => "permission_requested",
        EventV1::PermissionGrantRecorded(_) => "permission_grant_recorded",
        EventV1::PermissionResolved(_) => "permission_resolved",
        EventV1::EditProposed(_) => "edit_proposed",
        EventV1::EditApplied(_) => "edit_applied",
        EventV1::EditRejected(_) => "edit_rejected",
        EventV1::ArtifactWritten(_) => "artifact_written",
        EventV1::PolicyViolationDetected(_) => "policy_violation_detected",
        EventV1::UiIntentReceived(_) => "ui_intent_received",
        EventV1::WorkspaceSnapshot(_) => "workspace_snapshot",
        EventV1::WorkspaceReverted(_) => "workspace_reverted",
        EventV1::SessionCompaction(_) => "session_compaction",
        EventV1::BranchSummary(_) => "branch_summary",
    }
}

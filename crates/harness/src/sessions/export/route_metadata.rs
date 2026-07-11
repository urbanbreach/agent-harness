use std::collections::BTreeSet;

use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::proj::SessionCatalogEntry;
use serde_json::{json, Value};

use crate::cli_labels::provider_model_label;
use crate::replay::ReplaySummary;

use super::SessionExportRouteMetadata;

pub(super) fn session_export_route_metadata(
    events: &[EventEnvelopeV1],
    replay: &ReplaySummary,
    catalog: &SessionCatalogEntry,
) -> Vec<SessionExportRouteMetadata> {
    let mut metadata = vec![session_replay_route_metadata(events, replay, catalog)];
    metadata.extend(events.iter().filter_map(|event| {
        let EventV1::ToolCallFinished(payload) = &event.payload else {
            return None;
        };
        let output_json = payload.output_json.as_ref()?;
        let route = output_json.get("route")?.clone();
        Some(SessionExportRouteMetadata {
            source: "task_output".to_string(),
            tool_call_id: Some(payload.tool_call_id.to_string()),
            child_session_id: output_string_field(output_json, "child_session_id")
                .or_else(|| output_string_field(output_json, "session_id"))
                .or_else(|| output_string_field(output_json, "task_id")),
            child_request_id: output_string_field(output_json, "child_request_id")
                .or_else(|| output_string_field(output_json, "request_id")),
            route,
        })
    }));
    metadata
}

fn session_replay_route_metadata(
    events: &[EventEnvelopeV1],
    replay: &ReplaySummary,
    catalog: &SessionCatalogEntry,
) -> SessionExportRouteMetadata {
    let mut profiles = BTreeSet::new();
    let mut provider_models = BTreeSet::new();
    let mut tools = Vec::new();
    let mut permissions = Vec::new();

    for event in events {
        match &event.payload {
            EventV1::AgentSpawned(payload) => {
                profiles.insert(payload.profile.clone());
            }
            EventV1::ProviderRequestStarted(payload) => {
                if let Some(label) =
                    provider_model_label(Some(&payload.provider_id), Some(&payload.model_id))
                {
                    provider_models.insert(label);
                }
            }
            EventV1::ToolCallRequested(payload) => {
                tools.push(json!({
                    "tool_call_id": payload.tool_call_id,
                    "tool_id": payload.tool_id,
                    "seq": event.seq,
                }));
            }
            EventV1::PermissionRequested(payload) => {
                permissions.push(json!({
                    "permission_id": payload.permission_id,
                    "kind": payload.kind,
                    "tool_call_id": payload.tool_call_id,
                    "default_decision": payload.default_decision,
                    "seq": event.seq,
                }));
            }
            EventV1::PermissionResolved(payload) => {
                permissions.push(json!({
                    "permission_id": payload.permission_id,
                    "decision": payload.decision,
                    "reason": payload.reason,
                    "seq": event.seq,
                }));
            }
            EventV1::WorkspaceReverted(_)
            | EventV1::RunStarted(_)
            | EventV1::SessionTitleUpdated(_)
            | EventV1::RunFinished(_)
            | EventV1::RunFailed(_)
            | EventV1::AgentStopped(_)
            | EventV1::TaskScheduled(_)
            | EventV1::TaskCancelled(_)
            | EventV1::TaskCompleted(_)
            | EventV1::TaskResultLate(_)
            | EventV1::BackgroundTaskNotification(_)
            | EventV1::StaleDetected(_)
            | EventV1::UserMessageSubmitted(_)
            | EventV1::ProviderStreamDelta(_)
            | EventV1::ProviderReasoningDelta(_)
            | EventV1::ProviderRequestFinished(_)
            | EventV1::AssistantMessageFinished(_)
            | EventV1::CompactionRequested(_)
            | EventV1::CompactionWritten(_)
            | EventV1::CompactionApplied(_)
            | EventV1::CompactionFailed(_)
            | EventV1::ToolCallStarted(_)
            | EventV1::ToolCallFinished(_)
            | EventV1::PermissionGrantRecorded(_)
            | EventV1::EditProposed(_)
            | EventV1::EditApplied(_)
            | EventV1::EditRejected(_)
            | EventV1::ArtifactWritten(_)
            | EventV1::PolicyViolationDetected(_)
            | EventV1::UiIntentReceived(_)
            | EventV1::WorkspaceSnapshot(_) => {}
        }
    }

    SessionExportRouteMetadata {
        source: "session_replay".to_string(),
        tool_call_id: None,
        child_session_id: None,
        child_request_id: None,
        route: json!({
            "run_id": replay.run_id,
            "run_name": replay.run_name,
            "status": replay.status,
            "mode_source": catalog.mode_source,
            "profile_preset": catalog.profile_preset,
            "provider_model": catalog.provider_model,
            "profiles": profiles.into_iter().collect::<Vec<_>>(),
            "provider_models": provider_models.into_iter().collect::<Vec<_>>(),
            "tools": tools,
            "permissions": permissions,
            "artifact_count": replay.artifact_count,
            "child_session_count": replay.child_session_count,
            "is_resumable": replay.is_resumable,
            "resume_disabled_reason": replay.resume_disabled_reason,
        }),
    }
}

fn output_string_field(output_json: &Value, key: &str) -> Option<String> {
    output_json
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

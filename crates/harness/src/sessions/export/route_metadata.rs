use std::collections::BTreeSet;

use harness_core::proj::SessionCatalogEntry;
use harness_core::session::CanonicalSessionProjection;
use harness_core::transcript_projection::{ProjectedPart, ProjectedPermissionState};
use serde_json::{json, Value};

use crate::cli_labels::provider_model_label;
use crate::replay::ReplaySummary;

use super::SessionExportRouteMetadata;

pub(super) fn session_export_route_metadata(
    projection: &CanonicalSessionProjection,
    replay: &ReplaySummary,
    catalog: &SessionCatalogEntry,
) -> Vec<SessionExportRouteMetadata> {
    let mut metadata = vec![session_replay_route_metadata(projection, replay, catalog)];
    metadata.extend(projected_parts(projection).filter_map(|part| {
        let ProjectedPart::ToolCall(tool_call) = part else {
            return None;
        };
        let output_json = tool_call.output_json.as_ref()?;
        task_output_route_metadata(tool_call.tool_call_id.as_str(), output_json)
    }));
    metadata
}

fn session_replay_route_metadata(
    projection: &CanonicalSessionProjection,
    replay: &ReplaySummary,
    catalog: &SessionCatalogEntry,
) -> SessionExportRouteMetadata {
    let mut profiles = BTreeSet::new();
    let mut provider_models = BTreeSet::new();
    let mut tools = Vec::new();
    let mut permissions = Vec::new();

    profiles.extend(
        projection
            .transcript
            .session
            .agent_profiles
            .values()
            .cloned(),
    );
    provider_models.extend(projection.provider_request_starts().filter_map(|request| {
        provider_model_label(Some(request.provider_id), Some(request.model_id))
    }));
    for message in &projection.transcript.messages {
        if let Some(provider) = message.provider.as_ref() {
            if let Some(label) = provider_model_label(
                provider.provider_id.as_deref(),
                provider.model_id.as_deref(),
            ) {
                provider_models.insert(label);
            }
        }
        for part in &message.parts {
            match part {
                ProjectedPart::ToolCall(payload) => {
                    tools.push(json!({
                        "tool_call_id": payload.tool_call_id,
                        "tool_id": payload.tool_id,
                        "seq": payload.requested_seq,
                    }));
                }
                ProjectedPart::Permission(payload) => {
                    permissions.push(json!({
                        "permission_id": payload.permission_id,
                        "kind": payload.kind,
                        "tool_call_id": payload.tool_call_id,
                        "default_decision": payload.default_decision,
                        "seq": payload.provenance.first_seq,
                    }));
                    if matches!(payload.state, ProjectedPermissionState::Resolved) {
                        permissions.push(json!({
                            "permission_id": payload.permission_id,
                            "decision": payload.decision,
                            "reason": payload.reason,
                            "seq": payload.provenance.last_seq,
                        }));
                    }
                }
                ProjectedPart::Text(_)
                | ProjectedPart::Reasoning(_)
                | ProjectedPart::Compaction(_)
                | ProjectedPart::Artifact(_)
                | ProjectedPart::Lifecycle(_)
                | ProjectedPart::Task(_)
                | ProjectedPart::PolicyViolation(_)
                | ProjectedPart::UiIntent(_) => {}
            }
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

fn projected_parts(
    projection: &CanonicalSessionProjection,
) -> impl Iterator<Item = &ProjectedPart> {
    projection
        .transcript
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
}

fn task_output_route_metadata(
    tool_call_id: &str,
    output_json: &Value,
) -> Option<SessionExportRouteMetadata> {
    Some(SessionExportRouteMetadata {
        source: "task_output".to_string(),
        tool_call_id: Some(tool_call_id.to_string()),
        child_session_id: output_string_field(output_json, "child_session_id")
            .or_else(|| output_string_field(output_json, "session_id"))
            .or_else(|| output_string_field(output_json, "task_id")),
        child_request_id: output_string_field(output_json, "child_request_id")
            .or_else(|| output_string_field(output_json, "request_id")),
        route: output_json.get("route")?.clone(),
    })
}

fn output_string_field(output_json: &Value, key: &str) -> Option<String> {
    output_json
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

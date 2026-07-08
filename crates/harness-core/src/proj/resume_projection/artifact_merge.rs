use std::collections::BTreeMap;

use crate::event::{ArtifactWrittenEvent, ToolCallMetadata};

use super::{ResumeArtifactSnapshot, ResumeToolCallSnapshot};

pub(super) fn merge_session_artifact(
    session_artifacts: &mut BTreeMap<String, ResumeArtifactSnapshot>,
    tool_calls: &BTreeMap<String, ResumeToolCallSnapshot>,
    payload: &ArtifactWrittenEvent,
) {
    let tool_snapshot = payload
        .tool_call_id
        .as_ref()
        .and_then(|tool_call_id| tool_calls.get(tool_call_id.as_str()));
    let lineage = tool_snapshot
        .and_then(|snapshot| snapshot.metadata.as_ref())
        .and_then(|metadata| metadata.lineage.as_ref());
    let tool_id = tool_snapshot
        .and_then(|snapshot| snapshot.tool_id.clone())
        .or_else(|| {
            payload
                .tool_metadata
                .as_ref()
                .and_then(|metadata| metadata.canonical_tool_id.clone())
        });
    let key = artifact_snapshot_key(&payload.path, Some(payload.digest.as_str()));

    session_artifacts
        .entry(key)
        .and_modify(|artifact| {
            if artifact.bytes.is_none() {
                artifact.bytes = Some(payload.bytes);
            }
            if artifact.tool_call_id.is_none() {
                artifact.tool_call_id = payload.tool_call_id.clone();
            }
            if artifact.tool_id.is_none() {
                artifact.tool_id = tool_id.clone();
            }
            if artifact.artifact_kind.is_none() {
                artifact.artifact_kind = payload.metadata.get("artifact_kind").cloned();
            }
            if artifact.summary_contract_version.is_none() {
                artifact.summary_contract_version =
                    metadata_u32(&payload.metadata, "summary_contract_version");
            }
            if artifact.read_file_count.is_none() {
                artifact.read_file_count = metadata_u32(&payload.metadata, "read_file_count");
            }
            if artifact.modified_file_count.is_none() {
                artifact.modified_file_count =
                    metadata_u32(&payload.metadata, "modified_file_count");
            }
            if artifact.parent_tool_call_id.is_none() {
                artifact.parent_tool_call_id =
                    lineage.and_then(|lineage| lineage.parent_tool_call_id.clone());
            }
            if artifact.parent_task_id.is_none() {
                artifact.parent_task_id =
                    lineage.and_then(|lineage| lineage.parent_task_id.clone());
            }
            if artifact.parent_request_id.is_none() {
                artifact.parent_request_id =
                    lineage.and_then(|lineage| lineage.parent_request_id.clone());
            }
            if artifact.child_session_id.is_none() {
                artifact.child_session_id =
                    lineage.and_then(|lineage| lineage.child_session_id.clone());
            }
        })
        .or_insert_with(|| ResumeArtifactSnapshot {
            path: payload.path.clone(),
            digest: Some(payload.digest.clone()),
            bytes: Some(payload.bytes),
            tool_call_id: payload.tool_call_id.clone(),
            tool_id,
            artifact_kind: payload.metadata.get("artifact_kind").cloned(),
            summary_contract_version: metadata_u32(&payload.metadata, "summary_contract_version"),
            read_file_count: metadata_u32(&payload.metadata, "read_file_count"),
            modified_file_count: metadata_u32(&payload.metadata, "modified_file_count"),
            parent_tool_call_id: lineage.and_then(|lineage| lineage.parent_tool_call_id.clone()),
            parent_task_id: lineage.and_then(|lineage| lineage.parent_task_id.clone()),
            parent_request_id: lineage.and_then(|lineage| lineage.parent_request_id.clone()),
            child_session_id: lineage.and_then(|lineage| lineage.child_session_id.clone()),
        });
}

pub(super) fn merge_tool_metadata_artifacts(
    session_artifacts: &mut BTreeMap<String, ResumeArtifactSnapshot>,
    tool_call_id: &str,
    snapshot: &ResumeToolCallSnapshot,
    metadata: &ToolCallMetadata,
) {
    for artifact in &metadata.artifact_refs {
        let key = artifact_snapshot_key(&artifact.path, artifact.digest.as_deref());
        session_artifacts
            .entry(key)
            .and_modify(|entry| {
                if entry.tool_call_id.is_none() {
                    entry.tool_call_id = Some(tool_call_id.into());
                }
                if entry.tool_id.is_none() {
                    entry.tool_id = snapshot.tool_id.clone();
                }
                if entry.parent_tool_call_id.is_none() {
                    entry.parent_tool_call_id = metadata
                        .lineage
                        .as_ref()
                        .and_then(|lineage| lineage.parent_tool_call_id.clone());
                }
                if entry.parent_task_id.is_none() {
                    entry.parent_task_id = metadata
                        .lineage
                        .as_ref()
                        .and_then(|lineage| lineage.parent_task_id.clone());
                }
                if entry.parent_request_id.is_none() {
                    entry.parent_request_id = metadata
                        .lineage
                        .as_ref()
                        .and_then(|lineage| lineage.parent_request_id.clone());
                }
                if entry.child_session_id.is_none() {
                    entry.child_session_id = metadata
                        .lineage
                        .as_ref()
                        .and_then(|lineage| lineage.child_session_id.clone());
                }
            })
            .or_insert_with(|| ResumeArtifactSnapshot {
                path: artifact.path.clone(),
                digest: artifact.digest.clone(),
                bytes: None,
                tool_call_id: Some(tool_call_id.into()),
                tool_id: snapshot.tool_id.clone(),
                artifact_kind: None,
                summary_contract_version: None,
                read_file_count: None,
                modified_file_count: None,
                parent_tool_call_id: metadata
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.parent_tool_call_id.clone()),
                parent_task_id: metadata
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.parent_task_id.clone()),
                parent_request_id: metadata
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.parent_request_id.clone()),
                child_session_id: metadata
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.child_session_id.clone()),
            });
    }
}

fn metadata_u32(metadata: &BTreeMap<String, String>, key: &str) -> Option<u32> {
    metadata.get(key).and_then(|value| value.parse().ok())
}

fn artifact_snapshot_key(path: &str, digest: Option<&str>) -> String {
    let digest = digest.unwrap_or_default();
    format!("{path}\u{001f}{digest}")
}

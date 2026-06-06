use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::event::{EventEnvelopeV1, EventV1};
use crate::path_display::display_path;
use crate::session_paths::META_FILE_NAME;

use super::ChildSessionMaterializationError;
use super::StableSessionPrefix;

#[expect(
    clippy::too_many_arguments,
    reason = "lineage metadata writer keeps source and child identifiers explicit at the call site"
)]
pub(super) fn write_child_metadata(
    source_run_dir: &Path,
    child_run_dir: &Path,
    child_run_id: &str,
    source_run_id: Option<&str>,
    source_cutoff_event_id: Option<&str>,
    source_digest: &str,
    stable_prefix: &StableSessionPrefix,
    events: &[EventEnvelopeV1],
    artifact_count: usize,
) -> Result<(), ChildSessionMaterializationError> {
    let source_metadata = read_source_metadata(source_run_dir)?;
    let source_run_id = source_run_id
        .map(str::to_string)
        .or_else(|| run_dir_name(source_run_dir));
    let workspace_root = first_workspace_root(events)
        .or_else(|| source_metadata.string_field("workspace_root"))
        .unwrap_or_default();
    let run_name = source_run_id
        .as_deref()
        .map(|run_id| format!("Harness child of {run_id}"))
        .unwrap_or_else(|| "Harness child session".to_string());
    let config_digest = source_metadata
        .string_field("config_digest")
        .unwrap_or_else(|| "harness-lineage-materialized".to_string());
    let harness_version = source_metadata
        .string_field("harness_version")
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let recorded_runtime_context = source_metadata
        .0
        .get("recorded_runtime_context")
        .filter(|value| !value.is_null())
        .cloned();
    let created_at = materialization_created_at();
    let metadata = ChildRunMetadata {
        run_id: child_run_id.to_string(),
        run_name,
        workspace_root,
        created_at: Some(created_at.clone()),
        config_digest,
        harness_version,
        recorded_runtime_context,
        harness_lineage: HarnessLineageMetadata {
            harness_operation: "child_session_materialization".to_string(),
            harness_source_run_id: source_run_id.clone(),
            harness_source_cutoff_seq: stable_prefix.cutoff_seq,
            harness_source_cutoff_event_id: source_cutoff_event_id.map(str::to_string),
            harness_source_digest: source_digest.to_string(),
            harness_created_at: created_at,
            relationship: "child_session_materialization".to_string(),
            parent_run_id: source_run_id.clone(),
            source_run_id,
            source_cutoff_seq: stable_prefix.cutoff_seq,
            source_cutoff_event_id: source_cutoff_event_id.map(str::to_string),
            source_digest: source_digest.to_string(),
            source_event_count: stable_prefix.event_count,
            materialized_event_count: events.len(),
            materialized_artifact_count: artifact_count,
            event_rewrite_policy: "Harness child materialization regenerates event_id/run_id/seq, clears correlation_id and causation_id, rewrites only run-scoped stream keys, preserves copied payloads, and terminalizes stable live snapshots with a RunFinished marker.".to_string(),
            artifact_policy: "Harness child materialization copies only artifacts referenced by copied events after byte and digest validation.".to_string(),
        },
    };
    let mut body = serde_json::to_string_pretty(&metadata)
        .map_err(ChildSessionMaterializationError::SerializeMetadata)?;
    body.push('\n');

    let meta_path = child_run_dir.join(META_FILE_NAME);
    fs::write(&meta_path, body).map_err(|source| ChildSessionMaterializationError::WriteMetadata {
        path: display_path(&meta_path),
        source,
    })
}

fn read_source_metadata(
    source_run_dir: &Path,
) -> Result<SourceMetadata, ChildSessionMaterializationError> {
    let meta_path = source_run_dir.join(META_FILE_NAME);
    match fs::read_to_string(&meta_path) {
        Ok(body) => serde_json::from_str(&body)
            .map(SourceMetadata)
            .map_err(|source| ChildSessionMaterializationError::ParseMetadata {
                path: display_path(&meta_path),
                source,
            }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(SourceMetadata::default())
        }
        Err(source) => Err(ChildSessionMaterializationError::ReadMetadata {
            path: display_path(&meta_path),
            source,
        }),
    }
}

fn first_workspace_root(events: &[EventEnvelopeV1]) -> Option<String> {
    events.iter().find_map(|event| match &event.payload {
        EventV1::RunStarted(payload) => Some(payload.workspace_root.clone()),
        _ => None,
    })
}

fn materialization_created_at() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("unix_ms:{millis}")
}

fn run_dir_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

#[derive(Debug, Default)]
struct SourceMetadata(serde_json::Value);

impl SourceMetadata {
    fn string_field(&self, field: &str) -> Option<String> {
        self.0
            .get(field)
            .and_then(|value| value.as_str())
            .map(str::to_string)
    }
}

#[derive(Debug, Serialize)]
struct ChildRunMetadata {
    run_id: String,
    run_name: String,
    workspace_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
    config_digest: String,
    harness_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recorded_runtime_context: Option<serde_json::Value>,
    harness_lineage: HarnessLineageMetadata,
}

#[derive(Debug, Serialize)]
struct HarnessLineageMetadata {
    harness_operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    harness_source_run_id: Option<String>,
    harness_source_cutoff_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    harness_source_cutoff_event_id: Option<String>,
    harness_source_digest: String,
    harness_created_at: String,
    relationship: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_run_id: Option<String>,
    source_cutoff_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_cutoff_event_id: Option<String>,
    source_digest: String,
    source_event_count: usize,
    materialized_event_count: usize,
    materialized_artifact_count: usize,
    event_rewrite_policy: String,
    artifact_policy: String,
}

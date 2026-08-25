use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::agent::ProviderContextCheckpoint;
use crate::event::{EventEnvelopeV1, EventV1};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LegacyCompactionLifecycle {
    Started(String),
    Finished(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyCheckpointRecord {
    pub(crate) checkpoint_id: String,
    pub(crate) artifact_path: String,
    pub(crate) through_seq: u64,
    pub(crate) through_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyCheckpointArtifactRef {
    pub(crate) path: String,
    pub(crate) digest: Option<String>,
    pub(crate) bytes: u64,
}

#[derive(Debug, Error)]
pub(crate) enum LegacyCheckpointError {
    #[error(
        "compaction checkpoint `{checkpoint_id}` was applied without a matching written event"
    )]
    AppliedWithoutWritten { checkpoint_id: String },
    #[error(
        "compaction checkpoint `{checkpoint_id}` agent mismatch between applied `{applied_agent_id}` and written `{written_agent_id}`"
    )]
    AgentMismatch {
        checkpoint_id: String,
        applied_agent_id: String,
        written_agent_id: String,
    },
    #[error("failed to read checkpoint artifact {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid checkpoint artifact {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[expect(
    deprecated,
    reason = "the single legacy adapter retains read-only V1 compaction decoding until G010"
)]
pub(crate) fn compaction_lifecycle(event: &EventV1) -> Option<LegacyCompactionLifecycle> {
    match event {
        EventV1::CompactionRequested(payload) => Some(LegacyCompactionLifecycle::Started(
            payload.checkpoint_id.clone(),
        )),
        EventV1::CompactionWritten(payload) => Some(LegacyCompactionLifecycle::Finished(Some(
            payload.checkpoint_id.clone(),
        ))),
        EventV1::CompactionApplied(payload) => Some(LegacyCompactionLifecycle::Finished(Some(
            payload.checkpoint_id.clone(),
        ))),
        EventV1::CompactionFailed(payload) => Some(LegacyCompactionLifecycle::Finished(
            payload.checkpoint_id.clone(),
        )),
        _ => None,
    }
}

#[expect(
    deprecated,
    reason = "the single legacy adapter retains read-only V1 compaction decoding until G010"
)]
pub(crate) fn event_type_name(event: &EventV1) -> Option<&'static str> {
    match event {
        EventV1::CompactionRequested(_) => Some("compaction_requested"),
        EventV1::CompactionWritten(_) => Some("compaction_written"),
        EventV1::CompactionApplied(_) => Some("compaction_applied"),
        EventV1::CompactionFailed(_) => Some("compaction_failed"),
        _ => None,
    }
}

pub(crate) fn is_compaction_event(event: &EventV1) -> bool {
    event_type_name(event).is_some()
}

#[expect(
    deprecated,
    reason = "the single legacy adapter retains read-only V1 compaction decoding until G010"
)]
pub(crate) fn checkpoint_artifact(event: &EventV1) -> Option<LegacyCheckpointArtifactRef> {
    let EventV1::CompactionWritten(payload) = event else {
        return None;
    };
    Some(LegacyCheckpointArtifactRef {
        path: payload.artifact_path.clone(),
        digest: payload.artifact_digest.clone(),
        bytes: payload.artifact_bytes,
    })
}

#[expect(
    deprecated,
    reason = "the single legacy adapter retains read-only V1 compaction decoding until G010"
)]
pub(crate) fn discover_applied_checkpoints(
    events: &[EventEnvelopeV1],
) -> Result<BTreeMap<String, LegacyCheckpointRecord>, LegacyCheckpointError> {
    let mut written_by_id = BTreeMap::new();
    let mut latest_applied_by_agent: BTreeMap<String, String> = BTreeMap::new();
    for event in events {
        match &event.payload {
            EventV1::CompactionWritten(payload) => {
                written_by_id.insert(payload.checkpoint_id.clone(), payload.clone());
            }
            EventV1::CompactionApplied(payload) => {
                latest_applied_by_agent
                    .insert(payload.agent_id.clone(), payload.checkpoint_id.clone());
            }
            _ => {}
        }
    }

    latest_applied_by_agent
        .into_iter()
        .map(|(agent_id, checkpoint_id)| {
            let written = written_by_id.get(&checkpoint_id).ok_or_else(|| {
                LegacyCheckpointError::AppliedWithoutWritten {
                    checkpoint_id: checkpoint_id.clone(),
                }
            })?;
            if written.agent_id != agent_id {
                return Err(LegacyCheckpointError::AgentMismatch {
                    checkpoint_id,
                    applied_agent_id: agent_id,
                    written_agent_id: written.agent_id.clone(),
                });
            }
            Ok((
                agent_id,
                LegacyCheckpointRecord {
                    checkpoint_id,
                    artifact_path: written.artifact_path.clone(),
                    through_seq: written.through_seq,
                    through_request_id: written.through_request_id.clone(),
                },
            ))
        })
        .collect()
}

pub(crate) fn load_checkpoint(
    run_dir: &Path,
    checkpoint: &LegacyCheckpointRecord,
) -> Result<ProviderContextCheckpoint, LegacyCheckpointError> {
    let path = run_dir.join(&checkpoint.artifact_path);
    let body = fs::read_to_string(&path).map_err(|source| LegacyCheckpointError::Read {
        path: path.clone(),
        source,
    })?;
    serde_json::from_str(&body).map_err(|source| LegacyCheckpointError::Parse { path, source })
}

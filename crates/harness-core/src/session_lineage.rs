//! Pure helpers for session lineage operations.
//!
//! Contract:
//! - `fork = selected stable prefix`
//! - `clone = latest stable prefix`
//! - `tree = read-only lineage browser`

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event::{EventEnvelopeV1, EventV1, SCHEMA_VERSION};
use crate::proj::{RunStatus, SessionCatalogEntry};

const EVENTS_FILE_NAME: &str = "events.jsonl";
const META_FILE_NAME: &str = "meta.json";
const WRITER_LOCK_FILE_NAME: &str = ".writer.lock";
const ARTIFACTS_DIR_NAME: &str = "artifacts";
const CHILD_RUN_ID_PREFIX: &str = "run_harness_child";

static CHILD_RUN_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionLineageError {
    #[error("event log must start at seq=1 and remain contiguous: expected seq {expected}, got {actual}")]
    NonContiguousSeq { expected: u64, actual: u64 },
    #[error("event at seq {seq} uses schema_version {actual}; expected schema_version {expected}")]
    UnsupportedSchemaVersion {
        seq: u64,
        expected: u16,
        actual: u16,
    },
    #[error("events contain multiple run ids: expected `{expected}`, got `{actual}` at seq {seq}")]
    RunIdMismatch {
        expected: String,
        actual: String,
        seq: u64,
    },
    #[error("stable prefix cutoff seq {cutoff_seq} is outside event log range 0..={max_seq}")]
    CutoffOutOfRange { cutoff_seq: u64, max_seq: u64 },
    #[error("prefix ending at seq {cutoff_seq} is unstable: {reason}")]
    UnstablePrefix { cutoff_seq: u64, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableSessionPrefix {
    pub cutoff_seq: u64,
    pub event_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<RunStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildSessionMaterializationSourceKind {
    /// Source events came from the run directory on disk; an active writer lock must reject the clone.
    DiskRunDirectory,
    /// Source events came from a TUI-owned stable in-memory snapshot; the disk writer may still be live.
    TuiStableInMemorySnapshot,
}

#[derive(Debug, Clone, Copy)]
pub struct ChildSessionMaterializationRequest<'a> {
    pub source_run_dir: &'a Path,
    pub events: &'a [EventEnvelopeV1],
    pub stable_prefix: &'a StableSessionPrefix,
    pub source_kind: ChildSessionMaterializationSourceKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildSessionMaterializationResult {
    pub child_run_id: String,
    pub child_run_dir: PathBuf,
    pub source_run_id: Option<String>,
    pub source_cutoff_seq: u64,
    pub event_count: usize,
    pub artifact_count: usize,
}

#[derive(Debug, Error)]
pub enum ChildSessionMaterializationError {
    #[error(transparent)]
    StablePrefix(#[from] SessionLineageError),
    #[error("provided stable prefix does not match validation result for cutoff seq {cutoff_seq}")]
    StablePrefixMismatch { cutoff_seq: u64 },
    #[error("source run directory does not exist: {path}")]
    SourceRunDirectoryMissing { path: String },
    #[error("source run directory has no session-directory parent: {path}")]
    SourceRunDirectoryHasNoParent { path: String },
    #[error("source run is actively writer-locked: {path}")]
    SourceWriterLocked { path: String },
    #[error("failed to create temporary child run directory {path}: {source}")]
    CreateTempRunDirectory {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to create child run directory {path}: {source}")]
    CreateRunDirectory {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write child events {path}: {source}")]
    WriteEvents {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read source events {path}: {source}")]
    ReadSourceEvents {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse source event {path} at line {line}: {source}")]
    ParseSourceEvent {
        path: String,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("source event log changed while materializing child: {path}")]
    SourceEventLogChanged { path: String },
    #[error("failed to serialize child event envelope at seq {seq}: {source}")]
    SerializeEvent {
        seq: u64,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to read source metadata {path}: {source}")]
    ReadMetadata {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse source metadata {path}: {source}")]
    ParseMetadata {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize child metadata: {0}")]
    SerializeMetadata(#[source] serde_json::Error),
    #[error("failed to write child metadata {path}: {source}")]
    WriteMetadata {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("artifact path must be relative under artifacts/: {path}")]
    InvalidArtifactPath { path: String },
    #[error("artifact path must not traverse symlinks: {path}")]
    ArtifactSymlink { path: String },
    #[error("artifact reference conflicts for {path}: {detail}")]
    ConflictingArtifactReference { path: String, detail: String },
    #[error("referenced artifact is missing: {path}")]
    MissingArtifact { path: String },
    #[error("failed to read referenced artifact {path}: {source}")]
    ReadArtifact {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("artifact {path} has {actual} bytes; expected {expected}")]
    ArtifactByteMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },
    #[error("artifact {path} digest mismatch: expected {expected}, got {actual}")]
    ArtifactDigestMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("failed to create artifact parent directory {path}: {source}")]
    CreateArtifactDirectory {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write child artifact {path}: {source}")]
    WriteArtifact {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to publish child run directory {from} -> {to}: {source}")]
    PublishRunDirectory {
        from: String,
        to: String,
        #[source]
        source: std::io::Error,
    },
}

/// Materialize a fresh child run directory from a validated stable prefix.
///
/// This helper is intentionally non-replay: it writes a new sibling run directory and never mutates
/// the source run. Disk-backed sources reject `.writer.lock`; callers with a TUI-owned stable
/// in-memory snapshot may opt into `TuiStableInMemorySnapshot` because the copied event slice is the
/// stable source of truth, not the still-live file.
pub fn materialize_child_session(
    request: ChildSessionMaterializationRequest<'_>,
) -> Result<ChildSessionMaterializationResult, ChildSessionMaterializationError> {
    materialize_child_session_inner(request, None, || {}, |from, to| fs::rename(from, to))
}

fn materialize_child_session_inner<BeforePublish, Publish>(
    request: ChildSessionMaterializationRequest<'_>,
    path_plan: Option<(String, PathBuf, PathBuf)>,
    before_publish: BeforePublish,
    publish: Publish,
) -> Result<ChildSessionMaterializationResult, ChildSessionMaterializationError>
where
    BeforePublish: FnOnce(),
    Publish: FnOnce(&Path, &Path) -> std::io::Result<()>,
{
    let validated = validate_stable_prefix(request.events, request.stable_prefix.cutoff_seq)?;
    if &validated != request.stable_prefix {
        return Err(ChildSessionMaterializationError::StablePrefixMismatch {
            cutoff_seq: request.stable_prefix.cutoff_seq,
        });
    }

    if !request.source_run_dir.is_dir() {
        return Err(
            ChildSessionMaterializationError::SourceRunDirectoryMissing {
                path: display_path(request.source_run_dir),
            },
        );
    }

    if request.source_kind == ChildSessionMaterializationSourceKind::DiskRunDirectory {
        let lock_path = request.source_run_dir.join(WRITER_LOCK_FILE_NAME);
        if lock_path.exists() {
            return Err(ChildSessionMaterializationError::SourceWriterLocked {
                path: display_path(&lock_path),
            });
        }
    }

    let source_event_log_digest =
        if request.source_kind == ChildSessionMaterializationSourceKind::DiskRunDirectory {
            let digest = event_log_digest(request.events)?;
            ensure_source_event_log_unchanged(request.source_run_dir, &digest)?;
            Some(digest)
        } else {
            None
        };

    let session_dir = request.source_run_dir.parent().ok_or_else(|| {
        ChildSessionMaterializationError::SourceRunDirectoryHasNoParent {
            path: display_path(request.source_run_dir),
        }
    })?;
    let source_run_id = validated.run_id.clone();
    let (child_run_id, child_run_dir, temp_run_dir) =
        path_plan.unwrap_or_else(|| fresh_child_run_paths(session_dir));

    fs::create_dir(&temp_run_dir).map_err(|source| {
        ChildSessionMaterializationError::CreateTempRunDirectory {
            path: display_path(&temp_run_dir),
            source,
        }
    })?;
    let mut pending = PendingChildRun::new(temp_run_dir);

    let source_prefix_events = &request.events[..validated.event_count];
    let source_cutoff_event_id = source_prefix_events
        .last()
        .map(|event| event.event_id.clone());
    let source_digest = source_prefix_digest(source_prefix_events)?;
    let copied_events = rewrite_child_event_prefix(
        source_prefix_events,
        source_run_id.as_deref(),
        &child_run_id,
    );
    let artifact_specs = collect_referenced_artifacts(&copied_events)?;

    write_child_events(pending.path(), &copied_events)?;
    copy_referenced_artifacts(request.source_run_dir, pending.path(), &artifact_specs)?;
    write_child_metadata(
        request.source_run_dir,
        pending.path(),
        &child_run_id,
        source_run_id.as_deref(),
        source_cutoff_event_id.as_deref(),
        &source_digest,
        &validated,
        &copied_events,
        artifact_specs.len(),
    )?;
    before_publish();
    if let Some(digest) = source_event_log_digest.as_deref() {
        ensure_source_event_log_unchanged(request.source_run_dir, digest)?;
    }

    publish(pending.path(), &child_run_dir).map_err(|source| {
        ChildSessionMaterializationError::PublishRunDirectory {
            from: display_path(pending.path()),
            to: display_path(&child_run_dir),
            source,
        }
    })?;
    pending.mark_published();

    Ok(ChildSessionMaterializationResult {
        child_run_id,
        child_run_dir,
        source_run_id,
        source_cutoff_seq: validated.cutoff_seq,
        event_count: copied_events.len(),
        artifact_count: artifact_specs.len(),
    })
}

/// Rewrite one copied envelope for child materialization.
///
/// Policy: payloads, actors, timestamps, and monotonic times remain unchanged so replay observes the
/// same completed work. The envelope gets a fresh child `run_id`, contiguous child-local `seq`, and a
/// new event id derived from that child identity. `correlation_id` and `causation_id` are cleared so
/// the child log cannot imply causal links to the parent run's event ids; stream keys are only
/// rewritten for the run-scoped `run:<source>` key and otherwise preserved.
pub fn rewrite_child_event_envelope(
    source: &EventEnvelopeV1,
    source_run_id: Option<&str>,
    child_run_id: &str,
    child_seq: u64,
) -> EventEnvelopeV1 {
    let mut rewritten = source.clone();
    rewritten.event_id = child_event_id(child_run_id, child_seq);
    rewritten.seq = child_seq;
    rewritten.run_id = child_run_id.to_string();
    rewritten.correlation_id = None;
    rewritten.causation_id = None;
    rewritten.stream_key =
        rewrite_stream_key(source.stream_key.as_deref(), source_run_id, child_run_id);
    rewritten
}

/// Validate the selected stable prefix used by fork operations.
pub fn validate_fork_stable_prefix(
    events: &[EventEnvelopeV1],
    cutoff_seq: u64,
) -> Result<StableSessionPrefix, SessionLineageError> {
    validate_stable_prefix(events, cutoff_seq)
}

/// Select the latest stable prefix used by clone operations.
pub fn latest_clone_stable_prefix(
    events: &[EventEnvelopeV1],
) -> Result<StableSessionPrefix, SessionLineageError> {
    let max_seq = validate_event_log(events)?;

    for cutoff_seq in (1..=max_seq).rev() {
        let state = project_prefix_state(events, cutoff_seq);
        if state.unstable_reason().is_none() {
            return Ok(stable_prefix_from_state(cutoff_seq, &state));
        }
    }

    if events.is_empty() {
        Ok(StableSessionPrefix {
            cutoff_seq: 0,
            event_count: 0,
            run_id: None,
            status: None,
        })
    } else {
        Err(SessionLineageError::UnstablePrefix {
            cutoff_seq: max_seq,
            reason: "no stable completed prefix exists in the source event log".to_string(),
        })
    }
}

/// Validate an explicit stable prefix cutoff.
pub fn validate_stable_prefix(
    events: &[EventEnvelopeV1],
    cutoff_seq: u64,
) -> Result<StableSessionPrefix, SessionLineageError> {
    let max_seq = validate_event_log(events)?;
    if cutoff_seq > max_seq {
        return Err(SessionLineageError::CutoffOutOfRange {
            cutoff_seq,
            max_seq,
        });
    }

    let state = project_prefix_state(events, cutoff_seq);
    if let Some(reason) = state.unstable_reason() {
        return Err(SessionLineageError::UnstablePrefix { cutoff_seq, reason });
    }

    Ok(stable_prefix_from_state(cutoff_seq, &state))
}

fn rewrite_child_event_prefix(
    events: &[EventEnvelopeV1],
    source_run_id: Option<&str>,
    child_run_id: &str,
) -> Vec<EventEnvelopeV1> {
    events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            rewrite_child_event_envelope(event, source_run_id, child_run_id, index as u64 + 1)
        })
        .collect()
}

fn collect_referenced_artifacts(
    events: &[EventEnvelopeV1],
) -> Result<BTreeMap<PathBuf, ArtifactCopySpec>, ChildSessionMaterializationError> {
    let mut specs = BTreeMap::new();
    for event in events {
        match &event.payload {
            EventV1::ArtifactWritten(payload) => merge_artifact_spec(
                &mut specs,
                &payload.path,
                Some(payload.digest.as_str()),
                Some(payload.bytes),
            )?,
            EventV1::CompactionWritten(payload) => merge_artifact_spec(
                &mut specs,
                &payload.artifact_path,
                payload.artifact_digest.as_deref(),
                Some(payload.artifact_bytes),
            )?,
            EventV1::ToolCallRequested(payload) => {
                if let Some(metadata) = payload.metadata.as_ref() {
                    collect_metadata_artifacts(&mut specs, metadata)?;
                }
            }
            EventV1::ToolCallFinished(payload) => {
                if let Some(metadata) = payload.metadata.as_ref() {
                    collect_metadata_artifacts(&mut specs, metadata)?;
                }
            }
            EventV1::RunStarted(_)
            | EventV1::RunFinished(_)
            | EventV1::RunFailed(_)
            | EventV1::AgentSpawned(_)
            | EventV1::AgentStopped(_)
            | EventV1::TaskScheduled(_)
            | EventV1::TaskCancelled(_)
            | EventV1::TaskCompleted(_)
            | EventV1::TaskResultLate(_)
            | EventV1::StaleDetected(_)
            | EventV1::UserMessageSubmitted(_)
            | EventV1::ProviderRequestStarted(_)
            | EventV1::ProviderStreamDelta(_)
            | EventV1::ProviderReasoningDelta(_)
            | EventV1::ProviderRequestFinished(_)
            | EventV1::AssistantMessageFinished(_)
            | EventV1::CompactionRequested(_)
            | EventV1::CompactionApplied(_)
            | EventV1::CompactionFailed(_)
            | EventV1::ToolCallStarted(_)
            | EventV1::PermissionRequested(_)
            | EventV1::PermissionGrantRecorded(_)
            | EventV1::PermissionResolved(_)
            | EventV1::EditProposed(_)
            | EventV1::EditApplied(_)
            | EventV1::EditRejected(_)
            | EventV1::PolicyViolationDetected(_)
            | EventV1::UiIntentReceived(_) => {}
        }
    }
    Ok(specs)
}

fn collect_metadata_artifacts(
    specs: &mut BTreeMap<PathBuf, ArtifactCopySpec>,
    metadata: &crate::event::ToolCallMetadata,
) -> Result<(), ChildSessionMaterializationError> {
    for artifact in &metadata.artifact_refs {
        merge_artifact_spec(specs, &artifact.path, artifact.digest.as_deref(), None)?;
    }
    Ok(())
}

fn merge_artifact_spec(
    specs: &mut BTreeMap<PathBuf, ArtifactCopySpec>,
    path: &str,
    digest: Option<&str>,
    bytes: Option<u64>,
) -> Result<(), ChildSessionMaterializationError> {
    let relative = validate_artifact_path(path)?;
    let digest = digest.map(str::to_string);
    match specs.get_mut(&relative) {
        Some(existing) => {
            if let (Some(existing_digest), Some(new_digest)) = (&existing.digest, &digest) {
                if existing_digest != new_digest {
                    return Err(
                        ChildSessionMaterializationError::ConflictingArtifactReference {
                            path: path.to_string(),
                            detail: format!("digest {existing_digest} conflicts with {new_digest}"),
                        },
                    );
                }
            }
            if existing.digest.is_none() {
                existing.digest = digest;
            }
            if let (Some(existing_bytes), Some(new_bytes)) = (existing.bytes, bytes) {
                if existing_bytes != new_bytes {
                    return Err(
                        ChildSessionMaterializationError::ConflictingArtifactReference {
                            path: path.to_string(),
                            detail: format!("bytes {existing_bytes} conflicts with {new_bytes}"),
                        },
                    );
                }
            }
            if existing.bytes.is_none() {
                existing.bytes = bytes;
            }
        }
        None => {
            specs.insert(relative, ArtifactCopySpec { digest, bytes });
        }
    }
    Ok(())
}

fn validate_artifact_path(path: &str) -> Result<PathBuf, ChildSessionMaterializationError> {
    let relative = Path::new(path);
    if relative.is_absolute() || path.trim().is_empty() {
        return Err(ChildSessionMaterializationError::InvalidArtifactPath {
            path: path.to_string(),
        });
    }

    let mut components = relative.components();
    if components.next() != Some(Component::Normal(OsStr::new(ARTIFACTS_DIR_NAME))) {
        return Err(ChildSessionMaterializationError::InvalidArtifactPath {
            path: path.to_string(),
        });
    }
    if components.any(|component| !matches!(component, Component::Normal(_))) {
        return Err(ChildSessionMaterializationError::InvalidArtifactPath {
            path: path.to_string(),
        });
    }

    Ok(relative.to_path_buf())
}

fn write_child_events(
    run_dir: &Path,
    events: &[EventEnvelopeV1],
) -> Result<(), ChildSessionMaterializationError> {
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).map_err(|source| {
            ChildSessionMaterializationError::SerializeEvent {
                seq: event.seq,
                source,
            }
        })?;
        body.push_str(&line);
        body.push('\n');
    }
    fs::write(&events_path, body).map_err(|source| ChildSessionMaterializationError::WriteEvents {
        path: display_path(&events_path),
        source,
    })
}

fn copy_referenced_artifacts(
    source_run_dir: &Path,
    child_run_dir: &Path,
    specs: &BTreeMap<PathBuf, ArtifactCopySpec>,
) -> Result<(), ChildSessionMaterializationError> {
    for (relative, spec) in specs {
        let source_path = source_run_dir.join(relative);
        ensure_artifact_source_is_regular_file(source_run_dir, relative)?;
        let contents = fs::read(&source_path).map_err(|source| {
            ChildSessionMaterializationError::ReadArtifact {
                path: display_path(&source_path),
                source,
            }
        })?;
        if let Some(expected) = spec.bytes {
            let actual = contents.len() as u64;
            if actual != expected {
                return Err(ChildSessionMaterializationError::ArtifactByteMismatch {
                    path: display_path(&source_path),
                    expected,
                    actual,
                });
            }
        }
        if let Some(expected) = spec.digest.as_ref() {
            let actual = blake3::hash(&contents).to_hex().to_string();
            if !actual.starts_with(expected) {
                return Err(ChildSessionMaterializationError::ArtifactDigestMismatch {
                    path: display_path(&source_path),
                    expected: expected.clone(),
                    actual,
                });
            }
        }

        let child_path = child_run_dir.join(relative);
        if let Some(parent) = child_path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                ChildSessionMaterializationError::CreateArtifactDirectory {
                    path: display_path(parent),
                    source,
                }
            })?;
        }
        fs::write(&child_path, contents).map_err(|source| {
            ChildSessionMaterializationError::WriteArtifact {
                path: display_path(&child_path),
                source,
            }
        })?;
    }
    Ok(())
}

fn ensure_artifact_source_is_regular_file(
    source_run_dir: &Path,
    relative: &Path,
) -> Result<(), ChildSessionMaterializationError> {
    let mut path = source_run_dir.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(part) = component else {
            return Err(ChildSessionMaterializationError::InvalidArtifactPath {
                path: display_path(relative),
            });
        };
        path.push(part);
        let metadata = fs::symlink_metadata(&path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ChildSessionMaterializationError::MissingArtifact {
                    path: display_path(&path),
                }
            } else {
                ChildSessionMaterializationError::ReadArtifact {
                    path: display_path(&path),
                    source,
                }
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(ChildSessionMaterializationError::ArtifactSymlink {
                path: display_path(&path),
            });
        }
        let is_final = components.peek().is_none();
        if (is_final && !metadata.is_file()) || (!is_final && !metadata.is_dir()) {
            return Err(ChildSessionMaterializationError::MissingArtifact {
                path: display_path(&path),
            });
        }
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "lineage metadata writer keeps source and child identifiers explicit at the call site"
)]
fn write_child_metadata(
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
    let created_at = materialization_created_at();
    let metadata = ChildRunMetadata {
        run_id: child_run_id.to_string(),
        run_name,
        workspace_root,
        created_at: Some(created_at.clone()),
        config_digest,
        harness_version,
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
            event_rewrite_policy: "Harness child materialization regenerates event_id/run_id/seq, clears correlation_id and causation_id, rewrites only run-scoped stream keys, and preserves payloads.".to_string(),
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

fn source_prefix_digest(
    source_prefix_events: &[EventEnvelopeV1],
) -> Result<String, ChildSessionMaterializationError> {
    event_log_digest(source_prefix_events)
}

fn event_log_digest(
    events: &[EventEnvelopeV1],
) -> Result<String, ChildSessionMaterializationError> {
    let mut bytes = Vec::new();
    for event in events {
        serde_json::to_writer(&mut bytes, event).map_err(|source| {
            ChildSessionMaterializationError::SerializeEvent {
                seq: event.seq,
                source,
            }
        })?;
        bytes.push(b'\n');
    }
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn ensure_source_event_log_unchanged(
    source_run_dir: &Path,
    expected_digest: &str,
) -> Result<(), ChildSessionMaterializationError> {
    let events_path = source_run_dir.join(EVENTS_FILE_NAME);
    let actual_digest = read_source_event_log_digest(&events_path)?;
    if actual_digest != expected_digest {
        return Err(ChildSessionMaterializationError::SourceEventLogChanged {
            path: display_path(&events_path),
        });
    }
    Ok(())
}

fn read_source_event_log_digest(
    events_path: &Path,
) -> Result<String, ChildSessionMaterializationError> {
    let body = fs::read_to_string(events_path).map_err(|source| {
        ChildSessionMaterializationError::ReadSourceEvents {
            path: display_path(events_path),
            source,
        }
    })?;
    let events = body
        .lines()
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str::<EventEnvelopeV1>(line).map_err(|source| {
                ChildSessionMaterializationError::ParseSourceEvent {
                    path: display_path(events_path),
                    line: index + 1,
                    source,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    event_log_digest(&events)
}

fn materialization_created_at() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("unix_ms:{millis}")
}

fn fresh_child_run_paths(session_dir: &Path) -> (String, PathBuf, PathBuf) {
    loop {
        let child_run_id = fresh_child_run_id();
        let child_run_dir = session_dir.join(&child_run_id);
        let temp_run_dir = sibling_temp_run_dir(session_dir, &child_run_id);
        if !child_run_dir.exists() && !temp_run_dir.exists() {
            return (child_run_id, child_run_dir, temp_run_dir);
        }
    }
}

fn fresh_child_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = CHILD_RUN_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    format!("{CHILD_RUN_ID_PREFIX}_{nanos:x}_{counter:04}")
}

fn sibling_temp_run_dir(session_dir: &Path, child_run_id: &str) -> PathBuf {
    let counter = CHILD_RUN_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    session_dir.join(format!(".{child_run_id}.tmp-{counter:04}"))
}

fn child_event_id(child_run_id: &str, child_seq: u64) -> String {
    format!("evt-{child_run_id}-{child_seq:020}")
}

fn rewrite_stream_key(
    stream_key: Option<&str>,
    source_run_id: Option<&str>,
    child_run_id: &str,
) -> Option<String> {
    let stream_key = stream_key?;
    if source_run_id.is_some_and(|source_run_id| stream_key == format!("run:{source_run_id}")) {
        return Some(format!("run:{child_run_id}"));
    }
    Some(stream_key.to_string())
}

fn run_dir_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtifactCopySpec {
    digest: Option<String>,
    bytes: Option<u64>,
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

struct PendingChildRun {
    path: PathBuf,
    published: bool,
}

impl PendingChildRun {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            published: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn mark_published(&mut self) {
        self.published = true;
    }
}

impl Drop for PendingChildRun {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionLineageTree {
    pub roots: Vec<SessionLineageNode>,
}

impl SessionLineageTree {
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn len(&self) -> usize {
        self.roots.iter().map(SessionLineageNode::subtree_len).sum()
    }

    pub fn flatten(&self) -> Vec<SessionLineageRow<'_>> {
        let mut rows = Vec::new();
        for root in &self.roots {
            root.flatten_into(0, &mut rows);
        }
        rows
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLineageNode {
    pub entry: SessionCatalogEntry,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SessionLineageNode>,
}

impl SessionLineageNode {
    fn subtree_len(&self) -> usize {
        1 + self.children.iter().map(Self::subtree_len).sum::<usize>()
    }

    fn flatten_into<'a>(&'a self, depth: usize, rows: &mut Vec<SessionLineageRow<'a>>) {
        rows.push(SessionLineageRow {
            depth,
            entry: &self.entry,
        });
        for child in &self.children {
            child.flatten_into(depth + 1, rows);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLineageRow<'a> {
    pub depth: usize,
    pub entry: &'a SessionCatalogEntry,
}

/// Project a deterministic, read-only lineage tree over session catalog entries.
///
/// Entries whose `parent_session_id` is missing, blank, unknown, self-referential, or cyclic are
/// treated as roots so legacy or partially migrated catalogs remain browseable.
pub fn project_lineage_tree(
    entries: impl IntoIterator<Item = SessionCatalogEntry>,
) -> SessionLineageTree {
    let entries = entries
        .into_iter()
        .map(|entry| (entry.run_id.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let parent_by_id = entries
        .iter()
        .map(|(run_id, entry)| {
            (
                run_id.clone(),
                normalized_parent_id(entry.parent_session_id.as_deref()),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut children_by_parent = BTreeMap::<String, Vec<String>>::new();
    let mut roots = Vec::<String>::new();

    for run_id in sorted_ids(&entries) {
        let parent_id = parent_by_id
            .get(&run_id)
            .and_then(|parent_id| parent_id.as_deref());

        match parent_id {
            Some(parent_id)
                if entries.contains_key(parent_id)
                    && parent_id != run_id
                    && !parent_chain_reaches(&parent_by_id, parent_id, &run_id) =>
            {
                children_by_parent
                    .entry(parent_id.to_string())
                    .or_default()
                    .push(run_id);
            }
            _ => roots.push(run_id),
        }
    }

    for children in children_by_parent.values_mut() {
        children.sort_by(|left, right| compare_entries(&entries[left], &entries[right]));
    }
    roots.sort_by(|left, right| compare_entries(&entries[left], &entries[right]));

    SessionLineageTree {
        roots: roots
            .into_iter()
            .map(|run_id| build_node(&entries, &children_by_parent, &run_id))
            .collect(),
    }
}

fn validate_event_log(events: &[EventEnvelopeV1]) -> Result<u64, SessionLineageError> {
    let mut expected_seq = 1_u64;
    let mut run_id: Option<&str> = None;

    for event in events {
        if event.schema_version != SCHEMA_VERSION {
            return Err(SessionLineageError::UnsupportedSchemaVersion {
                seq: event.seq,
                expected: SCHEMA_VERSION,
                actual: event.schema_version,
            });
        }
        if event.seq != expected_seq {
            return Err(SessionLineageError::NonContiguousSeq {
                expected: expected_seq,
                actual: event.seq,
            });
        }
        expected_seq += 1;

        match run_id {
            None => run_id = Some(event.run_id.as_str()),
            Some(existing) if existing == event.run_id => {}
            Some(existing) => {
                return Err(SessionLineageError::RunIdMismatch {
                    expected: existing.to_string(),
                    actual: event.run_id.clone(),
                    seq: event.seq,
                })
            }
        }
    }

    Ok(events.last().map(|event| event.seq).unwrap_or(0))
}

fn stable_prefix_from_state(cutoff_seq: u64, state: &PrefixState) -> StableSessionPrefix {
    StableSessionPrefix {
        cutoff_seq,
        event_count: cutoff_seq as usize,
        run_id: state.run_id.clone(),
        status: state.lifecycle.status(),
    }
}

fn project_prefix_state(events: &[EventEnvelopeV1], cutoff_seq: u64) -> PrefixState {
    let mut state = PrefixState::default();
    for event in events.iter().take(cutoff_seq as usize) {
        state.apply(event);
    }
    state
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PrefixLifecycle {
    #[default]
    Empty,
    Active,
    Finished,
    Failed,
}

impl PrefixLifecycle {
    fn status(self) -> Option<RunStatus> {
        match self {
            Self::Empty | Self::Active => None,
            Self::Finished => Some(RunStatus::Finished),
            Self::Failed => Some(RunStatus::Failed),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PrefixState {
    run_id: Option<String>,
    lifecycle: PrefixLifecycle,
    tasks_in_flight: BTreeSet<String>,
    tool_calls_in_flight: BTreeSet<String>,
    provider_requests_in_flight: BTreeSet<String>,
    pending_permissions: BTreeSet<String>,
    compactions_in_flight: BTreeSet<String>,
    edits_in_flight: BTreeSet<String>,
}

impl PrefixState {
    fn apply(&mut self, event: &EventEnvelopeV1) {
        self.run_id.get_or_insert_with(|| event.run_id.clone());

        match &event.payload {
            EventV1::RunStarted(_) => {
                self.lifecycle = PrefixLifecycle::Active;
                self.tasks_in_flight.clear();
                self.tool_calls_in_flight.clear();
                self.provider_requests_in_flight.clear();
                self.pending_permissions.clear();
                self.compactions_in_flight.clear();
                self.edits_in_flight.clear();
            }
            EventV1::RunFinished(_) => self.lifecycle = PrefixLifecycle::Finished,
            EventV1::RunFailed(_) => self.lifecycle = PrefixLifecycle::Failed,
            EventV1::TaskScheduled(payload) => {
                self.tasks_in_flight.insert(payload.task_id.clone());
            }
            EventV1::TaskCancelled(payload) => {
                self.tasks_in_flight.remove(&payload.task_id);
            }
            EventV1::TaskCompleted(payload) => {
                self.tasks_in_flight.remove(&payload.task_id);
            }
            EventV1::TaskResultLate(payload) => {
                self.tasks_in_flight.remove(&payload.task_id);
            }
            EventV1::ProviderRequestStarted(payload) => {
                self.provider_requests_in_flight
                    .insert(payload.request_id.clone());
            }
            EventV1::ProviderRequestFinished(payload) => {
                self.provider_requests_in_flight.remove(&payload.request_id);
            }
            EventV1::ToolCallRequested(payload) => {
                self.tool_calls_in_flight
                    .insert(payload.tool_call_id.clone());
            }
            EventV1::ToolCallStarted(payload) => {
                self.tool_calls_in_flight
                    .insert(payload.tool_call_id.clone());
            }
            EventV1::ToolCallFinished(payload) => {
                self.tool_calls_in_flight.remove(&payload.tool_call_id);
            }
            EventV1::PermissionRequested(payload) => {
                self.pending_permissions
                    .insert(payload.permission_id.clone());
            }
            EventV1::PermissionResolved(payload) => {
                self.pending_permissions.remove(&payload.permission_id);
            }
            EventV1::CompactionRequested(payload) => {
                self.compactions_in_flight
                    .insert(payload.checkpoint_id.clone());
            }
            EventV1::CompactionWritten(payload) => {
                self.compactions_in_flight.remove(&payload.checkpoint_id);
            }
            EventV1::CompactionApplied(payload) => {
                self.compactions_in_flight.remove(&payload.checkpoint_id);
            }
            EventV1::CompactionFailed(payload) => {
                if let Some(checkpoint_id) = payload.checkpoint_id.as_ref() {
                    self.compactions_in_flight.remove(checkpoint_id);
                }
            }
            EventV1::EditProposed(payload) => {
                self.edits_in_flight.insert(payload.edit_id.clone());
            }
            EventV1::EditApplied(payload) => {
                self.edits_in_flight.remove(&payload.edit_id);
            }
            EventV1::EditRejected(payload) => {
                self.edits_in_flight.remove(&payload.edit_id);
            }
            EventV1::AgentSpawned(_)
            | EventV1::AgentStopped(_)
            | EventV1::StaleDetected(_)
            | EventV1::UserMessageSubmitted(_)
            | EventV1::ProviderStreamDelta(_)
            | EventV1::ProviderReasoningDelta(_)
            | EventV1::AssistantMessageFinished(_)
            | EventV1::PermissionGrantRecorded(_)
            | EventV1::ArtifactWritten(_)
            | EventV1::PolicyViolationDetected(_)
            | EventV1::UiIntentReceived(_) => {}
        }
    }

    fn unstable_reason(&self) -> Option<String> {
        if let Some(reason) =
            first_non_empty_reason("tasks are still in flight", &self.tasks_in_flight)
        {
            return Some(reason);
        }
        if let Some(reason) =
            first_non_empty_reason("tool calls are still in flight", &self.tool_calls_in_flight)
        {
            return Some(reason);
        }
        if let Some(reason) = first_non_empty_reason(
            "provider requests are still in flight",
            &self.provider_requests_in_flight,
        ) {
            return Some(reason);
        }
        if let Some(reason) = first_non_empty_reason(
            "pending permissions must be resolved",
            &self.pending_permissions,
        ) {
            return Some(reason);
        }
        if let Some(reason) = first_non_empty_reason(
            "compactions are still in flight",
            &self.compactions_in_flight,
        ) {
            return Some(reason);
        }
        if let Some(reason) =
            first_non_empty_reason("edits are still in flight", &self.edits_in_flight)
        {
            return Some(reason);
        }

        match self.lifecycle {
            PrefixLifecycle::Empty => None,
            PrefixLifecycle::Active => Some(
                "run is still active; include run_finished or run_failed before cutting"
                    .to_string(),
            ),
            PrefixLifecycle::Finished | PrefixLifecycle::Failed => None,
        }
    }
}

fn first_non_empty_reason(label: &str, values: &BTreeSet<String>) -> Option<String> {
    values.iter().next().map(|id| format!("{label}: {id}"))
}

fn normalized_parent_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parent_chain_reaches(
    parent_by_id: &BTreeMap<String, Option<String>>,
    start_parent_id: &str,
    target_id: &str,
) -> bool {
    let mut seen = BTreeSet::new();
    let mut current = Some(start_parent_id);

    while let Some(run_id) = current {
        if run_id == target_id {
            return true;
        }
        if !seen.insert(run_id.to_string()) {
            return false;
        }
        current = parent_by_id
            .get(run_id)
            .and_then(|parent| parent.as_deref());
    }

    false
}

fn sorted_ids(entries: &BTreeMap<String, SessionCatalogEntry>) -> Vec<String> {
    let mut ids = entries.keys().cloned().collect::<Vec<_>>();
    ids.sort_by(|left, right| compare_entries(&entries[left], &entries[right]));
    ids
}

fn compare_entries(left: &SessionCatalogEntry, right: &SessionCatalogEntry) -> Ordering {
    match (&left.last_updated_at, &right.last_updated_at) {
        (Some(left_updated), Some(right_updated)) => right_updated
            .cmp(left_updated)
            .then_with(|| left.run_id.cmp(&right.run_id)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.run_id.cmp(&right.run_id),
    }
}

fn build_node(
    entries: &BTreeMap<String, SessionCatalogEntry>,
    children_by_parent: &BTreeMap<String, Vec<String>>,
    run_id: &str,
) -> SessionLineageNode {
    SessionLineageNode {
        entry: entries[run_id].clone(),
        children: children_by_parent
            .get(run_id)
            .into_iter()
            .flatten()
            .map(|child_id| build_node(entries, children_by_parent, child_id))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{
        latest_clone_stable_prefix, materialize_child_session_inner, project_lineage_tree,
        validate_fork_stable_prefix, validate_stable_prefix, ChildSessionMaterializationError,
        ChildSessionMaterializationRequest, ChildSessionMaterializationSourceKind,
        SessionLineageError,
    };
    use crate::event::{
        ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1, PermissionDecision,
        PermissionRequestedEvent, PermissionResolvedEvent, ProviderRequestFinishedEvent,
        ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent, TaskScheduleState,
        TaskScheduledEvent, ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStatus,
        SCHEMA_VERSION,
    };
    use crate::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};

    #[test]
    fn session_lineage_projects_tree_root_child_sibling_deep_ordering() {
        let tree = project_lineage_tree(vec![
            entry("child-old", Some("root"), "2026-05-03T00:01:00Z"),
            entry("grandchild", Some("child-new"), "2026-05-03T00:03:00Z"),
            entry("root", None, "2026-05-03T00:00:00Z"),
            entry("child-new", Some("root"), "2026-05-03T00:02:00Z"),
        ]);

        let flattened = tree
            .flatten()
            .into_iter()
            .map(|row| (row.depth, row.entry.run_id.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(tree.len(), 4);
        assert_eq!(
            flattened,
            vec![
                (0, "root"),
                (1, "child-new"),
                (2, "grandchild"),
                (1, "child-old"),
            ]
        );
    }

    #[test]
    fn session_lineage_handles_empty_sessions() {
        let selected = validate_stable_prefix(&[], 0).expect("empty prefix is stable");
        let latest = latest_clone_stable_prefix(&[]).expect("empty clone prefix is stable");
        let tree = project_lineage_tree(Vec::new());

        assert_eq!(selected.cutoff_seq, 0);
        assert_eq!(selected.event_count, 0);
        assert_eq!(latest, selected);
        assert!(tree.is_empty());
    }

    #[test]
    fn session_lineage_accepts_stable_prefix() {
        let events = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "prompt".to_string(),
                    request_digest: "digest-req".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000001".to_string(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some("digest-out".to_string()),
                    usage: None,
                    metadata: None,
                }),
            ),
            envelope(
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
            envelope(
                6,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "resumed".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
        ];

        let fork = validate_fork_stable_prefix(&events, 5).expect("selected prefix is stable");
        let latest = latest_clone_stable_prefix(&events).expect("latest stable prefix exists");

        assert_eq!(fork.cutoff_seq, 5);
        assert_eq!(fork.event_count, 5);
        assert_eq!(fork.run_id.as_deref(), Some("run_session_lineage"));
        assert_eq!(fork.status, Some(RunStatus::Finished));
        assert_eq!(latest.cutoff_seq, 5);
    }

    #[test]
    fn session_lineage_rejects_in_flight_prefix() {
        let events = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:default:gpt-5".to_string()),
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "terminal but task remained open".to_string(),
                }),
            ),
        ];

        let err = validate_fork_stable_prefix(&events, 3).expect_err("task remains in flight");

        assert!(matches!(
            err,
            SessionLineageError::UnstablePrefix {
                cutoff_seq: 3,
                ref reason
            } if reason.contains("tasks are still in flight")
                && reason.contains("task_000001")
        ));
    }

    #[test]
    fn session_lineage_rejects_corrupt_non_contiguous_logs() {
        let non_contiguous = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ];
        assert!(matches!(
            validate_stable_prefix(&non_contiguous, 1),
            Err(SessionLineageError::NonContiguousSeq {
                expected: 2,
                actual: 3
            })
        ));

        let mut wrong_schema = vec![envelope(
            1,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        )];
        wrong_schema[0].schema_version = SCHEMA_VERSION + 1;
        assert!(matches!(
            validate_stable_prefix(&wrong_schema, 1),
            Err(SessionLineageError::UnsupportedSchemaVersion { seq: 1, .. })
        ));

        let mut run_mismatch = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ];
        run_mismatch[1].run_id = "run_other".to_string();
        assert!(matches!(
            validate_stable_prefix(&run_mismatch, 2),
            Err(SessionLineageError::RunIdMismatch { seq: 2, .. })
        ));
    }

    #[test]
    fn session_lineage_rejects_unstable_prefixes() {
        let active = vec![envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/workspace".to_string(),
            }),
        )];
        assert!(matches!(
            validate_stable_prefix(&active, 1),
            Err(SessionLineageError::UnstablePrefix { reason, .. })
                if reason.contains("run is still active")
        ));

        let pending_permission = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000001".to_string(),
                    kind: "bash".to_string(),
                    tool_call_id: None,
                    summary: "run command".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 1000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ];
        assert!(matches!(
            validate_stable_prefix(&pending_permission, 3),
            Err(SessionLineageError::UnstablePrefix { reason, .. })
                if reason.contains("pending permissions") && reason.contains("perm_000001")
        ));
    }

    #[test]
    fn session_lineage_clone_rejects_running_source_without_stable_prefix() {
        let events = vec![envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/workspace".to_string(),
            }),
        )];

        assert!(matches!(
            latest_clone_stable_prefix(&events),
            Err(SessionLineageError::UnstablePrefix { cutoff_seq: 1, reason })
                if reason.contains("no stable completed prefix")
        ));
    }

    #[test]
    fn session_lineage_handles_first_last_and_out_of_range_cutoffs() {
        let events = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000001".to_string(),
                    kind: "bash".to_string(),
                    tool_call_id: None,
                    summary: "run command".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 1000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                3,
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_000001".to_string(),
                    decision: PermissionDecision::Allow,
                    reason: Some("approved".to_string()),
                }),
            ),
            envelope(
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ];

        assert!(matches!(
            validate_stable_prefix(&events, 1),
            Err(SessionLineageError::UnstablePrefix { .. })
        ));
        assert_eq!(
            validate_stable_prefix(&events, 4)
                .expect("last cutoff is stable")
                .cutoff_seq,
            4
        );
        assert!(matches!(
            validate_stable_prefix(&events, 5),
            Err(SessionLineageError::CutoffOutOfRange {
                cutoff_seq: 5,
                max_seq: 4
            })
        ));
    }

    #[test]
    fn session_lineage_treats_legacy_entries_without_parent_metadata_as_roots() {
        let tree = project_lineage_tree(vec![
            entry("legacy-b", None, "2026-05-03T00:02:00Z"),
            entry("legacy-a", None, "2026-05-03T00:01:00Z"),
            entry("orphan", Some("missing-parent"), "2026-05-03T00:03:00Z"),
        ]);

        let flattened = tree
            .flatten()
            .into_iter()
            .map(|row| (row.depth, row.entry.run_id.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            flattened,
            vec![(0, "orphan"), (0, "legacy-b"), (0, "legacy-a")]
        );
    }

    #[test]
    fn session_lineage_tracks_tool_call_in_flight_cutoffs() {
        let events = vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    tool_id: "bash".to_string(),
                    args_summary: "{}".to_string(),
                    args_digest: "digest-tool".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished before tool result".to_string(),
                }),
            ),
            envelope(
                4,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("ok".to_string()),
                    output_digest: Some("digest-output".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
        ];

        assert!(matches!(
            validate_stable_prefix(&events, 3),
            Err(SessionLineageError::UnstablePrefix { reason, .. })
                if reason.contains("tool calls are still in flight")
        ));
        assert_eq!(
            validate_stable_prefix(&events, 4)
                .expect("tool completion closes prefix")
                .cutoff_seq,
            4
        );
    }

    #[test]
    fn session_lineage_rejects_source_event_log_changed_while_materializing() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_run_dir = temp_dir.path().join("run_session_lineage");
        fs::create_dir_all(&source_run_dir).expect("create source run dir");
        let events = finished_events();
        write_events_jsonl(&source_run_dir, &events);
        let prefix = validate_fork_stable_prefix(&events, events.len() as u64)
            .expect("source prefix is stable");

        let mut changed_events = events.clone();
        changed_events.push(envelope(
            3,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "changed".to_string(),
                workspace_root: "/workspace".to_string(),
            }),
        ));

        let err = materialize_child_session_inner(
            ChildSessionMaterializationRequest {
                source_run_dir: &source_run_dir,
                events: &events,
                stable_prefix: &prefix,
                source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
            },
            None,
            || write_events_jsonl(&source_run_dir, &changed_events),
            |from, to| fs::rename(from, to),
        )
        .expect_err("changed source event log must reject before publish");

        assert!(matches!(
            err,
            ChildSessionMaterializationError::SourceEventLogChanged { .. }
        ));
        assert_eq!(
            session_dir_entries(temp_dir.path()),
            vec!["run_session_lineage"]
        );
        assert_no_unpublished_temp_dirs(temp_dir.path());
    }

    #[test]
    fn session_lineage_destination_collision_cleans_temp_without_overwriting_existing_run() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_run_dir = temp_dir.path().join("run_session_lineage");
        fs::create_dir_all(&source_run_dir).expect("create source run dir");
        let events = finished_events();
        write_events_jsonl(&source_run_dir, &events);
        let prefix = validate_fork_stable_prefix(&events, events.len() as u64)
            .expect("source prefix is stable");
        let (child_run_id, child_run_dir, temp_run_dir) = planned_child_paths(temp_dir.path());
        fs::create_dir_all(&child_run_dir).expect("create colliding child dir");
        fs::write(child_run_dir.join("existing.txt"), "existing child")
            .expect("write existing child marker");

        let err = materialize_child_session_inner(
            ChildSessionMaterializationRequest {
                source_run_dir: &source_run_dir,
                events: &events,
                stable_prefix: &prefix,
                source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
            },
            Some((
                child_run_id.clone(),
                child_run_dir.clone(),
                temp_run_dir.clone(),
            )),
            || {},
            |from, to| fs::rename(from, to),
        )
        .expect_err("colliding child directory must reject publish");

        assert!(matches!(
            err,
            ChildSessionMaterializationError::PublishRunDirectory { .. }
        ));
        assert!(child_run_dir.join("existing.txt").exists());
        assert!(!child_run_dir.join("events.jsonl").exists());
        assert!(!temp_run_dir.exists());
        assert_no_unpublished_temp_dirs(temp_dir.path());
    }

    #[test]
    fn session_lineage_cross_device_publish_error_cleans_temp_without_fallback() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_run_dir = temp_dir.path().join("run_session_lineage");
        fs::create_dir_all(&source_run_dir).expect("create source run dir");
        let events = finished_events();
        write_events_jsonl(&source_run_dir, &events);
        let prefix = validate_fork_stable_prefix(&events, events.len() as u64)
            .expect("source prefix is stable");
        let (child_run_id, child_run_dir, temp_run_dir) = planned_child_paths(temp_dir.path());

        let err = materialize_child_session_inner(
            ChildSessionMaterializationRequest {
                source_run_dir: &source_run_dir,
                events: &events,
                stable_prefix: &prefix,
                source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
            },
            Some((
                child_run_id.clone(),
                child_run_dir.clone(),
                temp_run_dir.clone(),
            )),
            || {},
            |_, _| Err(std::io::Error::from_raw_os_error(18)),
        )
        .expect_err("cross-device rename error must not fall back to a non-atomic copy");

        assert!(matches!(
            err,
            ChildSessionMaterializationError::PublishRunDirectory { .. }
        ));
        assert!(!child_run_dir.exists());
        assert!(!temp_run_dir.exists());
        assert_no_unpublished_temp_dirs(temp_dir.path());
    }

    fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: "run_session_lineage".to_string(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_session_lineage".to_string()),
            payload,
        }
    }

    fn finished_events() -> Vec<EventEnvelopeV1> {
        vec![
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ]
    }

    fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
        let body = events
            .iter()
            .map(|event| serde_json::to_string(event).expect("serialize event"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write events");
    }

    fn planned_child_paths(session_dir: &Path) -> (String, PathBuf, PathBuf) {
        let child_run_id = "run_harness_child_planned".to_string();
        (
            child_run_id.clone(),
            session_dir.join(&child_run_id),
            session_dir.join(format!(".{child_run_id}.tmp-planned")),
        )
    }

    fn session_dir_entries(session_dir: &Path) -> Vec<String> {
        let mut entries = fs::read_dir(session_dir)
            .expect("read session dir")
            .map(|entry| {
                entry
                    .expect("dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }

    fn assert_no_unpublished_temp_dirs(session_dir: &Path) {
        for entry in fs::read_dir(session_dir).expect("read session dir") {
            let entry = entry.expect("dir entry");
            let name = entry.file_name();
            let name = name.to_string_lossy();
            assert!(
                !(name.starts_with(".run_harness_child") && name.contains(".tmp-")),
                "unpublished temp dir remained: {name}"
            );
        }
    }

    fn entry(
        run_id: &str,
        parent_session_id: Option<&str>,
        last_updated_at: &str,
    ) -> SessionCatalogEntry {
        SessionCatalogEntry {
            run_id: run_id.to_string(),
            run_name: Some(run_id.to_string()),
            status: Some(RunStatus::Finished),
            last_updated_at: Some(last_updated_at.to_string()),
            workspace_root: Some("/workspace".to_string()),
            profile_preset: Some("default".to_string()),
            provider_model: Some("default/gpt-5".to_string()),
            mode_source: SessionModeSource::InteractiveLive,
            is_resumable: true,
            resume_disabled_reason: None,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: parent_session_id.map(str::to_string),
        }
    }
}

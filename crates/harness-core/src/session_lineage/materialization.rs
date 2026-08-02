// allow: SIZE_OK — session management (lineage + projection + inspection)
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionResolvedEvent,
    RunFinishedEvent, TaskCancelledEvent, SCHEMA_VERSION,
};
use crate::path_display::display_path;
use crate::session_paths::{ARTIFACTS_DIR_NAME, EVENTS_FILE_NAME, WRITER_LOCK_FILE_NAME};

use super::stable_prefix::{
    validate_stable_prefix, validate_tui_fork_stable_prefix, SessionLineageError,
    StableSessionPrefix,
};

#[path = "materialization_metadata.rs"]
mod materialization_metadata;

use self::materialization_metadata::write_child_metadata;

const CHILD_RUN_ID_PREFIX: &str = "run_harness_child";

static CHILD_RUN_COUNTER: AtomicU64 = AtomicU64::new(1);

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

pub trait ChildRunIdSource {
    fn next_child_run_id(&self) -> String;
}

#[derive(Debug, Default)]
pub struct SystemChildRunIdSource;

impl ChildRunIdSource for SystemChildRunIdSource {
    fn next_child_run_id(&self) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let counter = CHILD_RUN_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        format!("{CHILD_RUN_ID_PREFIX}_{nanos:x}_{counter:04}")
    }
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
    #[error("could not allocate a free child run ID after {attempts} attempt(s) for `{child_run_id}`; the session directory may have stale temporary directories or existing destinations")]
    ChildRunIdCollision {
        attempts: usize,
        child_run_id: String,
    },
    #[error("max_retries must be greater than 0")]
    InvalidMaxRetries,
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
    materialize_child_session_with_child_run_id_source(request, &SystemChildRunIdSource, 1000)
}

pub fn materialize_child_session_with_child_run_id_source(
    request: ChildSessionMaterializationRequest<'_>,
    child_run_ids: &dyn ChildRunIdSource,
    max_retries: usize,
) -> Result<ChildSessionMaterializationResult, ChildSessionMaterializationError> {
    materialize_child_session_inner(
        request,
        child_run_ids,
        None,
        max_retries,
        || {},
        |from, to| fs::rename(from, to),
    )
}

pub(super) fn materialize_child_session_inner<BeforePublish, Publish>(
    request: ChildSessionMaterializationRequest<'_>,
    child_run_ids: &dyn ChildRunIdSource,
    path_plan: Option<(String, PathBuf, PathBuf)>,
    max_retries: usize,
    before_publish: BeforePublish,
    publish: Publish,
) -> Result<ChildSessionMaterializationResult, ChildSessionMaterializationError>
where
    BeforePublish: FnOnce(),
    Publish: FnOnce(&Path, &Path) -> std::io::Result<()>,
{
    if max_retries == 0 {
        return Err(ChildSessionMaterializationError::InvalidMaxRetries);
    }

    let validated = if request.source_kind
        == ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot
    {
        validate_tui_fork_stable_prefix(request.events, request.stable_prefix.cutoff_seq)?
    } else {
        validate_stable_prefix(request.events, request.stable_prefix.cutoff_seq)?
    };
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
        match path_plan {
            Some(plan) => plan,
            None => fresh_child_run_paths(session_dir, child_run_ids, max_retries).map_err(
                |last_id| ChildSessionMaterializationError::ChildRunIdCollision {
                    attempts: max_retries,
                    child_run_id: last_id,
                },
            )?,
        };

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
    let mut copied_events = rewrite_child_event_prefix(
        source_prefix_events,
        source_run_id.as_deref(),
        &child_run_id,
    );
    if request.source_kind == ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot
        && validated.status.is_none()
        && !copied_events.is_empty()
    {
        append_materialized_terminal_event(&mut copied_events, &child_run_id);
    }
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
    rewritten.run_id = crate::ids::RunId::from(child_run_id);
    rewritten.correlation_id = None;
    rewritten.causation_id = None;
    rewritten.stream_key =
        rewrite_stream_key(source.stream_key.as_deref(), source_run_id, child_run_id);
    rewritten
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
            rewrite_child_event_envelope(
                event,
                source_run_id,
                child_run_id,
                u64::try_from(index).unwrap_or(0) + 1,
            )
        })
        .collect()
}

fn append_materialized_terminal_event(events: &mut Vec<EventEnvelopeV1>, child_run_id: &str) {
    let open_state = project_materialized_live_open_state(events);
    for task_id in open_state.tasks_in_flight {
        append_materialized_system_event(
            events,
            child_run_id,
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: task_id.into(),
                reason: "fork snapshot terminalized copied live task state".to_string(),
                task_scope: None,
            }),
        );
    }
    for permission_id in open_state.pending_permissions {
        append_materialized_system_event(
            events,
            child_run_id,
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id,
                decision: PermissionDecision::Deny,
                reason: Some("fork snapshot terminalized copied live permission state".to_string()),
            }),
        );
    }
    append_materialized_system_event(
        events,
        child_run_id,
        EventV1::RunFinished(RunFinishedEvent {
            summary: "Harness child session materialized from stable live snapshot".to_string(),
        }),
    );
}

fn append_materialized_system_event(
    events: &mut Vec<EventEnvelopeV1>,
    child_run_id: &str,
    payload: EventV1,
) {
    let child_seq = u64::try_from(events.len()).unwrap_or(0) + 1;
    let mono_ms = events
        .last()
        .map(|event| event.mono_ms.saturating_add(1))
        .unwrap_or_default();
    let ts = events.last().and_then(|event| event.ts.clone());
    events.push(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: child_event_id(child_run_id, child_seq),
        seq: child_seq,
        run_id: child_run_id.to_string().into(),
        mono_ms,
        ts,
        actor: EventActor::new(ActorKind::System, Some("session-lineage".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{child_run_id}")),
        payload,
    });
}

#[derive(Debug, Clone, Default)]
struct MaterializedLiveOpenState {
    tasks_in_flight: BTreeSet<String>,
    pending_permissions: BTreeSet<String>,
}

fn project_materialized_live_open_state(events: &[EventEnvelopeV1]) -> MaterializedLiveOpenState {
    let mut state = MaterializedLiveOpenState::default();
    for event in events {
        match &event.payload {
            EventV1::TaskScheduled(payload) => {
                state.tasks_in_flight.insert(payload.task_id.to_string());
            }
            EventV1::TaskCancelled(payload) => {
                state.tasks_in_flight.remove(payload.task_id.as_str());
            }
            EventV1::TaskCompleted(payload) => {
                state.tasks_in_flight.remove(payload.task_id.as_str());
            }
            EventV1::TaskResultLate(payload) => {
                state.tasks_in_flight.remove(payload.task_id.as_str());
            }
            EventV1::PermissionRequested(payload) => {
                state
                    .pending_permissions
                    .insert(payload.permission_id.clone());
            }
            EventV1::PermissionResolved(payload) => {
                state.pending_permissions.remove(&payload.permission_id);
            }
            _ => {}
        }
    }
    state
}

#[allow(
    deprecated,
    reason = "deprecated event variants kept for backward compatibility with existing session logs"
)]
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
            | EventV1::SessionTitleUpdated(_)
            | EventV1::RunFinished(_)
            | EventV1::RunFailed(_)
            | EventV1::AgentSpawned(_)
            | EventV1::AgentStopped(_)
            | EventV1::TaskScheduled(_)
            | EventV1::TaskCancelled(_)
            | EventV1::TaskCompleted(_)
            | EventV1::TaskResultLate(_)
            | EventV1::BackgroundTaskNotification(_)
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
            | EventV1::UiIntentReceived(_)
            | EventV1::WorkspaceSnapshot(_)
            | EventV1::WorkspaceReverted(_)
            | EventV1::SessionCompaction(_)
            | EventV1::BranchSummary(_) => {}
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
            let actual = u64::try_from(contents.len()).unwrap_or(0);
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

fn fresh_child_run_paths(
    session_dir: &Path,
    child_run_ids: &dyn ChildRunIdSource,
    max_retries: usize,
) -> Result<(String, PathBuf, PathBuf), String> {
    let mut last_id = String::new();
    for _ in 0..max_retries {
        last_id = child_run_ids.next_child_run_id();
        let child_run_dir = session_dir.join(&last_id);
        let temp_run_dir = sibling_temp_run_dir(session_dir, &last_id);
        if !child_run_dir.exists() && !temp_run_dir.exists() {
            return Ok((last_id, child_run_dir, temp_run_dir));
        }
    }
    Err(last_id)
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtifactCopySpec {
    digest: Option<String>,
    bytes: Option<u64>,
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

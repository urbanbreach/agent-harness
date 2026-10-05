use super::*;
use crate::{
    clock::Clock,
    store::{EventStore, Journal},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildSessionMaterializationSourceKind {
    DiskRunDirectory,
    TuiStableInMemorySnapshot,
}
pub struct ChildSessionMaterializationRequest<'a> {
    pub source_run_dir: &'a Path,
    pub events: &'a [EventEnvelopeV1],
    pub stable_prefix: &'a StableSessionPrefix,
    pub source_kind: ChildSessionMaterializationSourceKind,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChildSessionMaterializationResult {
    pub child_run_id: String,
    pub child_run_dir: PathBuf,
    pub source_run_id: Option<String>,
    pub source_cutoff_seq: u64,
    pub event_count: usize,
    pub artifact_count: usize,
}
pub fn materialize_child_session(
    request: ChildSessionMaterializationRequest<'_>,
) -> Result<ChildSessionMaterializationResult, ChildSessionMaterializationError> {
    materialize_child_session_as(request, None)
}
pub fn materialize_child_session_as(
    request: ChildSessionMaterializationRequest<'_>,
    child_id: Option<&str>,
) -> Result<ChildSessionMaterializationResult, ChildSessionMaterializationError> {
    if let Some(id) = child_id {
        crate::store::validate_session_id(id)?;
    }
    let live =
        request.source_kind == ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot;
    let validated = super::validate(request.events, request.stable_prefix.cutoff_seq, live)?;
    if &validated != request.stable_prefix || validated.event_count == 0 {
        return Err(SessionLineageError::Invalid(
            "stable prefix does not match source history".into(),
        ));
    }
    if !fs::symlink_metadata(request.source_run_dir)?.is_dir() {
        return Err(SessionLineageError::Invalid(
            "source must be a directory, not a symlink".into(),
        ));
    }
    let source = request.source_run_dir.canonicalize()?;
    let parent = source.parent().ok_or_else(|| {
        SessionLineageError::Invalid("source has no session-directory parent".into())
    })?;
    let _source_lock = if live {
        None
    } else {
        crate::store::existing_writer_lock(&source)?
    };
    if !live {
        check_source(&source, request.events)?;
    }
    let prefix = &request.events[..validated.event_count];
    let mut metadata = crate::proj::read_run_metadata(&source)?;
    if metadata
        .as_ref()
        .is_some_and(|m| Some(&m.run_id) != validated.run_id.as_ref())
    {
        return Err(SessionLineageError::Invalid(
            "source metadata belongs to a different run".into(),
        ));
    }
    let staging = tempfile::Builder::new()
        .prefix(".lineage-")
        .tempdir_in(parent)?;
    let child_id = match child_id {
        Some(id) => id.to_owned(),
        None => format!(
            "run_harness_child{}",
            staging
                .path()
                .file_name()
                .ok_or_else(|| SessionLineageError::Invalid(
                    "staging directory has no name".into()
                ))?
                .to_string_lossy()
                .trim_start_matches(".lineage-")
        ),
    };
    let child_dir = staging.path().join(&child_id);
    crate::store::create_private_dir(&child_dir)?;
    let mut artifact_count = super::artifacts::copy(prefix, &source, &child_dir)?;
    let journal = Journal::open(staging.path(), &child_id, true)?;
    let redactor = crate::redact::DefaultRedactor::default();
    let children = super::identities::child_ids(prefix, &child_id);
    let source_meta =
        crate::store::read_private_bytes(&source.join(crate::proj::META_FILE_NAME), 1024 * 1024)?
            .map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes))
            .transpose()?;
    let projection_owner = source_meta
        .as_ref()
        .filter(|meta| {
            meta["harness_lineage"]["relationship"] == "task_child_session"
                && meta["harness_lineage"]["child_session_id"].as_str()
                    == validated.run_id.as_deref()
        })
        .and_then(|meta| meta["harness_lineage"]["parent_run_id"].as_str());
    let finalized = super::finalized::references(
        prefix,
        &source,
        &child_dir,
        validated
            .run_id
            .as_deref()
            .ok_or_else(|| SessionLineageError::Invalid("source run identity missing".into()))?,
        projection_owner,
        &child_id,
        &children,
    )?;
    artifact_count += finalized
        .values()
        .filter(|r| !r.state.sha256.is_empty())
        .count();
    let mut state = crate::proj::InFlight::default();
    for event in prefix {
        state.apply(event);
        let mut rewritten =
            rewrite_child_event_envelope(event, validated.run_id.as_deref(), &child_id, event.seq);
        rewritten = super::identities::remap(
            rewritten,
            &children,
            validated.run_id.as_deref(),
            projection_owner,
            &child_id,
            &finalized,
        )?;
        rewritten.payload = crate::redact::redact_event_payload(&redactor, rewritten.payload)?;
        journal.append(rewritten.into())?;
    }
    for terminal in state
        .terminals("interrupted at fork; outcome is unknown, inspect current state before retrying")
    {
        let seq = journal.next_seq()?;
        let mut terminal =
            rewrite_child_event_envelope(&terminal, validated.run_id.as_deref(), &child_id, seq);
        terminal = super::identities::remap(
            terminal,
            &children,
            validated.run_id.as_deref(),
            projection_owner,
            &child_id,
            &finalized,
        )?;
        terminal.payload = crate::redact::redact_event_payload(&redactor, terminal.payload)?;
        journal.append(terminal.into())?;
    }
    if !state.stable() {
        let template = prefix
            .last()
            .ok_or_else(|| SessionLineageError::Invalid("empty source prefix".into()))?;
        let mut terminal = rewrite_child_event_envelope(
            template,
            validated.run_id.as_deref(),
            &child_id,
            journal.next_seq()?,
        );
        terminal.actor = EventActor::new(ActorKind::System, None);
        terminal.correlation_id = None;
        terminal.payload = EventV1::RunFinished(RunFinishedEvent {
            summary: "fork snapshot settled; interrupted work was not executed".into(),
        });
        journal.append(terminal.into())?;
    }
    let event_count = usize::try_from(journal.next_seq()?.saturating_sub(1))
        .map_err(|_| SessionLineageError::Invalid("child event count overflow".into()))?;
    journal.close_writer()?;
    let clock = crate::clock::RealClock::new();
    let workspace = prefix
        .iter()
        .find_map(|e| match &e.payload {
            EventV1::RunStarted(e) => Some(e.workspace_root.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let meta = metadata.get_or_insert_with(|| crate::proj::RunMetadata {
        run_id: String::new(),
        run_name: String::new(),
        workspace_root: workspace,
        created_at: None,
        config_digest: String::new(),
        harness_version: env!("CARGO_PKG_VERSION").into(),
        recorded_runtime_context: None,
        mode_source: None,
    });
    meta.run_id = child_id.clone();
    meta.run_name = format!(
        "Harness child of {}",
        validated.run_id.as_deref().unwrap_or("session")
    );
    meta.created_at = clock.system_time_rfc3339_millis();
    let mut metadata = serde_json::to_value(meta)?;
    let mut digest = blake3::Hasher::new();
    serde_json::to_writer(&mut digest, prefix)?;
    metadata["harness_lineage"] = serde_json::json!({
        "relationship":"child_session_materialization", "parent_run_id":validated.run_id,
        "source_run_id":validated.run_id,"source_cutoff_seq":validated.cutoff_seq,
        "source_cutoff_event_id":prefix.last().map(|e|&e.event_id),
        "source_digest":digest.finalize().to_hex().to_string(),
        "source_event_count":validated.event_count,"materialized_event_count":event_count,
        "materialized_artifact_count":artifact_count
    });
    crate::redact::redact_in_place(&redactor, &mut metadata);
    crate::store::write_private_atomic(
        &child_dir.join(crate::proj::META_FILE_NAME),
        &serde_json::to_vec_pretty(&metadata)?,
    )?;
    if !live {
        check_source(&source, request.events)?;
    }
    let destination = parent.join(&child_id);
    // Reserve the destination only after validation; rename replaces our empty directory.
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&destination)?;
    if let Err(error) = fs::rename(&child_dir, &destination) {
        let _ = fs::remove_dir(&destination);
        return Err(error.into());
    }
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(ChildSessionMaterializationResult {
        child_run_id: child_id,
        child_run_dir: destination,
        source_run_id: validated.run_id,
        source_cutoff_seq: validated.cutoff_seq,
        event_count,
        artifact_count,
    })
}
fn check_source(source: &Path, events: &[EventEnvelopeV1]) -> Result<(), SessionLineageError> {
    let path = source.join("events.jsonl");
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() {
        return Err(SessionLineageError::Invalid(
            "source journal must be a regular file".into(),
        ));
    }
    let mut expected = events.iter();
    for event in crate::store::JournalReader::open(&path, metadata.len())? {
        if expected.next() != Some(&event?) {
            return Err(SessionLineageError::Invalid(
                "source journal changed".into(),
            ));
        }
    }
    if expected.next().is_some() {
        return Err(SessionLineageError::Invalid(
            "source journal changed".into(),
        ));
    }
    Ok(())
}
pub fn rewrite_child_event_envelope(
    source: &EventEnvelopeV1,
    source_run_id: Option<&str>,
    child_run_id: &str,
    child_seq: u64,
) -> EventEnvelopeV1 {
    let mut event = source.clone();
    event.run_id = child_run_id.into();
    event.seq = child_seq;
    event.event_id = format!("{child_run_id}-event-{child_seq}");
    event.causation_id = None;
    if let Some(run) = source_run_id
        && event.stream_key.as_deref() == Some(&format!("run:{run}"))
    {
        event.stream_key = Some(format!("run:{child_run_id}"));
    }
    // Turn/tool correlations remain valid inside the child and are required by resume.
    event
}

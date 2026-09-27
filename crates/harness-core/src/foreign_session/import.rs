use super::*;
use crate::{
    clock::Clock,
    store::{EventStore, Journal},
};
use std::fs;

pub fn import_foreign_session_as_replay(
    source: &Path,
    destination: &Path,
) -> Result<ForeignImportResult, ForeignSessionError> {
    let events = load_events(source)?;
    let _source_lock = crate::store::existing_writer_lock(source).map_err(|_| {
        source_error(
            source,
            "source writer is active or its ownership is unknown",
        )
    })?;
    let failed = |message: &str| ForeignSessionError::DestinationWrite {
        path: safe_path(destination),
        message: message.into(),
    };
    crate::store::validate_private_path(destination)
        .map_err(|_| failed("unsafe destination path"))?;
    let resolved_source = source
        .canonicalize()
        .map_err(|_| source_error(source, "cannot resolve source"))?;
    let resolved_destination = crate::tool::resolve_file_path(
        &std::env::current_dir().map_err(|_| failed("cannot resolve working directory"))?,
        destination,
    )
    .map_err(|_| failed("cannot resolve destination"))?;
    if resolved_destination.starts_with(&resolved_source) {
        return Err(failed("destination must not be inside the foreign source"));
    }
    if destination.exists() && !destination.is_dir() {
        return Err(ForeignSessionError::DestinationNotDirectory {
            path: safe_path(destination),
        });
    }
    crate::store::create_private_dir(destination)
        .map_err(|_| failed("cannot create session directory"))?;
    let staging = tempfile::Builder::new()
        .prefix(".foreign-")
        .tempdir_in(destination)
        .map_err(|_| failed("cannot stage import"))?;
    let suffix = staging
        .path()
        .file_name()
        .ok_or_else(|| failed("invalid staging directory"))?
        .to_string_lossy();
    let run_id = format!("run_imported_{}", suffix.trim_start_matches(".foreign-"));
    let staged_dir = staging.path().join(&run_id);
    let writer = Journal::open(staging.path(), &run_id, true)
        .map_err(|_| failed("cannot create imported journal"))?;
    let redactor = crate::redact::DefaultRedactor::default();
    for event in &events {
        let mut copied = crate::session_lineage::rewrite_child_event_envelope(
            event,
            events.first().map(|e| e.run_id.as_str()),
            &run_id,
            event.seq,
        );
        copied.payload = crate::redact::redact_event_payload(&redactor, copied.payload)
            .map_err(|_| failed("cannot redact source event"))?;
        writer
            .append(copied.into())
            .map_err(|_| failed("source contains unsupported durable data"))?;
    }
    writer
        .close_writer()
        .map_err(|_| failed("cannot close imported journal"))?;
    crate::session_lineage::copy_session_artifacts(&events, source, &staged_dir)
        .map_err(|_| failed("source artifact validation failed"))?;
    let metadata = crate::proj::RunMetadata {
        run_id: run_id.clone(),
        run_name: "Imported replay".into(),
        workspace_root: String::new(),
        created_at: crate::clock::RealClock::new().system_time_rfc3339_millis(),
        config_digest: String::new(),
        harness_version: env!("CARGO_PKG_VERSION").into(),
        recorded_runtime_context: None,
        mode_source: Some(SessionModeSource::ReplayOnly),
    };
    crate::store::write_private_atomic(
        &staged_dir.join(crate::proj::META_FILE_NAME),
        &serde_json::to_vec_pretty(&metadata)
            .map_err(|_| failed("cannot serialize import metadata"))?,
    )
    .map_err(|_| failed("cannot write import metadata"))?;
    if load_events(source)? != events {
        return Err(source_error(source, "source changed during import"));
    }
    let run_dir = destination.join(&run_id);
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&run_dir)
        .map_err(|_| failed("import destination already exists or cannot be created"))?;
    if fs::rename(&staged_dir, &run_dir).is_err() {
        let _ = fs::remove_dir(&run_dir);
        return Err(failed("cannot publish staged session"));
    }
    #[cfg(unix)]
    fs::File::open(destination)
        .and_then(|dir| dir.sync_all())
        .map_err(|_| failed("cannot sync imported session directory"))?;
    Ok(ForeignImportResult {
        run_id,
        run_dir,
        event_count: events.len(),
        source_path: source.into(),
        format: FORMAT.into(),
        mode_source: SessionModeSource::ReplayOnly,
    })
}

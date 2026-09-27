use crate::cli_io::CliIo;
use clap::Args;
use harness_core::proj::{
    project_run_summary, SessionCatalogEntry, SessionCatalogMetadata, SessionModeSource,
};
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
#[path = "replay/index.rs"]
mod index;
pub(crate) use index::rebuild as rebuild_session_catalog_index;

pub(crate) struct SessionInspectionEntry {
    pub(crate) run_dir: PathBuf,
    pub(crate) catalog: SessionCatalogEntry,
    pub(crate) sort_unix_ms: u128,
}
impl SessionInspectionEntry {
    pub(crate) fn is_visible_in_operator_history(&self) -> bool {
        self.catalog.mode_source != SessionModeSource::ScenarioFixture
    }
}
pub(crate) fn inspect_session_catalog(
    directory: &Path,
) -> Result<Vec<SessionInspectionEntry>, String> {
    let mut cached = index::load(directory);
    scan_catalog(directory, &mut cached)
}
fn scan_catalog(
    directory: &Path,
    cached: &mut std::collections::BTreeMap<String, index::Cached>,
) -> Result<Vec<SessionInspectionEntry>, String> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut result = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let run_dir = entry.path();
        let path = run_dir.join("events.jsonl");
        if !path.is_file() {
            continue;
        }
        if result.len() >= 10_000 {
            return Err("session catalog exceeds 10,000 entries".into());
        }
        let row = entry
            .file_name()
            .to_str()
            .and_then(|name| cached.remove(name))
            .filter(|row| {
                index::Fingerprint::read(&run_dir).ok().as_ref() == Some(&row.fingerprint)
            });
        result.push(match row {
            Some(row) => SessionInspectionEntry {
                sort_unix_ms: row.fingerprint.updated_ms(),
                run_dir,
                catalog: row.catalog,
            },
            None => inspect_session(&run_dir)?,
        });
    }
    result.sort_by(|a, b| {
        b.sort_unix_ms
            .cmp(&a.sort_unix_ms)
            .then_with(|| a.catalog.run_id.cmp(&b.catalog.run_id))
    });
    Ok(result)
}

pub(crate) fn inspect_session(run_dir: &Path) -> Result<SessionInspectionEntry, String> {
    let id = run_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("invalid session name")?;
    let path = run_dir.join("events.jsonl");
    let sort_unix_ms = path
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis());
    let (metadata, mut error) = match harness_core::proj::read_metadata_value(run_dir) {
        Ok(value) => (value, None),
        Err(error) => (None, Some(error.to_string())),
    };
    let typed = metadata
        .as_ref()
        .map(|value| serde_json::from_value::<SessionCatalogMetadata>(value.clone()))
        .transpose();
    let typed = match typed {
        Ok(value) => value,
        Err(e) => {
            error = Some(e.to_string());
            None
        }
    };
    let (events, error) = match read_bounded_history(run_dir) {
        Ok(events) => (events, error),
        Err(error) => (Vec::new(), Some(error)),
    };
    let mut catalog =
        harness_core::proj::project_session_catalog_entry(&events, id, typed.as_ref(), None, error)
            .or_else(|error| {
                harness_core::proj::project_session_catalog_entry(
                    &[],
                    id,
                    typed.as_ref(),
                    None,
                    Some(error.to_string()),
                )
            })
            .map_err(|e| e.to_string())?;
    if catalog.run_id != id {
        catalog.is_resumable = false;
        catalog.resume_disabled_reason =
            Some("session directory and journal identity disagree".into());
    }
    catalog
        .last_updated_at
        .get_or_insert_with(|| sort_unix_ms.to_string());
    if catalog.parent_session_id.is_none() {
        catalog.parent_session_id = metadata
            .as_ref()
            .and_then(|value| {
                value
                    .pointer("/harness_lineage/harness_source_run_id")
                    .or_else(|| value.pointer("/harness_lineage/parent_run_id"))
            })
            .and_then(serde_json::Value::as_str)
            .filter(|parent| *parent != catalog.run_id)
            .map(str::to_owned);
    }
    Ok(SessionInspectionEntry {
        run_dir: run_dir.into(),
        catalog,
        sort_unix_ms,
    })
}
pub(crate) fn read_bounded_history(
    run_dir: &Path,
) -> Result<Vec<harness_core::event::EventEnvelopeV1>, String> {
    let path = run_dir.join("events.jsonl");
    let length = harness_core::store::open_private_file(&path)
        .and_then(|f| f.metadata())
        .map_err(|e| e.to_string())?
        .len();
    if length > 64 * 1024 * 1024 {
        return Err("session journal exceeds the 64 MiB inspection limit".into());
    }
    harness_core::store::JournalReader::open(&path, length)
        .map_err(|e| e.to_string())?
        .map(|event| event.map_err(|e| e.to_string()))
        .collect()
}

#[derive(Args)]
pub(crate) struct ReplayCommand {
    #[arg(long)]
    session: PathBuf,
    #[arg(long)]
    json: bool,
}
pub(crate) fn execute(command: ReplayCommand, io: &mut CliIo<'_>) -> Result<(), String> {
    let output = report(&command.session)?;
    if command.json {
        serde_json::to_writer_pretty(&mut io.stdout, &output).map_err(|e| e.to_string())?;
        writeln!(io.stdout).map_err(|e| e.to_string())
    } else {
        writeln!(
            io.stdout,
            "{}: {} ({} events)",
            output["run_id"].as_str().unwrap_or("empty session"),
            output["status"].as_str().unwrap_or("unknown"),
            output["total_events"]
        )
        .map_err(|e| e.to_string())
    }
}

pub(crate) fn report(session: &Path) -> Result<serde_json::Value, String> {
    let events = read_bounded_history(session).map_err(|e| e.to_string())?;
    let summary = project_run_summary(&events).map_err(|e| e.to_string())?;
    let mut output = serde_json::to_value(&summary).map_err(|e| e.to_string())?;
    output["run_id"] = events.first().map(|e| e.run_id.as_str()).into();
    output["total_events"] = summary.counts.total_events.into();
    Ok(output)
}

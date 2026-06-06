use std::path::Path;

use harness_core::event::EventEnvelopeV1;
use harness_core::proj::RecordedRuntimeContext;
use harness_tui::app::{LaunchMetadata, SessionHistoryEntry};

use crate::cli_io::load_run_metadata;
use crate::replay::{inspect_session_catalog, SessionInspectionEntry};

use super::launch_metadata::replay_launch_metadata;

pub(super) fn load_startup_session_history_entries(
    session_dir: &Path,
) -> Result<Vec<SessionHistoryEntry>, String> {
    inspect_session_catalog(session_dir).map(|entries| {
        entries
            .into_iter()
            .filter(SessionInspectionEntry::is_visible_in_operator_history)
            .map(|entry| SessionHistoryEntry {
                run_dir: entry.run_dir,
                catalog: entry.catalog,
            })
            .collect()
    })
}

pub(super) fn load_live_session_history_entries(
    run_dir: &Path,
    fallback_session_dir: &Path,
) -> Result<Vec<SessionHistoryEntry>, String> {
    let session_dir = run_dir.parent().unwrap_or(fallback_session_dir);
    load_startup_session_history_entries(session_dir)
}

pub(super) fn load_recorded_runtime_context(run_dir: &Path) -> Option<RecordedRuntimeContext> {
    load_run_metadata(run_dir).and_then(|metadata| metadata.recorded_runtime_context)
}

pub(super) fn replay_launch_metadata_for_run(
    run_dir: &Path,
    historical_events: &[EventEnvelopeV1],
) -> LaunchMetadata {
    let recorded_runtime_context = load_recorded_runtime_context(run_dir);
    replay_launch_metadata(recorded_runtime_context.as_ref(), historical_events)
}

use std::path::Path;

use super::error::RunnerError;
use super::util::write_json;
use crate::tui_fidelity::AdapterKind;

pub(super) fn write_failure(
    evidence_dir: &Path,
    adapter: AdapterKind,
    pid: u32,
    phase: &str,
    action_timeline: &[serde_json::Value],
    stream: &[u8],
    error: &RunnerError,
) -> Result<(), RunnerError> {
    std::fs::create_dir_all(evidence_dir).map_err(|write_error| RunnerError::Io {
        path: evidence_dir.to_path_buf(),
        detail: write_error.to_string(),
    })?;
    let stream_path = evidence_dir.join("terminal-ansi.txt");
    std::fs::write(&stream_path, stream).map_err(|write_error| RunnerError::Io {
        path: stream_path,
        detail: write_error.to_string(),
    })?;
    write_json(
        &evidence_dir.join("action-timeline.json"),
        &serde_json::json!({
            "adapter": adapter,
            "pid": pid,
            "phase": phase,
            "actions": action_timeline,
            "primary_error": error.to_string(),
        }),
    )
}

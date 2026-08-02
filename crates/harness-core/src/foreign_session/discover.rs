use std::fs;
use std::path::{Path, PathBuf};

use super::{ForeignAgentKind, ForeignSessionCandidate, ForeignSessionError, MARKERS};

/// Discover foreign session candidates under `scan_root` (immediate children only).
///
/// Read-only: never writes, never touches a harness active session directory.
pub fn discover_foreign_sessions(
    scan_root: &Path,
) -> Result<Vec<ForeignSessionCandidate>, ForeignSessionError> {
    if !scan_root.is_dir() {
        return Err(ForeignSessionError::ScanRootNotDirectory {
            path: scan_root.display().to_string(),
        });
    }

    let entries = fs::read_dir(scan_root).map_err(|err| ForeignSessionError::ScanRootRead {
        path: scan_root.display().to_string(),
        message: err.to_string(),
    })?;

    let mut out = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                out.push(ForeignSessionCandidate::Rejected {
                    path: scan_root.to_path_buf(),
                    reason: format!("directory entry unreadable: {err}"),
                });
                continue;
            }
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        out.push(classify_candidate(&path));
    }

    out.sort_by(|left, right| left.path().cmp(right.path()));
    Ok(out)
}

/// Explicitly refuse mutating import into an active harness session.
pub fn refuse_import_into_active_session(
    foreign_path: &Path,
    active_session_dir: &Path,
) -> Result<(), ForeignSessionError> {
    let _ = foreign_path;
    Err(ForeignSessionError::ImportIntoActiveForbidden {
        active_session: active_session_dir.display().to_string(),
    })
}

pub(super) fn classify_candidate(path: &Path) -> ForeignSessionCandidate {
    let kind = infer_kind(path);
    let Some((marker_name, marker_path)) = first_marker(path) else {
        return ForeignSessionCandidate::Rejected {
            path: path.to_path_buf(),
            reason: "no foreign session markers found".to_string(),
        };
    };

    match validate_marker(&marker_path, marker_name) {
        Ok(()) => ForeignSessionCandidate::Discoverable {
            kind,
            path: path.to_path_buf(),
            marker: marker_name.to_string(),
        },
        Err(reason) => ForeignSessionCandidate::Corrupt {
            kind,
            path: path.to_path_buf(),
            reason,
        },
    }
}

fn first_marker(path: &Path) -> Option<(&'static str, PathBuf)> {
    for name in MARKERS {
        let candidate = path.join(name);
        if candidate.is_file() {
            return Some((*name, candidate));
        }
    }
    None
}

fn validate_marker(marker_path: &Path, marker_name: &str) -> Result<(), String> {
    let bytes = fs::read(marker_path).map_err(|err| {
        format!(
            "failed to read marker {marker_name} at {}: {err}",
            marker_path.display()
        )
    })?;
    if bytes.is_empty() {
        return Err(format!("marker {marker_name} is empty"));
    }

    if marker_name.ends_with(".json") {
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|err| format!("marker {marker_name} is not valid JSON: {err}"))?;
        if !value.is_object() && !value.is_array() {
            return Err(format!(
                "marker {marker_name} JSON root must be object or array"
            ));
        }
        return Ok(());
    }

    let text = String::from_utf8_lossy(&bytes);
    let mut saw_line = false;
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        saw_line = true;
        let value: serde_json::Value = serde_json::from_str(trimmed).map_err(|err| {
            format!(
                "marker {marker_name} line {} is not valid JSON: {err}",
                idx + 1
            )
        })?;
        if !value.is_object() && !value.is_array() {
            return Err(format!(
                "marker {marker_name} line {} JSON root must be object or array",
                idx + 1
            ));
        }
    }
    if !saw_line {
        return Err(format!("marker {marker_name} has no non-empty JSONL lines"));
    }
    Ok(())
}

fn infer_kind(path: &Path) -> ForeignAgentKind {
    let haystack = path.to_string_lossy().to_ascii_lowercase();
    if haystack.contains("codex") {
        ForeignAgentKind::Codex
    } else if haystack.contains("claude") {
        ForeignAgentKind::Claude
    } else if haystack.contains("opencode") || haystack.contains("open-code") {
        ForeignAgentKind::OpenCode
    } else {
        ForeignAgentKind::Unknown
    }
}

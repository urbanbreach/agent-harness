use super::*;

pub fn discover_foreign_sessions(
    root: &Path,
) -> Result<Vec<ForeignSessionCandidate>, ForeignSessionError> {
    let failed = |message: &str| ForeignSessionError::ScanRootRead {
        path: safe_path(root),
        message: message.into(),
    };
    crate::store::validate_private_path(root).map_err(|_| failed("unsafe scan path"))?;
    if !root.is_dir() {
        return Err(ForeignSessionError::ScanRootNotDirectory {
            path: safe_path(root),
        });
    }
    let mut paths = std::fs::read_dir(root)
        .map_err(|_| failed("cannot list directory"))?
        .take(513)
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| failed("cannot read directory entry"))?;
    if paths.len() > 512 {
        return Err(failed(
            "scan exceeds 512 candidates; choose a narrower root",
        ));
    }
    paths.sort();
    let mut bytes = 0;
    let mut candidates = Vec::new();
    for path in paths {
        for marker in MARKERS {
            bytes += std::fs::symlink_metadata(path.join(marker))
                .map_or(0, |m| m.len().min(MAX_BYTES + 1));
        }
        if bytes > MAX_BYTES {
            return Err(failed("scan exceeds 64 MiB; choose a narrower root"));
        }
        candidates.push(classify(&path));
    }
    Ok(candidates)
}
fn classify(path: &Path) -> ForeignSessionCandidate {
    let rejected = |reason: &str| ForeignSessionCandidate::Rejected {
        path: path.into(),
        reason: reason.into(),
    };
    if crate::store::validate_private_path(path).is_err() || !path.is_dir() {
        return rejected("candidate must be a directory without symlinks");
    }
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let kind = if name.contains("codex") {
        ForeignAgentKind::Codex
    } else if name.contains("claude") {
        ForeignAgentKind::Claude
    } else if name.contains("opencode") {
        ForeignAgentKind::OpenCode
    } else {
        ForeignAgentKind::Unknown
    };
    let Some(marker) = MARKERS
        .iter()
        .find(|marker| std::fs::symlink_metadata(path.join(marker)).is_ok())
    else {
        return rejected("no recognized session marker");
    };
    let valid = if *marker == "events.jsonl" {
        load_events(path).is_ok()
    } else {
        valid_descriptor(&path.join(marker), marker.ends_with("jsonl"))
    };
    if valid {
        ForeignSessionCandidate::Discoverable {
            kind,
            path: path.into(),
            marker: (*marker).into(),
        }
    } else {
        ForeignSessionCandidate::Corrupt {
            kind,
            path: path.into(),
            reason: "session marker is unreadable, malformed, or oversized".into(),
        }
    }
}
fn valid_descriptor(path: &Path, jsonl: bool) -> bool {
    let Ok(Some(bytes)) = crate::store::read_private_bytes(path, 1024 * 1024) else {
        return false;
    };
    if jsonl {
        let mut lines = bytes
            .split(|b| *b == b'\n')
            .filter(|s| !s.iter().all(u8::is_ascii_whitespace))
            .peekable();
        lines.peek().is_some()
            && lines.all(|s| serde_json::from_slice::<serde_json::Value>(s).is_ok())
    } else {
        serde_json::from_slice::<serde_json::Value>(&bytes).is_ok()
    }
}

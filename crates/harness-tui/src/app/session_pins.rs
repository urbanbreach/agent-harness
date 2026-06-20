use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SESSION_PINS_SCHEMA_VERSION: u8 = 1;
const SESSION_PINS_FILE_NAME: &str = "session-pins.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSessionPins {
    schema_version: u8,
    pinned_run_ids: Vec<String>,
}

pub fn session_pins_path_for_session_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("tui").join(SESSION_PINS_FILE_NAME)
}

pub(super) fn load_session_pins(path: &Path) -> Result<BTreeSet<String>, String> {
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(err) => {
            return Err(format!(
                "failed to read session pins {}: {err}",
                path.display()
            ))
        }
    };

    let pins = serde_json::from_str::<PersistedSessionPins>(&body)
        .map_err(|err| format!("failed to parse session pins {}: {err}", path.display()))?;
    if pins.schema_version != SESSION_PINS_SCHEMA_VERSION {
        return Err(format!(
            "unsupported session pins schema {} in {}",
            pins.schema_version,
            path.display()
        ));
    }

    Ok(pins.pinned_run_ids.into_iter().collect())
}

pub(super) fn save_session_pins(path: &Path, pins: &BTreeSet<String>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create session pins dir {}: {err}",
                parent.display()
            )
        })?;
    }

    let pins = PersistedSessionPins {
        schema_version: SESSION_PINS_SCHEMA_VERSION,
        pinned_run_ids: pins.iter().cloned().collect(),
    };
    let body = serde_json::to_vec_pretty(&pins)
        .map_err(|err| format!("failed to serialize session pins: {err}"))?;
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, body).map_err(|err| {
        format!(
            "failed to write session pins {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path)
        .map_err(|err| format!("failed to replace session pins {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn session_pins_round_trip() {
        let dir = tempdir().unwrap();
        let path = session_pins_path_for_session_dir(dir.path());
        let mut pins = BTreeSet::new();
        pins.insert("run-abc".to_string());
        pins.insert("run-xyz".to_string());

        save_session_pins(&path, &pins).unwrap();
        let loaded = load_session_pins(&path).unwrap();
        assert_eq!(loaded, pins);
    }

    #[test]
    fn session_pins_missing_file_returns_empty() {
        let dir = tempdir().unwrap();
        let path = session_pins_path_for_session_dir(dir.path());
        let loaded = load_session_pins(&path).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn session_pins_path_under_tui_subdir() {
        let dir = tempdir().unwrap();
        let path = session_pins_path_for_session_dir(dir.path());
        assert!(path.ends_with("tui/session-pins.json"));
    }
}

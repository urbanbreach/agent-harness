use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const PROMPT_STASH_SCHEMA_VERSION: u8 = 1;
const PROMPT_STASH_FILE_NAME: &str = "prompt-stash.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptStashEntry {
    pub text: String,
    pub cursor: usize,
    pub selection_anchor: Option<usize>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Default)]
pub struct PromptStashState {
    pub entries: Vec<PromptStashEntry>,
    pub path: Option<PathBuf>,
    pub list_visible: bool,
    pub list_selected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPromptStash {
    schema_version: u8,
    entries: Vec<PromptStashEntry>,
}

pub fn prompt_stash_path_for_session_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("tui").join(PROMPT_STASH_FILE_NAME)
}

pub(super) fn load_prompt_stash(path: &Path) -> Result<Vec<PromptStashEntry>, String> {
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(format!(
                "failed to read prompt stash {}: {err}",
                path.display()
            ))
        }
    };

    let stash = serde_json::from_str::<PersistedPromptStash>(&body)
        .map_err(|err| format!("failed to parse prompt stash {}: {err}", path.display()))?;
    if stash.schema_version != PROMPT_STASH_SCHEMA_VERSION {
        return Err(format!(
            "unsupported prompt stash schema {} in {}",
            stash.schema_version,
            path.display()
        ));
    }

    Ok(stash.entries)
}

pub(super) fn save_prompt_stash(path: &Path, entries: &[PromptStashEntry]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create prompt stash dir {}: {err}",
                parent.display()
            )
        })?;
    }

    let stash = PersistedPromptStash {
        schema_version: PROMPT_STASH_SCHEMA_VERSION,
        entries: entries.to_vec(),
    };
    let body = serde_json::to_vec_pretty(&stash)
        .map_err(|err| format!("failed to serialize prompt stash: {err}"))?;
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, body).map_err(|err| {
        format!(
            "failed to write prompt stash {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path)
        .map_err(|err| format!("failed to replace prompt stash {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prompt_stash_round_trip() {
        let dir = tempdir().unwrap();
        let path = prompt_stash_path_for_session_dir(dir.path());
        let entries = vec![
            PromptStashEntry {
                text: "first stashed prompt".to_string(),
                cursor: 5,
                selection_anchor: Some(2),
                timestamp: 1_700_000_000_000,
            },
            PromptStashEntry {
                text: "second stashed prompt".to_string(),
                cursor: 10,
                selection_anchor: None,
                timestamp: 1_700_000_001_000,
            },
        ];

        save_prompt_stash(&path, &entries).unwrap();
        let loaded = load_prompt_stash(&path).unwrap();
        assert_eq!(loaded, entries);
    }

    #[test]
    fn prompt_stash_missing_file_returns_empty() {
        let dir = tempdir().unwrap();
        let path = prompt_stash_path_for_session_dir(dir.path());
        let loaded = load_prompt_stash(&path).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn prompt_stash_path_under_tui_subdir() {
        let dir = tempdir().unwrap();
        let path = prompt_stash_path_for_session_dir(dir.path());
        assert!(path.ends_with("tui/prompt-stash.json"));
    }

    #[test]
    fn prompt_stash_rejects_unknown_schema_version() {
        let dir = tempdir().unwrap();
        let path = prompt_stash_path_for_session_dir(dir.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let body = r#"{"schema_version":99,"entries":[]}"#;
        fs::write(&path, body).unwrap();
        let result = load_prompt_stash(&path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("unsupported prompt stash schema"));
    }
}

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const PROMPT_HISTORY_SCHEMA_VERSION: u8 = 1;
const PROMPT_HISTORY_FILE_NAME: &str = "prompt-history.json";
const PROMPT_STASH_FILE_NAME: &str = "prompt-stash.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PromptHistoryDraft {
    pub(super) text: String,
    pub(super) cursor: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPromptHistory {
    schema_version: u8,
    prompts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersistedPromptStashEntry {
    pub(super) text: String,
    pub(super) cursor: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPromptStash {
    schema_version: u8,
    prompts: Vec<PersistedPromptStashEntry>,
}

pub fn prompt_history_path_for_session_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("tui").join(PROMPT_HISTORY_FILE_NAME)
}

pub(super) fn prompt_stash_path_for_history_path(path: &Path) -> PathBuf {
    path.with_file_name(PROMPT_STASH_FILE_NAME)
}

pub(super) fn load_prompt_history(path: &Path) -> Result<Vec<String>, String> {
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(format!(
                "failed to read prompt history {}: {err}",
                path.display()
            ))
        }
    };

    let history = serde_json::from_str::<PersistedPromptHistory>(&body)
        .map_err(|err| format!("failed to parse prompt history {}: {err}", path.display()))?;
    if history.schema_version != PROMPT_HISTORY_SCHEMA_VERSION {
        return Err(format!(
            "unsupported prompt history schema {} in {}",
            history.schema_version,
            path.display()
        ));
    }

    Ok(history.prompts)
}

pub(super) fn save_prompt_history(path: &Path, prompts: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create prompt history dir {}: {err}",
                parent.display()
            )
        })?;
    }

    let history = PersistedPromptHistory {
        schema_version: PROMPT_HISTORY_SCHEMA_VERSION,
        prompts: prompts.to_vec(),
    };
    let body = serde_json::to_vec_pretty(&history)
        .map_err(|err| format!("failed to serialize prompt history: {err}"))?;
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, body).map_err(|err| {
        format!(
            "failed to write prompt history {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path)
        .map_err(|err| format!("failed to replace prompt history {}: {err}", path.display()))
}

pub(super) fn load_prompt_stash(path: &Path) -> Result<Vec<PersistedPromptStashEntry>, String> {
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
    if stash.schema_version != PROMPT_HISTORY_SCHEMA_VERSION {
        return Err(format!(
            "unsupported prompt stash schema {} in {}",
            stash.schema_version,
            path.display()
        ));
    }

    Ok(stash.prompts)
}

pub(super) fn save_prompt_stash(
    path: &Path,
    prompts: &[PersistedPromptStashEntry],
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create prompt stash dir {}: {err}",
                parent.display()
            )
        })?;
    }

    let stash = PersistedPromptStash {
        schema_version: PROMPT_HISTORY_SCHEMA_VERSION,
        prompts: prompts.to_vec(),
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

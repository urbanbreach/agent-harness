use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const PROMPT_HISTORY_SCHEMA_VERSION: u8 = 1;
const PROMPT_HISTORY_FILE_NAME: &str = "prompt-history.json";

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

pub fn prompt_history_path_for_session_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("tui").join(PROMPT_HISTORY_FILE_NAME)
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

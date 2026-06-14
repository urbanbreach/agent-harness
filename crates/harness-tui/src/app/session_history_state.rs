use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::SessionHistoryEntry;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionHistoryUiState {
    path: Option<PathBuf>,
    pinned_run_ids: BTreeSet<String>,
    delete_armed_run_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct SessionHistoryUiStateFile {
    pinned_run_ids: BTreeSet<String>,
}

pub(crate) fn session_history_state_path_for_session_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("tui").join("session-history.json")
}

impl SessionHistoryUiState {
    pub(crate) fn load_from_path(path: Option<PathBuf>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        let pinned_run_ids = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<SessionHistoryUiStateFile>(&raw).ok())
            .map(|state| state.pinned_run_ids)
            .unwrap_or_default();
        Self {
            path: Some(path),
            pinned_run_ids,
            delete_armed_run_id: None,
        }
    }

    pub(crate) fn set_path(&mut self, path: Option<PathBuf>) {
        *self = Self::load_from_path(path);
    }

    pub(crate) fn is_pinned(&self, entry: &SessionHistoryEntry) -> bool {
        self.pinned_run_ids.contains(&entry.catalog.run_id)
    }

    pub(crate) fn toggle_pin(&mut self, entry: &SessionHistoryEntry) {
        if !self.pinned_run_ids.insert(entry.catalog.run_id.clone()) {
            self.pinned_run_ids.remove(&entry.catalog.run_id);
        }
        self.delete_armed_run_id = None;
        self.persist();
    }

    pub(crate) fn arm_delete(&mut self, run_id: &str) {
        self.delete_armed_run_id = Some(run_id.to_string());
    }

    pub(crate) fn disarm_delete(&mut self) {
        self.delete_armed_run_id = None;
    }

    pub(crate) fn delete_armed(&self, run_id: &str) -> bool {
        self.delete_armed_run_id.as_deref() == Some(run_id)
    }

    fn persist(&self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_string_pretty(&SessionHistoryUiStateFile {
            pinned_run_ids: self.pinned_run_ids.clone(),
        }) {
            let _ = fs::write(path, raw);
        }
    }
}

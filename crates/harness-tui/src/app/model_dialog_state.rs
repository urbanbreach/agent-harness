use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::ModelOption;

const RECENT_LIMIT: usize = 12;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ModelDialogState {
    path: Option<PathBuf>,
    favorites: BTreeSet<ModelIdentity>,
    recents: Vec<ModelIdentity>,
    active_dialog: ModelDialogKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ModelDialogKind {
    #[default]
    Model,
    Provider,
    Variant,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
struct ModelIdentity {
    profile: String,
    provider: String,
    model: String,
    variant: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct ModelDialogStateFile {
    favorites: BTreeSet<ModelIdentity>,
    recents: Vec<ModelIdentity>,
}

pub(crate) fn model_dialog_state_path_for_session_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("tui").join("models.json")
}

impl ModelDialogState {
    pub(crate) fn load_from_path(path: Option<PathBuf>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        let file = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<ModelDialogStateFile>(&raw).ok())
            .unwrap_or_default();
        Self {
            path: Some(path),
            favorites: file.favorites,
            recents: file.recents,
            active_dialog: ModelDialogKind::Model,
        }
    }

    pub(crate) fn set_path(&mut self, path: Option<PathBuf>) {
        *self = Self::load_from_path(path);
    }

    pub(crate) fn is_favorite(&self, option: &ModelOption) -> bool {
        self.favorites.contains(&ModelIdentity::from(option))
    }

    pub(crate) fn toggle_favorite(&mut self, option: &ModelOption) {
        let identity = ModelIdentity::from(option);
        if !self.favorites.insert(identity.clone()) {
            self.favorites.remove(&identity);
        }
        self.persist();
    }

    pub(crate) fn record_recent(&mut self, option: &ModelOption) {
        let identity = ModelIdentity::from(option);
        self.recents.retain(|recent| recent != &identity);
        self.recents.insert(0, identity);
        self.recents.truncate(RECENT_LIMIT);
        self.persist();
    }

    pub(crate) fn recent_options<'a>(&self, options: &'a [ModelOption]) -> Vec<&'a ModelOption> {
        self.recents
            .iter()
            .filter_map(|recent| options.iter().find(|option| recent.matches(option)))
            .collect()
    }

    pub(crate) fn active_dialog(&self) -> ModelDialogKind {
        self.active_dialog
    }

    pub(crate) fn set_active_dialog(&mut self, active_dialog: ModelDialogKind) {
        self.active_dialog = active_dialog;
    }

    fn persist(&self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_string_pretty(&ModelDialogStateFile {
            favorites: self.favorites.clone(),
            recents: self.recents.clone(),
        }) {
            let _ = fs::write(path, raw);
        }
    }
}

impl From<&ModelOption> for ModelIdentity {
    fn from(option: &ModelOption) -> Self {
        Self {
            profile: option.profile.clone(),
            provider: option.provider.clone(),
            model: option.model.clone(),
            variant: option.variant().map(str::to_string),
        }
    }
}

impl ModelIdentity {
    fn matches(&self, option: &ModelOption) -> bool {
        self.profile == option.profile
            && self.provider == option.provider
            && self.model == option.model
            && self.variant.as_deref() == option.variant()
    }
}

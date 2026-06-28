use std::fs;
use std::path::{Path, PathBuf};

use harness_tui::app::{LaunchMetadata, ModelOption};
use serde::{Deserialize, Serialize};

const MODEL_SELECTION_STATE_FILE: &str = "model.json";
const MODEL_SELECTION_SCHEMA_VERSION: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersistedModelSelection {
    pub(super) schema_version: u8,
    pub(super) config_digest: String,
    pub(super) profile: String,
    pub(super) provider: String,
    pub(super) model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) variant: Option<String>,
}

fn model_selection_state_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HARNESS_MODEL_SELECTION_STATE_FILE") {
        let path = PathBuf::from(path);
        return (!path.as_os_str().is_empty()).then_some(path);
    }

    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        let state_home = PathBuf::from(state_home);
        if !state_home.as_os_str().is_empty() {
            return Some(state_home.join("harness").join(MODEL_SELECTION_STATE_FILE));
        }
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| !home.as_os_str().is_empty())
        .map(|home| {
            home.join(".local")
                .join("state")
                .join("harness")
                .join(MODEL_SELECTION_STATE_FILE)
        })
}

pub(super) fn load_persisted_model_selection_from_path(
    path: &Path,
) -> Option<PersistedModelSelection> {
    let body = fs::read_to_string(path).ok()?;
    let selection = serde_json::from_str::<PersistedModelSelection>(&body).ok()?;
    persisted_model_selection_valid(&selection).then_some(selection)
}

fn persisted_model_selection_valid(selection: &PersistedModelSelection) -> bool {
    selection.schema_version == MODEL_SELECTION_SCHEMA_VERSION
        && model_selection_value_present(&selection.config_digest)
        && model_selection_value_present(&selection.profile)
        && model_selection_value_present(&selection.provider)
        && model_selection_value_present(&selection.model)
        && selection
            .variant
            .as_deref()
            .is_none_or(model_selection_value_present)
}

pub(super) fn model_selection_value_present(value: &str) -> bool {
    !value.trim().is_empty()
}

pub(super) fn save_persisted_model_selection(
    launch_metadata: &LaunchMetadata,
    config_digest: &str,
) -> Result<(), String> {
    let Some(path) = model_selection_state_path() else {
        return Ok(());
    };
    save_persisted_model_selection_to_path(&path, launch_metadata, config_digest)
}

pub(super) fn save_persisted_model_selection_to_path(
    path: &Path,
    launch_metadata: &LaunchMetadata,
    config_digest: &str,
) -> Result<(), String> {
    let Some(selection) = persisted_model_selection_from_metadata(launch_metadata, config_digest)
    else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create model selection state dir {}: {err}",
                parent.display()
            )
        })?;
    }

    let body = serde_json::to_vec_pretty(&selection)
        .map_err(|err| format!("failed to serialize model selection state: {err}"))?;
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, body).map_err(|err| {
        format!(
            "failed to write model selection state {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path).map_err(|err| {
        format!(
            "failed to replace model selection state {}: {err}",
            path.display()
        )
    })
}

fn persisted_model_selection_from_metadata(
    launch_metadata: &LaunchMetadata,
    config_digest: &str,
) -> Option<PersistedModelSelection> {
    if !model_selection_value_present(config_digest) {
        return None;
    }

    Some(PersistedModelSelection {
        schema_version: MODEL_SELECTION_SCHEMA_VERSION,
        config_digest: config_digest.to_string(),
        profile: launch_metadata.profile().to_string(),
        provider: launch_metadata.provider().to_string(),
        model: launch_metadata.model()?.to_string(),
        variant: launch_metadata.variant().map(str::to_string),
    })
}

pub(super) fn apply_persisted_model_selection(
    launch_metadata: LaunchMetadata,
    config_digest: &str,
) -> LaunchMetadata {
    let Some(path) = model_selection_state_path() else {
        return launch_metadata;
    };
    apply_persisted_model_selection_from_path(launch_metadata, &path, config_digest)
}

pub(super) fn apply_persisted_model_selection_from_path(
    launch_metadata: LaunchMetadata,
    path: &Path,
    config_digest: &str,
) -> LaunchMetadata {
    let Some(selection) = load_persisted_model_selection_from_path(path) else {
        return launch_metadata;
    };
    if selection.config_digest != config_digest {
        return launch_metadata;
    }
    apply_model_selection_to_launch_metadata(launch_metadata, &selection)
}

pub(super) fn apply_model_selection_to_launch_metadata(
    launch_metadata: LaunchMetadata,
    selection: &PersistedModelSelection,
) -> LaunchMetadata {
    let Some(option) = matching_persisted_model_option(&launch_metadata, selection) else {
        return launch_metadata;
    };
    LaunchMetadata::from_model_option(option)
        .with_available_models(launch_metadata.available_models().to_vec())
        .with_switchable_profiles(launch_metadata.switchable_profiles().to_vec())
}

fn matching_persisted_model_option<'a>(
    launch_metadata: &'a LaunchMetadata,
    selection: &PersistedModelSelection,
) -> Option<&'a ModelOption> {
    if !persisted_model_selection_valid(selection) {
        return None;
    }

    let active_profile = launch_metadata.profile();
    launch_metadata.available_models().iter().find(|option| {
        option.profile == active_profile
            && option.provider == selection.provider
            && option.model == selection.model
            && option.variant() == selection.variant.as_deref()
    })
}

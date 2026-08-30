//! Project-file write path for a narrow set of registry-backed settings.
//!
//! Keeps runtime/TUI contracts separate: only runtime public keys are written to
//! `harness.json{,c}`. Secrets fail closed. This is not a full settings migration
//! or multi-layer write engine.

use std::fs;
use std::path::Path;

use thiserror::Error;

use super::loader::load_config_from_file;
use super::settings_registry::{
    resolve_setting_id, setting_definition, SettingMutability, SettingSensitivity, SettingSurface,
};

/// Errors from settings editor project-file writes.
#[derive(Debug, Error)]
pub enum SettingWriteError {
    #[error("setting `{0}` is not registered")]
    UnknownSetting(String),
    #[error("setting `{0}` is secret and cannot be edited")]
    SecretSetting(String),
    #[error("setting `{0}` is not editable")]
    NotEditable(String),
    #[error("setting `{0}` does not support project-file write yet")]
    UnsupportedWrite(String),
    #[error("setting `{0}` belongs to the TUI surface; use tui.json write path")]
    WrongSurface(String),
    #[error("failed to read config file {path}: {source}")]
    ReadFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write config file {path}: {source}")]
    WriteFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config: {0}")]
    Parse(String),
    #[error("config must be a JSON object at top level")]
    InvalidRoot,
    #[error("failed to reload effective config from {path}: {reason}")]
    Reload { path: String, reason: String },
}

const HASHLINE_EDIT_ID: &str = "hashline_edit";
const COMPACTION_ENABLED_ID: &str = "runtime.compaction.enabled";
const COMPACTION_AUTO_RETRY_OVERFLOW_ID: &str = "runtime.compaction.auto_retry_overflow";
const COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID: &str =
    "runtime.compaction.structured_summary_contract";
const COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID: &str = "runtime.compaction.estimated_token_triggers";
const DETERMINISTIC_ENABLED_ID: &str = "runtime.deterministic.enabled";

struct BoolSetting {
    id: &'static str,
    parents: &'static [&'static str],
    key: &'static str,
    legacy_root_alias: Option<&'static str>,
    default: bool,
    effective: fn(&crate::config::HarnessConfig) -> bool,
}

static BOOL_SETTINGS: [BoolSetting; 6] = [
    BoolSetting {
        id: HASHLINE_EDIT_ID,
        parents: &[],
        key: HASHLINE_EDIT_ID,
        legacy_root_alias: Some("hashlineEdit"),
        default: true,
        effective: |config| config.hashline_edit,
    },
    BoolSetting {
        id: COMPACTION_ENABLED_ID,
        parents: &["runtime", "compaction"],
        key: "enabled",
        legacy_root_alias: None,
        default: true,
        effective: |config| config.runtime.compaction.enabled,
    },
    BoolSetting {
        id: COMPACTION_AUTO_RETRY_OVERFLOW_ID,
        parents: &["runtime", "compaction"],
        key: "auto_retry_overflow",
        legacy_root_alias: None,
        default: true,
        effective: |config| config.runtime.compaction.auto_retry_overflow,
    },
    BoolSetting {
        id: COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID,
        parents: &["runtime", "compaction"],
        key: "structured_summary_contract",
        legacy_root_alias: None,
        default: true,
        effective: |config| config.runtime.compaction.structured_summary_contract,
    },
    BoolSetting {
        id: COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID,
        parents: &["runtime", "compaction"],
        key: "estimated_token_triggers",
        legacy_root_alias: None,
        default: true,
        effective: |config| config.runtime.compaction.estimated_token_triggers,
    },
    BoolSetting {
        id: DETERMINISTIC_ENABLED_ID,
        parents: &["runtime", "deterministic"],
        key: "enabled",
        legacy_root_alias: None,
        default: false,
        effective: |config| config.runtime.deterministic.enabled,
    },
];

fn guard_writable(setting_id: &str) -> Result<(), SettingWriteError> {
    let Some(def) = setting_definition(setting_id) else {
        return Err(SettingWriteError::UnknownSetting(setting_id.to_string()));
    };
    if matches!(def.sensitivity, SettingSensitivity::Secret) {
        return Err(SettingWriteError::SecretSetting(setting_id.to_string()));
    }
    if !matches!(def.mutability, SettingMutability::Editable) {
        return Err(SettingWriteError::NotEditable(setting_id.to_string()));
    }
    if matches!(def.surface, SettingSurface::Tui) {
        return Err(SettingWriteError::WrongSurface(setting_id.to_string()));
    }
    Ok(())
}

fn parse_root_object(
    path: &Path,
) -> Result<serde_json::Map<String, serde_json::Value>, SettingWriteError> {
    let raw = fs::read_to_string(path).map_err(|source| SettingWriteError::ReadFile {
        path: path.display().to_string(),
        source,
    })?;
    let value: serde_json::Value =
        json5::from_str(&raw).map_err(|err| SettingWriteError::Parse(err.to_string()))?;
    value
        .as_object()
        .cloned()
        .ok_or(SettingWriteError::InvalidRoot)
}

fn write_root_object(
    path: &Path,
    root: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), SettingWriteError> {
    let body = serde_json::to_vec_pretty(&serde_json::Value::Object(root.clone()))
        .map_err(|err| SettingWriteError::Parse(err.to_string()))?;
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, &body).map_err(|source| SettingWriteError::WriteFile {
        path: temp_path.display().to_string(),
        source,
    })?;
    fs::rename(&temp_path, path).map_err(|source| SettingWriteError::WriteFile {
        path: path.display().to_string(),
        source,
    })?;
    Ok(())
}

fn reload_config(path: &Path) -> Result<crate::config::HarnessConfig, SettingWriteError> {
    load_config_from_file(path).map_err(|err| SettingWriteError::Reload {
        path: path.display().to_string(),
        reason: err.to_string(),
    })
}

fn ensure_object_mut<'a>(
    parent: &'a mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<&'a mut serde_json::Map<String, serde_json::Value>, SettingWriteError> {
    if !parent.contains_key(key) {
        parent.insert(
            key.to_string(),
            serde_json::Value::Object(serde_json::Map::new()),
        );
    }
    parent
        .get_mut(key)
        .and_then(|value| value.as_object_mut())
        .ok_or_else(|| {
            SettingWriteError::Parse(format!("config key `{key}` must be a JSON object"))
        })
}

fn bool_setting(setting_id: &str) -> Result<&'static BoolSetting, SettingWriteError> {
    let canonical = resolve_setting_id(setting_id).unwrap_or(setting_id);
    guard_writable(canonical)?;
    BOOL_SETTINGS
        .iter()
        .find(|setting| setting.id == canonical)
        .ok_or_else(|| SettingWriteError::UnsupportedWrite(canonical.to_string()))
}

fn read_bool(path: &Path, setting: &BoolSetting) -> Result<bool, SettingWriteError> {
    Ok((setting.effective)(&reload_config(path)?))
}

fn write_bool(path: &Path, setting: &BoolSetting, value: bool) -> Result<bool, SettingWriteError> {
    let mut root = parse_root_object(path)?;
    let mut object = &mut root;
    for parent in setting.parents {
        object = ensure_object_mut(object, parent)?;
    }
    object.insert(setting.key.to_string(), serde_json::Value::Bool(value));
    if let Some(alias) = setting.legacy_root_alias {
        root.remove(alias);
    }
    write_root_object(path, &root)?;
    read_bool(path, setting)
}

fn reset_bool(path: &Path, setting: &BoolSetting) -> Result<bool, SettingWriteError> {
    let def = setting_definition(setting.id)
        .ok_or_else(|| SettingWriteError::UnknownSetting(setting.id.to_string()))?;
    let default = def
        .default_value
        .map(|raw| raw == "true")
        .unwrap_or(setting.default);
    write_bool(path, setting, default)
}

/// Read the effective `hashline_edit` value after loading a project runtime config file.
pub fn read_effective_hashline_edit(path: &Path) -> Result<bool, SettingWriteError> {
    read_bool(path, bool_setting(HASHLINE_EDIT_ID)?)
}

/// Read the effective `runtime.compaction.enabled` value after loading a project runtime config.
pub fn read_effective_compaction_enabled(path: &Path) -> Result<bool, SettingWriteError> {
    read_bool(path, bool_setting(COMPACTION_ENABLED_ID)?)
}

/// Read the effective `runtime.compaction.auto_retry_overflow` value after loading a project runtime config.
pub fn read_effective_compaction_auto_retry_overflow(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    read_bool(path, bool_setting(COMPACTION_AUTO_RETRY_OVERFLOW_ID)?)
}

/// Read the effective `runtime.deterministic.enabled` value after loading a project runtime config.
pub fn read_effective_deterministic_enabled(path: &Path) -> Result<bool, SettingWriteError> {
    read_bool(path, bool_setting(DETERMINISTIC_ENABLED_ID)?)
}

/// Persist `hashline_edit` to a project runtime config file and return the reloaded value.
pub fn write_project_hashline_edit(path: &Path, value: bool) -> Result<bool, SettingWriteError> {
    write_bool(path, bool_setting(HASHLINE_EDIT_ID)?, value)
}

/// Persist `runtime.compaction.enabled` under `runtime.compaction.enabled` and reload.
pub fn write_project_compaction_enabled(
    path: &Path,
    value: bool,
) -> Result<bool, SettingWriteError> {
    write_bool(path, bool_setting(COMPACTION_ENABLED_ID)?, value)
}

/// Persist `runtime.compaction.auto_retry_overflow` and reload.
pub fn write_project_compaction_auto_retry_overflow(
    path: &Path,
    value: bool,
) -> Result<bool, SettingWriteError> {
    write_bool(
        path,
        bool_setting(COMPACTION_AUTO_RETRY_OVERFLOW_ID)?,
        value,
    )
}

/// Read the effective `runtime.compaction.structured_summary_contract` value after loading a project runtime config.
pub fn read_effective_compaction_structured_summary_contract(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    read_bool(
        path,
        bool_setting(COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID)?,
    )
}

/// Persist `runtime.compaction.structured_summary_contract` and reload.
pub fn write_project_compaction_structured_summary_contract(
    path: &Path,
    value: bool,
) -> Result<bool, SettingWriteError> {
    write_bool(
        path,
        bool_setting(COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID)?,
        value,
    )
}

/// Reset `runtime.compaction.structured_summary_contract` to the registry default (`true`) and reload.
pub fn reset_project_compaction_structured_summary_contract(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    reset_bool(
        path,
        bool_setting(COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID)?,
    )
}

/// Read the effective `runtime.compaction.estimated_token_triggers` value after loading a project runtime config.
pub fn read_effective_compaction_estimated_token_triggers(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    read_bool(path, bool_setting(COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID)?)
}

/// Persist `runtime.compaction.estimated_token_triggers` and reload.
pub fn write_project_compaction_estimated_token_triggers(
    path: &Path,
    value: bool,
) -> Result<bool, SettingWriteError> {
    write_bool(
        path,
        bool_setting(COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID)?,
        value,
    )
}

/// Reset `runtime.compaction.estimated_token_triggers` to the registry default (`true`) and reload.
pub fn reset_project_compaction_estimated_token_triggers(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    reset_bool(path, bool_setting(COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID)?)
}

/// Persist `runtime.deterministic.enabled` under `runtime.deterministic.enabled` and reload.
pub fn write_project_deterministic_enabled(
    path: &Path,
    value: bool,
) -> Result<bool, SettingWriteError> {
    write_bool(path, bool_setting(DETERMINISTIC_ENABLED_ID)?, value)
}

/// Reset `hashline_edit` to the registry default (`true`) in the project file and reload.
pub fn reset_project_hashline_edit(path: &Path) -> Result<bool, SettingWriteError> {
    reset_bool(path, bool_setting(HASHLINE_EDIT_ID)?)
}

/// Reset `runtime.compaction.enabled` to the registry default (`true`) and reload.
pub fn reset_project_compaction_enabled(path: &Path) -> Result<bool, SettingWriteError> {
    reset_bool(path, bool_setting(COMPACTION_ENABLED_ID)?)
}

/// Reset `runtime.compaction.auto_retry_overflow` to the registry default (`true`) and reload.
pub fn reset_project_compaction_auto_retry_overflow(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    reset_bool(path, bool_setting(COMPACTION_AUTO_RETRY_OVERFLOW_ID)?)
}

/// Reset `runtime.deterministic.enabled` to the registry default (`false`) and reload.
pub fn reset_project_deterministic_enabled(path: &Path) -> Result<bool, SettingWriteError> {
    reset_bool(path, bool_setting(DETERMINISTIC_ENABLED_ID)?)
}

/// Fail-closed write entry for supported bool settings.
pub fn write_project_setting_bool(
    path: &Path,
    setting_id: &str,
    value: bool,
) -> Result<bool, SettingWriteError> {
    write_bool(path, bool_setting(setting_id)?, value)
}

/// Fail-closed reset entry for supported bool settings.
pub fn reset_project_setting_to_default(
    path: &Path,
    setting_id: &str,
) -> Result<String, SettingWriteError> {
    Ok(reset_bool(path, bool_setting(setting_id)?)?.to_string())
}

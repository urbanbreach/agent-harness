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

fn reload_hashline_edit(path: &Path) -> Result<bool, SettingWriteError> {
    Ok(reload_config(path)?.hashline_edit)
}

fn reload_compaction_enabled(path: &Path) -> Result<bool, SettingWriteError> {
    Ok(reload_config(path)?.runtime.compaction.enabled)
}

fn reload_compaction_auto_retry_overflow(path: &Path) -> Result<bool, SettingWriteError> {
    Ok(reload_config(path)?.runtime.compaction.auto_retry_overflow)
}

fn reload_compaction_structured_summary_contract(path: &Path) -> Result<bool, SettingWriteError> {
    Ok(reload_config(path)?
        .runtime
        .compaction
        .structured_summary_contract)
}

fn reload_compaction_estimated_token_triggers(path: &Path) -> Result<bool, SettingWriteError> {
    Ok(reload_config(path)?
        .runtime
        .compaction
        .estimated_token_triggers)
}

fn reload_deterministic_enabled(path: &Path) -> Result<bool, SettingWriteError> {
    Ok(reload_config(path)?.runtime.deterministic.enabled)
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

/// Read the effective `hashline_edit` value after loading a project runtime config file.
pub fn read_effective_hashline_edit(path: &Path) -> Result<bool, SettingWriteError> {
    guard_writable(HASHLINE_EDIT_ID)?;
    reload_hashline_edit(path)
}

/// Read the effective `runtime.compaction.enabled` value after loading a project runtime config.
pub fn read_effective_compaction_enabled(path: &Path) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_ENABLED_ID)?;
    reload_compaction_enabled(path)
}

/// Read the effective `runtime.compaction.auto_retry_overflow` value after loading a project runtime config.
pub fn read_effective_compaction_auto_retry_overflow(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_AUTO_RETRY_OVERFLOW_ID)?;
    reload_compaction_auto_retry_overflow(path)
}

/// Read the effective `runtime.deterministic.enabled` value after loading a project runtime config.
pub fn read_effective_deterministic_enabled(path: &Path) -> Result<bool, SettingWriteError> {
    guard_writable(DETERMINISTIC_ENABLED_ID)?;
    reload_deterministic_enabled(path)
}

/// Persist `hashline_edit` to a project runtime config file and return the reloaded value.
pub fn write_project_hashline_edit(path: &Path, value: bool) -> Result<bool, SettingWriteError> {
    guard_writable(HASHLINE_EDIT_ID)?;
    let mut root = parse_root_object(path)?;
    root.insert(HASHLINE_EDIT_ID.to_string(), serde_json::Value::Bool(value));
    root.remove("hashlineEdit");
    write_root_object(path, &root)?;
    reload_hashline_edit(path)
}

/// Persist `runtime.compaction.enabled` under `runtime.compaction.enabled` and reload.
pub fn write_project_compaction_enabled(
    path: &Path,
    value: bool,
) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_ENABLED_ID)?;
    let mut root = parse_root_object(path)?;
    let runtime = ensure_object_mut(&mut root, "runtime")?;
    let compaction = ensure_object_mut(runtime, "compaction")?;
    compaction.insert("enabled".to_string(), serde_json::Value::Bool(value));
    write_root_object(path, &root)?;
    reload_compaction_enabled(path)
}

/// Persist `runtime.compaction.auto_retry_overflow` and reload.
pub fn write_project_compaction_auto_retry_overflow(
    path: &Path,
    value: bool,
) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_AUTO_RETRY_OVERFLOW_ID)?;
    let mut root = parse_root_object(path)?;
    let runtime = ensure_object_mut(&mut root, "runtime")?;
    let compaction = ensure_object_mut(runtime, "compaction")?;
    compaction.insert(
        "auto_retry_overflow".to_string(),
        serde_json::Value::Bool(value),
    );
    write_root_object(path, &root)?;
    reload_compaction_auto_retry_overflow(path)
}

/// Read the effective `runtime.compaction.structured_summary_contract` value after loading a project runtime config.
pub fn read_effective_compaction_structured_summary_contract(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID)?;
    reload_compaction_structured_summary_contract(path)
}

/// Persist `runtime.compaction.structured_summary_contract` and reload.
pub fn write_project_compaction_structured_summary_contract(
    path: &Path,
    value: bool,
) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID)?;
    let mut root = parse_root_object(path)?;
    let runtime = ensure_object_mut(&mut root, "runtime")?;
    let compaction = ensure_object_mut(runtime, "compaction")?;
    compaction.insert(
        "structured_summary_contract".to_string(),
        serde_json::Value::Bool(value),
    );
    write_root_object(path, &root)?;
    reload_compaction_structured_summary_contract(path)
}

/// Reset `runtime.compaction.structured_summary_contract` to the registry default (`true`) and reload.
pub fn reset_project_compaction_structured_summary_contract(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID)?;
    let def = setting_definition(COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID).ok_or_else(|| {
        SettingWriteError::UnknownSetting(COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID.to_string())
    })?;
    let default = def.default_value.map(|raw| raw == "true").unwrap_or(true);
    write_project_compaction_structured_summary_contract(path, default)
}

/// Read the effective `runtime.compaction.estimated_token_triggers` value after loading a project runtime config.
pub fn read_effective_compaction_estimated_token_triggers(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID)?;
    reload_compaction_estimated_token_triggers(path)
}

/// Persist `runtime.compaction.estimated_token_triggers` and reload.
pub fn write_project_compaction_estimated_token_triggers(
    path: &Path,
    value: bool,
) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID)?;
    let mut root = parse_root_object(path)?;
    let runtime = ensure_object_mut(&mut root, "runtime")?;
    let compaction = ensure_object_mut(runtime, "compaction")?;
    compaction.insert(
        "estimated_token_triggers".to_string(),
        serde_json::Value::Bool(value),
    );
    write_root_object(path, &root)?;
    reload_compaction_estimated_token_triggers(path)
}

/// Reset `runtime.compaction.estimated_token_triggers` to the registry default (`true`) and reload.
pub fn reset_project_compaction_estimated_token_triggers(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID)?;
    let def = setting_definition(COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID).ok_or_else(|| {
        SettingWriteError::UnknownSetting(COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID.to_string())
    })?;
    let default = def.default_value.map(|raw| raw == "true").unwrap_or(true);
    write_project_compaction_estimated_token_triggers(path, default)
}

/// Persist `runtime.deterministic.enabled` under `runtime.deterministic.enabled` and reload.
pub fn write_project_deterministic_enabled(
    path: &Path,
    value: bool,
) -> Result<bool, SettingWriteError> {
    guard_writable(DETERMINISTIC_ENABLED_ID)?;
    let mut root = parse_root_object(path)?;
    let runtime = ensure_object_mut(&mut root, "runtime")?;
    let deterministic = ensure_object_mut(runtime, "deterministic")?;
    deterministic.insert("enabled".to_string(), serde_json::Value::Bool(value));
    write_root_object(path, &root)?;
    reload_deterministic_enabled(path)
}

/// Reset `hashline_edit` to the registry default (`true`) in the project file and reload.
pub fn reset_project_hashline_edit(path: &Path) -> Result<bool, SettingWriteError> {
    guard_writable(HASHLINE_EDIT_ID)?;
    let def = setting_definition(HASHLINE_EDIT_ID)
        .ok_or_else(|| SettingWriteError::UnknownSetting(HASHLINE_EDIT_ID.to_string()))?;
    let default = def.default_value.map(|raw| raw == "true").unwrap_or(true);
    write_project_hashline_edit(path, default)
}

/// Reset `runtime.compaction.enabled` to the registry default (`true`) and reload.
pub fn reset_project_compaction_enabled(path: &Path) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_ENABLED_ID)?;
    let def = setting_definition(COMPACTION_ENABLED_ID)
        .ok_or_else(|| SettingWriteError::UnknownSetting(COMPACTION_ENABLED_ID.to_string()))?;
    let default = def.default_value.map(|raw| raw == "true").unwrap_or(true);
    write_project_compaction_enabled(path, default)
}

/// Reset `runtime.compaction.auto_retry_overflow` to the registry default (`true`) and reload.
pub fn reset_project_compaction_auto_retry_overflow(
    path: &Path,
) -> Result<bool, SettingWriteError> {
    guard_writable(COMPACTION_AUTO_RETRY_OVERFLOW_ID)?;
    let def = setting_definition(COMPACTION_AUTO_RETRY_OVERFLOW_ID).ok_or_else(|| {
        SettingWriteError::UnknownSetting(COMPACTION_AUTO_RETRY_OVERFLOW_ID.to_string())
    })?;
    let default = def.default_value.map(|raw| raw == "true").unwrap_or(true);
    write_project_compaction_auto_retry_overflow(path, default)
}

/// Reset `runtime.deterministic.enabled` to the registry default (`false`) and reload.
pub fn reset_project_deterministic_enabled(path: &Path) -> Result<bool, SettingWriteError> {
    guard_writable(DETERMINISTIC_ENABLED_ID)?;
    let def = setting_definition(DETERMINISTIC_ENABLED_ID)
        .ok_or_else(|| SettingWriteError::UnknownSetting(DETERMINISTIC_ENABLED_ID.to_string()))?;
    let default = def.default_value.map(|raw| raw == "true").unwrap_or(false);
    write_project_deterministic_enabled(path, default)
}

/// Fail-closed write entry for supported bool settings.
pub fn write_project_setting_bool(
    path: &Path,
    setting_id: &str,
    value: bool,
) -> Result<bool, SettingWriteError> {
    let canonical = resolve_setting_id(setting_id).unwrap_or(setting_id);
    guard_writable(canonical)?;
    match canonical {
        HASHLINE_EDIT_ID => write_project_hashline_edit(path, value),
        COMPACTION_ENABLED_ID => write_project_compaction_enabled(path, value),
        COMPACTION_AUTO_RETRY_OVERFLOW_ID => {
            write_project_compaction_auto_retry_overflow(path, value)
        }
        COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID => {
            write_project_compaction_structured_summary_contract(path, value)
        }
        COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID => {
            write_project_compaction_estimated_token_triggers(path, value)
        }
        DETERMINISTIC_ENABLED_ID => write_project_deterministic_enabled(path, value),
        other => Err(SettingWriteError::UnsupportedWrite(other.to_string())),
    }
}

/// Fail-closed reset entry for supported bool settings.
pub fn reset_project_setting_to_default(
    path: &Path,
    setting_id: &str,
) -> Result<String, SettingWriteError> {
    let canonical = resolve_setting_id(setting_id).unwrap_or(setting_id);
    guard_writable(canonical)?;
    match canonical {
        HASHLINE_EDIT_ID => {
            let value = reset_project_hashline_edit(path)?;
            Ok(if value { "true" } else { "false" }.to_string())
        }
        COMPACTION_ENABLED_ID => {
            let value = reset_project_compaction_enabled(path)?;
            Ok(if value { "true" } else { "false" }.to_string())
        }
        COMPACTION_AUTO_RETRY_OVERFLOW_ID => {
            let value = reset_project_compaction_auto_retry_overflow(path)?;
            Ok(if value { "true" } else { "false" }.to_string())
        }
        COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID => {
            let value = reset_project_compaction_structured_summary_contract(path)?;
            Ok(if value { "true" } else { "false" }.to_string())
        }
        COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID => {
            let value = reset_project_compaction_estimated_token_triggers(path)?;
            Ok(if value { "true" } else { "false" }.to_string())
        }
        DETERMINISTIC_ENABLED_ID => {
            let value = reset_project_deterministic_enabled(path)?;
            Ok(if value { "true" } else { "false" }.to_string())
        }
        other => Err(SettingWriteError::UnsupportedWrite(other.to_string())),
    }
}

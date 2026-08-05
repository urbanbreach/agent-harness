use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::TaskGateError;
use crate::tui_fidelity_compare::hash_bytes;

pub(super) fn read_text(path: &Path) -> Result<String, TaskGateError> {
    fs::read_to_string(path).map_err(|error| io_error(path, error))
}

pub(super) fn read_json(path: &Path) -> Result<Value, TaskGateError> {
    let text = read_text(path)?;
    serde_json::from_str(&text).map_err(|error| TaskGateError::Json {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

pub(super) fn write_unique_json(path: &Path, value: &Value) -> Result<(), TaskGateError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| TaskGateError::Invalid(error.to_string()))?;
    if path.exists() {
        return Err(TaskGateError::Invalid(format!(
            "receipt replay is rejected: {}",
            path.display()
        )));
    }
    atomic_create(path, &bytes)
}

pub(super) fn atomic_create(path: &Path, bytes: &[u8]) -> Result<(), TaskGateError> {
    let parent = path
        .parent()
        .ok_or_else(|| TaskGateError::Invalid("receipt has no parent".to_owned()))?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let temporary = path.with_extension("task-gate-tmp");
    if temporary.exists() {
        return Err(TaskGateError::Invalid(format!(
            "stale temporary receipt exists: {}",
            temporary.display()
        )));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| io_error(&temporary, error))?;
    file.write_all(bytes)
        .map_err(|error| io_error(&temporary, error))?;
    file.sync_all()
        .map_err(|error| io_error(&temporary, error))?;
    fs::rename(&temporary, path).map_err(|error| io_error(path, error))
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), TaskGateError> {
    let parent = path
        .parent()
        .ok_or_else(|| TaskGateError::Invalid("plan has no parent".to_owned()))?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let temporary = path.with_extension("task-gate-tmp");
    fs::write(&temporary, bytes).map_err(|error| io_error(&temporary, error))?;
    fs::rename(&temporary, path).map_err(|error| io_error(path, error))
}

pub(super) fn atomic_write_json(path: &Path, value: &Value) -> Result<(), TaskGateError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| TaskGateError::Invalid(error.to_string()))?;
    atomic_write(path, &bytes)
}

pub(super) fn digest(bytes: &[u8]) -> Result<String, TaskGateError> {
    hash_bytes(bytes).map_err(|error| TaskGateError::Invalid(error.to_string()))
}

pub(super) fn now_millis() -> Result<u128, TaskGateError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|error| TaskGateError::Invalid(format!("system clock: {error}")))
}

fn io_error(path: &Path, error: impl fmt::Display) -> TaskGateError {
    TaskGateError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    }
}

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::tui_fidelity_compare::hash_bytes;
use crate::tui_fidelity_obligation::VerificationKey;

use super::{KeyState, StagingError, STATE_SCHEMA};

static WRITE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct KeyRecord {
    pub schema_version: String,
    pub key_id: String,
    pub state: KeyState,
    pub failed_attempts: u8,
    pub detail: Option<String>,
    pub artifact_path: Option<String>,
    pub artifact_sha256: Option<String>,
}

impl KeyRecord {
    pub(super) fn pending(key_id: String) -> Self {
        Self {
            schema_version: STATE_SCHEMA.to_owned(),
            key_id,
            state: KeyState::Pending,
            failed_attempts: 0,
            detail: None,
            artifact_path: None,
            artifact_sha256: None,
        }
    }
}

#[derive(Serialize)]
pub(super) struct SealManifest {
    pub schema_version: String,
    pub candidate: String,
    pub attempt: String,
    pub records: Vec<KeyRecord>,
}

pub(super) fn read_record(path: &Path) -> Result<KeyRecord, StagingError> {
    serde_json::from_slice(&read(path)?).map_err(|error| StagingError::Json(error.to_string()))
}

pub(super) fn key_digest(key: &VerificationKey) -> Result<String, StagingError> {
    hash_bytes(key.stable_id()?.as_bytes())
        .map_err(|error| StagingError::Invalid(error.to_string()))
}

pub(super) fn digest_file(path: &Path) -> Result<String, StagingError> {
    hash_bytes(&read(path)?).map_err(|error| StagingError::Invalid(error.to_string()))
}

pub(super) fn validate_artifact(root: &Path, record: &KeyRecord) -> Result<(), StagingError> {
    let path = record
        .artifact_path
        .as_deref()
        .ok_or_else(|| StagingError::Invalid("passed key has no artifact path".to_owned()))?;
    let expected = record
        .artifact_sha256
        .as_deref()
        .ok_or_else(|| StagingError::Invalid("passed key has no artifact digest".to_owned()))?;
    if digest_file(&root.join(path))? == expected {
        Ok(())
    } else {
        Err(StagingError::Invalid(
            "passed artifact digest differs".to_owned(),
        ))
    }
}

pub(super) fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), StagingError> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|error| StagingError::Json(error.to_string()))?;
    let temporary = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        WRITE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temporary, bytes).map_err(|error| io_error(&temporary, error))?;
    fs::rename(&temporary, path).map_err(|error| io_error(path, error))
}

pub(super) fn validate_component(
    value: &str,
    name: &str,
    digest: bool,
) -> Result<(), StagingError> {
    let valid = value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if value.is_empty() || !valid || (digest && value.len() < 40) {
        Err(StagingError::Invalid(format!("invalid {name}")))
    } else {
        Ok(())
    }
}

pub(super) fn io_error(path: &Path, error: impl std::fmt::Display) -> StagingError {
    StagingError::Io {
        path: PathBuf::from(path),
        detail: error.to_string(),
    }
}

fn read(path: &Path) -> Result<Vec<u8>, StagingError> {
    fs::read(path).map_err(|error| io_error(path, error))
}

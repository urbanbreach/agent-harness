//! Atomic load/save for the durable extension descriptor registry.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::extension_manifest::{
    load_extension_manifest_from_path, ExtensionManifestSummary, EXTENSION_MANIFEST_FILE_NAME,
};

use super::{ExtensionRegistryEntry, ExtensionRegistryError, STORE_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExtensionRegistryDocument {
    pub version: u32,
    pub entries: BTreeMap<String, ExtensionRegistryEntry>,
}

impl ExtensionRegistryDocument {
    pub(super) fn empty() -> Self {
        Self {
            version: STORE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

pub(super) fn load_or_empty(
    path: &Path,
) -> Result<ExtensionRegistryDocument, ExtensionRegistryError> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let doc: ExtensionRegistryDocument =
                serde_json::from_str(&raw).map_err(|err| ExtensionRegistryError::Parse {
                    path: path.display().to_string(),
                    detail: err.to_string(),
                })?;
            if doc.version != STORE_VERSION {
                return Err(ExtensionRegistryError::UnsupportedVersion {
                    path: path.display().to_string(),
                    version: doc.version,
                });
            }
            Ok(doc)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(ExtensionRegistryDocument::empty()),
        Err(err) => Err(ExtensionRegistryError::Read {
            path: path.display().to_string(),
            source: err,
        }),
    }
}

pub(super) fn save(
    path: &Path,
    doc: &ExtensionRegistryDocument,
) -> Result<(), ExtensionRegistryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ExtensionRegistryError::CreateParent {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let body = serde_json::to_vec_pretty(doc).map_err(|err| ExtensionRegistryError::Write {
        path: path.display().to_string(),
        source: io::Error::other(err),
    })?;
    let unique = now_unix_ms();
    let temp_path = path.with_extension(format!("json.tmp.{}.{}", std::process::id(), unique));
    write_file_atomically(&temp_path, path, &body)
}

fn write_file_atomically(
    temp_path: &Path,
    final_path: &Path,
    body: &[u8],
) -> Result<(), ExtensionRegistryError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .map_err(|source| ExtensionRegistryError::Write {
            path: temp_path.display().to_string(),
            source,
        })?;
    file.write_all(body)
        .and_then(|_| file.sync_all())
        .map_err(|source| ExtensionRegistryError::Write {
            path: temp_path.display().to_string(),
            source,
        })?;
    drop(file);
    fs::rename(temp_path, final_path).map_err(|source| ExtensionRegistryError::Replace {
        path: final_path.display().to_string(),
        source,
    })?;
    Ok(())
}

pub(super) fn workspace_relative(
    workspace_root: &Path,
    absolute: &Path,
) -> Result<String, ExtensionRegistryError> {
    let key = absolute
        .strip_prefix(workspace_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .map_err(|_| ExtensionRegistryError::InvalidPath {
            path: absolute.display().to_string(),
        })?;
    if key.is_empty() || key.split('/').any(|seg| seg == "..") {
        return Err(ExtensionRegistryError::InvalidPath { path: key });
    }
    Ok(key)
}

pub(super) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

pub(super) fn resolve_manifest_path(
    scan_root: &Path,
    summary: &ExtensionManifestSummary,
) -> Result<PathBuf, ExtensionRegistryError> {
    let direct = scan_root.join(EXTENSION_MANIFEST_FILE_NAME);
    if direct.is_file() {
        if let Ok(manifest) = load_extension_manifest_from_path(&direct) {
            if manifest.id == summary.extension_id {
                return Ok(direct);
            }
        }
    }
    let Ok(entries) = fs::read_dir(scan_root) else {
        return Err(ExtensionRegistryError::InvalidPath {
            path: scan_root.display().to_string(),
        });
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let candidate = path.join(EXTENSION_MANIFEST_FILE_NAME);
        if !candidate.is_file() {
            continue;
        }
        if let Ok(manifest) = load_extension_manifest_from_path(&candidate) {
            if manifest.id == summary.extension_id {
                return Ok(candidate);
            }
        }
    }
    Err(ExtensionRegistryError::InvalidPath {
        path: format!(
            "{} (missing manifest for {})",
            scan_root.display(),
            summary.extension_id
        ),
    })
}

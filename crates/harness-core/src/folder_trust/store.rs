//! Workspace-scoped folder-trust persistence (atomic JSON, no secret fields).

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::FolderTrustDecision;

/// Relative store path under a workspace root.
pub const FOLDER_TRUST_RELATIVE_PATH: &str = ".agent-harness/folder-trust.json";

const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FolderTrustDocument {
    version: u32,
    entries: BTreeMap<String, FolderTrustEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FolderTrustEntry {
    decision: FolderTrustDecision,
    updated_at_unix_ms: u64,
}

impl FolderTrustDocument {
    fn empty() -> Self {
        Self {
            version: STORE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

/// Redacted summary safe for logs / doctor / support (no secrets by construction).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderTrustSummary {
    pub workspace_key: String,
    pub decision: Option<FolderTrustDecision>,
    pub store_path: String,
    pub entry_count: usize,
}

/// Failures loading or updating the trust store.
#[derive(Debug, Error)]
pub enum FolderTrustError {
    #[error("failed to create folder-trust parent directory {path}: {source}")]
    CreateParent {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to read folder-trust store {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse folder-trust store {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("unsupported folder-trust store version {version} in {path}")]
    UnsupportedVersion { path: String, version: u32 },
    #[error("failed to write folder-trust store {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to replace folder-trust store {path}: {source}")]
    Replace {
        path: String,
        #[source]
        source: io::Error,
    },
}

/// Persistent folder-trust store for one on-disk file.
#[derive(Debug, Clone)]
pub struct FolderTrustStore {
    path: PathBuf,
}

impl FolderTrustStore {
    /// Open a store at an explicit path (does not require the file to exist yet).
    pub fn open(store_path: impl Into<PathBuf>) -> Self {
        Self {
            path: store_path.into(),
        }
    }

    /// Default store path for a workspace root.
    pub fn default_path_for_workspace(workspace_root: &Path) -> PathBuf {
        workspace_root.join(FOLDER_TRUST_RELATIVE_PATH)
    }

    /// Open the default workspace-scoped store.
    pub fn for_workspace(workspace_root: &Path) -> Self {
        Self::open(Self::default_path_for_workspace(workspace_root))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the decision for `workspace_root`, if any.
    pub fn get(
        &self,
        workspace_root: &Path,
    ) -> Result<Option<FolderTrustDecision>, FolderTrustError> {
        let doc = self.load_or_empty()?;
        let key = workspace_key(workspace_root);
        Ok(doc.entries.get(&key).map(|entry| entry.decision))
    }

    /// Persist allow/deny for `workspace_root` (atomic replace).
    pub fn set(
        &self,
        workspace_root: &Path,
        decision: FolderTrustDecision,
    ) -> Result<(), FolderTrustError> {
        let mut doc = self.load_or_empty()?;
        let key = workspace_key(workspace_root);
        doc.entries.insert(
            key,
            FolderTrustEntry {
                decision,
                updated_at_unix_ms: now_unix_ms(),
            },
        );
        self.save(&doc)
    }

    /// Redacted summary for a workspace (safe for logs / doctor).
    pub fn summarize(&self, workspace_root: &Path) -> Result<FolderTrustSummary, FolderTrustError> {
        let doc = self.load_or_empty()?;
        let key = workspace_key(workspace_root);
        let decision = doc.entries.get(&key).map(|entry| entry.decision);
        Ok(FolderTrustSummary {
            workspace_key: key,
            decision,
            store_path: self.path.display().to_string(),
            entry_count: doc.entries.len(),
        })
    }

    fn load_or_empty(&self) -> Result<FolderTrustDocument, FolderTrustError> {
        match fs::read_to_string(&self.path) {
            Ok(raw) => {
                let doc: FolderTrustDocument =
                    serde_json::from_str(&raw).map_err(|err| FolderTrustError::Parse {
                        path: self.path.display().to_string(),
                        detail: err.to_string(),
                    })?;
                if doc.version != STORE_VERSION {
                    return Err(FolderTrustError::UnsupportedVersion {
                        path: self.path.display().to_string(),
                        version: doc.version,
                    });
                }
                Ok(doc)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(FolderTrustDocument::empty()),
            Err(err) => Err(FolderTrustError::Read {
                path: self.path.display().to_string(),
                source: err,
            }),
        }
    }

    fn save(&self, doc: &FolderTrustDocument) -> Result<(), FolderTrustError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| FolderTrustError::CreateParent {
                path: parent.display().to_string(),
                source,
            })?;
        }

        let body = serde_json::to_vec_pretty(doc).map_err(|err| FolderTrustError::Write {
            path: self.path.display().to_string(),
            source: io::Error::other(err),
        })?;

        let unique = now_unix_ms();
        let temp_path =
            self.path
                .with_extension(format!("json.tmp.{}.{}", std::process::id(), unique));

        write_file_atomically(&temp_path, &self.path, &body)
    }
}

fn workspace_key(workspace_root: &Path) -> String {
    let canonical = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    canonical.display().to_string()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn write_file_atomically(
    temp_path: &Path,
    final_path: &Path,
    body: &[u8],
) -> Result<(), FolderTrustError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .map_err(|source| FolderTrustError::Write {
            path: temp_path.display().to_string(),
            source,
        })?;
    restrict_file_permissions(temp_path).map_err(|source| FolderTrustError::Write {
        path: temp_path.display().to_string(),
        source,
    })?;
    file.write_all(body)
        .and_then(|_| file.sync_all())
        .map_err(|source| FolderTrustError::Write {
            path: temp_path.display().to_string(),
            source,
        })?;
    drop(file);
    fs::rename(temp_path, final_path).map_err(|source| FolderTrustError::Replace {
        path: final_path.display().to_string(),
        source,
    })?;
    restrict_file_permissions(final_path).map_err(|source| FolderTrustError::Write {
        path: final_path.display().to_string(),
        source,
    })?;
    Ok(())
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
fn restrict_file_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

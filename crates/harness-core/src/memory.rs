//! Durable cross-session memory MVP.
//!
//! Persists key/value notes under `.agent-harness/memory/entries.json` for a
//! workspace root. Values are redacted on write using the same secret patterns
//! as the rest of the runtime. This is separate from provider-context
//! operational memory (compaction facts).

pub mod scope;
pub use scope::{MemoryScope, ScopedMemoryEntry};

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::redact::{DefaultRedactor, Redactor};

/// Relative directory under a workspace root that holds durable memory.
pub const MEMORY_RELATIVE_DIR: &str = ".agent-harness/memory";
/// Default JSON document name inside [`MEMORY_RELATIVE_DIR`].
pub const MEMORY_ENTRIES_FILE: &str = "entries.json";

const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryDocument {
    version: u32,
    entries: BTreeMap<String, MemoryEntryRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MemoryEntryRecord {
    value: String,
    updated_at_unix_ms: u64,
    #[serde(default)]
    scope: MemoryScope,
}

impl MemoryDocument {
    fn empty() -> Self {
        Self {
            version: STORE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

/// One durable memory entry (already redacted if loaded from store).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    pub updated_at_unix_ms: u64,
}

/// Failures loading or updating durable memory.
#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("memory key must be non-empty after trim")]
    EmptyKey,
    #[error("failed to create durable memory parent directory {path}: {source}")]
    CreateParent {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to read durable memory store {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse durable memory store {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("unsupported durable memory store version {version} in {path}")]
    UnsupportedVersion { path: String, version: u32 },
    #[error("failed to write durable memory store {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to replace durable memory store {path}: {source}")]
    Replace {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("memory key not found: {key}")]
    NotFound { key: String },
}

/// Workspace-scoped durable memory store (atomic JSON, redacted on write).
#[derive(Debug, Clone)]
pub struct DurableMemoryStore {
    path: PathBuf,
}

impl DurableMemoryStore {
    /// Open a store at an explicit entries file path.
    pub fn open(store_path: impl Into<PathBuf>) -> Self {
        Self {
            path: store_path.into(),
        }
    }

    /// Default entries path for a workspace root.
    pub fn default_path_for_workspace(workspace_root: &Path) -> PathBuf {
        workspace_root
            .join(MEMORY_RELATIVE_DIR)
            .join(MEMORY_ENTRIES_FILE)
    }

    /// Open the default workspace-scoped store.
    pub fn for_workspace(workspace_root: &Path) -> Self {
        Self::open(Self::default_path_for_workspace(workspace_root))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Insert or update `key` with redacted `value` and flush to disk.
    pub fn put(&self, key: &str, value: &str) -> Result<MemoryEntry, MemoryError> {
        let key = normalize_key(key)?;
        let redactor = DefaultRedactor::default();
        let value = redactor.redact_text(value);
        let updated_at_unix_ms = now_unix_ms();
        let mut doc = self.load_or_empty()?;
        doc.entries.insert(
            key.clone(),
            MemoryEntryRecord {
                value: value.clone(),
                updated_at_unix_ms,
                scope: MemoryScope::default(),
            },
        );
        self.flush(&doc)?;
        Ok(MemoryEntry {
            key,
            value,
            updated_at_unix_ms,
        })
    }

    /// Load one entry by exact key.
    pub fn get(&self, key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        let key = normalize_key(key)?;
        let doc = self.load_or_empty()?;
        Ok(doc.entries.get(&key).map(|record| MemoryEntry {
            key: key.clone(),
            value: record.value.clone(),
            updated_at_unix_ms: record.updated_at_unix_ms,
        }))
    }

    /// Search by key prefix or case-insensitive substring match on key/value.
    pub fn search(&self, query: &str) -> Result<Vec<MemoryEntry>, MemoryError> {
        let query = query.trim();
        let doc = self.load_or_empty()?;
        if query.is_empty() {
            return Ok(doc
                .entries
                .iter()
                .map(|(key, record)| MemoryEntry {
                    key: key.clone(),
                    value: record.value.clone(),
                    updated_at_unix_ms: record.updated_at_unix_ms,
                })
                .collect());
        }

        let query_lower = query.to_ascii_lowercase();
        Ok(doc
            .entries
            .iter()
            .filter(|(key, record)| {
                key.starts_with(query)
                    || key.to_ascii_lowercase().contains(&query_lower)
                    || record.value.to_ascii_lowercase().contains(&query_lower)
            })
            .map(|(key, record)| MemoryEntry {
                key: key.clone(),
                value: record.value.clone(),
                updated_at_unix_ms: record.updated_at_unix_ms,
            })
            .collect())
    }

    /// Force-load and rewrite the store (no-op when empty/missing).
    pub fn flush_existing(&self) -> Result<(), MemoryError> {
        let doc = self.load_or_empty()?;
        if doc.entries.is_empty() && !self.path.exists() {
            return Ok(());
        }
        self.flush(&doc)
    }

    fn load_or_empty(&self) -> Result<MemoryDocument, MemoryError> {
        match fs::read_to_string(&self.path) {
            Ok(raw) => {
                let doc: MemoryDocument =
                    serde_json::from_str(&raw).map_err(|err| MemoryError::Parse {
                        path: self.path.display().to_string(),
                        detail: err.to_string(),
                    })?;
                if doc.version != STORE_VERSION {
                    return Err(MemoryError::UnsupportedVersion {
                        path: self.path.display().to_string(),
                        version: doc.version,
                    });
                }
                Ok(doc)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(MemoryDocument::empty()),
            Err(err) => Err(MemoryError::Read {
                path: self.path.display().to_string(),
                source: err,
            }),
        }
    }

    fn flush(&self, doc: &MemoryDocument) -> Result<(), MemoryError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| MemoryError::CreateParent {
                path: parent.display().to_string(),
                source,
            })?;
        }

        let body = serde_json::to_vec_pretty(doc).map_err(|err| MemoryError::Write {
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

fn normalize_key(key: &str) -> Result<String, MemoryError> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(MemoryError::EmptyKey);
    }
    Ok(trimmed.to_string())
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn redact_value(value: &str) -> String {
    let redactor = DefaultRedactor::default();
    redactor.redact_text(value)
}

fn write_file_atomically(
    temp_path: &Path,
    final_path: &Path,
    body: &[u8],
) -> Result<(), MemoryError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .map_err(|source| MemoryError::Write {
            path: temp_path.display().to_string(),
            source,
        })?;
    restrict_file_permissions(temp_path).map_err(|source| MemoryError::Write {
        path: temp_path.display().to_string(),
        source,
    })?;
    file.write_all(body)
        .and_then(|_| file.sync_all())
        .map_err(|source| MemoryError::Write {
            path: temp_path.display().to_string(),
            source,
        })?;
    drop(file);
    fs::rename(temp_path, final_path).map_err(|source| MemoryError::Replace {
        path: final_path.display().to_string(),
        source,
    })?;
    restrict_file_permissions(final_path).map_err(|source| MemoryError::Write {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn put_get_survives_store_drop_and_reload() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path();

        {
            let store = DurableMemoryStore::for_workspace(workspace);
            let written = store
                .put("project.preference", "prefer nextest")
                .unwrap_or_abort();
            assert_eq!(written.key, "project.preference");
            assert_eq!(written.value, "prefer nextest");
            assert!(store.path().is_file());
        }

        let reloaded = DurableMemoryStore::for_workspace(workspace);
        let entry = reloaded
            .get("project.preference")
            .unwrap_or_abort()
            .expect("entry should survive process restart");
        assert_eq!(entry.value, "prefer nextest");
    }

    #[test]
    fn put_updates_existing_key() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = DurableMemoryStore::for_workspace(temp.path());
        store.put("note", "v1").unwrap_or_abort();
        store.put("note", "v2").unwrap_or_abort();
        let entry = store.get("note").unwrap_or_abort().unwrap_or_abort();
        assert_eq!(entry.value, "v2");
    }

    #[test]
    fn search_matches_key_prefix_and_substring() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = DurableMemoryStore::for_workspace(temp.path());
        store.put("prefs.editor", "helix").unwrap_or_abort();
        store.put("prefs.shell", "zsh").unwrap_or_abort();
        store.put("todo", "fix helix config").unwrap_or_abort();

        let prefix = store.search("prefs.").unwrap_or_abort();
        assert_eq!(prefix.len(), 2);

        let substring = store.search("helix").unwrap_or_abort();
        assert_eq!(substring.len(), 2);
        assert!(substring.iter().any(|entry| entry.key == "prefs.editor"));
        assert!(substring.iter().any(|entry| entry.key == "todo"));
    }

    #[test]
    fn put_redacts_secret_like_values() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = DurableMemoryStore::for_workspace(temp.path());
        let written = store
            .put("creds", "token sk-abcdefghijklmnopqrstuvwxyz")
            .unwrap_or_abort();
        assert!(!written.value.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(written.value.contains("[REDACTED_API_KEY]"));

        let raw = fs::read_to_string(store.path()).unwrap_or_abort();
        assert!(!raw.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(raw.contains("[REDACTED_API_KEY]"));
    }

    #[test]
    fn empty_key_is_rejected() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = DurableMemoryStore::for_workspace(temp.path());
        let err = store.put("   ", "x").expect_err("empty key");
        assert!(matches!(err, MemoryError::EmptyKey));
    }
}

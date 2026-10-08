use crate::redact::{DefaultRedactor, Redactor};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub const MEMORY_DIR: &str = "memory";
pub const MEMORY_ENTRIES_FILE: &str = "entries.json";
const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    Global,
    #[default]
    Workspace,
    Session,
}
impl MemoryScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Workspace => "workspace",
            Self::Session => "session",
        }
    }
}
pub mod scope {
    pub use super::{MemoryScope, ScopedMemoryEntry};
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    pub updated_at_unix_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedMemoryEntry {
    pub key: String,
    pub value: String,
    pub updated_at_unix_ms: u64,
    pub scope: MemoryScope,
}
impl From<ScopedMemoryEntry> for MemoryEntry {
    fn from(entry: ScopedMemoryEntry) -> Self {
        Self {
            key: entry.key,
            value: entry.value,
            updated_at_unix_ms: entry.updated_at_unix_ms,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory key must be non-empty after trim")]
    EmptyKey,
    #[error("memory key not found: {key}")]
    NotFound { key: String },
    #[error("unsupported durable memory store version {version} in {path}")]
    UnsupportedVersion { path: String, version: u32 },
    #[error("{0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    entries: BTreeMap<String, Record>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    value: String,
    updated_at_unix_ms: u64,
    #[serde(default)]
    scope: MemoryScope,
}
impl Record {
    fn entry(self, key: String) -> ScopedMemoryEntry {
        ScopedMemoryEntry {
            key,
            value: self.value,
            updated_at_unix_ms: self.updated_at_unix_ms,
            scope: self.scope,
        }
    }
}
#[derive(Debug, Clone)]
pub struct DurableMemoryStore {
    path: PathBuf,
}
impl DurableMemoryStore {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    /// Open memory under `ProjectPaths::runtime_dir()` without creating storage.
    pub fn for_runtime(runtime_dir: &Path) -> Self {
        Self::open(runtime_dir.join(MEMORY_DIR).join(MEMORY_ENTRIES_FILE))
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn put(&self, key: &str, value: &str) -> Result<MemoryEntry, MemoryError> {
        self.put_scoped(key, value, MemoryScope::Workspace)
            .map(Into::into)
    }
    pub fn put_scoped(
        &self,
        key: &str,
        value: &str,
        scope: MemoryScope,
    ) -> Result<ScopedMemoryEntry, MemoryError> {
        let key = normalize_key(key)?.to_owned();
        if value.len() > 64 * 1024 {
            return Err(MemoryError::Invalid("memory value exceeds 64 KiB"));
        }
        let record = Record {
            value: crate::redact::redact_artifact_text(value),
            updated_at_unix_ms: now(),
            scope,
        };
        self.update(move |entries| {
            entries.insert(key.clone(), record.clone());
            Ok((record.entry(key), true))
        })
    }
    pub fn get(&self, key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        self.get_scoped(key).map(|entry| entry.map(Into::into))
    }
    pub fn get_scoped(&self, key: &str) -> Result<Option<ScopedMemoryEntry>, MemoryError> {
        let key = normalize_key(key)?;
        Ok(self
            .load()?
            .entries
            .remove(key)
            .map(|record| record.entry(key.into())))
    }
    pub fn search(&self, query: &str) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(self
            .search_scoped(query, None)?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub fn search_scoped(
        &self,
        query: &str,
        scope_filter: Option<MemoryScope>,
    ) -> Result<Vec<ScopedMemoryEntry>, MemoryError> {
        let query = query.trim().to_ascii_lowercase();
        Ok(self
            .load()?
            .entries
            .into_iter()
            .filter(|(key, record)| {
                scope_filter.is_none_or(|scope| scope == record.scope)
                    && (query.is_empty()
                        || key.to_ascii_lowercase().contains(&query)
                        || record.value.to_ascii_lowercase().contains(&query))
            })
            .map(|(key, record)| record.entry(key))
            .collect())
    }
    pub fn flush_existing(&self) -> Result<(), MemoryError> {
        if !self.path.try_exists()? {
            return Ok(());
        }
        self.update(|_| Ok(((), true)))
    }
    pub fn consolidate(
        &self,
        source: MemoryScope,
        target: MemoryScope,
    ) -> Result<usize, MemoryError> {
        self.update(|entries| {
            let mut count = 0;
            let timestamp = now();
            for record in entries.values_mut().filter(|r| r.scope == source) {
                record.scope = target;
                record.updated_at_unix_ms = timestamp;
                count += 1;
            }
            Ok((count, count > 0))
        })
    }
    pub fn trace(&self, key: &str) -> Result<ScopedMemoryEntry, MemoryError> {
        let key = normalize_key(key)?.to_owned();
        self.update(|entries| {
            let record = entries
                .get_mut(&key)
                .ok_or_else(|| MemoryError::NotFound { key: key.clone() })?;
            record.updated_at_unix_ms = now().max(record.updated_at_unix_ms.saturating_add(1));
            Ok((record.clone().entry(key), true))
        })
    }
    pub fn release(&self, key: &str) -> Result<bool, MemoryError> {
        let key = normalize_key(key)?;
        self.update(|entries| {
            let removed = entries.remove(key).is_some();
            Ok((removed, removed))
        })
    }
    pub fn release_scope(&self, scope: MemoryScope) -> Result<usize, MemoryError> {
        self.update(|entries| {
            let before = entries.len();
            entries.retain(|_, r| r.scope != scope);
            let removed = before - entries.len();
            Ok((removed, removed > 0))
        })
    }
    pub fn list_by_scope(
        &self,
    ) -> Result<BTreeMap<MemoryScope, Vec<ScopedMemoryEntry>>, MemoryError> {
        let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for entry in self.search_scoped("", None)? {
            groups.entry(entry.scope).or_default().push(entry);
        }
        Ok(groups)
    }
    fn validate_path(&self) -> Result<(), MemoryError> {
        for path in self.path.ancestors() {
            match fs::symlink_metadata(path) {
                Ok(meta) if meta.is_symlink() => {
                    return Err(MemoryError::Invalid("memory paths cannot be symlinks"))
                }
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
        }
        Ok(())
    }
    fn load(&self) -> Result<Document, MemoryError> {
        self.validate_path()?;
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Document {
                    version: 1,
                    entries: BTreeMap::new(),
                })
            }
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
            return Err(MemoryError::Invalid(
                "memory store must be a regular file under 4 MiB",
            ));
        }
        let mut bytes = Vec::new();
        fs::File::open(&self.path)?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BYTES {
            return Err(MemoryError::Invalid("memory store exceeds 4 MiB"));
        }
        let mut document: Document = serde_json::from_slice(&bytes)?;
        if document.version != 1 {
            return Err(MemoryError::UnsupportedVersion {
                path: self.path.display().to_string(),
                version: document.version,
            });
        }
        for (key, record) in &mut document.entries {
            normalize_key(key)?;
            record.value = crate::redact::redact_artifact_text(&record.value);
        }
        Ok(document)
    }
    fn update<T>(
        &self,
        edit: impl FnOnce(&mut BTreeMap<String, Record>) -> Result<(T, bool), MemoryError>,
    ) -> Result<T, MemoryError> {
        self.validate_path()?;
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        crate::store::create_private_dir(parent)?;
        let lock = fs::File::open(parent)?;
        lock.lock()?;
        let mut document = self.load()?;
        let (result, changed) = edit(&mut document.entries)?;
        if changed {
            let bytes = serde_json::to_vec(&document)?;
            if bytes.len() > MAX_BYTES {
                return Err(MemoryError::Invalid("memory store exceeds 4 MiB"));
            }
            crate::store::write_private_atomic(&self.path, &bytes)?;
        }
        Ok(result)
    }
}
fn normalize_key(key: &str) -> Result<&str, MemoryError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(MemoryError::EmptyKey);
    }
    if key.len() > 1024
        || key.chars().any(char::is_control)
        || DefaultRedactor::default().redact_text(key) != key
    {
        return Err(MemoryError::Invalid(
            "memory key is too long, contains controls, or contains a credential",
        ));
    }
    Ok(key)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

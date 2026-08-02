//! Scoped memory operations: global/workspace/session scope, consolidation,
//! trace, and release.
//!
//! All scoped writes go through the same atomic JSON store and redaction
//! pipeline as the base [`DurableMemoryStore`]. Secrets are never persisted.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    normalize_key, now_unix_ms, redact_value, DurableMemoryStore, MemoryDocument,
    MemoryEntryRecord, MemoryError,
};

/// Memory scope: global (user-wide), workspace (project), or session (ephemeral).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    /// User-wide memory shared across all workspaces.
    Global,
    /// Project-scoped memory for the current workspace.
    #[default]
    Workspace,
    /// Session-scoped memory, ephemeral per run.
    Session,
}

impl MemoryScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Workspace => "workspace",
            Self::Session => "session",
        }
    }
}

/// One durable scoped memory entry (already redacted if loaded from store).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedMemoryEntry {
    pub key: String,
    pub value: String,
    pub updated_at_unix_ms: u64,
    pub scope: MemoryScope,
}

impl DurableMemoryStore {
    /// Insert or update `key` with redacted `value` under a specific scope.
    pub fn put_scoped(
        &self,
        key: &str,
        value: &str,
        scope: MemoryScope,
    ) -> Result<ScopedMemoryEntry, MemoryError> {
        let key = normalize_key(key)?;
        let value = redact_value(value);
        let updated_at_unix_ms = now_unix_ms();
        let mut doc = self.load_or_empty()?;
        doc.entries.insert(
            key.clone(),
            MemoryEntryRecord {
                value: value.clone(),
                updated_at_unix_ms,
                scope,
            },
        );
        self.flush(&doc)?;
        Ok(ScopedMemoryEntry {
            key,
            value,
            updated_at_unix_ms,
            scope,
        })
    }

    /// Load one scoped entry by exact key.
    pub fn get_scoped(&self, key: &str) -> Result<Option<ScopedMemoryEntry>, MemoryError> {
        let key = normalize_key(key)?;
        let doc = self.load_or_empty()?;
        Ok(doc.entries.get(&key).map(|record| ScopedMemoryEntry {
            key: key.clone(),
            value: record.value.clone(),
            updated_at_unix_ms: record.updated_at_unix_ms,
            scope: record.scope,
        }))
    }

    /// Search by key prefix or case-insensitive substring, optionally filtered
    /// by scope. When `scope_filter` is `None`, all scopes are searched.
    pub fn search_scoped(
        &self,
        query: &str,
        scope_filter: Option<MemoryScope>,
    ) -> Result<Vec<ScopedMemoryEntry>, MemoryError> {
        let query = query.trim();
        let doc = self.load_or_empty()?;
        let query_lower = query.to_ascii_lowercase();
        let results: Vec<ScopedMemoryEntry> = doc
            .entries
            .iter()
            .filter(|(key, record)| {
                if let Some(scope) = scope_filter {
                    if record.scope != scope {
                        return false;
                    }
                }
                if query.is_empty() {
                    return true;
                }
                let key_match =
                    key.starts_with(query) || key.to_ascii_lowercase().contains(&query_lower);
                let value_match = record.value.to_ascii_lowercase().contains(&query_lower);
                key_match || value_match
            })
            .map(|(key, record)| ScopedMemoryEntry {
                key: key.clone(),
                value: record.value.clone(),
                updated_at_unix_ms: record.updated_at_unix_ms,
                scope: record.scope,
            })
            .collect();
        Ok(results)
    }

    /// Consolidate entries from a source scope into a target scope, merging
    /// values for keys that exist in both (source overwrites target).
    ///
    /// Returns the number of entries consolidated.
    pub fn consolidate(
        &self,
        source: MemoryScope,
        target: MemoryScope,
    ) -> Result<usize, MemoryError> {
        let mut doc = self.load_or_empty()?;
        let source_entries: Vec<(String, MemoryEntryRecord)> = doc
            .entries
            .iter()
            .filter(|(_, r)| r.scope == source)
            .map(|(k, r)| (k.clone(), r.clone()))
            .collect();
        let count = source_entries.len();
        let now = now_unix_ms();
        for (key, mut record) in source_entries {
            record.scope = target;
            record.updated_at_unix_ms = now;
            doc.entries.insert(key, record);
        }
        if count > 0 {
            self.flush(&doc)?;
        }
        Ok(count)
    }

    /// Trace (record) memory access for a key. This is a no-op metadata update
    /// that bumps `updated_at_unix_ms` without changing the value, so the
    /// access trace is visible in the durable store's timestamp.
    ///
    /// Returns the traced entry, or `NotFound` when the key does not exist.
    pub fn trace(&self, key: &str) -> Result<ScopedMemoryEntry, MemoryError> {
        let key = normalize_key(key)?;
        let mut doc = self.load_or_empty()?;
        let record = doc
            .entries
            .get_mut(&key)
            .ok_or(MemoryError::NotFound { key: key.clone() })?;
        record.updated_at_unix_ms = now_unix_ms();
        let result = ScopedMemoryEntry {
            key: key.clone(),
            value: record.value.clone(),
            updated_at_unix_ms: record.updated_at_unix_ms,
            scope: record.scope,
        };
        self.flush(&doc)?;
        Ok(result)
    }

    /// Release (drop) a single memory entry by key. Returns `true` when an
    /// entry was removed, `false` when the key was not found.
    pub fn release(&self, key: &str) -> Result<bool, MemoryError> {
        let key = normalize_key(key)?;
        let mut doc = self.load_or_empty()?;
        let removed = doc.entries.remove(&key).is_some();
        if removed {
            self.flush(&doc)?;
        }
        Ok(removed)
    }

    /// Release all entries in a given scope. Returns the count removed.
    pub fn release_scope(&self, scope: MemoryScope) -> Result<usize, MemoryError> {
        let mut doc = self.load_or_empty()?;
        let before = doc.entries.len();
        doc.entries.retain(|_, r| r.scope != scope);
        let removed = before - doc.entries.len();
        if removed > 0 {
            self.flush(&doc)?;
        }
        Ok(removed)
    }

    /// List all entries grouped by scope (for TUI/memory modal display).
    pub fn list_by_scope(
        &self,
    ) -> Result<BTreeMap<MemoryScope, Vec<ScopedMemoryEntry>>, MemoryError> {
        let scoped = self.search_scoped("", None)?;
        let mut grouped: BTreeMap<MemoryScope, Vec<ScopedMemoryEntry>> = BTreeMap::new();
        for entry in scoped {
            grouped.entry(entry.scope).or_default().push(entry);
        }
        Ok(grouped)
    }
}

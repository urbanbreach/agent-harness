use super::*;
use std::io::Write;
pub const EDIT_ATTRIBUTION_JOURNAL_REL: &str = ".agent-harness/edit-attribution.jsonl";

#[derive(Debug, thiserror::Error)]
pub enum EditAttributionError {
    #[error("create attribution parent {path}: {source}")]
    CreateParent {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("read attribution journal {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("invalid attribution journal {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("write attribution journal {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("invalid attribution path `{path}`")]
    InvalidPath { path: String },
    #[error("no attribution for `{path}`")]
    NotFound { path: String },
    #[error("no agent snapshot for `{path}`")]
    NoAgentSnapshot { path: String },
    #[error("restore attribution path {path}: {source}")]
    Restore {
        path: String,
        #[source]
        source: io::Error,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditAttributionQuery {
    pub path: String,
    pub source: EditSource,
    pub content_sha256: String,
    pub drifted: bool,
    pub one_line: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevertAttributionResult {
    pub path: String,
    pub restored_sha256: String,
    pub bytes_written: usize,
}
pub struct EditAttributionJournal {
    workspace_root: PathBuf,
    journal_path: PathBuf,
    state: records::State,
}
impl EditAttributionJournal {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, EditAttributionError> {
        let mut journal = Self::empty(root);
        crate::store::validate_private_path(&journal.workspace_root).map_err(|source| {
            EditAttributionError::Read {
                path: journal.workspace_root.display().to_string(),
                source,
            }
        })?;
        journal.state = records::load(&journal.journal_path)?;
        Ok(journal)
    }
    pub fn empty(root: impl Into<PathBuf>) -> Self {
        let workspace_root = root.into();
        Self {
            journal_path: workspace_root.join(EDIT_ATTRIBUTION_JOURNAL_REL),
            workspace_root,
            state: records::State::default(),
        }
    }
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }
    pub fn tracker(&self) -> &EditAttributionTracker {
        &self.state.tracker
    }
    pub fn summary(&self) -> EditAttributionSummary {
        self.state.tracker.summary()
    }
    pub fn list(&self) -> Vec<&AttributedEdit> {
        self.state.tracker.list()
    }
    pub fn query(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<EditAttributionQuery, EditAttributionError> {
        let key = self.key(path.as_ref())?;
        let state = records::load(&self.journal_path)?;
        let entry = state
            .tracker
            .get(&key)
            .ok_or_else(|| EditAttributionError::NotFound { path: key.clone() })?;
        Ok(EditAttributionQuery {
            path: key.clone(),
            source: entry.source,
            content_sha256: entry.content_sha256.clone(),
            drifted: state.tracker.is_drifted(&key),
            one_line: entry.one_line(),
        })
    }
    pub fn record_agent_tool_edit(
        &mut self,
        path: impl AsRef<Path>,
        content: &[u8],
        mtime: Option<SystemTime>,
    ) -> Result<AttributedEdit, EditAttributionError> {
        self.record(path.as_ref(), content, mtime, EditSource::AgentTool)
    }
    pub fn observe_external(
        &mut self,
        path: impl AsRef<Path>,
        content: &[u8],
        mtime: Option<SystemTime>,
    ) -> Result<AttributedEdit, EditAttributionError> {
        self.record(path.as_ref(), content, mtime, EditSource::External)
    }
    fn record(
        &mut self,
        path: &Path,
        content: &[u8],
        mtime: Option<SystemTime>,
        source: EditSource,
    ) -> Result<AttributedEdit, EditAttributionError> {
        let key = self.key(path)?;
        records::validate_content(&self.journal_path, content)?;
        let _lock = crate::store::lock_private_parent(&self.journal_path)
            .map_err(|source| self.write_error(source))?;
        let mut state = records::load(&self.journal_path)?;
        let entry = if source == EditSource::AgentTool {
            state.snapshots.insert(key.clone(), content.into());
            state.tracker.record_agent_tool_edit(&key, content, mtime)
        } else {
            state.tracker.observe_external(&key, content, mtime)
        };
        records::save(&self.journal_path, &state)?;
        self.state = state;
        Ok(entry)
    }
    pub fn revert_path(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<RevertAttributionResult, EditAttributionError> {
        let key = self.key(path.as_ref())?;
        let _lock = crate::store::lock_private_parent(&self.journal_path)
            .map_err(|source| self.write_error(source))?;
        let mut state = records::load(&self.journal_path)?;
        let bytes = state
            .snapshots
            .get(&key)
            .ok_or_else(|| EditAttributionError::NoAgentSnapshot { path: key.clone() })?;
        let target = self.workspace_root.join(&key);
        let (before, _) =
            hash_path_contents(&target).map_err(|source| EditAttributionError::Restore {
                path: key.clone(),
                source,
            })?;
        let permissions = fs::metadata(&target)
            .map_err(|source| EditAttributionError::Restore {
                path: key.clone(),
                source,
            })?
            .permissions();
        let restore_error = |source| EditAttributionError::Restore {
            path: key.clone(),
            source,
        };
        let rollback = |error| match replace_file(&target, &before, permissions.clone()) {
            Ok(()) => error,
            Err(source) => restore_error(io::Error::other(format!(
                "{error}; rollback failed: {source}"
            ))),
        };
        replace_file(&target, bytes, permissions.clone())
            .map_err(|source| rollback(restore_error(source)))?;
        let entry = state.tracker.record_agent_tool_edit(&key, bytes, None);
        let result = RevertAttributionResult {
            path: key.clone(),
            restored_sha256: entry.content_sha256,
            bytes_written: bytes.len(),
        };
        records::save(&self.journal_path, &state).map_err(rollback)?;
        self.state = state;
        Ok(result)
    }
    pub fn diff(&self, path: impl AsRef<Path>) -> Result<DiffResult, EditAttributionError> {
        let (key, snapshot, current) = self.comparison(path.as_ref())?;
        Ok(compute_diff(&key, &snapshot, &current))
    }
    pub fn blame(&self, path: impl AsRef<Path>) -> Result<BlameResult, EditAttributionError> {
        let (key, snapshot, current) = self.comparison(path.as_ref())?;
        Ok(compute_blame(&key, &snapshot, &current))
    }
    fn comparison(&self, path: &Path) -> Result<(String, Vec<u8>, Vec<u8>), EditAttributionError> {
        let key = self.key(path)?;
        let mut state = records::load(&self.journal_path)?;
        let snapshot = state
            .snapshots
            .remove(&key)
            .ok_or_else(|| EditAttributionError::NoAgentSnapshot { path: key.clone() })?;
        let current =
            crate::store::read_private_bytes(&self.workspace_root.join(&key), 8 * 1024 * 1024)
                .map_err(|source| EditAttributionError::Read {
                    path: key.clone(),
                    source,
                })?
                .unwrap_or_default();
        Ok((key, snapshot, current))
    }
    fn key(&self, path: &Path) -> Result<String, EditAttributionError> {
        let invalid = || EditAttributionError::InvalidPath {
            path: path.display().to_string(),
        };
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.workspace_root)
                .map_err(|_| invalid())?
        } else {
            path
        };
        relative.to_str().ok_or_else(invalid)?;
        let key = normalize_path(relative);
        records::validate_key(&key).map_err(|()| invalid())?;
        crate::store::validate_private_path(&self.workspace_root.join(&key))
            .map_err(|_| invalid())?;
        Ok(key)
    }
    fn write_error(&self, source: io::Error) -> EditAttributionError {
        EditAttributionError::Write {
            path: self.journal_path.display().to_string(),
            source,
        }
    }
}
pub(crate) fn replace_file(
    path: &Path,
    bytes: &[u8],
    permissions: fs::Permissions,
) -> io::Result<()> {
    crate::store::validate_private_path(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged.as_file().set_permissions(permissions)?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

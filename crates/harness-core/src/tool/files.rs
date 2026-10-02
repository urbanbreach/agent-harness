use super::{ToolContext, ToolError};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, TryLockError, Weak},
};
type WriteLockSender = tokio::sync::broadcast::Sender<(PathBuf, bool)>;

#[derive(Clone, Default)]
pub struct ToolRunState {
    reads: Arc<Mutex<BTreeMap<PathBuf, String>>>,
    writes: Arc<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>>,
    write_events: Arc<Mutex<Option<WriteLockSender>>>,
}
impl ToolRunState {
    /// A fresh owner has private reads but shares the run's write serialization.
    pub fn fresh_owner(&self) -> Self {
        Self {
            reads: Arc::default(),
            writes: Arc::clone(&self.writes),
            write_events: Arc::clone(&self.write_events),
        }
    }
    pub fn read_snapshot(&self) -> Result<BTreeMap<PathBuf, String>, ToolError> {
        self.reads
            .lock()
            .map(|reads| reads.clone())
            .map_err(|_| ToolError::Execution("file state lock failed".into()))
    }
    pub fn with_read_snapshot(&self, reads: BTreeMap<PathBuf, String>) -> Result<Self, ToolError> {
        if reads.len() > 1024 {
            return Err(ToolError::InvalidArguments(
                "read state exceeds 1024 paths".into(),
            ));
        }
        Ok(Self {
            reads: Arc::new(Mutex::new(reads)),
            writes: Arc::clone(&self.writes),
            write_events: Arc::clone(&self.write_events),
        })
    }
    /// Observe actual canonical-path lock attempts; this grants no write capability.
    #[doc(hidden)]
    pub fn subscribe_write_locks(
        &self,
    ) -> Result<tokio::sync::broadcast::Receiver<(PathBuf, bool)>, ToolError> {
        let mut events = self
            .write_events
            .lock()
            .map_err(|_| ToolError::Execution("file write observer lock failed".into()))?;
        Ok(events
            .get_or_insert_with(|| tokio::sync::broadcast::channel(16).0)
            .subscribe())
    }
    pub fn record_read(&self, path: &Path, digest: String) -> Result<(), ToolError> {
        let mut reads = self
            .reads
            .lock()
            .map_err(|_| ToolError::Execution("file state lock failed".into()))?;
        remember(&mut reads, path, digest);
        Ok(())
    }
    pub fn edit<T>(
        &self,
        path: &Path,
        write: impl FnOnce(Option<&str>) -> Result<(T, String), ToolError>,
    ) -> Result<T, ToolError> {
        // Callers resolve canonical paths at the permission/execution boundary.
        // Weak entries retain only currently held/waited-on locks, not every past path.
        let path_lock = {
            let mut writes = self
                .writes
                .lock()
                .map_err(|_| ToolError::Execution("file write lock failed".into()))?;
            writes.retain(|_, lock| lock.strong_count() > 0);
            match writes.get(path).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(Mutex::new(()));
                    writes.insert(path.into(), Arc::downgrade(&lock));
                    lock
                }
            }
        };
        let acquired = path_lock.try_lock();
        if let Some(events) = self
            .write_events
            .lock()
            .map_err(|_| ToolError::Execution("file write observer lock failed".into()))?
            .as_ref()
        {
            let _ = events.send((
                path.into(),
                matches!(acquired, Err(TryLockError::WouldBlock)),
            ));
        }
        let _write = match acquired {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => path_lock
                .lock()
                .map_err(|_| ToolError::Execution("file write lock failed".into()))?,
            Err(TryLockError::Poisoned(_)) => {
                return Err(ToolError::Execution("file write lock failed".into()))
            }
        };
        let previous = self
            .reads
            .lock()
            .map_err(|_| ToolError::Execution("file state lock failed".into()))?
            .get(path)
            .cloned();
        let (output, digest) = write(previous.as_deref())?;
        let mut reads = self
            .reads
            .lock()
            .map_err(|_| ToolError::Execution("file state lock failed".into()))?;
        remember(&mut reads, path, digest);
        Ok(output)
    }
}

fn remember(reads: &mut BTreeMap<PathBuf, String>, path: &Path, digest: String) {
    // ponytail: ordered eviction caps retention at 1024 paths; use LRU only if rereads become costly.
    if reads.len() >= 1024 && !reads.contains_key(path) {
        reads.pop_first();
    }
    reads.insert(path.into(), digest);
}

/// Resolves existing symlinks and the nearest existing parent for a new file.
pub fn resolve_file_path(root: &Path, input: &Path) -> Result<PathBuf, ToolError> {
    let joined = root.join(input);
    let mut candidate = joined.as_path();
    let mut missing = Vec::new();
    loop {
        match candidate.canonicalize() {
            Ok(mut path) => {
                for name in missing.into_iter().rev() {
                    path.push(name);
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = candidate.file_name().ok_or_else(|| {
                    ToolError::InvalidArguments("path has an unresolved parent".into())
                })?;
                missing.push(name);
                candidate = candidate.parent().ok_or_else(|| {
                    ToolError::InvalidArguments("path has no existing parent".into())
                })?;
            }
            Err(error) => return Err(error.into()),
        }
    }
}
impl ToolContext {
    pub fn external_path_authorized(&self, path: &Path) -> bool {
        self.external_directory_allow_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
    }
    pub fn resolve_workspace_path(&self, input: &Path) -> Result<PathBuf, ToolError> {
        let path = resolve_file_path(&self.workspace_root, input)?;
        let requested = self.workspace_root.join(input);
        if self
            .approved_paths
            .iter()
            .any(|(original, target)| original == &requested && target != &path)
        {
            return Err(ToolError::Execution("path changed after approval".into()));
        }
        if !self.policy_roots.iter().any(|root| path.starts_with(root))
            && !self.external_path_authorized(&path)
        {
            return Err(ToolError::Execution(
                "path is outside the authorized workspace".into(),
            ));
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_sessions_forget_old_reads_but_keep_a_fresh_read_usable() -> Result<(), ToolError> {
        let state = ToolRunState::default();
        let first = Path::new("a");
        state.record_read(first, "old".into())?;
        for n in 0..1024 {
            state.record_read(&PathBuf::from(format!("file-{n:04}")), n.to_string())?;
        }
        state.edit(first, |previous| {
            assert!(
                previous.is_none(),
                "retention must stay bounded across many file reads"
            );
            Ok(((), "current".into()))
        })?;
        state.record_read(first, "fresh".into())?;
        state.edit(first, |previous| {
            assert_eq!(
                previous,
                Some("fresh"),
                "a newly read file must remain editable even when retention is full"
            );
            Ok(((), "written".into()))
        })?;
        Ok(())
    }
}

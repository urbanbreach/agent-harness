use super::{ToolContext, ToolError};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub struct ToolRunState(Arc<Mutex<BTreeMap<PathBuf, String>>>);
impl ToolRunState {
    pub fn record_read(&self, path: &Path, digest: String) -> Result<(), ToolError> {
        let mut reads = self
            .0
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
        // ponytail: edits in a run share one lock; use per-path locks if edit throughput warrants it.
        let mut reads = self
            .0
            .lock()
            .map_err(|_| ToolError::Execution("file state lock failed".into()))?;
        let (output, digest) = write(reads.get(path).map(String::as_str))?;
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
        if !path.starts_with(&self.workspace_root) && !self.external_path_authorized(&path) {
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

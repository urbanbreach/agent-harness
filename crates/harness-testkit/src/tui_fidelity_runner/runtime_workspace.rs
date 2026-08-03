use std::fs;
use std::path::PathBuf;

use super::error::RunnerError;
use crate::tui_fidelity::AdapterKind;

pub(super) struct OwnedRuntimeWorkspace {
    root: Option<PathBuf>,
    base: PathBuf,
    owns_base: bool,
}

impl OwnedRuntimeWorkspace {
    pub fn create(
        repo_root: &PathBuf,
        relative_base: &str,
        scenario_id: &str,
    ) -> Result<Self, RunnerError> {
        let base = repo_root.join(relative_base);
        let owns_base = !base.exists();
        fs::create_dir_all(&base).map_err(|error| io_error(&base, error))?;
        let temp = tempfile::Builder::new()
            .prefix(&format!("{scenario_id}-"))
            .tempdir_in(&base)
            .map_err(|error| io_error(&base, error))?;
        Ok(Self {
            root: Some(temp.keep()),
            base,
            owns_base,
        })
    }

    pub fn adapter_dir(&self, adapter: AdapterKind) -> Result<PathBuf, RunnerError> {
        let root = self.root.as_ref().ok_or_else(|| RunnerError::Io {
            path: self.base.clone(),
            detail: "runtime workspace was already cleaned".to_owned(),
        })?;
        let adapter_name = match adapter {
            AdapterKind::Grok => "grok",
            AdapterKind::Harness => "harness",
        };
        let path = root.join(adapter_name);
        fs::create_dir_all(&path).map_err(|error| io_error(&path, error))?;
        Ok(path)
    }

    pub fn root(&self) -> Option<&PathBuf> {
        self.root.as_ref()
    }

    pub fn cleanup(&mut self) -> Result<PathBuf, RunnerError> {
        let Some(root) = self.root.take() else {
            return Ok(self.base.clone());
        };
        if let Err(error) = fs::remove_dir_all(&root) {
            self.root = Some(root.clone());
            return Err(io_error(&root, error));
        }
        if self.owns_base
            && fs::read_dir(&self.base)
                .map_err(|error| io_error(&self.base, error))?
                .next()
                .is_none()
        {
            fs::remove_dir(&self.base).map_err(|error| io_error(&self.base, error))?;
        }
        Ok(root)
    }
}

impl Drop for OwnedRuntimeWorkspace {
    fn drop(&mut self) {
        if let Some(root) = self.root.take() {
            let _ = fs::remove_dir_all(root);
        }
        if self.owns_base {
            let _ = fs::remove_dir(&self.base);
        }
    }
}

fn io_error(path: &std::path::Path, error: impl std::fmt::Display) -> RunnerError {
    RunnerError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    }
}

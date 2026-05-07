use std::fs;
use std::path::{Path, PathBuf};

pub struct WorkspaceFixture {
    _temp_dir: tempfile::TempDir,
    workspace: PathBuf,
}

impl WorkspaceFixture {
    pub fn temp_dir(&self) -> &Path {
        self._temp_dir.path()
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }
}

pub fn setup_workspace() -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(temp_dir.path().join("workspace")).expect("workspace");
    temp_dir
}

pub fn setup_workspace_fixture() -> WorkspaceFixture {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    WorkspaceFixture {
        _temp_dir: temp_dir,
        workspace,
    }
}

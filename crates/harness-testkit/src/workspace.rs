use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct TestWorkspace(tempfile::TempDir);
impl TestWorkspace {
    pub fn new() -> io::Result<Self> {
        tempfile::tempdir().map(Self)
    }
    pub fn root(&self) -> &Path {
        self.0.path()
    }
    pub fn sessions_dir(&self) -> PathBuf {
        self.root().join("sessions")
    }
}
pub fn with_workspace<T>(run: impl FnOnce(&TestWorkspace) -> T) -> io::Result<T> {
    Ok(run(&TestWorkspace::new()?))
}

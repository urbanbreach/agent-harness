use std::{fs::File, io::Read, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEnvironment {
    pub working_directory: PathBuf,
    pub workspace_root: PathBuf,
    pub is_git_repository: bool,
    pub git_branch: Option<String>,
}
impl WorkspaceEnvironment {
    pub fn current() -> Self {
        Self::discover(std::env::current_dir().unwrap_or_else(|_| ".".into()))
    }
    pub fn discover(working_directory: impl Into<PathBuf>) -> Self {
        let working_directory = working_directory.into();
        let start = working_directory
            .canonicalize()
            .unwrap_or_else(|_| working_directory.clone());
        let root = start.ancestors().find(|root| root.join(".git").exists());
        let git_branch = root.and_then(|root| {
            let mut git_dir = root.join(".git");
            if git_dir.is_file() {
                let marker = read_reference(&git_dir)?;
                git_dir = root.join(marker.strip_prefix("gitdir: ")?.trim_end());
            }
            let head = read_reference(&git_dir.join("HEAD"))?;
            head.trim()
                .strip_prefix("ref: refs/heads/")
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        });
        Self {
            workspace_root: root
                .map_or_else(|| working_directory.clone(), |root| root.to_path_buf()),
            is_git_repository: root.is_some(),
            working_directory,
            git_branch,
        }
    }
    pub fn full_label(&self) -> String {
        self.label(self.working_directory.display())
    }
    pub fn short_label(&self) -> String {
        self.label(
            self.working_directory
                .file_name()
                .unwrap_or(self.working_directory.as_os_str())
                .to_string_lossy(),
        )
    }
    fn label(&self, path: impl std::fmt::Display) -> String {
        match self.git_branch.as_deref().filter(|b| !b.trim().is_empty()) {
            Some(branch) => format!("{path}:{branch}"),
            None => path.to_string(),
        }
    }
}
fn read_reference(path: &std::path::Path) -> Option<String> {
    let mut value = String::new();
    File::open(path)
        .ok()?
        .take(4097)
        .read_to_string(&mut value)
        .ok()?;
    (value.len() <= 4096).then_some(value)
}

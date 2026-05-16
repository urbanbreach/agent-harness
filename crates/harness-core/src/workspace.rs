use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEnvironment {
    pub working_directory: PathBuf,
    pub workspace_root: PathBuf,
    pub is_git_repository: bool,
    pub git_branch: Option<String>,
}

impl WorkspaceEnvironment {
    pub fn current() -> Self {
        let working_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::discover(working_directory)
    }

    pub fn discover(working_directory: impl Into<PathBuf>) -> Self {
        let working_directory = working_directory.into();
        let git_root = git_text(&working_directory, &["rev-parse", "--show-toplevel"])
            .map(|root| resolve_git_path(&working_directory, &root))
            .or_else(|| ancestor_git_root(&working_directory));
        let is_git_repository = git_root.is_some();
        let workspace_root = git_root.unwrap_or_else(|| working_directory.clone());
        let git_branch = if is_git_repository {
            git_text(
                &working_directory,
                &["symbolic-ref", "--quiet", "--short", "HEAD"],
            )
        } else {
            None
        };

        Self {
            working_directory,
            workspace_root,
            is_git_repository,
            git_branch,
        }
    }

    pub fn full_label(&self) -> String {
        path_with_branch(&self.working_directory, self.git_branch.as_deref())
    }

    pub fn short_label(&self) -> String {
        let directory = self
            .working_directory
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.working_directory.clone());
        path_with_branch(&directory, self.git_branch.as_deref())
    }
}

fn path_with_branch(path: &Path, branch: Option<&str>) -> String {
    let mut label = path.display().to_string();
    if let Some(branch) = branch.filter(|branch| !branch.trim().is_empty()) {
        label.push(':');
        label.push_str(branch);
    }
    label
}

fn ancestor_git_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|path| path.join(".git").exists())
        .map(Path::to_path_buf)
}

fn git_text(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.quotepath=false",
        ])
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn resolve_git_path(cwd: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value.trim());
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn tempdir_without_git_ancestor() -> tempfile::TempDir {
        for base in ["/var/tmp", "/dev/shm", "/tmp"] {
            let base = Path::new(base);
            if !base.is_dir() || ancestor_git_root(base).is_some() {
                continue;
            }
            if let Ok(temp_dir) = tempfile::Builder::new()
                .prefix("harness-workspace-nongit-")
                .tempdir_in(base)
            {
                if ancestor_git_root(temp_dir.path()).is_none() {
                    return temp_dir;
                }
            }
        }
        panic!("no writable temp parent without a .git ancestor");
    }

    #[test]
    fn non_git_directory_uses_working_directory_as_workspace_root() {
        let temp_dir = tempdir_without_git_ancestor();
        let environment = WorkspaceEnvironment::discover(temp_dir.path());

        assert_eq!(environment.working_directory, temp_dir.path());
        assert_eq!(environment.workspace_root, temp_dir.path());
        assert!(!environment.is_git_repository);
        assert_eq!(environment.git_branch, None);
    }

    #[test]
    fn git_discovery_ignores_inherited_git_environment() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let temp_dir = tempdir_without_git_ancestor();
        let inherited_work_tree = tempfile::tempdir().expect("inherited git worktree");
        std::fs::create_dir(inherited_work_tree.path().join(".git")).expect("git marker");

        let previous_work_tree = std::env::var_os("GIT_WORK_TREE");
        let previous_dir = std::env::var_os("GIT_DIR");
        std::env::set_var("GIT_WORK_TREE", inherited_work_tree.path());
        std::env::set_var("GIT_DIR", inherited_work_tree.path().join(".git"));
        let environment = WorkspaceEnvironment::discover(temp_dir.path());
        match previous_work_tree {
            Some(value) => std::env::set_var("GIT_WORK_TREE", value),
            None => std::env::remove_var("GIT_WORK_TREE"),
        }
        match previous_dir {
            Some(value) => std::env::set_var("GIT_DIR", value),
            None => std::env::remove_var("GIT_DIR"),
        }

        assert_eq!(environment.working_directory, temp_dir.path());
        assert_eq!(environment.workspace_root, temp_dir.path());
        assert!(!environment.is_git_repository);
    }

    #[test]
    fn git_marker_ancestor_becomes_workspace_root_without_git_binary() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let nested = temp_dir.path().join("crates").join("harness");
        std::fs::create_dir_all(&nested).expect("nested directory");
        std::fs::create_dir(temp_dir.path().join(".git")).expect("git marker");

        let environment = WorkspaceEnvironment::discover(&nested);

        assert_eq!(environment.working_directory, nested);
        assert_eq!(environment.workspace_root, temp_dir.path());
        assert!(environment.is_git_repository);
    }

    #[test]
    fn labels_append_branch_when_available() {
        let environment = WorkspaceEnvironment {
            working_directory: PathBuf::from("/workspace/project"),
            workspace_root: PathBuf::from("/workspace"),
            is_git_repository: true,
            git_branch: Some("dev".to_string()),
        };

        assert_eq!(environment.full_label(), "/workspace/project:dev");
        assert_eq!(environment.short_label(), "project:dev");
    }
}

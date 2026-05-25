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
        Self::discover_with_git_probe(working_directory, &RealWorkspaceGitProbe)
    }

    fn discover_with_git_probe(
        working_directory: impl Into<PathBuf>,
        git_probe: &dyn WorkspaceGitProbe,
    ) -> Self {
        let working_directory = working_directory.into();
        let git_root = git_probe
            .git_text(&working_directory, &["rev-parse", "--show-toplevel"])
            .map(|root| resolve_git_path(&working_directory, &root))
            .or_else(|| ancestor_git_root(&working_directory));
        let is_git_repository = git_root.is_some();
        let workspace_root = git_root.unwrap_or_else(|| working_directory.clone());
        let git_branch = if is_git_repository {
            git_probe.git_text(
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

trait WorkspaceGitProbe {
    fn git_text(&self, cwd: &Path, args: &[&str]) -> Option<String>;
}

struct RealWorkspaceGitProbe;

impl WorkspaceGitProbe for RealWorkspaceGitProbe {
    fn git_text(&self, cwd: &Path, args: &[&str]) -> Option<String> {
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
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let text = String::from_utf8(output.stdout).ok()?;
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }
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
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakeWorkspaceGitProbe {
        responses: BTreeMap<Vec<String>, Option<String>>,
        calls: RefCell<Vec<(PathBuf, Vec<String>)>>,
    }

    impl FakeWorkspaceGitProbe {
        fn with_response(mut self, args: &[&str], response: Option<&str>) -> Self {
            self.responses.insert(
                args.iter().map(ToString::to_string).collect(),
                response.map(ToString::to_string),
            );
            self
        }
    }

    impl WorkspaceGitProbe for FakeWorkspaceGitProbe {
        fn git_text(&self, cwd: &Path, args: &[&str]) -> Option<String> {
            let args = args.iter().map(ToString::to_string).collect::<Vec<_>>();
            self.calls
                .borrow_mut()
                .push((cwd.to_path_buf(), args.clone()));
            self.responses.get(&args).cloned().flatten()
        }
    }

    #[test]
    fn non_git_directory_uses_working_directory_as_workspace_root() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let git_probe = FakeWorkspaceGitProbe::default();
        let environment =
            WorkspaceEnvironment::discover_with_git_probe(temp_dir.path(), &git_probe);

        assert_eq!(environment.working_directory, temp_dir.path());
        assert_eq!(environment.workspace_root, temp_dir.path());
        assert!(!environment.is_git_repository);
        assert_eq!(environment.git_branch, None);
    }

    #[test]
    fn git_marker_ancestor_becomes_workspace_root_without_git_binary() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let nested = temp_dir.path().join("crates").join("harness");
        std::fs::create_dir_all(&nested).expect("nested directory");
        std::fs::create_dir(temp_dir.path().join(".git")).expect("git marker");
        let git_probe = FakeWorkspaceGitProbe::default();

        let environment = WorkspaceEnvironment::discover_with_git_probe(&nested, &git_probe);

        assert_eq!(environment.working_directory, nested);
        assert_eq!(environment.workspace_root, temp_dir.path());
        assert!(environment.is_git_repository);
    }

    #[test]
    fn git_probe_reports_workspace_root_and_branch_without_spawning() {
        let working_directory = PathBuf::from("/workspace/project/crates/harness");
        let git_probe = FakeWorkspaceGitProbe::default()
            .with_response(
                &["rev-parse", "--show-toplevel"],
                Some("/workspace/project"),
            )
            .with_response(
                &["symbolic-ref", "--quiet", "--short", "HEAD"],
                Some("feature/test-seams"),
            );

        let environment =
            WorkspaceEnvironment::discover_with_git_probe(&working_directory, &git_probe);

        assert_eq!(environment.working_directory, working_directory);
        assert_eq!(
            environment.workspace_root,
            PathBuf::from("/workspace/project")
        );
        assert!(environment.is_git_repository);
        assert_eq!(
            environment.git_branch.as_deref(),
            Some("feature/test-seams")
        );
        let calls = git_probe.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, vec!["rev-parse", "--show-toplevel"]);
        assert_eq!(
            calls[1].1,
            vec!["symbolic-ref", "--quiet", "--short", "HEAD"]
        );
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

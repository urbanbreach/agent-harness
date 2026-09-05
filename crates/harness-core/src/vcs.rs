//! VCS status and diff for workspace inspection.
//!
//! Parses `git status --porcelain` and `git diff` output into structured
//! types. Read-only: never stages, commits, checks out, or mutates the
//! repository. Path-qualified entries are workspace-relative and reject
//! traversal outside the repository root.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// VCS status/diff failures.
#[derive(Debug, Error)]
pub enum VcsError {
    #[error("workspace is not a git repository: {path}")]
    NotAGitRepository { path: String },
    #[error("git status failed: {detail}")]
    StatusFailed { detail: String },
    #[error("git diff failed: {detail}")]
    DiffFailed { detail: String },
    #[error("path escapes repository root: {path}")]
    PathTraversal { path: String },
}

/// One file entry in `git status --porcelain` output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsStatusEntry {
    /// Workspace-relative path (forward slashes).
    pub path: String,
    /// Staged status code (X column).
    pub staged: char,
    /// Worktree status code (Y column).
    pub worktree: char,
    /// True when the path is untracked (`??`).
    pub untracked: bool,
}

/// Summary of `git status --porcelain` for a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsStatus {
    pub entries: Vec<VcsStatusEntry>,
    pub modified: usize,
    pub staged: usize,
    pub untracked: usize,
    pub deleted: usize,
}

impl VcsStatus {
    pub fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn one_line(&self) -> String {
        format!(
            "vcs status: {} modified, {} staged, {} untracked, {} deleted ({} total)",
            self.modified,
            self.staged,
            self.untracked,
            self.deleted,
            self.entries.len()
        )
    }
}

/// Result of `git diff` for a path (or the whole worktree).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsDiff {
    /// Workspace-relative path when scoped to one file, or empty for full diff.
    pub path: String,
    /// Raw unified diff text from `git diff`.
    pub unified_diff: String,
    /// Number of files changed (from `--numstat` summary).
    pub files_changed: usize,
    /// Total insertions across all hunks.
    pub insertions: usize,
    /// Total deletions across all hunks.
    pub deletions: usize,
}

impl VcsDiff {
    pub fn is_empty(&self) -> bool {
        self.unified_diff.is_empty() && self.files_changed == 0
    }

    pub fn one_line(&self) -> String {
        if self.path.is_empty() {
            format!(
                "vcs diff: {} files, +{} -{}",
                self.files_changed, self.insertions, self.deletions
            )
        } else {
            format!(
                "vcs diff `{}`: +{} -{}",
                self.path, self.insertions, self.deletions
            )
        }
    }
}

/// Collect `git status --porcelain` for `repository_root`.
///
/// Read-only: never stages, commits, or mutates the repository.
pub fn collect_vcs_status(repository_root: &Path) -> Result<VcsStatus, VcsError> {
    if !is_git_repository(repository_root) {
        return Err(VcsError::NotAGitRepository {
            path: repository_root.display().to_string(),
        });
    }
    let porcelain = git_output(repository_root, &["status", "--porcelain=v1"])
        .map_err(|detail| VcsError::StatusFailed { detail })?;
    let entries = parse_porcelain(&porcelain);
    let mut modified = 0usize;
    let mut staged = 0usize;
    let mut untracked = 0usize;
    let mut deleted = 0usize;
    for entry in &entries {
        if entry.untracked {
            untracked += 1;
            continue;
        }
        if entry.staged != ' ' && entry.staged != '?' {
            staged += 1;
        }
        if entry.worktree == 'M' {
            modified += 1;
        }
        if entry.staged == 'D' || entry.worktree == 'D' {
            deleted += 1;
        }
    }
    Ok(VcsStatus {
        entries,
        modified,
        staged,
        untracked,
        deleted,
    })
}

/// Collect `git diff` for a specific path (or the whole worktree when `path` is `None`).
///
/// Read-only: never stages, commits, or mutates the repository.
pub fn collect_vcs_diff(repository_root: &Path, path: Option<&str>) -> Result<VcsDiff, VcsError> {
    if !is_git_repository(repository_root) {
        return Err(VcsError::NotAGitRepository {
            path: repository_root.display().to_string(),
        });
    }
    let path_key = match path {
        Some(p) => {
            let key = normalize_relative_path(p)?;
            validate_no_traversal(&key)?;
            key
        }
        None => String::new(),
    };

    let mut diff_args = vec!["diff", "HEAD", "--no-color"];
    if !path_key.is_empty() {
        diff_args.push("--");
        diff_args.push(&path_key);
    }
    let unified = git_output(repository_root, &diff_args)
        .map_err(|detail| VcsError::DiffFailed { detail })?;

    let mut numstat_args = vec!["diff", "HEAD", "--numstat"];
    if !path_key.is_empty() {
        numstat_args.push("--");
        numstat_args.push(&path_key);
    }
    let numstat = git_output(repository_root, &numstat_args)
        .map_err(|detail| VcsError::DiffFailed { detail })?;

    let (files_changed, insertions, deletions) = parse_numstat(&numstat);

    Ok(VcsDiff {
        path: path_key,
        unified_diff: unified,
        files_changed,
        insertions,
        deletions,
    })
}

/// Collect a combined status+diff snapshot for operator display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsSnapshot {
    pub status: VcsStatus,
    pub diff: VcsDiff,
}

/// Collect both status and a full-worktree diff in one call.
pub fn collect_vcs_snapshot(repository_root: &Path) -> Result<VcsSnapshot, VcsError> {
    let status = collect_vcs_status(repository_root)?;
    let diff = collect_vcs_diff(repository_root, None)?;
    Ok(VcsSnapshot { status, diff })
}

fn parse_porcelain(raw: &str) -> Vec<VcsStatusEntry> {
    let mut entries = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let bytes = line.as_bytes();
        if bytes.len() < 3 {
            continue;
        }
        let staged = bytes[0] as char;
        let worktree = bytes[1] as char;
        let path = line[3..].to_string();
        let untracked = staged == '?' && worktree == '?';
        entries.push(VcsStatusEntry {
            path,
            staged,
            worktree,
            untracked,
        });
    }
    entries
}

fn parse_numstat(raw: &str) -> (usize, usize, usize) {
    let mut files = 0usize;
    let mut insertions = 0usize;
    let mut deletions = 0usize;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let ins = parts.next().unwrap_or("0");
        let del = parts.next().unwrap_or("0");
        if ins == "-" || del == "-" {
            files += 1;
            continue;
        }
        insertions += ins.parse::<usize>().unwrap_or(0);
        deletions += del.parse::<usize>().unwrap_or(0);
        files += 1;
    }
    (files, insertions, deletions)
}

fn normalize_relative_path(path: &str) -> Result<String, VcsError> {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(VcsError::PathTraversal {
            path: path.to_string(),
        });
    }
    Ok(normalized)
}

fn validate_no_traversal(path: &str) -> Result<(), VcsError> {
    for component in path.split('/') {
        if component == ".." {
            return Err(VcsError::PathTraversal {
                path: path.to_string(),
            });
        }
    }
    Ok(())
}

fn is_git_repository(path: &Path) -> bool {
    git_output(path, &["rev-parse", "--is-inside-work-tree"])
        .is_ok_and(|text| text.trim() == "true")
}

pub(crate) fn git_output(cwd: &Path, args: &[&str]) -> Result<String, String> {
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
        .map_err(|err| format!("failed to spawn git: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or("git command failed");
        return Err(detail.to_string());
    }

    String::from_utf8(output.stdout).map_err(|err| format!("git output was not utf-8: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use std::fs;
    use std::process::Command as GitCommand;

    fn init_git_repo(path: &Path) {
        run_git(path, &["init", "-b", "main"]);
        run_git(path, &["config", "user.email", "vcs@example.com"]);
        run_git(path, &["config", "user.name", "VCS Test"]);
        fs::write(path.join("README.md"), "seed\n").unwrap_or_abort();
        run_git(path, &["add", "README.md"]);
        run_git(path, &["commit", "-m", "seed"]);
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = GitCommand::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap_or_abort();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn collect_vcs_status_reports_clean_repo() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let status = collect_vcs_status(&repo).unwrap_or_abort();

        assert!(status.is_clean());
        assert_eq!(status.entries.len(), 0);
        assert_eq!(status.modified, 0);
        assert_eq!(status.untracked, 0);
        assert!(status.one_line().contains("0 total"));
    }

    #[test]
    fn collect_vcs_status_reports_modified_and_untracked() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);
        fs::write(repo.join("tracked.txt"), "original\n").unwrap_or_abort();
        run_git(&repo, &["add", "tracked.txt"]);
        run_git(&repo, &["commit", "-m", "add tracked"]);
        fs::write(repo.join("tracked.txt"), "modified\n").unwrap_or_abort();
        fs::write(repo.join("new.txt"), "untracked\n").unwrap_or_abort();

        let status = collect_vcs_status(&repo).unwrap_or_abort();

        assert!(!status.is_clean());
        assert_eq!(status.entries.len(), 2);
        assert!(status.untracked >= 1);
        assert!(status.modified >= 1);
        let paths: Vec<&str> = status.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"tracked.txt"));
        assert!(paths.contains(&"new.txt"));
    }

    #[test]
    fn collect_vcs_status_fails_closed_on_non_git_repository() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let not_a_repo = temp.path().join("plain");
        fs::create_dir_all(&not_a_repo).unwrap_or_abort();

        let err = collect_vcs_status(&not_a_repo).expect_err("non-git should fail");
        assert!(matches!(err, VcsError::NotAGitRepository { .. }));
    }

    #[test]
    fn collect_vcs_diff_returns_empty_for_clean_repo() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let diff = collect_vcs_diff(&repo, None).unwrap_or_abort();

        assert!(diff.is_empty());
        assert_eq!(diff.files_changed, 0);
    }

    #[test]
    fn collect_vcs_diff_reports_changes_for_modified_file() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);
        fs::write(repo.join("src.rs"), "fn main() {}\n").unwrap_or_abort();
        run_git(&repo, &["add", "src.rs"]);

        let diff = collect_vcs_diff(&repo, Some("src.rs")).unwrap_or_abort();

        assert!(!diff.is_empty());
        assert_eq!(diff.path, "src.rs");
        assert!(diff.insertions > 0);
        assert!(diff.unified_diff.contains("fn main()"));
    }

    #[test]
    fn collect_vcs_diff_rejects_path_traversal() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let err = collect_vcs_diff(&repo, Some("../etc/passwd")).expect_err("traversal must fail");
        assert!(matches!(err, VcsError::PathTraversal { .. }));
    }

    #[test]
    fn collect_vcs_diff_rejects_absolute_path() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let err =
            collect_vcs_diff(&repo, Some("/etc/passwd")).expect_err("absolute path must fail");
        assert!(matches!(err, VcsError::PathTraversal { .. }));
    }

    #[test]
    fn collect_vcs_snapshot_combines_status_and_diff() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);
        fs::write(repo.join("changed.rs"), "pub fn x() {}\n").unwrap_or_abort();
        run_git(&repo, &["add", "changed.rs"]);

        let snapshot = collect_vcs_snapshot(&repo).unwrap_or_abort();

        assert!(!snapshot.status.is_clean());
        assert!(snapshot.diff.files_changed > 0);
        assert!(snapshot.diff.insertions > 0);
    }

    #[test]
    fn parse_numstat_counts_insertions_and_deletions() {
        let raw = "3\t1\tsrc/a.rs\n0\t5\tsrc/b.rs\n";
        let (files, ins, del) = parse_numstat(raw);
        assert_eq!(files, 2);
        assert_eq!(ins, 3);
        assert_eq!(del, 6);
    }

    #[test]
    fn parse_numstat_handles_binary_marker() {
        let raw = "-\t-\tbin/file\n";
        let (files, ins, del) = parse_numstat(raw);
        assert_eq!(files, 1);
        assert_eq!(ins, 0);
        assert_eq!(del, 0);
    }

    #[test]
    fn vcs_status_one_line_is_human_readable() {
        let status = VcsStatus {
            entries: vec![VcsStatusEntry {
                path: "a.rs".to_string(),
                staged: 'M',
                worktree: ' ',
                untracked: false,
            }],
            modified: 0,
            staged: 1,
            untracked: 0,
            deleted: 0,
        };
        let line = status.one_line();
        assert!(line.contains("1 staged"));
        assert!(line.contains("1 total"));
    }

    #[test]
    fn vcs_diff_one_line_scoped_and_unscoped() {
        let scoped = VcsDiff {
            path: "src/a.rs".to_string(),
            unified_diff: String::new(),
            files_changed: 1,
            insertions: 3,
            deletions: 1,
        };
        let unscoped = VcsDiff {
            path: String::new(),
            unified_diff: String::new(),
            files_changed: 2,
            insertions: 5,
            deletions: 2,
        };
        assert!(scoped.one_line().contains("src/a.rs"));
        assert!(scoped.one_line().contains("+3"));
        assert!(unscoped.one_line().contains("2 files"));
    }
}

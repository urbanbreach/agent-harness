// allow: SIZE_OK — git worktree lifecycle (create/list/remove + porcelain parse + isolation tests)
//! Git worktree lifecycle for isolated Harness sessions.
//!
//! Clean-room product surface: create a linked worktree under a deterministic
//! path, list worktrees under the session parent, remove/cleanup safely, roll
//! back partial failures, and report actionable non-git errors.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

/// Default relative base for session worktrees under a repository root.
pub const DEFAULT_WORKTREE_RELATIVE_BASE: &str = ".agent-harness/worktrees";

/// Branch name prefix for Harness-created worktrees.
pub const WORKTREE_BRANCH_PREFIX: &str = "harness/wt-";

/// Result of a successful worktree creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedWorktree {
    /// Absolute path to the new worktree checkout.
    pub path: PathBuf,
    /// Branch checked out in the worktree.
    pub branch: String,
    /// Repository root that owns the worktree.
    pub repository_root: PathBuf,
    /// Stable slug used for path and branch naming.
    pub slug: String,
}

/// Worktree lifecycle failures.
#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("workspace is not a git repository: {path}")]
    NotAGitRepository { path: String },
    #[error("worktree path already exists: {path}")]
    PathCollision { path: String },
    #[error("branch already exists: {branch}")]
    BranchCollision { branch: String },
    #[error("failed to create worktree parent directory {path}: {source}")]
    ParentDirectory {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("git worktree add failed: {detail}")]
    GitFailed { detail: String },
    #[error("worktree creation rolled back after failure: {detail}")]
    RolledBack { detail: String },
    #[error("worktree not found: {path}")]
    NotFound { path: String },
    #[error("refusing to remove primary worktree: {path}")]
    PrimaryWorktree { path: String },
    #[error("worktree path is outside the session worktree parent: {path}")]
    UnsafeRemovePath { path: String },
    #[error("git worktree remove failed: {detail}")]
    RemoveFailed { detail: String },
}

/// Listed git worktree entry for a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedWorktree {
    /// Absolute path to the worktree checkout.
    pub path: PathBuf,
    /// Checked-out branch name when not detached.
    pub branch: Option<String>,
    /// HEAD commit object name when reported by git.
    pub head: Option<String>,
    /// True when this is a bare worktree entry.
    pub bare: bool,
    /// True when HEAD is detached.
    pub detached: bool,
    /// True when the path is under the Harness session worktree parent.
    pub harness_managed: bool,
    /// Slug derived from the path basename when under the session parent.
    pub slug: Option<String>,
}

/// Options for removing a session worktree.
#[derive(Debug, Clone)]
pub struct RemoveWorktreeOptions<'a> {
    /// Repository root that owns the linked worktree.
    pub repository_root: &'a Path,
    /// Path to the linked worktree checkout (must be under the session parent).
    pub path: &'a Path,
    /// Directory that contains session worktrees; defaults to
    /// `{repository_root}/.agent-harness/worktrees`.
    pub worktree_parent: Option<&'a Path>,
    /// When true, delete a `harness/wt-*` branch after a successful remove.
    pub delete_branch: bool,
    /// Pass `--force` to `git worktree remove`.
    pub force: bool,
}

/// Options for creating a session worktree.
#[derive(Debug, Clone)]
pub struct CreateWorktreeOptions<'a> {
    /// Repository root (must be a git work tree).
    pub repository_root: &'a Path,
    /// Directory that will contain the new worktree folder.
    /// Defaults to `{repository_root}/.agent-harness/worktrees` when `None`.
    pub worktree_parent: Option<&'a Path>,
    /// Optional stable slug. When omitted, a time-based slug is generated.
    pub slug: Option<&'a str>,
    /// Optional base ref for the new branch (default: HEAD).
    pub start_point: Option<&'a str>,
}

/// Create an isolated git worktree for a new Harness session.
///
/// Naming is deterministic for a given slug:
/// - path: `{parent}/{slug}`
/// - branch: `harness/wt-{slug}`
///
/// Partial failures attempt rollback (`git worktree remove --force` + branch delete).
pub fn create_session_worktree(
    options: CreateWorktreeOptions<'_>,
) -> Result<CreatedWorktree, WorktreeError> {
    let repository_root = options.repository_root;
    if !is_git_repository(repository_root) {
        return Err(WorktreeError::NotAGitRepository {
            path: repository_root.display().to_string(),
        });
    }

    let slug = options
        .slug
        .map(sanitize_slug)
        .filter(|slug| !slug.is_empty())
        .unwrap_or_else(generate_slug);
    let branch = format!("{WORKTREE_BRANCH_PREFIX}{slug}");
    let parent = options
        .worktree_parent
        .map(Path::to_path_buf)
        .unwrap_or_else(|| repository_root.join(DEFAULT_WORKTREE_RELATIVE_BASE));
    let path = parent.join(&slug);

    if path.exists() {
        return Err(WorktreeError::PathCollision {
            path: path.display().to_string(),
        });
    }
    if branch_exists(repository_root, &branch) {
        return Err(WorktreeError::BranchCollision { branch });
    }

    fs::create_dir_all(&parent).map_err(|source| WorktreeError::ParentDirectory {
        path: parent.display().to_string(),
        source,
    })?;

    let start_point = options.start_point.unwrap_or("HEAD");
    let add_result = git_output(
        repository_root,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &path.display().to_string(),
            start_point,
        ],
    );

    if let Err(detail) = add_result {
        let rollback_detail = rollback_partial_worktree(repository_root, &path, &branch);
        return Err(WorktreeError::RolledBack {
            detail: format!("{detail}; rollback: {rollback_detail}"),
        });
    }

    Ok(CreatedWorktree {
        path,
        branch,
        repository_root: repository_root.to_path_buf(),
        slug,
    })
}

/// List git worktrees for `repository_root`, optionally scoped to a parent.
///
/// When `worktree_parent` is `Some` (or defaults to the Harness session parent),
/// entries under that parent are marked `harness_managed` with a path slug.
pub fn list_session_worktrees(
    repository_root: &Path,
    worktree_parent: Option<&Path>,
) -> Result<Vec<ListedWorktree>, WorktreeError> {
    if !is_git_repository(repository_root) {
        return Err(WorktreeError::NotAGitRepository {
            path: repository_root.display().to_string(),
        });
    }

    let parent = worktree_parent
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_worktree_parent(repository_root));
    let parent_canon = canonicalize_best_effort(&parent);
    let porcelain = git_output(repository_root, &["worktree", "list", "--porcelain"])
        .map_err(|detail| WorktreeError::GitFailed { detail })?;

    Ok(parse_worktree_list_porcelain(&porcelain, &parent_canon))
}

/// Remove a linked session worktree under the Harness parent (safe cleanup).
///
/// Refuses the primary worktree and any path outside the session worktree parent.
/// Partial failures surface `RemoveFailed` without deleting unrelated branches.
pub fn remove_session_worktree(options: RemoveWorktreeOptions<'_>) -> Result<(), WorktreeError> {
    let repository_root = options.repository_root;
    if !is_git_repository(repository_root) {
        return Err(WorktreeError::NotAGitRepository {
            path: repository_root.display().to_string(),
        });
    }

    let parent = options
        .worktree_parent
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_worktree_parent(repository_root));
    let target = canonicalize_best_effort(options.path);
    let parent_canon = canonicalize_best_effort(&parent);
    let primary = canonicalize_best_effort(repository_root);

    if target == primary {
        return Err(WorktreeError::PrimaryWorktree {
            path: target.display().to_string(),
        });
    }
    if !path_is_under(&target, &parent_canon) {
        return Err(WorktreeError::UnsafeRemovePath {
            path: target.display().to_string(),
        });
    }

    let listed = list_session_worktrees(repository_root, Some(&parent))?;
    let entry = listed
        .iter()
        .find(|item| canonicalize_best_effort(&item.path) == target)
        .ok_or_else(|| WorktreeError::NotFound {
            path: target.display().to_string(),
        })?;

    let branch = entry.branch.clone();
    let path_arg = target.display().to_string();
    let remove_result = if options.force {
        git_output(
            repository_root,
            &["worktree", "remove", "--force", &path_arg],
        )
    } else {
        git_output(repository_root, &["worktree", "remove", &path_arg])
    };
    remove_result.map_err(|detail| WorktreeError::RemoveFailed { detail })?;

    if options.delete_branch {
        if let Some(branch) = branch.as_deref() {
            if branch.starts_with(WORKTREE_BRANCH_PREFIX) && branch_exists(repository_root, branch)
            {
                let _ = git_output(repository_root, &["branch", "-D", branch]);
            }
        }
    }

    Ok(())
}

/// Default parent directory for session worktrees under `repository_root`.
pub fn default_worktree_parent(repository_root: &Path) -> PathBuf {
    repository_root.join(DEFAULT_WORKTREE_RELATIVE_BASE)
}

/// Sanitize a user/system slug into a path- and branch-safe token.
pub fn sanitize_slug(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else if (ch == '/' || ch == '.' || ch == ' ') && !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.starts_with('-') {
        out.remove(0);
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.chars().take(48).collect()
}

fn generate_slug() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("{millis:x}")
}

fn is_git_repository(path: &Path) -> bool {
    git_output(path, &["rev-parse", "--is-inside-work-tree"])
        .ok()
        .is_some_and(|text| text.trim() == "true")
}

fn branch_exists(repository_root: &Path, branch: &str) -> bool {
    git_output(
        repository_root,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
}

fn rollback_partial_worktree(repository_root: &Path, path: &Path, branch: &str) -> String {
    let mut notes = Vec::new();
    if path.exists() {
        match git_output(
            repository_root,
            &["worktree", "remove", "--force", &path.display().to_string()],
        ) {
            Ok(_) => notes.push("worktree removed".to_string()),
            Err(err) => {
                notes.push(format!("worktree remove failed: {err}"));
                if let Err(remove_err) = fs::remove_dir_all(path) {
                    notes.push(format!("directory remove failed: {remove_err}"));
                } else {
                    notes.push("directory removed".to_string());
                }
            }
        }
    }
    match git_output(repository_root, &["branch", "-D", branch]) {
        Ok(_) => notes.push(format!("branch {branch} deleted")),
        Err(err) => notes.push(format!("branch delete skipped/failed: {err}")),
    }
    if notes.is_empty() {
        "nothing to roll back".to_string()
    } else {
        notes.join("; ")
    }
}

fn canonicalize_best_effort(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn path_is_under(path: &Path, parent: &Path) -> bool {
    let path = canonicalize_best_effort(path);
    let parent = canonicalize_best_effort(parent);
    path.starts_with(&parent)
}

fn parse_worktree_list_porcelain(porcelain: &str, parent_canon: &Path) -> Vec<ListedWorktree> {
    let mut out = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;
    let mut head: Option<String> = None;
    let mut bare = false;
    let mut detached = false;

    let flush = |path: &mut Option<PathBuf>,
                 branch: &mut Option<String>,
                 head: &mut Option<String>,
                 bare: &mut bool,
                 detached: &mut bool,
                 out: &mut Vec<ListedWorktree>| {
        let Some(worktree_path) = path.take() else {
            *branch = None;
            *head = None;
            *bare = false;
            *detached = false;
            return;
        };
        let path_canon = canonicalize_best_effort(&worktree_path);
        let harness_managed = path_is_under(&path_canon, parent_canon);
        let slug = if harness_managed {
            worktree_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        } else {
            None
        };
        out.push(ListedWorktree {
            path: worktree_path,
            branch: branch.take(),
            head: head.take(),
            bare: *bare,
            detached: *detached,
            harness_managed,
            slug,
        });
        *bare = false;
        *detached = false;
    };

    for line in porcelain.lines() {
        if line.is_empty() {
            flush(
                &mut path,
                &mut branch,
                &mut head,
                &mut bare,
                &mut detached,
                &mut out,
            );
            continue;
        }
        if let Some(rest) = line.strip_prefix("worktree ") {
            flush(
                &mut path,
                &mut branch,
                &mut head,
                &mut bare,
                &mut detached,
                &mut out,
            );
            path = Some(PathBuf::from(rest));
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            head = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("branch ") {
            let name = rest.strip_prefix("refs/heads/").unwrap_or(rest).to_string();
            branch = Some(name);
            detached = false;
        } else if line == "detached" {
            detached = true;
            branch = None;
        } else if line == "bare" {
            bare = true;
        }
    }
    flush(
        &mut path,
        &mut branch,
        &mut head,
        &mut bare,
        &mut detached,
        &mut out,
    );
    out
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<String, String> {
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
    use crate::event::{ActorKind, EventActor, EventV1, RunStartedEvent, SCHEMA_VERSION};
    use crate::store::{EventEnvelopeWithoutSeqV1, EventStore, JsonlFileEventStore};
    use crate::UnwrapOrAbort;
    use std::process::Command as GitCommand;
    use std::process::Command;

    fn init_git_repo(path: &Path) {
        run_git(path, &["init", "-b", "main"]);
        run_git(path, &["config", "user.email", "worktree@example.com"]);
        run_git(path, &["config", "user.name", "Worktree Test"]);
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

    fn create_slug(repo: &Path, slug: &str) -> CreatedWorktree {
        create_session_worktree(CreateWorktreeOptions {
            repository_root: repo,
            worktree_parent: None,
            slug: Some(slug),
            start_point: None,
        })
        .unwrap_or_abort()
    }

    #[test]
    fn create_session_worktree_creates_path_and_branch() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let created = create_slug(&repo, "slice-alpha");

        assert_eq!(created.slug, "slice-alpha");
        assert_eq!(created.branch, "harness/wt-slice-alpha");
        assert_eq!(
            created.path,
            repo.join(DEFAULT_WORKTREE_RELATIVE_BASE)
                .join("slice-alpha")
        );
        assert!(created.path.join("README.md").is_file());
        assert!(branch_exists(&repo, &created.branch));
        assert_eq!(
            git_output(&created.path, &["rev-parse", "--abbrev-ref", "HEAD"])
                .unwrap_or_abort()
                .trim(),
            created.branch
        );
    }

    #[test]
    fn create_session_worktree_cow_fastpath_overlays_untracked_file() {
        // arrange
        // act
        // assert
        // Given: git worktree + untracked overlay source in repo
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);
        fs::write(repo.join(".harness-cow-overlay"), b"cow-overlay-payload\n").unwrap_or_abort();
        let created = create_slug(&repo, "cow-overlay");
        assert!(!created.path.join(".harness-cow-overlay").exists());

        // When: product COW overlay into the real worktree checkout
        let report = crate::cow_worktree::apply_cow_worktree_fastpath(
            &repo,
            &created.path,
            &[".harness-cow-overlay", "absent-overlay.bin"],
        );

        // Then: structured availability + real overlay outcomes on disk
        assert!(report.availability.is_available() || report.availability.is_unavailable());
        assert_eq!(report.overlays.len(), 2);
        assert!(report.overlays[1].is_unavailable());
        if report.overlays[0].is_cloned() {
            assert_eq!(
                fs::read(created.path.join(".harness-cow-overlay")).unwrap_or_abort(),
                b"cow-overlay-payload\n"
            );
            assert!(report.has_cloned_overlay());
        } else {
            assert!(report.overlays[0].is_unavailable());
        }
        assert!(report.one_line().contains("COW worktree fastpath:"));
    }

    #[test]
    fn create_session_worktree_rejects_non_git_repository() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let err = create_session_worktree(CreateWorktreeOptions {
            repository_root: temp.path(),
            worktree_parent: None,
            slug: Some("nope"),
            start_point: None,
        })
        .expect_err("non-git should fail");
        assert!(matches!(err, WorktreeError::NotAGitRepository { .. }));
    }

    #[test]
    fn list_session_worktrees_fails_closed_on_non_git_repository() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let not_a_repo = temp.path().join("not-a-repo");
        fs::create_dir_all(&not_a_repo).unwrap_or_abort();

        let err = list_session_worktrees(&not_a_repo, None).expect_err("non-git should fail");

        assert!(matches!(err, WorktreeError::NotAGitRepository { .. }));
    }

    #[test]
    fn create_session_worktree_rejects_path_collision() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        create_slug(&repo, "taken");

        let err = create_session_worktree(CreateWorktreeOptions {
            repository_root: &repo,
            worktree_parent: None,
            slug: Some("taken"),
            start_point: None,
        })
        .expect_err("collision should fail");
        assert!(matches!(err, WorktreeError::PathCollision { .. }));
        assert!(repo
            .join(DEFAULT_WORKTREE_RELATIVE_BASE)
            .join("taken")
            .is_dir());
        assert!(branch_exists(&repo, "harness/wt-taken"));
    }

    #[test]
    fn create_session_worktree_rejects_branch_collision_without_partial_path() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);
        run_git(&repo, &["branch", "harness/wt-preexisting"]);

        let err = create_session_worktree(CreateWorktreeOptions {
            repository_root: &repo,
            worktree_parent: None,
            slug: Some("preexisting"),
            start_point: None,
        })
        .expect_err("branch collision should fail");
        assert!(matches!(err, WorktreeError::BranchCollision { .. }));
        assert!(!repo
            .join(DEFAULT_WORKTREE_RELATIVE_BASE)
            .join("preexisting")
            .exists());
    }

    #[test]
    fn create_session_worktree_rolls_back_invalid_start_point() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let err = create_session_worktree(CreateWorktreeOptions {
            repository_root: &repo,
            worktree_parent: None,
            slug: Some("broken-start"),
            start_point: Some("refs/does-not-exist"),
        })
        .expect_err("invalid start_point should roll back");
        assert!(matches!(err, WorktreeError::RolledBack { .. }));
        assert!(!repo
            .join(DEFAULT_WORKTREE_RELATIVE_BASE)
            .join("broken-start")
            .exists());
        assert!(!branch_exists(&repo, "harness/wt-broken-start"));
    }

    #[test]
    fn list_session_worktrees_returns_created_entries_under_parent() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let first = create_slug(&repo, "alpha");
        let second = create_slug(&repo, "beta");

        let listed = list_session_worktrees(&repo, None).unwrap_or_abort();
        let managed: Vec<_> = listed
            .into_iter()
            .filter(|item| item.harness_managed)
            .collect();
        assert_eq!(managed.len(), 2);
        let paths: Vec<_> = managed.iter().map(|item| item.path.clone()).collect();
        assert!(paths.contains(&first.path));
        assert!(paths.contains(&second.path));
        assert!(managed.iter().any(|item| {
            item.slug.as_deref() == Some("alpha")
                && item.branch.as_deref() == Some("harness/wt-alpha")
        }));
        assert!(managed.iter().any(|item| {
            item.slug.as_deref() == Some("beta")
                && item.branch.as_deref() == Some("harness/wt-beta")
        }));
    }

    #[test]
    fn remove_session_worktree_deletes_path_and_optional_branch() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let created = create_slug(&repo, "cleanup-me");
        assert!(created.path.is_dir());
        assert!(branch_exists(&repo, &created.branch));

        remove_session_worktree(RemoveWorktreeOptions {
            repository_root: &repo,
            path: &created.path,
            worktree_parent: None,
            delete_branch: true,
            force: true,
        })
        .unwrap_or_abort();

        assert!(!created.path.exists());
        assert!(!branch_exists(&repo, &created.branch));
        let managed = list_session_worktrees(&repo, None)
            .unwrap_or_abort()
            .into_iter()
            .filter(|item| item.harness_managed)
            .count();
        assert_eq!(managed, 0);
    }

    #[test]
    fn remove_session_worktree_refuses_primary_and_unsafe_paths() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let primary_err = remove_session_worktree(RemoveWorktreeOptions {
            repository_root: &repo,
            path: &repo,
            worktree_parent: None,
            delete_branch: false,
            force: true,
        })
        .expect_err("primary worktree must be refused");
        assert!(matches!(primary_err, WorktreeError::PrimaryWorktree { .. }));

        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap_or_abort();
        let unsafe_err = remove_session_worktree(RemoveWorktreeOptions {
            repository_root: &repo,
            path: &outside,
            worktree_parent: None,
            delete_branch: false,
            force: true,
        })
        .expect_err("outside parent must be refused");
        assert!(matches!(unsafe_err, WorktreeError::UnsafeRemovePath { .. }));
    }

    #[test]
    fn two_worktrees_have_distinct_paths_and_branches() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let left = create_slug(&repo, "iso-left");
        let right = create_slug(&repo, "iso-right");

        assert_ne!(left.path, right.path);
        assert_ne!(left.branch, right.branch);
        fs::write(left.path.join("left-only.txt"), "left\n").unwrap_or_abort();
        fs::write(right.path.join("right-only.txt"), "right\n").unwrap_or_abort();
        assert!(!right.path.join("left-only.txt").exists());
        assert!(!left.path.join("right-only.txt").exists());
    }

    #[test]
    fn concurrent_worktree_sessions_use_isolated_event_paths() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        let left = create_slug(&repo, "sess-left");
        let right = create_slug(&repo, "sess-right");

        let session_root = temp.path().join("sessions");
        let store_left =
            JsonlFileEventStore::open(&session_root, "run-left", true).unwrap_or_abort();
        let store_right =
            JsonlFileEventStore::open(&session_root, "run-right", true).unwrap_or_abort();

        assert_ne!(store_left.file_path(), store_right.file_path());
        assert!(store_left.file_path().ends_with("run-left/events.jsonl"));
        assert!(store_right.file_path().ends_with("run-right/events.jsonl"));

        let left_event = store_left
            .append(run_started_draft(
                "run-left",
                left.path.to_string_lossy().as_ref(),
                1,
            ))
            .unwrap_or_abort();
        let right_event = store_right
            .append(run_started_draft(
                "run-right",
                right.path.to_string_lossy().as_ref(),
                2,
            ))
            .unwrap_or_abort();

        match &left_event.payload {
            EventV1::RunStarted(payload) => {
                assert_eq!(payload.workspace_root, left.path.to_string_lossy());
                assert_ne!(payload.workspace_root, right.path.to_string_lossy());
            }
            other => panic!("expected RunStarted, got {other:?}"),
        }
        match &right_event.payload {
            EventV1::RunStarted(payload) => {
                assert_eq!(payload.workspace_root, right.path.to_string_lossy());
            }
            other => panic!("expected RunStarted, got {other:?}"),
        }

        let left_log = fs::read_to_string(store_left.file_path()).unwrap_or_abort();
        let right_log = fs::read_to_string(store_right.file_path()).unwrap_or_abort();
        assert!(left_log.contains("sess-left"));
        assert!(!left_log.contains("sess-right"));
        assert!(right_log.contains("sess-right"));
        assert!(!right_log.contains("sess-left"));
    }

    fn run_started_draft(
        run_id: &str,
        workspace_root: &str,
        marker: u64,
    ) -> EventEnvelopeWithoutSeqV1 {
        EventEnvelopeWithoutSeqV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{marker:04}"),
            run_id: run_id.to_string().into(),
            mono_ms: marker,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some(format!("run:{run_id}")),
            payload: EventV1::RunStarted(RunStartedEvent {
                run_name: format!("run-{marker}").into(),
                workspace_root: workspace_root.to_string(),
            }),
        }
    }

    #[test]
    fn sanitize_slug_is_path_safe() {
        // arrange
        // act
        // assert
        assert_eq!(sanitize_slug("Hello/World.Test"), "hello-world-test");
        assert_eq!(sanitize_slug("  a__b  "), "a__b");
    }
}

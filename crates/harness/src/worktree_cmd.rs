//! Product CLI for Harness session worktrees (`harness worktree`).
//!
//! Surfaces list/select/cleanup over [`harness_core::worktree`]:
//! - `list` calls [`list_session_worktrees`] and reports managed session
//!   worktrees (and foreign ones with `--all`);
//! - `remove <slug>` lets the operator select one listed worktree by slug and
//!   removes it through [`remove_session_worktree`] (parent-scoped, with primary
//!   and unsafe-path refusal and optional `harness/wt-*` branch deletion);
//! - `cleanup` lists the managed session worktrees and removes all of them.
//!
//! The create/enter half of the worktree lifecycle lives in the TUI new-live
//! flow (`worktree.create`); this command owns the list/select/cleanup product
//! surface and never rewrites event logs or executes providers.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::worktree::{
    list_session_worktrees, remove_session_worktree, ListedWorktree, RemoveWorktreeOptions,
    WorktreeError,
};
use serde::Serialize;

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct WorktreeCommand {
    #[command(subcommand)]
    command: WorktreeSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum WorktreeSubcommand {
    /// List Harness session worktrees (managed by default; --all adds foreign).
    List(WorktreeListCommand),
    /// Select and remove one session worktree by slug (with its harness/wt-* branch).
    Remove(WorktreeRemoveCommand),
    /// Remove all managed session worktrees under the repository (bulk cleanup).
    Cleanup(WorktreeCleanupCommand),
}

#[derive(Debug, Args, Clone, Default)]
struct WorktreeListCommand {
    /// Repository root (defaults to current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Include non-Harness (foreign) worktrees.
    #[arg(long, default_value_t = false)]
    all: bool,
}

#[derive(Debug, Args, Clone)]
struct WorktreeRemoveCommand {
    /// Slug of the session worktree to remove (see `harness worktree list`).
    slug: String,
    /// Repository root (defaults to current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Pass --force to `git worktree remove`.
    #[arg(long, default_value_t = false)]
    force: bool,
    /// Keep the harness/wt-* branch instead of deleting it.
    #[arg(long, default_value_t = false)]
    keep_branch: bool,
}

#[derive(Debug, Args, Clone, Default)]
struct WorktreeCleanupCommand {
    /// Repository root (defaults to current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Pass --force to `git worktree remove` for each worktree.
    #[arg(long, default_value_t = false)]
    force: bool,
    /// Keep harness/wt-* branches instead of deleting them.
    #[arg(long, default_value_t = false)]
    keep_branch: bool,
}

#[derive(Debug, Serialize)]
struct WorktreeListJson {
    repository_root: String,
    managed_count: usize,
    worktrees: Vec<WorktreeEntryJson>,
}

#[derive(Debug, Serialize)]
struct WorktreeEntryJson {
    path: String,
    branch: Option<String>,
    head: Option<String>,
    harness_managed: bool,
    slug: Option<String>,
    bare: bool,
    detached: bool,
}

impl From<&ListedWorktree> for WorktreeEntryJson {
    fn from(item: &ListedWorktree) -> Self {
        Self {
            path: item.path.display().to_string(),
            branch: item.branch.clone(),
            head: item.head.clone(),
            harness_managed: item.harness_managed,
            slug: item.slug.clone(),
            bare: item.bare,
            detached: item.detached,
        }
    }
}

#[derive(Debug, Serialize)]
struct WorktreeRemoveJson {
    removed: bool,
    slug: String,
    path: String,
    branch: Option<String>,
    delete_branch: bool,
}

#[derive(Debug, Serialize)]
struct WorktreeCleanupJson {
    repository_root: String,
    removed_count: usize,
    failed_count: usize,
    removed: Vec<WorktreeRemoveJson>,
    failed: Vec<WorktreeCleanupFailure>,
}

#[derive(Debug, Serialize)]
struct WorktreeCleanupFailure {
    slug: Option<String>,
    path: String,
    error: String,
}

pub(crate) fn execute_with_io(command: WorktreeCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    match command.command {
        WorktreeSubcommand::List(cmd) => run_list(cmd, io, deps),
        WorktreeSubcommand::Remove(cmd) => run_remove(cmd, io, deps),
        WorktreeSubcommand::Cleanup(cmd) => run_cleanup(cmd, io, deps),
    }
}

fn resolve_repository(
    workspace: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<PathBuf, i32> {
    match workspace {
        Some(path) => Ok(path),
        None => deps.current_dir().map_err(|err| {
            let _ = writeln!(
                io.stderr,
                "worktree: failed to resolve current working directory: {err}"
            );
            2
        }),
    }
}

fn write_json(io: &mut CliIo<'_>, value: &impl Serialize) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "worktree: failed to serialize JSON: {err}");
            1
        }
    }
}

fn map_worktree_error(io: &mut CliIo<'_>, err: WorktreeError) -> i32 {
    let _ = writeln!(io.stderr, "worktree: {err}");
    1
}

fn run_list(cmd: WorktreeListCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let repository_root = match resolve_repository(cmd.workspace, io, deps) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let listed = match list_session_worktrees(&repository_root, None) {
        Ok(listed) => listed,
        Err(err) => return map_worktree_error(io, err),
    };
    let managed_count = listed.iter().filter(|item| item.harness_managed).count();
    let worktrees: Vec<WorktreeEntryJson> = listed
        .iter()
        .filter(|item| cmd.all || item.harness_managed)
        .map(WorktreeEntryJson::from)
        .collect();
    write_json(
        io,
        &WorktreeListJson {
            repository_root: repository_root.display().to_string(),
            managed_count,
            worktrees,
        },
    )
}

fn run_remove(cmd: WorktreeRemoveCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let repository_root = match resolve_repository(cmd.workspace, io, deps) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let listed = match list_session_worktrees(&repository_root, None) {
        Ok(listed) => listed,
        Err(err) => return map_worktree_error(io, err),
    };
    let Some(entry) = listed
        .iter()
        .find(|item| item.harness_managed && item.slug.as_deref() == Some(cmd.slug.as_str()))
    else {
        let _ = writeln!(
            io.stderr,
            "worktree: no managed session worktree with slug `{}` (run `harness worktree list` to see slugs)",
            cmd.slug
        );
        return 1;
    };

    let path = entry.path.clone();
    let branch = entry.branch.clone();
    let delete_branch = !cmd.keep_branch;
    if let Err(err) = remove_session_worktree(RemoveWorktreeOptions {
        repository_root: &repository_root,
        path: &path,
        worktree_parent: None,
        delete_branch,
        force: cmd.force,
    }) {
        return map_worktree_error(io, err);
    }

    write_json(
        io,
        &WorktreeRemoveJson {
            removed: true,
            slug: cmd.slug,
            path: path.display().to_string(),
            branch,
            delete_branch,
        },
    )
}

fn run_cleanup(cmd: WorktreeCleanupCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let repository_root = match resolve_repository(cmd.workspace, io, deps) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let listed = match list_session_worktrees(&repository_root, None) {
        Ok(listed) => listed,
        Err(err) => return map_worktree_error(io, err),
    };
    let managed: Vec<ListedWorktree> = listed
        .into_iter()
        .filter(|item| item.harness_managed)
        .collect();
    let delete_branch = !cmd.keep_branch;

    let mut removed = Vec::new();
    let mut failed = Vec::new();
    for entry in managed {
        let path = entry.path;
        let slug = entry.slug;
        let branch = entry.branch;
        match remove_session_worktree(RemoveWorktreeOptions {
            repository_root: &repository_root,
            path: &path,
            worktree_parent: None,
            delete_branch,
            force: cmd.force,
        }) {
            Ok(()) => removed.push(WorktreeRemoveJson {
                removed: true,
                slug: slug.unwrap_or_default(),
                path: path.display().to_string(),
                branch,
                delete_branch,
            }),
            Err(err) => failed.push(WorktreeCleanupFailure {
                slug,
                path: path.display().to_string(),
                error: err.to_string(),
            }),
        }
    }

    let removed_count = removed.len();
    let failed_count = failed.len();
    let emit_code = write_json(
        io,
        &WorktreeCleanupJson {
            repository_root: repository_root.display().to_string(),
            removed_count,
            failed_count,
            removed,
            failed,
        },
    );
    emit_code.max(if failed_count == 0 { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use harness_core::worktree::{create_session_worktree, CreateWorktreeOptions};
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;
    use std::process::Command as GitCommand;
    use tempfile::tempdir;

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

    fn init_git_repo(path: &Path) {
        run_git(path, &["init", "-b", "main"]);
        run_git(path, &["config", "user.email", "worktree@example.com"]);
        run_git(path, &["config", "user.name", "Worktree Test"]);
        fs::write(path.join("README.md"), "seed\n").unwrap_or_abort();
        run_git(path, &["add", "README.md"]);
        run_git(path, &["commit", "-m", "seed"]);
    }

    fn create_slug(repo: &Path, slug: &str) {
        create_session_worktree(CreateWorktreeOptions {
            repository_root: repo,
            worktree_parent: None,
            slug: Some(slug),
            start_point: None,
        })
        .unwrap_or_abort();
    }

    fn branch_exists(repo: &Path, branch: &str) -> bool {
        GitCommand::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ])
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn run_cli(cwd: &Path, args: &[&str]) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let deps = CliDeps::real().with_current_dir(cwd.to_path_buf());
        let mut argv: Vec<&str> = vec!["harness", "worktree"];
        argv.extend_from_slice(args);
        let outcome = crate::run(argv, &mut io, deps);
        (
            outcome.code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn list_remove_cleanup_roundtrip_via_cli_handlers() {
        // arrange — a real git repo with two managed session worktrees
        let dir = tempdir().unwrap_or_abort();
        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);
        create_slug(&repo, "alpha");
        create_slug(&repo, "beta");

        // act + assert — list shows both managed worktrees
        let (code, stdout, stderr) = run_cli(&repo, &["list"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"managed_count\": 2"), "stdout: {stdout}");
        assert!(stdout.contains("\"slug\": \"alpha\""), "stdout: {stdout}");
        assert!(stdout.contains("\"slug\": \"beta\""), "stdout: {stdout}");

        // act + assert — select + remove alpha (path and branch gone)
        let (code, stdout, stderr) = run_cli(&repo, &["remove", "alpha", "--force"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"slug\": \"alpha\""), "stdout: {stdout}");
        assert!(stdout.contains("\"removed\": true"), "stdout: {stdout}");
        assert!(!repo.join(".agent-harness/worktrees/alpha").exists());
        assert!(!branch_exists(&repo, "harness/wt-alpha"));

        // act + assert — cleanup removes the remaining beta worktree
        let (code, stdout, stderr) = run_cli(&repo, &["cleanup", "--force"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"removed_count\": 1"), "stdout: {stdout}");
        assert!(!repo.join(".agent-harness/worktrees/beta").exists());
        assert!(!branch_exists(&repo, "harness/wt-beta"));

        // assert — nothing managed remains
        let (code, stdout, stderr) = run_cli(&repo, &["list"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"managed_count\": 0"), "stdout: {stdout}");
    }

    #[test]
    fn remove_unknown_slug_fails_closed() {
        // arrange
        let dir = tempdir().unwrap_or_abort();
        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);
        create_slug(&repo, "alpha");

        // act
        let (code, _stdout, stderr) = run_cli(&repo, &["remove", "ghost"]);

        // assert
        assert_eq!(code, 1);
        assert!(stderr.contains("no managed session worktree with slug `ghost`"));
        // the unrelated worktree survives
        assert!(repo.join(".agent-harness/worktrees/alpha").exists());
    }

    #[test]
    fn list_fails_closed_on_non_git_repository() {
        // arrange
        let dir = tempdir().unwrap_or_abort();
        let not_a_repo = dir.path().join("plain");
        fs::create_dir_all(&not_a_repo).unwrap_or_abort();

        // act
        let (code, _stdout, stderr) = run_cli(&not_a_repo, &["list"]);

        // assert
        assert_eq!(code, 1);
        assert!(stderr.contains("not a git repository"));
    }

    #[test]
    fn cleanup_on_empty_repo_reports_zero_removed() {
        // arrange — a git repo with no session worktrees
        let dir = tempdir().unwrap_or_abort();
        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo).unwrap_or_abort();
        init_git_repo(&repo);

        // act
        let (code, stdout, stderr) = run_cli(&repo, &["cleanup"]);

        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"removed_count\": 0"), "stdout: {stdout}");
        assert!(stdout.contains("\"failed_count\": 0"), "stdout: {stdout}");
    }
}

//! End-to-end CLI proof for `worktree.list_select_cleanup` (A-CAPABILITIES).
//!
//! Drives the full `harness worktree list|remove|cleanup` surface in-process via
//! [`harness::run`] so argument parsing → command dispatch → the
//! `harness_core::worktree` backend → real git/filesystem effects are exercised
//! together. Worktrees are seeded with the same backend `create_session_worktree`
//! the TUI new-live flow uses, then listed, selected+removed, and bulk-cleaned.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command as GitCommand;

use harness::{run, CliDeps, CliIo, UnwrapOrAbort};
use harness_core::worktree::{
    create_session_worktree, list_session_worktrees, CreateWorktreeOptions,
};
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

fn seed_worktree(repo: &Path, slug: &str) -> PathBuf {
    create_session_worktree(CreateWorktreeOptions {
        repository_root: repo,
        worktree_parent: None,
        slug: Some(slug),
        start_point: None,
    })
    .unwrap_or_abort()
    .path
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

fn managed_slugs(repo: &Path) -> Vec<String> {
    list_session_worktrees(repo, None)
        .unwrap_or_abort()
        .into_iter()
        .filter(|item| item.harness_managed)
        .filter_map(|item| item.slug)
        .collect()
}

fn worktree_cli(repo: &Path, args: &[&str]) -> (i32, String, String) {
    let mut stdin = Cursor::new(Vec::<u8>::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let mut argv: Vec<&str> = vec!["harness", "worktree"];
    argv.extend_from_slice(args);
    let outcome = run(
        argv,
        &mut io,
        CliDeps::real().with_current_dir(repo.to_path_buf()),
    );
    (
        outcome.code,
        String::from_utf8_lossy(&stdout).to_string(),
        String::from_utf8_lossy(&stderr).to_string(),
    )
}

#[test]
fn worktree_list_select_remove_and_cleanup_end_to_end_via_cli() {
    // arrange — a real git repo with two backend-created session worktrees
    let dir = tempdir().unwrap_or_abort();
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    let alpha_path = seed_worktree(&repo, "alpha");
    let beta_path = seed_worktree(&repo, "beta");
    assert_eq!(
        managed_slugs(&repo),
        vec!["alpha".to_string(), "beta".to_string()]
    );

    // act — `harness worktree list` surfaces both managed worktrees
    let (code, stdout, stderr) = worktree_cli(&repo, &["list"]);

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("\"managed_count\": 2"), "stdout: {stdout}");
    assert!(stdout.contains("\"slug\": \"alpha\""), "stdout: {stdout}");
    assert!(stdout.contains("\"slug\": \"beta\""), "stdout: {stdout}");

    // act — `harness worktree remove alpha` selects + cleans up one worktree
    let (code, stdout, stderr) = worktree_cli(&repo, &["remove", "alpha", "--force"]);

    // assert — the worktree path AND its harness/wt-* branch are gone on disk
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("\"slug\": \"alpha\""), "stdout: {stdout}");
    assert!(stdout.contains("\"removed\": true"), "stdout: {stdout}");
    assert!(!alpha_path.exists(), "alpha checkout must be removed");
    assert!(
        !branch_exists(&repo, "harness/wt-alpha"),
        "branch must be deleted"
    );
    assert_eq!(managed_slugs(&repo), vec!["beta".to_string()]);

    // act — `harness worktree cleanup` bulk-removes the remaining worktree
    let (code, stdout, stderr) = worktree_cli(&repo, &["cleanup", "--force"]);

    // assert — nothing managed remains and beta is gone on disk
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("\"removed_count\": 1"), "stdout: {stdout}");
    assert!(stdout.contains("\"failed_count\": 0"), "stdout: {stdout}");
    assert!(!beta_path.exists(), "beta checkout must be removed");
    assert!(
        !branch_exists(&repo, "harness/wt-beta"),
        "branch must be deleted"
    );
    assert!(managed_slugs(&repo).is_empty());
}

#[test]
fn worktree_remove_selects_only_the_named_slug() {
    // arrange — two worktrees; remove must touch only the selected one
    let dir = tempdir().unwrap_or_abort();
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    let alpha_path = seed_worktree(&repo, "alpha");
    let beta_path = seed_worktree(&repo, "beta");

    // act
    let (code, _stdout, stderr) = worktree_cli(&repo, &["remove", "beta"]);

    // assert — non-forced removal of a clean worktree succeeds for beta only
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(!beta_path.exists(), "beta checkout must be removed");
    assert!(
        alpha_path.exists(),
        "alpha must survive an unrelated select"
    );
    assert_eq!(managed_slugs(&repo), vec!["alpha".to_string()]);
}

#[test]
fn worktree_remove_unknown_slug_fails_closed_via_cli() {
    // arrange
    let dir = tempdir().unwrap_or_abort();
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    let alpha_path = seed_worktree(&repo, "alpha");

    // act
    let (code, _stdout, stderr) = worktree_cli(&repo, &["remove", "ghost"]);

    // assert — the command fails without touching existing worktrees
    assert_eq!(code, 1);
    assert!(
        stderr.contains("no managed session worktree with slug `ghost`"),
        "stderr: {stderr}"
    );
    assert!(alpha_path.exists());
}

#[test]
fn worktree_list_fails_closed_on_non_git_repository_via_cli() {
    // arrange
    let dir = tempdir().unwrap_or_abort();
    let not_a_repo = dir.path().join("plain");
    fs::create_dir_all(&not_a_repo).unwrap_or_abort();

    // act
    let (code, _stdout, stderr) = worktree_cli(&not_a_repo, &["list"]);

    // assert
    assert_eq!(code, 1);
    assert!(stderr.contains("not a git repository"), "stderr: {stderr}");
}

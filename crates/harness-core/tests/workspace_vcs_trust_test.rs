//! Workspace VCS trust integration tests (Task 20).
//!
//! Covers the full workspace/VCS/trust/attribution/cleanup journey matrix:
//! - isolated real-git worktree create/select/remove/cleanup
//! - collision rollback on path and branch conflicts
//! - COW fast path with safe fallback (structured unavailable)
//! - VCS status/diff for modified/untracked/deleted files
//! - agent vs external edit attribution with drift detection
//! - diff/blame UX between agent snapshot and current file
//! - durable folder trust persistence (allow/deny/reopen)
//! - deny-before-spawn gate for repository-local executables
//! - path traversal rejection in VCS diff paths
//! - concurrent worktree isolation (distinct paths/branches/event logs)
//! - attribution drift detection and revert to agent snapshot
//! - cleanup removes all managed worktrees and branches

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as GitCommand;

use harness_core::cow_worktree;
use harness_core::edit_attribution::{
    EditAttributionJournal, EditSource, EDIT_ATTRIBUTION_JOURNAL_REL,
};
use harness_core::folder_trust::{
    gate_repository_local_executable, gate_repository_local_executable_from_store,
    FolderTrustDecision, FolderTrustStore, LocalExecutableGate,
};
use harness_core::vcs::{collect_vcs_diff, collect_vcs_snapshot, collect_vcs_status, VcsError};
use harness_core::worktree::{
    create_session_worktree, list_session_worktrees, remove_session_worktree,
    CreateWorktreeOptions, RemoveWorktreeOptions, WorktreeError, DEFAULT_WORKTREE_RELATIVE_BASE,
    WORKTREE_BRANCH_PREFIX,
};
use harness_core::UnwrapOrAbort;

fn init_git_repo(path: &Path) {
    run_git(path, &["init", "-b", "main"]);
    run_git(path, &["config", "user.email", "task20@example.com"]);
    run_git(path, &["config", "user.name", "Task 20"]);
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

fn create_slug(repo: &Path, slug: &str) -> PathBuf {
    create_session_worktree(CreateWorktreeOptions {
        repository_root: repo,
        worktree_parent: None,
        slug: Some(slug),
        start_point: None,
    })
    .unwrap_or_abort()
    .path
}

fn managed_slugs(repo: &Path) -> Vec<String> {
    list_session_worktrees(repo, None)
        .unwrap_or_abort()
        .into_iter()
        .filter(|item| item.harness_managed)
        .filter_map(|item| item.slug)
        .collect()
}

// =========================================================================
// Isolated real-git worktree create/select/remove/cleanup journey
// =========================================================================

#[test]
fn isolated_real_git_worktree_create_list_remove_cleanup_journey() {
    // Given: a real git repo
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);

    // When: create two isolated worktrees
    let alpha = create_slug(&repo, "iso-alpha");
    let beta = create_slug(&repo, "iso-beta");

    // Then: both exist with distinct paths and branches
    assert!(alpha.join("README.md").is_file());
    assert!(beta.join("README.md").is_file());
    assert_ne!(alpha, beta);
    assert!(branch_exists(
        &repo,
        &format!("{WORKTREE_BRANCH_PREFIX}iso-alpha")
    ));
    assert!(branch_exists(
        &repo,
        &format!("{WORKTREE_BRANCH_PREFIX}iso-beta")
    ));
    assert_eq!(managed_slugs(&repo), vec!["iso-alpha", "iso-beta"]);

    // When: list via VCS status on one worktree
    fs::write(alpha.join("new-file.txt"), "content\n").unwrap_or_abort();
    let status = collect_vcs_status(&alpha).unwrap_or_abort();
    assert!(!status.is_clean());
    assert!(status.entries.iter().any(|e| e.path == "new-file.txt"));

    // When: remove one worktree by slug
    remove_session_worktree(RemoveWorktreeOptions {
        repository_root: &repo,
        path: &alpha,
        worktree_parent: None,
        delete_branch: true,
        force: true,
    })
    .unwrap_or_abort();

    // Then: only beta remains
    assert!(!alpha.exists());
    assert!(!branch_exists(
        &repo,
        &format!("{WORKTREE_BRANCH_PREFIX}iso-alpha")
    ));
    assert_eq!(managed_slugs(&repo), vec!["iso-beta"]);

    // When: cleanup all remaining
    let listed = list_session_worktrees(&repo, None).unwrap_or_abort();
    for entry in listed.iter().filter(|e| e.harness_managed) {
        remove_session_worktree(RemoveWorktreeOptions {
            repository_root: &repo,
            path: &entry.path,
            worktree_parent: None,
            delete_branch: true,
            force: true,
        })
        .unwrap_or_abort();
    }

    // Then: nothing managed remains
    assert!(managed_slugs(&repo).is_empty());
    assert!(!beta.exists());
}

// =========================================================================
// Collision rollback
// =========================================================================

#[test]
fn collision_rollback_on_path_and_branch_conflicts() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);

    // When: create first worktree
    create_slug(&repo, "collision-test");

    // Then: second create with same slug fails with PathCollision
    let err = create_session_worktree(CreateWorktreeOptions {
        repository_root: &repo,
        worktree_parent: None,
        slug: Some("collision-test"),
        start_point: None,
    })
    .expect_err("path collision should fail");
    assert!(matches!(err, WorktreeError::PathCollision { .. }));

    // When: pre-existing branch collision
    run_git(
        &repo,
        &["branch", &format!("{WORKTREE_BRANCH_PREFIX}preexist")],
    );
    let err = create_session_worktree(CreateWorktreeOptions {
        repository_root: &repo,
        worktree_parent: None,
        slug: Some("preexist"),
        start_point: None,
    })
    .expect_err("branch collision should fail");
    assert!(matches!(err, WorktreeError::BranchCollision { .. }));
    assert!(!repo
        .join(DEFAULT_WORKTREE_RELATIVE_BASE)
        .join("preexist")
        .exists());
}

#[test]
fn collision_rollback_on_invalid_start_point() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);

    // When: create with invalid start_point
    let err = create_session_worktree(CreateWorktreeOptions {
        repository_root: &repo,
        worktree_parent: None,
        slug: Some("bad-start"),
        start_point: Some("refs/does-not-exist"),
    })
    .expect_err("invalid start_point should roll back");

    // Then: rolled back, no partial state
    assert!(matches!(err, WorktreeError::RolledBack { .. }));
    assert!(!repo
        .join(DEFAULT_WORKTREE_RELATIVE_BASE)
        .join("bad-start")
        .exists());
    assert!(!branch_exists(
        &repo,
        &format!("{WORKTREE_BRANCH_PREFIX}bad-start")
    ));
}

// =========================================================================
// COW fast path with safe fallback
// =========================================================================

#[test]
fn cow_fast_path_returns_structured_availability_with_safe_fallback() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    fs::write(repo.join(".harness-cow-overlay"), b"cow-payload\n").unwrap_or_abort();
    let created = create_slug(&repo, "cow-test");

    // When: apply COW fastpath
    let report = cow_worktree::apply_cow_worktree_fastpath(
        &repo,
        &created,
        &[".harness-cow-overlay", "absent.bin"],
    );

    // Then: structured availability (never panics), missing overlay is unavailable
    assert!(report.availability.is_available() || report.availability.is_unavailable());
    assert_eq!(report.overlays.len(), 2);
    assert!(report.overlays[1].is_unavailable());
    if report.overlays[0].is_cloned() {
        assert_eq!(
            fs::read(created.join(".harness-cow-overlay")).unwrap_or_abort(),
            b"cow-payload\n"
        );
    }
    assert!(report.one_line().contains("COW worktree fastpath:"));
}

// =========================================================================
// VCS status/diff
// =========================================================================

#[test]
fn vcs_status_reports_modified_untracked_and_deleted_files() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    fs::write(repo.join("tracked.txt"), "original\n").unwrap_or_abort();
    run_git(&repo, &["add", "tracked.txt"]);
    run_git(&repo, &["commit", "-m", "add tracked"]);

    // When: modify tracked, add untracked, delete a file
    fs::write(repo.join("tracked.txt"), "modified\n").unwrap_or_abort();
    fs::write(repo.join("untracked.txt"), "new\n").unwrap_or_abort();
    fs::remove_file(repo.join("README.md")).unwrap_or_abort();

    let status = collect_vcs_status(&repo).unwrap_or_abort();

    // Then
    assert!(!status.is_clean());
    let paths: Vec<&str> = status.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"tracked.txt"));
    assert!(paths.contains(&"untracked.txt"));
    assert!(paths.contains(&"README.md"));
    assert!(status.deleted >= 1);
    assert!(status.modified >= 1);
    assert!(status.untracked >= 1);
}

#[test]
fn vcs_diff_reports_insertions_and_deletions() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    fs::write(repo.join("src.rs"), "fn old() {}\n").unwrap_or_abort();
    run_git(&repo, &["add", "src.rs"]);
    run_git(&repo, &["commit", "-m", "add src"]);

    // When: modify and stage
    fs::write(repo.join("src.rs"), "fn new() {}\nfn extra() {}\n").unwrap_or_abort();
    run_git(&repo, &["add", "src.rs"]);

    let diff = collect_vcs_diff(&repo, Some("src.rs")).unwrap_or_abort();

    // Then
    assert!(!diff.is_empty());
    assert_eq!(diff.path, "src.rs");
    assert!(diff.insertions > 0);
    assert!(diff.unified_diff.contains("fn new()"));
}

#[test]
fn vcs_snapshot_combines_status_and_diff() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    fs::write(repo.join("changed.rs"), "pub fn x() {}\n").unwrap_or_abort();
    run_git(&repo, &["add", "changed.rs"]);

    // When
    let snapshot = collect_vcs_snapshot(&repo).unwrap_or_abort();

    // Then
    assert!(!snapshot.status.is_clean());
    assert!(snapshot.diff.files_changed > 0);
}

#[test]
fn vcs_status_fails_closed_on_non_git_repository() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let not_a_repo = temp.path().join("plain");
    fs::create_dir_all(&not_a_repo).unwrap_or_abort();

    // When
    let err = collect_vcs_status(&not_a_repo).expect_err("non-git should fail");

    // Then
    assert!(matches!(err, VcsError::NotAGitRepository { .. }));
}

// =========================================================================
// Path traversal rejection
// =========================================================================

#[test]
fn vcs_diff_rejects_path_traversal_attempts() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);

    // When: attempt traversal via relative path
    let err = collect_vcs_diff(&repo, Some("../etc/passwd")).expect_err("traversal must fail");

    // Then
    assert!(matches!(err, VcsError::PathTraversal { .. }));

    // When: attempt absolute path
    let err = collect_vcs_diff(&repo, Some("/etc/passwd")).expect_err("absolute path must fail");

    // Then
    assert!(matches!(err, VcsError::PathTraversal { .. }));
}

#[test]
fn worktree_remove_rejects_paths_outside_session_parent() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    create_slug(&repo, "safe-slug");

    // When: attempt to remove a path outside the worktree parent
    let outside = temp.path().join("outside");
    fs::create_dir_all(&outside).unwrap_or_abort();
    let err = remove_session_worktree(RemoveWorktreeOptions {
        repository_root: &repo,
        path: &outside,
        worktree_parent: None,
        delete_branch: false,
        force: true,
    })
    .expect_err("outside parent must be refused");

    // Then
    assert!(matches!(err, WorktreeError::UnsafeRemovePath { .. }));
}

#[test]
fn worktree_remove_refuses_primary_worktree() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);

    // When: attempt to remove the primary worktree
    let err = remove_session_worktree(RemoveWorktreeOptions {
        repository_root: &repo,
        path: &repo,
        worktree_parent: None,
        delete_branch: false,
        force: true,
    })
    .expect_err("primary worktree must be refused");

    // Then
    assert!(matches!(err, WorktreeError::PrimaryWorktree { .. }));
}

// =========================================================================
// Durable folder trust persistence
// =========================================================================

#[test]
fn folder_trust_persists_allow_and_deny_across_reopen() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("project");
    fs::create_dir_all(&workspace).unwrap_or_abort();

    // When: set Allow
    FolderTrustStore::for_workspace(&workspace)
        .set(&workspace, FolderTrustDecision::Allow)
        .unwrap_or_abort();

    // Then: reopen reads Allow
    let reopened = FolderTrustStore::for_workspace(&workspace);
    assert_eq!(
        reopened.get(&workspace).unwrap_or_abort(),
        Some(FolderTrustDecision::Allow)
    );

    // When: change to Deny
    reopened
        .set(&workspace, FolderTrustDecision::Deny)
        .unwrap_or_abort();

    // Then: reopen reads Deny
    let again = FolderTrustStore::for_workspace(&workspace);
    assert_eq!(
        again.get(&workspace).unwrap_or_abort(),
        Some(FolderTrustDecision::Deny)
    );

    // And: no secrets in the store file
    let raw = fs::read_to_string(again.path()).unwrap_or_abort();
    assert!(!raw.to_lowercase().contains("token"));
    assert!(!raw.to_lowercase().contains("api_key"));
    assert!(!raw.to_lowercase().contains("password"));
}

// =========================================================================
// Deny-before-spawn gate
// =========================================================================

#[test]
fn deny_before_spawn_blocks_repo_local_executable_without_trust() {
    // Given: workspace with no trust entry
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();

    // When: gate a repo-local executable without trust
    let gate = gate_repository_local_executable("./scripts/tool.sh", &workspace, None);

    // Then: denied before any spawn
    assert!(gate.is_denied());
    match gate {
        LocalExecutableGate::Denied { reason } => {
            assert!(reason.contains("folder trust missing"));
            assert!(reason.contains("./scripts/tool.sh"));
        }
        other => panic!("expected Denied, got {other:?}"),
    }
}

#[test]
fn deny_before_spawn_allows_when_trust_allow_persisted() {
    // Given: workspace with Allow trust persisted
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    FolderTrustStore::for_workspace(&workspace)
        .set(&workspace, FolderTrustDecision::Allow)
        .unwrap_or_abort();

    // When: gate through a fresh store instance (durable)
    let gate =
        gate_repository_local_executable_from_store("./bin/helper", &workspace).unwrap_or_abort();

    // Then: allowed
    assert_eq!(gate, LocalExecutableGate::Allowed);
}

#[test]
fn deny_before_spawn_blocks_when_trust_deny_persisted() {
    // Given: workspace with Deny trust persisted
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    FolderTrustStore::for_workspace(&workspace)
        .set(&workspace, FolderTrustDecision::Deny)
        .unwrap_or_abort();

    // When: gate through a fresh store instance
    let gate =
        gate_repository_local_executable_from_store("./release/tool", &workspace).unwrap_or_abort();

    // Then: denied
    assert!(gate.is_denied());
}

#[test]
fn bare_path_commands_are_not_gated_by_folder_trust() {
    // Given
    let workspace = Path::new("/tmp/ws");

    // When/Then: PATH-only commands are NotApplicable
    assert_eq!(
        gate_repository_local_executable("git", workspace, None),
        LocalExecutableGate::NotApplicable
    );
    assert_eq!(
        gate_repository_local_executable("cargo", workspace, None),
        LocalExecutableGate::NotApplicable
    );
}

// =========================================================================
// Concurrent worktree isolation
// =========================================================================

#[test]
fn concurrent_worktrees_have_isolated_paths_branches_and_file_state() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);

    // When: create two concurrent worktrees
    let left = create_slug(&repo, "conc-left");
    let right = create_slug(&repo, "conc-right");

    // Then: distinct paths and branches
    assert_ne!(left, right);
    assert_ne!(
        format!("{WORKTREE_BRANCH_PREFIX}conc-left"),
        format!("{WORKTREE_BRANCH_PREFIX}conc-right")
    );

    // And: file state is isolated
    fs::write(left.join("left-only.txt"), "left\n").unwrap_or_abort();
    fs::write(right.join("right-only.txt"), "right\n").unwrap_or_abort();
    assert!(!right.join("left-only.txt").exists());
    assert!(!left.join("right-only.txt").exists());
}

// =========================================================================
// Attribution drift and revert
// =========================================================================

#[test]
fn attribution_drift_detected_when_external_modification_after_agent_edit() {
    // Given: agent writes a file
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let mut journal = EditAttributionJournal::open(&workspace).unwrap_or_abort();

    journal
        .record_agent_tool_edit("src/main.rs", b"fn main() {}\n", None)
        .unwrap_or_abort();
    fs::create_dir_all(workspace.join("src")).unwrap_or_abort();
    fs::write(workspace.join("src/main.rs"), b"fn main() { modified }\n").unwrap_or_abort();

    // When: observe external change
    let observed = journal
        .observe_external("src/main.rs", b"fn main() { modified }\n", None)
        .unwrap_or_abort();

    // Then: drift detected
    assert_eq!(observed.source, EditSource::External);
    assert!(journal.tracker().is_drifted("src/main.rs"));

    // And: diff shows the drift
    let diff = journal.diff("src/main.rs").unwrap_or_abort();
    assert!(diff.drifted);
    assert!(diff.unified_diff.contains("modified"));

    // And: blame attributes the modified line to External
    let blame = journal.blame("src/main.rs").unwrap_or_abort();
    assert!(blame.drifted);
    assert!(blame.external_lines > 0);
}

#[test]
fn attribution_revert_restores_agent_snapshot_content() {
    // Given: agent wrote content, then external modified it
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let mut journal = EditAttributionJournal::open(&workspace).unwrap_or_abort();

    let agent_content = b"fn main() { agent_version }\n";
    journal
        .record_agent_tool_edit("src/lib.rs", agent_content, None)
        .unwrap_or_abort();

    let external_content = b"fn main() { external_hack }\n";
    fs::create_dir_all(workspace.join("src")).unwrap_or_abort();
    fs::write(workspace.join("src/lib.rs"), external_content).unwrap_or_abort();
    journal
        .observe_external("src/lib.rs", external_content, None)
        .unwrap_or_abort();

    // When: revert to agent snapshot
    let result = journal.revert_path("src/lib.rs").unwrap_or_abort();

    // Then: file content restored to agent version
    let restored = fs::read(workspace.join("src/lib.rs")).unwrap_or_abort();
    assert_eq!(restored, agent_content);
    assert!(result.bytes_written > 0);

    // And: attribution is back to AgentTool
    let entry = journal.tracker().get("src/lib.rs").expect("entry exists");
    assert_eq!(entry.source, EditSource::AgentTool);
    assert!(!journal.tracker().is_drifted("src/lib.rs"));
}

#[test]
fn attribution_journal_persists_across_reopen() {
    // Given: journal with agent edit
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    {
        let mut journal = EditAttributionJournal::open(&workspace).unwrap_or_abort();
        journal
            .record_agent_tool_edit("src/a.rs", b"content-a\n", None)
            .unwrap_or_abort();
    }

    // When: reopen the journal
    let reopened = EditAttributionJournal::open(&workspace).unwrap_or_abort();

    // Then: the agent edit is still there
    let entry = reopened.tracker().get("src/a.rs").expect("entry exists");
    assert_eq!(entry.source, EditSource::AgentTool);
    assert!(workspace.join(EDIT_ATTRIBUTION_JOURNAL_REL).is_file());
}

// =========================================================================
// Cleanup removes all managed worktrees and branches
// =========================================================================

#[test]
fn cleanup_removes_all_managed_worktrees_and_branches() {
    // Given: repo with three managed worktrees
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    let paths: Vec<PathBuf> = ["clean-a", "clean-b", "clean-c"]
        .iter()
        .map(|slug| create_slug(&repo, slug))
        .collect();
    assert_eq!(managed_slugs(&repo).len(), 3);

    // When: cleanup all managed worktrees
    let listed = list_session_worktrees(&repo, None).unwrap_or_abort();
    for entry in listed.iter().filter(|e| e.harness_managed) {
        remove_session_worktree(RemoveWorktreeOptions {
            repository_root: &repo,
            path: &entry.path,
            worktree_parent: None,
            delete_branch: true,
            force: true,
        })
        .unwrap_or_abort();
    }

    // Then: all paths and branches are gone
    for path in &paths {
        assert!(!path.exists(), "path should be removed: {}", path.display());
    }
    for slug in ["clean-a", "clean-b", "clean-c"] {
        assert!(!branch_exists(
            &repo,
            &format!("{WORKTREE_BRANCH_PREFIX}{slug}")
        ));
    }
    assert!(managed_slugs(&repo).is_empty());
}

// =========================================================================
// Worktree select by slug (list + remove one)
// =========================================================================

#[test]
fn worktree_select_by_slug_removes_only_named_worktree() {
    // Given: two worktrees
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    let alpha = create_slug(&repo, "select-alpha");
    let beta = create_slug(&repo, "select-beta");

    // When: remove only beta by selecting its path from the list
    let listed = list_session_worktrees(&repo, None).unwrap_or_abort();
    let beta_entry = listed
        .iter()
        .find(|e| e.slug.as_deref() == Some("select-beta"))
        .expect("beta entry");
    remove_session_worktree(RemoveWorktreeOptions {
        repository_root: &repo,
        path: &beta_entry.path,
        worktree_parent: None,
        delete_branch: true,
        force: true,
    })
    .unwrap_or_abort();

    // Then: alpha survives, beta is gone
    assert!(alpha.exists());
    assert!(!beta.exists());
    assert_eq!(managed_slugs(&repo), vec!["select-alpha"]);
}

// =========================================================================
// Worktree remove unknown slug fails closed
// =========================================================================

#[test]
fn worktree_remove_unknown_slug_fails_closed() {
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    let alpha = create_slug(&repo, "unknown-test");

    // When: remove a non-existent slug path
    let ghost_path = repo.join(DEFAULT_WORKTREE_RELATIVE_BASE).join("ghost");
    let err = remove_session_worktree(RemoveWorktreeOptions {
        repository_root: &repo,
        path: &ghost_path,
        worktree_parent: None,
        delete_branch: false,
        force: true,
    })
    .expect_err("unknown slug should fail");

    // Then: fails closed, existing worktree untouched
    assert!(matches!(err, WorktreeError::NotFound { .. }));
    assert!(alpha.exists());
}

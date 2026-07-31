//! Clean-room parity contracts for local workspace safety and VCS workflows.

mod workspace_worktree_trust_vcs_sandbox_parity_support;

use std::fs;
use std::path::Path;
use std::process::Command;

use harness_core::edit_attribution::{EditAttributionError, EditAttributionJournal, EditSource};
use harness_core::folder_trust::{
    gate_repository_local_executable_from_store, FolderTrustDecision, FolderTrustStore,
    LocalExecutableGate,
};
use harness_core::jujutsu::{detect_jujutsu_workspace, jj_status, JujutsuWorkflowResult};
use harness_core::sandbox::{
    evaluate_network_confinement, evaluate_network_confinement_with_landlock,
    list_os_profiles_for_platform, probe_os_sandbox_product_for_platform, LandlockSupport,
    NetworkConfinementStatus, SandboxNetworkPolicy, SandboxPathRoots, SandboxPlatform,
    SandboxPolicy,
};
use harness_core::store::{EventStore, JsonlFileEventStore};
use harness_core::vcs::{collect_vcs_diff, collect_vcs_snapshot, VcsError};
use harness_core::workspace::WorkspaceEnvironment;
use harness_core::worktree::{
    create_session_worktree, list_session_worktrees, remove_session_worktree,
    CreateWorktreeOptions, RemoveWorktreeOptions, WorktreeError,
};
use harness_core::UnwrapOrAbort;
use workspace_worktree_trust_vcs_sandbox_parity_support::run_started_draft;

fn init_git_repo(path: &Path) {
    run_git(path, &["init", "-b", "main"]);
    run_git(path, &["config", "user.email", "parity@example.com"]);
    run_git(path, &["config", "user.name", "Parity Contract"]);
    fs::write(path.join("README.md"), "seed\n").unwrap_or_abort();
    run_git(path, &["add", "README.md"]);
    run_git(path, &["commit", "-m", "seed"]);
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_abort();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create_worktree<'a>(repo: &'a Path, slug: &'a str) -> harness_core::worktree::CreatedWorktree {
    create_session_worktree(CreateWorktreeOptions {
        repository_root: repo,
        worktree_parent: None,
        slug: Some(slug),
        start_point: None,
    })
    .unwrap_or_abort()
}

#[test]
fn trust_precedes_local_mutation_and_allowed_path_command_runs() {
    // Given: a workspace without a trust decision.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();

    // When: a repository-local executable is gated before trust.
    let before_trust = gate_repository_local_executable_from_store("./scripts/mutate", &workspace)
        .unwrap_or_abort();

    // Then: it is denied before a spawn can mutate the workspace.
    assert!(before_trust.is_denied());

    // When: trust is persisted and the fresh store gates again.
    FolderTrustStore::for_workspace(&workspace)
        .set(&workspace, FolderTrustDecision::Allow)
        .unwrap_or_abort();
    let after_trust = gate_repository_local_executable_from_store("./scripts/mutate", &workspace)
        .unwrap_or_abort();

    // Then: local execution is allowed, while a PATH command actually runs.
    assert_eq!(after_trust, LocalExecutableGate::Allowed);
    let output = Command::new("git")
        .arg("--version")
        .output()
        .unwrap_or_abort();
    assert!(output.status.success());
}

#[test]
fn worktrees_are_selectable_isolated_and_safe_to_cleanup() {
    // Given: a real repository with two linked session worktrees.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    let left = create_worktree(&repo, "left-session");
    let right = create_worktree(&repo, "right-session");

    // When: each session writes a local file and selects itself from the managed listing.
    fs::write(left.path.join("left.txt"), "left\n").unwrap_or_abort();
    fs::write(right.path.join("right.txt"), "right\n").unwrap_or_abort();
    let session_root = temp.path().join("sessions");
    let left_store = JsonlFileEventStore::open(&session_root, "left-run", true).unwrap_or_abort();
    let right_store = JsonlFileEventStore::open(&session_root, "right-run", true).unwrap_or_abort();
    left_store
        .append(run_started_draft(
            "left-run",
            left.path.to_string_lossy().as_ref(),
            1,
        ))
        .unwrap_or_abort();
    right_store
        .append(run_started_draft(
            "right-run",
            right.path.to_string_lossy().as_ref(),
            2,
        ))
        .unwrap_or_abort();
    let selected = list_session_worktrees(&repo, None)
        .unwrap_or_abort()
        .into_iter()
        .find(|entry| entry.slug.as_deref() == Some("right-session"))
        .expect("right worktree is listed");

    // Then: roots and local file state remain isolated.
    assert_ne!(left.path, right.path);
    assert_eq!(selected.path, right.path);
    assert!(!left.path.join("right.txt").exists());
    assert!(!right.path.join("left.txt").exists());
    assert_ne!(left_store.file_path(), right_store.file_path());
    let left_log = fs::read_to_string(left_store.file_path()).unwrap_or_abort();
    let right_log = fs::read_to_string(right_store.file_path()).unwrap_or_abort();
    assert!(left_log.contains(left.path.to_string_lossy().as_ref()));
    assert!(!left_log.contains(right.path.to_string_lossy().as_ref()));
    assert!(right_log.contains(right.path.to_string_lossy().as_ref()));
    assert!(!right_log.contains(left.path.to_string_lossy().as_ref()));
    assert_eq!(
        WorkspaceEnvironment::discover(&left.path).workspace_root,
        left.path
    );

    // When: a cleanup target escapes the managed parent.
    let err = remove_session_worktree(RemoveWorktreeOptions {
        repository_root: &repo,
        path: &repo,
        worktree_parent: None,
        delete_branch: true,
        force: true,
    })
    .expect_err("primary worktree must be protected");

    // Then: cleanup fails closed and selected worktree cleanup only removes that session.
    assert!(matches!(err, WorktreeError::PrimaryWorktree { .. }));
    remove_session_worktree(RemoveWorktreeOptions {
        repository_root: &repo,
        path: &selected.path,
        worktree_parent: None,
        delete_branch: true,
        force: true,
    })
    .unwrap_or_abort();
    assert!(left.path.exists());
    assert!(!right.path.exists());
}

#[test]
fn git_jujutsu_checkpoint_and_path_safety_are_structured() {
    // Given: a Git workspace with a tracked change and a Jujutsu marker.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap_or_abort();
    init_git_repo(&repo);
    fs::write(repo.join("checkpoint.txt"), "checkpoint\n").unwrap_or_abort();
    run_git(&repo, &["add", "checkpoint.txt"]);
    fs::create_dir_all(repo.join(".jj")).unwrap_or_abort();

    // When: VCS checkpoint/status and optional Jujutsu status are inspected.
    let checkpoint = collect_vcs_snapshot(&repo).unwrap_or_abort();
    let jj = jj_status(&repo);

    // Then: Git exposes the change, while Jujutsu remains honest if unavailable.
    assert!(!checkpoint.status.is_clean());
    assert!(checkpoint.diff.files_changed > 0);
    assert!(detect_jujutsu_workspace(&repo).is_repo());
    assert!(matches!(
        jj,
        JujutsuWorkflowResult::Ok { .. } | JujutsuWorkflowResult::Unavailable { .. }
    ));

    // When: an inspection path attempts to escape the workspace.
    let err = collect_vcs_diff(&repo, Some("../outside")).expect_err("traversal must fail");

    // Then: the inspection boundary rejects it before invoking Git on the path.
    assert!(matches!(err, VcsError::PathTraversal { .. }));
}

#[test]
fn attribution_survives_restart_supports_blame_and_revert() {
    // Given: an agent edit recorded in a workspace journal.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("workspace");
    let path = workspace.join("src/lib.rs");
    fs::create_dir_all(path.parent().expect("source parent")).unwrap_or_abort();
    fs::write(&path, "agent\n").unwrap_or_abort();
    let mut journal = EditAttributionJournal::open(&workspace).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/lib.rs", b"agent\n", None)
        .unwrap_or_abort();
    fs::write(&path, "external\n").unwrap_or_abort();

    // When: a reopened journal inspects drift and restores its checkpoint.
    let mut reopened = EditAttributionJournal::open(&workspace).unwrap_or_abort();
    let diff = reopened.diff("src/lib.rs").unwrap_or_abort();
    let blame = reopened.blame("src/lib.rs").unwrap_or_abort();
    let reverted = reopened.revert_path("src/lib.rs").unwrap_or_abort();

    // Then: attribution survives restart and revert restores the agent snapshot.
    assert!(diff.drifted);
    assert_eq!(blame.external_lines, 1);
    assert_eq!(reverted.bytes_written, b"agent\n".len());
    assert_eq!(fs::read(&path).unwrap_or_abort(), b"agent\n");
    assert_eq!(
        reopened.query("src/lib.rs").unwrap_or_abort().source,
        EditSource::AgentTool
    );
}

#[cfg(unix)]
#[test]
fn attribution_revert_rejects_symlink_that_escapes_workspace() {
    // Given: a journal path whose final component is a symlink to an external file.
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("workspace");
    let outside = temp.path().join("outside.txt");
    fs::create_dir_all(workspace.join("src")).unwrap_or_abort();
    fs::write(workspace.join("src/escape.rs"), "agent\n").unwrap_or_abort();
    let mut journal = EditAttributionJournal::open(&workspace).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/escape.rs", b"agent\n", None)
        .unwrap_or_abort();
    fs::remove_file(workspace.join("src/escape.rs")).unwrap_or_abort();
    fs::write(&outside, "outside\n").unwrap_or_abort();
    symlink(&outside, workspace.join("src/escape.rs")).unwrap_or_abort();

    // When: a revert targets the symlink.
    let err = journal
        .revert_path("src/escape.rs")
        .expect_err("symlink escape must be rejected");

    // Then: the external target is never followed or overwritten.
    assert!(matches!(err, EditAttributionError::InvalidPath { .. }));
    assert_eq!(fs::read(&outside).unwrap_or_abort(), b"outside\n");
}

#[test]
fn sandbox_profiles_and_network_status_are_truthful_on_every_platform() {
    // Given: explicit roots and every supported platform surface.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let roots = SandboxPathRoots {
        workspace_root: temp.path().join("workspace"),
        harness_state_dir: temp.path().join("state"),
        temp_dir: temp.path().join("temporary"),
    };

    // When: sandbox profiles and deny-network readiness are evaluated.
    for platform in [
        SandboxPlatform::Linux,
        SandboxPlatform::Macos,
        SandboxPlatform::Windows,
    ] {
        let profiles = list_os_profiles_for_platform(platform);
        let product = probe_os_sandbox_product_for_platform(platform, Some(&roots));
        assert_eq!(profiles.len(), 4);
        assert!(profiles
            .iter()
            .any(|profile| profile.policy == SandboxPolicy::Off));
        assert_eq!(product.fs_plan_summaries.len(), 3);
    }
    let unavailable = LandlockSupport::Unavailable {
        reason: "contract probe".to_string(),
    };

    // Then: non-Linux and unsupported Linux never claim deny-network enforcement.
    assert!(matches!(
        evaluate_network_confinement_with_landlock(
            &SandboxNetworkPolicy::DenyAll,
            SandboxPlatform::Macos,
            &unavailable
        ),
        NetworkConfinementStatus::Unavailable { .. }
    ));
    assert!(matches!(
        evaluate_network_confinement(&SandboxNetworkPolicy::DenyAll),
        NetworkConfinementStatus::Available { .. } | NetworkConfinementStatus::Unavailable { .. }
    ));
}

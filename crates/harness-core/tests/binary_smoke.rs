use harness_core::{workspace::WorkspaceEnvironment, worktree::*};
use std::{fs, path::Path, process::Command};

#[test]
fn worktrees_keep_changes_isolated_and_refuse_unsafe_cleanup(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real Git checks".into());
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repository with spaces");
    fs::create_dir(&root)?;
    git(&root, &["init", "-q", "-b", "main"])?;
    git(
        &root,
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "--allow-empty",
            "-qm",
            "initial",
        ],
    )?;
    fs::create_dir(root.join("nested"))?;
    let environment = WorkspaceEnvironment::discover(root.join("nested"));
    assert_eq!(environment.workspace_root, root);
    assert_eq!(environment.git_branch.as_deref(), Some("main"));
    let parent = temp.path().join("checkouts\nwith spaces");
    let options = CreateWorktreeOptions {
        repository_root: &root,
        worktree_parent: Some(&parent),
        slug: Some("one"),
        start_point: None,
    };
    let created = create_session_worktree(options.clone())?;
    assert_eq!(
        WorkspaceEnvironment::discover(&created.path)
            .git_branch
            .as_deref(),
        Some(created.branch.as_str())
    );
    assert!(create_session_worktree(options).is_err());
    let listed = list_session_worktrees(&root, Some(&parent))?;
    assert!(listed.iter().any(|entry| entry.path == created.path
        && entry.harness_managed
        && entry.branch.as_deref() == Some(created.branch.as_str())));
    fs::write(created.path.join("untracked"), b"keep me")?;
    assert!(!root.join("untracked").exists());
    let remove = RemoveWorktreeOptions {
        repository_root: &root,
        path: &created.path,
        worktree_parent: Some(&parent),
        delete_branch: true,
        force: false,
    };
    assert!(remove_session_worktree(remove.clone()).is_err());
    assert_eq!(fs::read(created.path.join("untracked"))?, b"keep me");
    assert!(remove_session_worktree(RemoveWorktreeOptions {
        path: &root,
        ..remove.clone()
    })
    .is_err());
    assert!(remove_session_worktree(RemoveWorktreeOptions {
        worktree_parent: Some(&root),
        ..remove.clone()
    })
    .is_err());
    git(&created.path, &["checkout", "--detach", "-q"])?;
    assert!(WorkspaceEnvironment::discover(&created.path)
        .git_branch
        .is_none());
    git(&created.path, &["checkout", "-q", &created.branch])?;
    remove_session_worktree(RemoveWorktreeOptions {
        force: true,
        ..remove
    })?;
    assert!(!created.path.exists());
    assert!(!Command::new("git")
        .current_dir(&root)
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", created.branch)
        ])
        .status()?
        .success());
    let bad = CreateWorktreeOptions {
        repository_root: &root,
        worktree_parent: Some(&parent),
        slug: Some("bad"),
        start_point: Some("missing-ref"),
    };
    assert!(create_session_worktree(bad).is_err());
    assert!(!parent.join("bad").exists());
    assert_eq!(list_session_worktrees(&root, Some(&parent))?.len(), 1);
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn reflinks_preserve_source_and_publish_only_complete_copies(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::cow_worktree::*;
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real reflink checks".into());
    }
    let temp = tempfile::Builder::new()
        .prefix("cow-signoff-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::create_dir_all(source.join("nested"))?;
    fs::create_dir_all(source.join(".git"))?;
    fs::write(source.join("nested/file"), b"original")?;
    fs::write(source.join(".git/secret"), b"skip repository metadata")?;
    let entries = fs::read_dir(&source)?.count();
    let _ = detect_cow_worktree_fastpath(&source);
    assert_eq!(fs::read_dir(&source)?.count(), entries);
    let copy = try_cow_clone_tree(&source, &destination);
    assert!(
        copy.is_cloned(),
        "real reflink signoff failed: {}",
        copy.one_line()
    );
    fs::write(destination.join("nested/file"), b"changed")?;
    assert_eq!(fs::read(source.join("nested/file"))?, b"original");
    assert!(!destination.join(".git").exists());
    assert!(try_cow_clone_file(
        &source.join("nested/file"),
        &destination.join("nested/file")
    )
    .is_unavailable());
    assert_eq!(fs::read(destination.join("nested/file"))?, b"changed");
    let report =
        apply_cow_worktree_fastpath(&source, &destination, &["../escape", "nested/../../escape"]);
    assert_eq!(
        summarize_cow_clone_outcomes(&report.overlays).unavailable,
        2
    );
    std::os::unix::fs::symlink(temp.path(), source.join("outside"))?;
    let failed = temp.path().join("failed");
    assert!(try_cow_clone_tree(&source, &failed).is_unavailable());
    assert!(!failed.exists());
    let fifo = source.join("fifo");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )?;
    let fifo_destination = temp.path().join("fifo-copy");
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = send.send(try_cow_clone_file(&fifo, &fifo_destination));
    });
    assert!(receive
        .recv_timeout(std::time::Duration::from_secs(1))?
        .is_unavailable());
    worker.join().map_err(|_| "clone worker failed")?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn landlock_restricts_only_the_worker_and_blocks_workspace_writes_and_outside_reads(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::sandbox::*;
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real Landlock checks".into());
    }
    let temp = tempfile::tempdir()?;
    let roots = SandboxPathRoots {
        workspace_root: temp.path().join("workspace"),
        harness_state_dir: temp.path().join("state"),
        temp_dir: temp.path().join("scratch"),
    };
    for path in [
        &roots.workspace_root,
        &roots.harness_state_dir,
        &roots.temp_dir,
    ] {
        fs::create_dir(path)?;
    }
    let source = roots.workspace_root.join("source");
    let outside = temp.path().join("secret");
    fs::write(&source, "source")?;
    fs::write(&outside, "private")?;
    let worker_outside = outside.clone();
    let worker = std::thread::spawn(move || -> Result<(), String> {
        let plan = build_fs_plan(SandboxPolicy::ReadOnly, &roots).ok_or("missing plan")?;
        apply_landlock_fs_plan(&plan)?;
        assert_eq!(
            fs::read_to_string(&source).map_err(|e| e.to_string())?,
            "source"
        );
        assert!(fs::write(&source, "changed").is_err());
        assert!(fs::read_to_string(worker_outside).is_err());
        fs::write(roots.harness_state_dir.join("receipt"), "ok").map_err(|e| e.to_string())?;
        fs::write(roots.temp_dir.join("scratch"), "ok").map_err(|e| e.to_string())?;
        Ok(())
    });
    worker.join().map_err(|_| "Landlock worker failed")??;
    assert_eq!(fs::read_to_string(outside)?, "private");
    fs::write(
        temp.path().join("workspace/source"),
        "parent remains unrestricted",
    )?;
    Ok(())
}

use super::*;
use std::process::Stdio;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeWorkspaceReceipt {
    pub payload_version: u16,
    pub child_id: String,
    pub snapshot_ref: String,
    pub worktree_path: String,
    pub removed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct WorkspaceOwnership {
    pub(super) path: PathBuf,
    source: PathBuf,
    identity: Option<(u64, u64)>,
}

fn owned_destination(
    path: PathBuf,
    source: PathBuf,
) -> Result<WorkspaceOwnership, CoordinatorError> {
    let metadata = std::fs::symlink_metadata(&path)?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        Some((metadata.dev(), metadata.ino()))
    };
    #[cfg(not(unix))]
    let identity = {
        let _ = metadata;
        None
    };
    Ok(WorkspaceOwnership {
        path,
        source,
        identity,
    })
}

pub(super) fn sanitize_cwd(value: Option<&str>) -> Option<String> {
    let value = optional(value)?
        .trim_matches(['\'', '"', '`'])
        .trim()
        .to_owned();
    if value == "~" || value.starts_with("~/") {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join(value.strip_prefix("~/").unwrap_or_default())
                .to_string_lossy()
                .into_owned()
        })
    } else {
        optional(Some(&value))
    }
}

async fn git(
    cwd: &Path,
    arguments: &[&str],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, CoordinatorError> {
    let mut command = tokio::process::Command::new("git");
    command
        .current_dir(cwd)
        .args(arguments)
        .kill_on_drop(true)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    if let Some(input) = input {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| native_invalid("git input channel closed".into()))?;
        stdin.write_all(input).await?;
        stdin.shutdown().await?;
    }
    let output = child.wait_with_output().await?;
    if !output.status.success() {
        return Err(native_invalid(
            String::from_utf8_lossy(&output.stderr).trim().into(),
        ));
    }
    if output.stdout.len() > 8 * 1024 * 1024 {
        return Err(native_invalid(
            "worktree snapshot exceeds the supported byte limit".into(),
        ));
    }
    Ok(output.stdout)
}

pub(super) async fn prepare(
    id: &str,
    cwd: &Path,
    workspace_root: &Path,
    run_dir: &Path,
    isolation: SubagentIsolationMode,
    source: Option<ResolvedSubagentContext>,
    snapshot_ref: Option<String>,
    cancellation: CancellationToken,
    checkpoint: Option<WorkspaceCreationCheckpoint>,
) -> Result<PreparedSubagent, CoordinatorError> {
    let shared = || PreparedSubagent {
        cwd: cwd.into(),
        worktree: None,
        snapshot_ref: None,
        ownership: None,
        failure: None,
    };
    if cancellation.is_cancelled() {
        return Err(CoordinatorError::Cancelled("Subagent was cancelled".into()));
    }
    let source_path = source
        .as_ref()
        .and_then(|context| match &context.isolation {
            ResolvedSubagentIsolation::Worktree { path } => Some(PathBuf::from(path)),
            ResolvedSubagentIsolation::SharedWorkspace => None,
        });
    if let Some(path) = source_path {
        if let Some(reference) = snapshot_ref.as_deref() {
            let destination = run_dir.join("worktrees").join(id);
            tokio::fs::create_dir_all(
                destination
                    .parent()
                    .ok_or_else(|| native_invalid("worktree destination has no parent".into()))?,
            )
            .await?;
            if tokio::fs::create_dir(&destination).await.is_err() {
                // A directory not reserved by this attempt is never cleanup
                // ownership, even if Git would accept it as an empty target.
                return Ok(shared());
            }
            let ownership = owned_destination(destination.clone(), workspace_root.into())?;
            let destination_text = destination.to_string_lossy().into_owned();
            if git(
                workspace_root,
                &["worktree", "add", "--detach", &destination_text, reference],
                None,
            )
            .await
            .is_ok()
            {
                return finish_created(
                    PreparedSubagent {
                        cwd: destination.clone(),
                        worktree: Some(destination),
                        snapshot_ref,
                        ownership: Some(ownership),
                        failure: None,
                    },
                    &cancellation,
                    checkpoint,
                )
                .await;
            }
            cleanup_owned(&ownership).await?;
            return Ok(shared());
        }
        if path.is_dir() {
            return Ok(PreparedSubagent {
                cwd: path.clone(),
                worktree: Some(path),
                snapshot_ref: None,
                ownership: None,
                failure: None,
            });
        }
        return Ok(shared());
    }
    if source.is_some() || isolation == SubagentIsolationMode::None {
        return Ok(shared());
    }
    let destination = run_dir.join("worktrees").join(id);
    tokio::fs::create_dir_all(
        destination
            .parent()
            .ok_or_else(|| native_invalid("worktree destination has no parent".into()))?,
    )
    .await?;
    if tokio::fs::create_dir(&destination).await.is_err() {
        return Ok(shared());
    }
    let ownership = owned_destination(destination.clone(), cwd.into())?;
    let path = destination.to_string_lossy().into_owned();
    let create = async {
        git(cwd, &["worktree", "add", "--detach", &path, "HEAD"], None).await?;
        if let Some(checkpoint) = checkpoint {
            let _ = checkpoint.entered.send(destination.clone());
            tokio::select! {
                biased;
                () = cancellation.cancelled() => {},
                () = checkpoint.proceed.notified() => {},
            }
        }
        if cancellation.is_cancelled() {
            return Err(CoordinatorError::Cancelled("Subagent was cancelled".into()));
        }
        let patch = git(cwd, &["diff", "--binary", "HEAD", "--"], None).await?;
        if !patch.is_empty() {
            git(&destination, &["apply", "--binary", "-"], Some(&patch)).await?;
        }
        let untracked = git(
            cwd,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            None,
        )
        .await?;
        for relative in untracked
            .split(|byte| *byte == 0)
            .filter(|value| !value.is_empty())
        {
            copy_untracked_file(cwd, &destination, relative).await?;
        }
        Ok::<(), CoordinatorError>(())
    }
    .await;
    if create.is_err() {
        // Creation failure is an explicitly shared fallback, never mislabeled
        // isolation. Remove only the worktree this preparation created.
        if let Err(error) = cleanup_owned(&ownership).await {
            return Ok(PreparedSubagent {
                cwd: destination.clone(),
                worktree: Some(destination),
                snapshot_ref: None,
                ownership: Some(ownership),
                failure: Some(error),
            });
        }
        if cancellation.is_cancelled() {
            return Err(CoordinatorError::Cancelled("Subagent was cancelled".into()));
        }
        return Ok(shared());
    }
    finish_created(
        PreparedSubagent {
            cwd: destination.clone(),
            worktree: Some(destination),
            snapshot_ref: None,
            ownership: Some(ownership),
            failure: None,
        },
        &cancellation,
        None,
    )
    .await
}

async fn copy_untracked_file(
    cwd: &Path,
    destination: &Path,
    relative: &[u8],
) -> Result<(), CoordinatorError> {
    let relative = std::str::from_utf8(relative)
        .map_err(|_| native_invalid("untracked worktree path is not UTF-8".into()))?;
    let from = cwd.join(relative);
    let to = destination.join(relative);
    let metadata = tokio::fs::symlink_metadata(&from).await?;
    if metadata.file_type().is_symlink() {
        #[cfg(unix)]
        {
            let target = tokio::fs::read_link(&from).await?;
            if let Some(parent) = to.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::symlink(target, to).await?;
        }
        #[cfg(not(unix))]
        return Err(native_invalid(
            "worktree symlink copying is unavailable".into(),
        ));
    } else if metadata.is_file() {
        if metadata.len() > 8 * 1024 * 1024 {
            return Err(native_invalid(
                "untracked worktree file exceeds the supported byte limit".into(),
            ));
        }
        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(&from, &to).await?;
    }
    Ok(())
}

async fn finish_created(
    mut prepared: PreparedSubagent,
    cancellation: &CancellationToken,
    checkpoint: Option<WorkspaceCreationCheckpoint>,
) -> Result<PreparedSubagent, CoordinatorError> {
    if let Some(checkpoint) = checkpoint {
        if let Some(ownership) = &prepared.ownership {
            let _ = checkpoint.entered.send(ownership.path.clone());
            tokio::select! {
                biased;
                () = cancellation.cancelled() => {},
                () = checkpoint.proceed.notified() => {},
            }
        }
    }
    if cancellation.is_cancelled() {
        if let Some(ownership) = &prepared.ownership {
            if let Err(error) = cleanup_owned(ownership).await {
                prepared.failure = Some(error);
                return Ok(prepared);
            }
        }
        return Err(CoordinatorError::Cancelled("Subagent was cancelled".into()));
    }
    Ok(prepared)
}

async fn cleanup_owned(ownership: &WorkspaceOwnership) -> Result<(), CoordinatorError> {
    if !ownership.path.exists() {
        return Ok(());
    }
    let actual = owned_destination(ownership.path.clone(), ownership.source.clone())?;
    if ownership.identity.is_none() || actual.identity != ownership.identity {
        return Err(CoordinatorError::Native {
            code: "workspace_ownership_unavailable".into(),
            message: "the worktree path no longer identifies this attempt's created directory"
                .into(),
        });
    }
    if remove(&ownership.path, &ownership.source).await.is_err() {
        let registered = git(
            &ownership.source,
            &["worktree", "list", "--porcelain"],
            None,
        )
        .await?;
        let path = format!("worktree {}", ownership.path.to_string_lossy());
        if String::from_utf8_lossy(&registered)
            .lines()
            .any(|line| line == path)
        {
            return Err(native_invalid(format!(
                "could not remove the owned worktree {}",
                ownership.path.display()
            )));
        }
        // Partial add failure can leave only the exact directory reserved by
        // this attempt. Never prune unrelated worktree registrations.
        tokio::fs::remove_dir_all(&ownership.path).await?;
    }
    Ok(())
}

pub(super) async fn cleanup(prepared: PreparedSubagent) -> Result<(), CoordinatorError> {
    if let Some(ownership) = &prepared.ownership {
        cleanup_owned(ownership).await?;
    }
    Ok(())
}

/// Create a byte-preserving tree with a private index. The live worktree's
/// index and history remain untouched. Ignored files are included before any
/// disposal decision, so a successful snapshot preserves all file content.
pub(super) async fn snapshot(
    id: &str,
    worktree: &Path,
    source: &Path,
    scratch: &Path,
) -> Result<String, CoordinatorError> {
    let index = tempfile::Builder::new()
        .prefix("subagent-index-")
        .tempfile_in(scratch)?;
    let index_path = index.path().to_path_buf();
    // Git requires an absent index file rather than an empty temporary file.
    std::fs::remove_file(&index_path)?;
    let run = |args: Vec<String>| {
        let path = index_path.clone();
        async move {
            let output = tokio::process::Command::new("git")
                .current_dir(worktree)
                .env("GIT_INDEX_FILE", path)
                .env("GIT_AUTHOR_NAME", "Harness")
                .env("GIT_AUTHOR_EMAIL", "harness@localhost")
                .env("GIT_COMMITTER_NAME", "Harness")
                .env("GIT_COMMITTER_EMAIL", "harness@localhost")
                .args(args)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output()
                .await?;
            if !output.status.success() {
                return Err(native_invalid(
                    String::from_utf8_lossy(&output.stderr).trim().into(),
                ));
            }
            Ok::<_, CoordinatorError>(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        }
    };
    run(vec!["read-tree".into(), "HEAD".into()]).await?;
    run(vec![
        "add".into(),
        "--all".into(),
        "--force".into(),
        ".".into(),
    ])
    .await?;
    let tree = run(vec!["write-tree".into()]).await?;
    let head = run(vec!["rev-parse".into(), "HEAD".into()]).await?;
    let commit = run(vec![
        "commit-tree".into(),
        tree.clone(),
        "-p".into(),
        head,
        "-m".into(),
        format!("Subagent {id} snapshot"),
    ])
    .await?;
    // A named source ref is required before a worktree may be removed.
    let reference = format!("refs/grok/subagents/{id}");
    git(source, &["update-ref", &reference, &commit], None).await?;
    let preserved = run(vec!["rev-parse".into(), format!("{reference}^{{tree}}")]).await?;
    if preserved != tree {
        return Err(native_invalid(
            "worktree snapshot tree did not preserve its content".into(),
        ));
    }
    Ok(reference)
}

pub(super) async fn remove(worktree: &Path, source: &Path) -> Result<(), CoordinatorError> {
    let path = worktree.to_string_lossy();
    git(source, &["worktree", "remove", "--force", &path], None)
        .await
        .map(|_| ())
}

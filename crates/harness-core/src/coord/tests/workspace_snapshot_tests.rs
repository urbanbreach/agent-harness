use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::clock::FakeClock;
use crate::config::{FormatterConfig, FormatterOverride};
use crate::coord::formatter::run_formatter_for_path;
use crate::coord::CoordinatorHandle;
use crate::event::EventV1;

use super::*;

pub(super) async fn snapshot_captures_workspace_and_emits_event() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("a.txt"), "alpha").unwrap_or_abort();
    fs::write(workspace.join("b.txt"), "beta").unwrap_or_abort();
    assert!(std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(&workspace)
        .status()
        .unwrap_or_abort()
        .success());
    fs::write(workspace.join(".gitignore"), "ignored/\n").unwrap_or_abort();
    fs::create_dir(workspace.join("ignored")).unwrap_or_abort();
    fs::write(workspace.join("ignored/generated.bin"), [0_u8, 255]).unwrap_or_abort();
    fs::write(
        workspace.join("config.jsonc"),
        r#"{provider: {options: {apiKey: "snapshot-sensitive-fixture"}}}"#,
    )
    .unwrap_or_abort();

    let handle = spawn_coordinator(
        test_config(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(crate::redact::DefaultRedactor::default()),
    );
    let run = handle
        .start_run("snapshot_run", &workspace)
        .await
        .unwrap_or_abort();

    let summary = handle
        .snapshot_workspace("req_000001")
        .await
        .unwrap_or_abort();

    assert_eq!(summary.request_id, "req_000001".into());
    assert_eq!(summary.file_count, 4);
    let artifact =
        fs::read_to_string(run.artifacts_dir.join(&summary.artifact_path)).unwrap_or_abort();
    assert!(!artifact.contains("snapshot-sensitive-fixture"));
    assert!(!artifact.contains("generated.bin"));
    let payload: serde_json::Value = serde_json::from_str(&artifact).unwrap_or_abort();
    assert!(payload["config.jsonc"]["content"].is_null());
    handle
        .revert_workspace("req_000001")
        .await
        .unwrap_or_abort();
    assert_eq!(
        fs::read(workspace.join("ignored/generated.bin")).unwrap_or_abort(),
        [0, 255]
    );
    assert!(summary.artifact_path.starts_with("snapshots/"));
    assert!(run.artifacts_dir.join(&summary.artifact_path).is_file());

    let events = read_events(&run.events_path);
    let snapshot_event = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::WorkspaceSnapshot(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(snapshot_event.request_id, "req_000001".into());
    assert_eq!(snapshot_event.file_count, 4);
    assert!(!snapshot_event.artifact_digest.is_empty());
}

pub(super) async fn revert_restores_workspace_from_snapshot() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("asset.png"), [0_u8, 255, 1]).unwrap_or_abort();
    fs::write(workspace.join("keep.txt"), "keep-original").unwrap_or_abort();
    fs::write(workspace.join("change.txt"), "change-original").unwrap_or_abort();
    fs::write(workspace.join("remove.txt"), "remove-original").unwrap_or_abort();
    fs::write(workspace.join("notes..txt"), "two-dots").unwrap_or_abort();

    let handle = spawn_coordinator(
        test_config(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(crate::redact::DefaultRedactor::default()),
    );
    let run = handle
        .start_run("revert_run", &workspace)
        .await
        .unwrap_or_abort();

    handle
        .snapshot_workspace("req_revert_001")
        .await
        .unwrap_or_abort();

    // Apply changes after the snapshot.
    fs::write(workspace.join("change.txt"), "change-modified").unwrap_or_abort();
    fs::remove_file(workspace.join("remove.txt")).unwrap_or_abort();
    fs::remove_file(workspace.join("notes..txt")).unwrap_or_abort();
    fs::write(workspace.join("add.txt"), "add-new").unwrap_or_abort();

    let summary = handle
        .revert_workspace("req_revert_001")
        .await
        .unwrap_or_abort();

    assert_successful_workspace_revert(&workspace, &run.events_path, &summary);
    fs::write(workspace.join("asset.png"), [0_u8, 254, 2]).unwrap_or_abort();
    let partial = handle
        .revert_workspace("req_revert_001")
        .await
        .unwrap_or_abort();
    assert_eq!(partial.failed_paths.len(), 1);
    assert_eq!(partial.failed_paths[0].0, "asset.png");
    assert_eq!(
        fs::read(workspace.join("asset.png")).unwrap_or_abort(),
        vec![0, 254, 2]
    );

    let outside = temp_dir.path().join("outside");
    fs::create_dir(&outside).unwrap_or_abort();
    fs::write(outside.join("sentinel.txt"), "external-original").unwrap_or_abort();
    revert_rejects_invalid_snapshot_paths(&handle, &workspace, &run.artifacts_dir, &outside).await;
    #[cfg(unix)]
    {
        revert_rejects_missing_targets_behind_symlinks(&handle, &workspace, &outside).await;
        revert_rejects_external_current_files(&handle, &workspace, &outside, &run.events_path)
            .await;
    }
}

fn assert_successful_workspace_revert(
    workspace: &Path,
    events_path: &Path,
    summary: &crate::coord::WorkspaceRevertSummary,
) {
    assert!(summary.restored_paths.contains(&"change.txt".to_string()));
    assert!(summary.restored_paths.contains(&"remove.txt".to_string()));
    assert_eq!(
        fs::read_to_string(workspace.join("notes..txt")).unwrap_or_abort(),
        "two-dots"
    );
    assert!(summary.removed_paths.contains(&"add.txt".to_string()));
    assert!(summary.failed_paths.is_empty());

    assert_eq!(
        fs::read_to_string(workspace.join("keep.txt")).unwrap_or_abort(),
        "keep-original"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("change.txt")).unwrap_or_abort(),
        "change-original"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("remove.txt")).unwrap_or_abort(),
        "remove-original"
    );
    assert!(!workspace.join("add.txt").exists());

    let events = read_events(events_path);
    let reverted_event = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::WorkspaceReverted(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(reverted_event.snapshot_request_id, "req_revert_001");
    assert_eq!(reverted_event.restored_paths, summary.restored_paths);
    assert!(reverted_event
        .removed_paths
        .contains(&"add.txt".to_string()));
    assert_eq!(
        fs::read(workspace.join("asset.png")).unwrap_or_abort(),
        vec![0, 255, 1]
    );
}

async fn revert_rejects_invalid_snapshot_paths(
    handle: &CoordinatorHandle,
    workspace: &Path,
    artifacts_dir: &Path,
    outside: &Path,
) {
    let artifact_path = artifacts_dir.join("snapshots/req_revert_001.json");
    let original_artifact = fs::read(&artifact_path).unwrap_or_abort();
    let mut artifact: serde_json::Value =
        serde_json::from_slice(&original_artifact).unwrap_or_abort();
    let sentinel = outside.join("sentinel.txt");
    let absolute_inside = workspace.join("keep.txt").to_string_lossy().into_owned();
    let absolute_outside = sentinel.to_string_lossy();
    fs::write(workspace.join("change.txt"), "keep-modified").unwrap_or_abort();
    fs::write(workspace.join("add.txt"), "keep-added").unwrap_or_abort();
    fs::remove_file(workspace.join("remove.txt")).unwrap_or_abort();
    let restore_entry = artifact["keep.txt"].clone();
    for path in [
        "",
        ".",
        "../outside/sentinel.txt",
        "nested/../keep.txt",
        &absolute_inside,
        &absolute_outside,
    ] {
        artifact
            .as_object_mut()
            .unwrap_or_abort()
            .insert(path.to_string(), restore_entry.clone());
        fs::write(
            &artifact_path,
            serde_json::to_vec(&artifact).unwrap_or_abort(),
        )
        .unwrap_or_abort();
        let rejected = handle
            .revert_workspace("req_revert_001")
            .await
            .unwrap_or_abort();
        assert!(rejected.restored_paths.is_empty(), "{path}");
        assert!(rejected.removed_paths.is_empty(), "{path}");
        assert_eq!(rejected.failed_paths.len(), 1, "{path}");
        assert_eq!(rejected.failed_paths[0].0, path);
        assert_eq!(
            fs::read_to_string(workspace.join("change.txt")).unwrap_or_abort(),
            "keep-modified"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("add.txt")).unwrap_or_abort(),
            "keep-added"
        );
        assert!(!workspace.join("remove.txt").exists());
        assert_eq!(fs::read(&sentinel).unwrap_or_abort(), b"external-original");
        artifact.as_object_mut().unwrap_or_abort().remove(path);
    }
    fs::write(&artifact_path, original_artifact).unwrap_or_abort();
}

#[cfg(unix)]
async fn revert_rejects_missing_targets_behind_symlinks(
    handle: &CoordinatorHandle,
    workspace: &Path,
    outside: &Path,
) {
    use std::os::unix::fs::symlink;
    let sentinel = outside.join("sentinel.txt");
    let parent = workspace.join("nested");
    fs::create_dir(&parent).unwrap_or_abort();
    fs::write(parent.join("deleted.txt"), "restore-me").unwrap_or_abort();
    handle
        .snapshot_workspace("req_symlink")
        .await
        .unwrap_or_abort();
    fs::write(workspace.join("change.txt"), "still-modified").unwrap_or_abort();
    fs::remove_dir_all(&parent).unwrap_or_abort();
    symlink(&outside, &parent).unwrap_or_abort();
    let rejected = handle
        .revert_workspace("req_symlink")
        .await
        .unwrap_or_abort();
    assert!(rejected.restored_paths.is_empty());
    assert!(rejected.removed_paths.is_empty());
    assert_eq!(rejected.failed_paths[0].0, "nested/deleted.txt");
    assert!(!outside.join("deleted.txt").exists());
    assert_eq!(fs::read(&sentinel).unwrap_or_abort(), b"external-original");
    assert_eq!(
        fs::read_to_string(workspace.join("change.txt")).unwrap_or_abort(),
        "still-modified"
    );
}

#[cfg(unix)]
async fn revert_rejects_external_current_files(
    handle: &CoordinatorHandle,
    workspace: &Path,
    outside: &Path,
    events_path: &Path,
) {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let parent = workspace.join("nested");
    let sentinel = outside.join("sentinel.txt");
    // Git can enumerate a tracked file through a replaced ancestor. Reject it
    // before reading, even when it would only be removed by the revert.
    fs::remove_file(&parent).unwrap_or_abort();
    fs::create_dir(&parent).unwrap_or_abort();
    fs::write(parent.join("sentinel.txt"), "tracked").unwrap_or_abort();
    for args in [vec!["init", "--quiet"], vec!["add", "nested/sentinel.txt"]] {
        assert!(std::process::Command::new("git")
            .current_dir(&workspace)
            .args(args)
            .status()
            .unwrap_or_abort()
            .success());
    }
    fs::remove_dir_all(&parent).unwrap_or_abort();
    symlink(&outside, &parent).unwrap_or_abort();
    fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o000)).unwrap_or_abort();
    let rejected = handle
        .revert_workspace("req_revert_001")
        .await
        .unwrap_or_abort();
    fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o600)).unwrap_or_abort();
    assert!(rejected.restored_paths.is_empty());
    assert!(rejected.removed_paths.is_empty());
    assert_eq!(rejected.failed_paths[0].0, "nested/sentinel.txt");
    assert!(rejected.failed_paths[0].1.contains("escapes workspace"));
    assert_eq!(fs::read(&sentinel).unwrap_or_abort(), b"external-original");
    assert_eq!(
        fs::read_to_string(workspace.join("change.txt")).unwrap_or_abort(),
        "still-modified"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("add.txt")).unwrap_or_abort(),
        "keep-added"
    );
    assert!(!workspace.join("remove.txt").exists());
    let events = read_events(events_path);
    let payload = events
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::WorkspaceReverted(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
    assert!(payload.restored_paths.is_empty());
    assert!(payload.removed_paths.is_empty());
    assert_eq!(payload.failed_paths[0].path, "nested/sentinel.txt");
}

pub(super) async fn formatter_runs_configured_command_on_edited_file() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let script = temp_dir.path().join("format.sh");
    fs::write(&script, "#!/bin/sh\nsed -i 's/old/new/g' \"$1\"\n").unwrap_or_abort();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap_or_abort().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap_or_abort();
    }

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "_lang_txt".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(vec![script.to_string_lossy().to_string()]),
            environment: None,
            extensions: Some(vec![".txt".to_string()]),
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let file_path = workspace.join("test.txt");
    fs::write(&file_path, "old content").unwrap_or_abort();

    run_formatter_for_path(&config, &workspace, "test.txt")
        .await
        .unwrap_or_abort();

    let content = fs::read_to_string(&file_path).unwrap_or_abort();
    assert_eq!(content, "new content");
}

pub(super) async fn formatter_disabled_skips_command() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let file_path = workspace.join("test.txt");
    fs::write(&file_path, "old content").unwrap_or_abort();

    let config = FormatterConfig {
        enabled: false,
        experimental_oxfmt: false,
        overrides: BTreeMap::new(),
    };

    run_formatter_for_path(&config, &workspace, "test.txt")
        .await
        .unwrap_or_abort();

    let content = fs::read_to_string(&file_path).unwrap_or_abort();
    assert_eq!(content, "old content");
}

pub(super) async fn formatter_missing_language_is_no_op() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let file_path = workspace.join("test.unknown");
    fs::write(&file_path, "old content").unwrap_or_abort();

    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides: BTreeMap::new(),
    };

    run_formatter_for_path(&config, &workspace, "test.unknown")
        .await
        .unwrap_or_abort();

    let content = fs::read_to_string(&file_path).unwrap_or_abort();
    assert_eq!(content, "old content");
}

pub(super) async fn formatter_failure_returns_warning_without_panic() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let file_path = workspace.join("test.txt");
    fs::write(&file_path, "content").unwrap_or_abort();

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "_lang_txt".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(vec!["false".to_string()]),
            environment: None,
            extensions: Some(vec![".txt".to_string()]),
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let err = run_formatter_for_path(&config, &workspace, "test.txt")
        .await
        .expect_err("failing formatter returns Err");
    assert!(
        err.contains("formatter `false` failed"),
        "error surfaces failing command: {err}"
    );

    let content = fs::read_to_string(&file_path).unwrap_or_abort();
    assert_eq!(content, "content");
}

pub(super) async fn replay_of_reverted_session_does_not_restore_files() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("target.txt"), "original").unwrap_or_abort();

    let handle = spawn_coordinator(
        test_config(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(crate::redact::DefaultRedactor::default()),
    );
    let _run = handle
        .start_run("revert_replay_demo", &workspace)
        .await
        .unwrap_or_abort();

    handle
        .snapshot_workspace("snap_replay_absence")
        .await
        .unwrap_or_abort();

    fs::write(workspace.join("target.txt"), "modified").unwrap_or_abort();

    handle
        .revert_workspace("snap_replay_absence")
        .await
        .unwrap_or_abort();

    assert_eq!(
        fs::read_to_string(workspace.join("target.txt")).unwrap_or_abort(),
        "original"
    );

    let replay_workspace = temp_dir.path().join("replay_workspace");
    fs::create_dir_all(&replay_workspace).unwrap_or_abort();
    fs::write(replay_workspace.join("target.txt"), "modified").unwrap_or_abort();

    let replay_store = handle.event_store().await.unwrap_or_abort();
    let mut stream = replay_store.replay(1).unwrap_or_abort();
    while stream.next().await.is_some() {}

    assert_eq!(
        fs::read_to_string(replay_workspace.join("target.txt")).unwrap_or_abort(),
        "modified",
        "replaying WorkspaceReverted must be side-effect free and leave workspace unchanged"
    );
}

pub(super) async fn live_rustfmt_formats_and_diff_reflects_post_format_content() {
    if std::process::Command::new("rustfmt")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: rustfmt not available");
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();

    let original = "fn main(){println!(\"hello\");}\n";
    fs::write(workspace.join("test.rs"), original).unwrap_or_abort();

    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides: BTreeMap::new(),
    };

    run_formatter_for_path(&config, &workspace, "test.rs")
        .await
        .unwrap_or_abort();

    let formatted = fs::read_to_string(workspace.join("test.rs")).unwrap_or_abort();
    assert_ne!(formatted, original, "rustfmt should have changed the file");
    assert!(
        formatted.contains("fn main() {"),
        "rustfmt should add space after fn main"
    );

    let before_normalized = original.replace("\r\n", "\n");
    let formatted_normalized = formatted.replace("\r\n", "\n");
    assert_ne!(
        before_normalized, formatted_normalized,
        "normalized content should differ"
    );

    let diff = similar::TextDiff::from_lines(&before_normalized, &formatted_normalized)
        .unified_diff()
        .to_string();
    assert!(
        diff.contains("-fn main(){println!(\"hello\");}"),
        "diff should show original unformatted line"
    );
    assert!(
        diff.contains("+fn main() {"),
        "diff should show formatted line"
    );
}

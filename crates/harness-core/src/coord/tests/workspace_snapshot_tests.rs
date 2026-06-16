use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use crate::clock::FakeClock;
use crate::config::{FormatterConfig, FormatterLanguageConfig};
use crate::coord::formatter::run_formatter_for_path;
use crate::event::EventV1;

use super::*;

pub(super) async fn snapshot_captures_workspace_and_emits_event() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("a.txt"), "alpha").expect("write a.txt");
    fs::write(workspace.join("b.txt"), "beta").expect("write b.txt");

    let handle = spawn_coordinator(
        test_config(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(crate::redact::DefaultRedactor::default()),
    );
    let run = handle
        .start_run("snapshot_run", &workspace)
        .await
        .expect("start run");

    let summary = handle
        .snapshot_workspace("req_000001")
        .await
        .expect("snapshot workspace");

    assert_eq!(summary.request_id, "req_000001");
    assert_eq!(summary.file_count, 2);
    assert!(summary.artifact_path.starts_with("snapshots/"));
    assert!(run.artifacts_dir.join(&summary.artifact_path).is_file());

    let events = read_events(&run.events_path);
    let snapshot_event = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::WorkspaceSnapshot(payload) => Some(payload),
            _ => None,
        })
        .expect("workspace snapshot event emitted");
    assert_eq!(snapshot_event.request_id, "req_000001");
    assert_eq!(snapshot_event.file_count, 2);
    assert!(!snapshot_event.artifact_digest.is_empty());
}

pub(super) async fn revert_restores_workspace_from_snapshot() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("keep.txt"), "keep-original").expect("write keep");
    fs::write(workspace.join("change.txt"), "change-original").expect("write change");
    fs::write(workspace.join("remove.txt"), "remove-original").expect("write remove");

    let handle = spawn_coordinator(
        test_config(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(crate::redact::DefaultRedactor::default()),
    );
    let run = handle
        .start_run("revert_run", &workspace)
        .await
        .expect("start run");

    handle
        .snapshot_workspace("req_revert_001")
        .await
        .expect("snapshot workspace");

    // Apply changes after the snapshot.
    fs::write(workspace.join("change.txt"), "change-modified").expect("modify change.txt");
    fs::remove_file(workspace.join("remove.txt")).expect("remove remove.txt");
    fs::write(workspace.join("add.txt"), "add-new").expect("add add.txt");

    let summary = handle
        .revert_workspace("req_revert_001")
        .await
        .expect("revert workspace");

    assert!(summary.restored_paths.contains(&"change.txt".to_string()));
    assert!(summary.restored_paths.contains(&"remove.txt".to_string()));
    assert!(summary.removed_paths.contains(&"add.txt".to_string()));
    assert!(summary.failed_paths.is_empty());

    assert_eq!(
        fs::read_to_string(workspace.join("keep.txt")).expect("read keep"),
        "keep-original"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("change.txt")).expect("read change"),
        "change-original"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("remove.txt")).expect("read remove"),
        "remove-original"
    );
    assert!(!workspace.join("add.txt").exists());

    let events = read_events(&run.events_path);
    let reverted_event = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::WorkspaceReverted(payload) => Some(payload),
            _ => None,
        })
        .expect("workspace reverted event emitted");
    assert_eq!(reverted_event.snapshot_request_id, "req_revert_001");
    assert!(reverted_event
        .restored_paths
        .contains(&"change.txt".to_string()));
    assert!(reverted_event
        .restored_paths
        .contains(&"remove.txt".to_string()));
    assert!(reverted_event
        .removed_paths
        .contains(&"add.txt".to_string()));
}

pub(super) async fn formatter_runs_configured_command_on_edited_file() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    let script = temp_dir.path().join("format.sh");
    fs::write(&script, "#!/bin/sh\nsed -i 's/old/new/g' \"$1\"\n").expect("write formatter script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("chmod");
    }

    let mut languages = BTreeMap::new();
    languages.insert(
        "txt".to_string(),
        FormatterLanguageConfig {
            command: vec![script.to_string_lossy().to_string()],
        },
    );
    let config = FormatterConfig {
        enabled: true,
        languages,
    };

    let file_path = workspace.join("test.txt");
    fs::write(&file_path, "old content").expect("write file");

    run_formatter_for_path(&config, &workspace, "test.txt")
        .await
        .expect("formatter succeeds");

    let content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(content, "new content");
}

pub(super) async fn formatter_disabled_skips_command() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    let file_path = workspace.join("test.txt");
    fs::write(&file_path, "old content").expect("write file");

    let config = FormatterConfig {
        enabled: false,
        languages: BTreeMap::new(),
    };

    run_formatter_for_path(&config, &workspace, "test.txt")
        .await
        .expect("disabled formatter returns Ok");

    let content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(content, "old content");
}

pub(super) async fn formatter_missing_language_is_no_op() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    let file_path = workspace.join("test.unknown");
    fs::write(&file_path, "old content").expect("write file");

    let config = FormatterConfig {
        enabled: true,
        languages: BTreeMap::new(),
    };

    run_formatter_for_path(&config, &workspace, "test.unknown")
        .await
        .expect("missing language returns Ok");

    let content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(content, "old content");
}

pub(super) async fn formatter_failure_returns_warning_without_panic() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    let file_path = workspace.join("test.txt");
    fs::write(&file_path, "content").expect("write file");

    let mut languages = BTreeMap::new();
    languages.insert(
        "txt".to_string(),
        FormatterLanguageConfig {
            command: vec!["false".to_string()],
        },
    );
    let config = FormatterConfig {
        enabled: true,
        languages,
    };

    let err = run_formatter_for_path(&config, &workspace, "test.txt")
        .await
        .expect_err("failing formatter returns Err");
    assert!(
        err.contains("formatter `false` failed"),
        "error surfaces failing command: {err}"
    );

    let content = fs::read_to_string(&file_path).expect("read file");
    assert_eq!(content, "content");
}

pub(super) async fn replay_of_reverted_session_does_not_restore_files() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("target.txt"), "original").expect("write target");

    let handle = spawn_coordinator(
        test_config(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(crate::redact::DefaultRedactor::default()),
    );
    let _run = handle
        .start_run("revert_replay_demo", &workspace)
        .await
        .expect("start run");

    handle
        .snapshot_workspace("snap_replay_absence")
        .await
        .expect("snapshot workspace");

    fs::write(workspace.join("target.txt"), "modified").expect("modify target");

    handle
        .revert_workspace("snap_replay_absence")
        .await
        .expect("revert workspace");

    assert_eq!(
        fs::read_to_string(workspace.join("target.txt")).expect("read original workspace"),
        "original"
    );

    let replay_workspace = temp_dir.path().join("replay_workspace");
    fs::create_dir_all(&replay_workspace).expect("create replay workspace");
    fs::write(replay_workspace.join("target.txt"), "modified").expect("seed replay workspace");

    let replay_store = handle.event_store().await.expect("get event store");
    let mut stream = replay_store.replay(1).expect("replay events");
    while stream.next().await.is_some() {}

    assert_eq!(
        fs::read_to_string(replay_workspace.join("target.txt")).expect("read replay workspace"),
        "modified",
        "replaying WorkspaceReverted must be side-effect free and leave workspace unchanged"
    );
}

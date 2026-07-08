use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use crate::clock::FakeClock;
use crate::config::{FormatterConfig, FormatterOverride};
use crate::coord::formatter::run_formatter_for_path;
use crate::event::EventV1;

use super::*;

pub(super) async fn snapshot_captures_workspace_and_emits_event() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("a.txt"), "alpha").unwrap_or_abort();
    fs::write(workspace.join("b.txt"), "beta").unwrap_or_abort();

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
        .unwrap_or_abort();
    assert_eq!(snapshot_event.request_id, "req_000001".into());
    assert_eq!(snapshot_event.file_count, 2);
    assert!(!snapshot_event.artifact_digest.is_empty());
}

pub(super) async fn revert_restores_workspace_from_snapshot() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("keep.txt"), "keep-original").unwrap_or_abort();
    fs::write(workspace.join("change.txt"), "change-original").unwrap_or_abort();
    fs::write(workspace.join("remove.txt"), "remove-original").unwrap_or_abort();

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
    fs::write(workspace.join("add.txt"), "add-new").unwrap_or_abort();

    let summary = handle
        .revert_workspace("req_revert_001")
        .await
        .unwrap_or_abort();

    assert!(summary.restored_paths.contains(&"change.txt".to_string()));
    assert!(summary.restored_paths.contains(&"remove.txt".to_string()));
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

    let events = read_events(&run.events_path);
    let reverted_event = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::WorkspaceReverted(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
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

use std::fs;
use std::sync::Arc;

use crate::clock::FakeClock;
use crate::event::EventV1;

use super::*;

const DOTENV_SECRET: &str = "UMANS_AI_CODING_PLAN_API_KEY='sk-live-secret'";

pub(super) async fn snapshot_omits_dotenv_files_from_artifacts() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("a.txt"), "alpha").expect("write a.txt");
    fs::write(workspace.join(".umans.env"), DOTENV_SECRET).expect("write dotenv secret");

    let handle = spawn_coordinator(
        test_config(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(crate::redact::DefaultRedactor::default()),
    );
    let run = handle
        .start_run("snapshot_secret_run", &workspace)
        .await
        .expect("start run");

    let summary = handle
        .snapshot_workspace("req_secret_001")
        .await
        .expect("snapshot workspace");

    assert_eq!(summary.file_count, 1);
    let artifact = fs::read_to_string(run.artifacts_dir.join(&summary.artifact_path))
        .expect("read snapshot artifact");
    assert!(artifact.contains("a.txt"));
    assert!(!artifact.contains(".umans.env"));
    assert!(!artifact.contains("UMANS_AI_CODING_PLAN_API_KEY"));
    assert!(!artifact.contains("sk-live-secret"));
}

pub(super) async fn revert_ignores_dotenv_files_missing_from_snapshot() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("a.txt"), "alpha").expect("write a.txt");

    let handle = spawn_coordinator(
        test_config(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(crate::redact::DefaultRedactor::default()),
    );
    let _run = handle
        .start_run("revert_secret_run", &workspace)
        .await
        .expect("start run");

    handle
        .snapshot_workspace("req_revert_secret_001")
        .await
        .expect("snapshot workspace");

    fs::write(workspace.join("add.txt"), "add-new").expect("write add.txt");
    fs::write(workspace.join(".umans.env"), DOTENV_SECRET).expect("write dotenv secret");

    let summary = handle
        .revert_workspace("req_revert_secret_001")
        .await
        .expect("revert workspace");

    assert!(summary.removed_paths.contains(&"add.txt".to_string()));
    assert!(!summary.removed_paths.contains(&".umans.env".to_string()));
    assert!(!summary.restored_paths.contains(&".umans.env".to_string()));
    assert!(summary.failed_paths.is_empty());
    assert!(!workspace.join("add.txt").exists());
    assert_eq!(
        fs::read_to_string(workspace.join(".umans.env")).expect("read dotenv secret"),
        DOTENV_SECRET
    );
}

pub(super) async fn revert_ignores_dotenv_files_already_in_snapshot_artifact() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("a.txt"), "alpha").expect("write a.txt");

    let handle = spawn_coordinator(
        test_config(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(crate::redact::DefaultRedactor::default()),
    );
    let run = handle
        .start_run("legacy_snapshot_secret_run", &workspace)
        .await
        .expect("start run");

    let snapshot = handle
        .snapshot_workspace("req_legacy_secret_001")
        .await
        .expect("snapshot workspace");
    let artifact_path = run.artifacts_dir.join(&snapshot.artifact_path);
    let mut artifact: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&artifact_path).expect("read snapshot artifact"))
            .expect("parse snapshot artifact");
    artifact
        .as_object_mut()
        .expect("snapshot artifact object")
        .insert(
            ".umans.env".to_string(),
            serde_json::json!({
                "digest": "legacy-secret-digest",
                "content": DOTENV_SECRET,
            }),
        );
    fs::write(
        &artifact_path,
        serde_json::to_vec(&artifact).expect("serialize artifact"),
    )
    .expect("write legacy snapshot artifact");

    let summary = handle
        .revert_workspace("req_legacy_secret_001")
        .await
        .expect("revert workspace");

    assert!(!workspace.join(".umans.env").exists());
    assert!(!summary.removed_paths.contains(&".umans.env".to_string()));
    assert!(!summary.restored_paths.contains(&".umans.env".to_string()));
    assert!(summary.failed_paths.is_empty());

    let events = read_events(&run.events_path);
    let reverted_event = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::WorkspaceReverted(payload) => Some(payload),
            _ => None,
        })
        .expect("workspace reverted event emitted");
    assert!(!reverted_event
        .restored_paths
        .contains(&".umans.env".to_string()));
    assert!(!reverted_event
        .removed_paths
        .contains(&".umans.env".to_string()));
    assert!(reverted_event
        .failed_paths
        .iter()
        .all(|failure| failure.path != ".umans.env"));
}

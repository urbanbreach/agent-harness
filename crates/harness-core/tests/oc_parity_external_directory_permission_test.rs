//! RED (T2): outside-workspace read/bash/edit must Ask kind `external_directory`.

use std::path::PathBuf;

use harness_core::UnwrapOrAbort;
use serde_json::json;

mod common;

#[path = "common/oc_parity_permission_fixtures.rs"]
mod oc_parity;

use oc_parity::{
    parity_coordinator, permission_kinds_for_tool_call, request_bash, request_edit_external,
    request_read, tool_finished_status, wait_for_tool_settled, KIND_EXTERNAL_DIRECTORY,
};

#[tokio::test]
async fn read_outside_workspace_emits_external_directory_ask() {
    // Given: path outside workspace root
    // When: `read` is requested for that path
    // Then: PermissionRequested kind `external_directory` (not silent allow / CommandBlocked-only)
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run(
            "oc_parity_read_external_directory",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();

    let external = PathBuf::from("/tmp/harness-oc-parity-external-read.env");
    let tool_call_id = request_read(&coordinator, json!(external)).await;
    let events = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let kinds = permission_kinds_for_tool_call(&events, &tool_call_id);
    assert!(
        kinds.iter().any(|kind| kind == KIND_EXTERNAL_DIRECTORY),
        "read outside workspace must emit PermissionRequested kind={KIND_EXTERNAL_DIRECTORY}; \
         got kinds={kinds:?}; finished={:?}. \
         Desired: coordinator pre-eval Ask (T7), not silent allow or CommandBlocked-only.",
        tool_finished_status(&events, &tool_call_id)
    );
}

#[tokio::test]
async fn bash_outside_workspace_emits_external_directory_ask() {
    // Given: bash args that reference a path outside the workspace
    // When: `bash` is requested
    // Then: PermissionRequested kind `external_directory`
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run(
            "oc_parity_bash_external_directory",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();

    let tool_call_id =
        request_bash(&coordinator, "cat /tmp/harness-oc-parity-external.txt").await;
    let events = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let kinds = permission_kinds_for_tool_call(&events, &tool_call_id);
    assert!(
        kinds.iter().any(|kind| kind == KIND_EXTERNAL_DIRECTORY),
        "bash with outside-workspace path must emit PermissionRequested \
         kind={KIND_EXTERNAL_DIRECTORY}; got kinds={kinds:?}; finished={:?}",
        tool_finished_status(&events, &tool_call_id)
    );
}

#[tokio::test]
async fn edit_outside_workspace_emits_external_directory_ask() {
    // Given: edit target outside workspace
    // When: `edit` is requested
    // Then: PermissionRequested kind `external_directory`
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run(
            "oc_parity_edit_external_directory",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();

    let external = PathBuf::from("/tmp/harness-oc-parity-external-edit.rs");
    let tool_call_id = request_edit_external(&coordinator, external).await;
    let events = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let kinds = permission_kinds_for_tool_call(&events, &tool_call_id);
    assert!(
        kinds.iter().any(|kind| kind == KIND_EXTERNAL_DIRECTORY),
        "edit outside workspace must emit PermissionRequested kind={KIND_EXTERNAL_DIRECTORY}; \
         got kinds={kinds:?}; finished={:?}",
        tool_finished_status(&events, &tool_call_id)
    );
}

//! RED (T2): native `read` of env paths must Ask; `.env.example` / normal source Allow.

use harness_core::event::ToolCallStatus;
use harness_core::UnwrapOrAbort;
use serde_json::json;

mod common;

#[path = "common/oc_parity_permission_fixtures.rs"]
mod oc_parity;

use oc_parity::{
    parity_coordinator, permission_kinds_for_tool_call, request_read, tool_finished_status,
    wait_for_tool_settled, KIND_READ,
};

#[tokio::test]
async fn read_foo_env_emits_permission_requested_kind_read() {
    // Given: allow-default policy and workspace-relative `foo.env`
    // When: native `read` is requested
    // Then: PermissionRequested kind `read` (Ask)
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run("oc_parity_read_foo_env", temp_dir.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let tool_call_id = request_read(&coordinator, json!("foo.env")).await;
    let events = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let kinds = permission_kinds_for_tool_call(&events, &tool_call_id);
    assert!(
        kinds.iter().any(|kind| kind == KIND_READ),
        "read of foo.env must emit PermissionRequested kind={KIND_READ} (Ask); \
         got kinds={kinds:?}; finished={:?}. \
         Today ReadFs maps to no permission kind so reads skip the gate.",
        tool_finished_status(&events, &tool_call_id)
    );
}

#[tokio::test]
async fn read_env_local_emits_permission_requested_kind_read() {
    // Given: `.env.local` (*.env.* ask pattern)
    // When: native `read` is requested
    // Then: PermissionRequested kind `read`
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run("oc_parity_read_env_local", temp_dir.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let tool_call_id = request_read(&coordinator, json!(".env.local")).await;
    let events = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let kinds = permission_kinds_for_tool_call(&events, &tool_call_id);
    assert!(
        kinds.iter().any(|kind| kind == KIND_READ),
        "read of .env.local must emit PermissionRequested kind={KIND_READ}; \
         got kinds={kinds:?}; finished={:?}",
        tool_finished_status(&events, &tool_call_id)
    );
}

#[tokio::test]
async fn read_env_example_allows_without_permission_requested() {
    // Given: `.env.example` allow-listed after `*.env.*` ask (last-match wins)
    // When: native `read` is requested
    // Then: no PermissionRequested kind `read`; tool succeeds
    assert!(
        harness_core::perm::permission_kind_for_tool("read").is_some(),
        "precondition: tool id `read` must map to a permission kind before Allow \
         for .env.example is meaningful; today mapping is None"
    );

    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run("oc_parity_read_env_example", temp_dir.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let tool_call_id = request_read(&coordinator, json!(".env.example")).await;
    let events = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let kinds = permission_kinds_for_tool_call(&events, &tool_call_id);
    assert!(
        !kinds.iter().any(|kind| kind == KIND_READ),
        ".env.example must Allow without PermissionRequested kind={KIND_READ}; got {kinds:?}"
    );
    assert_eq!(
        tool_finished_status(&events, &tool_call_id),
        Some(ToolCallStatus::Succeeded),
        ".env.example read should complete successfully without an ask prompt"
    );
}

#[tokio::test]
async fn read_normal_source_allows_without_permission_requested() {
    // Given: ordinary workspace source path
    // When: native `read` is requested
    // Then: Allow (no read ask)
    assert!(
        harness_core::perm::permission_kind_for_tool("read").is_some(),
        "precondition: tool id `read` must map to a permission kind; today mapping is None"
    );

    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run(
            "oc_parity_read_normal_source",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();

    let tool_call_id = request_read(&coordinator, json!("src/main.rs")).await;
    let events = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let kinds = permission_kinds_for_tool_call(&events, &tool_call_id);
    assert!(
        !kinds.iter().any(|kind| kind == KIND_READ),
        "normal source read must not Ask kind={KIND_READ}; got {kinds:?}"
    );
    assert_eq!(
        tool_finished_status(&events, &tool_call_id),
        Some(ToolCallStatus::Succeeded),
        "normal source read should succeed without permission prompt"
    );
}

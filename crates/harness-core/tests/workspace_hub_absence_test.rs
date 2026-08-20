//! Task 10 — remote workspace hub absence and local workspace journey tests.
//!
//! Verifies that the remote workspace hub surface (connect/bind/upload/recover
//! via curl/HTTP) has been removed and that the local file-backed workspace
//! hub journey still passes.
//!
//! Plan reference: §1.2 (Scope OUT — remote workspace hub), §1.4 (removal
//! compatibility matrix), Task 10 QA: `--absence remote-workspace
//! --local-workspace`.

use std::fs;

use harness_core::workspace_hub_local::{
    run_local_workspace_hub_product, LocalHubOpResult, LocalWorkspaceHub, LocalWorkspaceHubError,
    WORKSPACE_HUB_ROOT_REL, WORKSPACE_HUB_STATE_REL,
};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Absence: no remote workspace hub public surface remains
// ---------------------------------------------------------------------------

/// The `workspace_hub` module must not export any remote hub types or
/// functions. After Task 10, the module is a documentation-only stub; all
/// remote types (WorkspaceHubAvailability, WorkspaceHubConnectResult,
/// WorkspaceHubBindResult, WorkspaceHubUploadResult,
/// WorkspaceHubRecoveryResult, WorkspaceHubOutcomeSummary) and remote
/// functions (evaluate_workspace_hub, connect_workspace_hub,
/// bind_workspace_hub, upload_to_workspace_hub, recover_workspace_hub,
/// probe_workspace_hub_product, walk_workspace_hub_*) are removed.
#[test]
fn workspace_hub_module_has_no_remote_public_surface() {
    // arrange
    // The module still exists (aggregator lib.rs still declares it until
    // Task 17 removes the declaration), but it must not export any remote
    // types or functions. We verify absence by checking that the source file
    // contains no remote hub type or function definitions.
    let source = include_str!("../src/workspace_hub.rs");

    // Remote type names that must be absent from the module source.
    let remote_symbols = [
        "WorkspaceHubAvailability",
        "WorkspaceHubConnectResult",
        "WorkspaceHubBindResult",
        "WorkspaceHubUploadResult",
        "WorkspaceHubRecoveryResult",
        "WorkspaceHubOutcomeSummary",
        "WorkspaceHubProductProbe",
        "evaluate_workspace_hub",
        "connect_workspace_hub",
        "bind_workspace_hub",
        "upload_to_workspace_hub",
        "recover_workspace_hub",
        "probe_workspace_hub_product",
        "walk_workspace_hub_connect",
        "walk_workspace_hub_bind",
        "walk_workspace_hub_upload",
        "walk_workspace_hub_recover",
        "summarize_workspace_hub_outcomes",
        "probe_http_endpoint",
        "DEFAULT_WORKSPACE_HUB_CONNECT_PROBES",
        "DEFAULT_WORKSPACE_HUB_BIND_PROBES",
        "DEFAULT_WORKSPACE_HUB_UPLOAD_PROBES",
        "DEFAULT_WORKSPACE_HUB_RECOVER_PROBES",
    ];

    // act
    for symbol in &remote_symbols {
        // assert
        assert!(
            !source.contains(symbol),
            "remote workspace hub symbol `{symbol}` must be absent from workspace_hub.rs"
        );
    }
}

/// The `workspace_hub` module source must not contain any curl, HTTP, or
/// network process-spawning code.
#[test]
fn workspace_hub_module_has_no_curl_or_network_code() {
    // arrange
    let source = include_str!("../src/workspace_hub.rs");

    let forbidden_patterns = [
        "curl",
        "Command::new",
        "std::process::Command",
        "http://",
        "https://",
        "probe_http_endpoint",
        "reqwest",
        "hyper",
        "TcpStream",
        "UdpSocket",
    ];

    // act
    for pattern in &forbidden_patterns {
        // assert
        assert!(
            !source.contains(pattern),
            "network/curl pattern `{pattern}` must be absent from workspace_hub.rs"
        );
    }
}

// ---------------------------------------------------------------------------
// Local workspace journey: file-backed connect/bind/upload/recover
// ---------------------------------------------------------------------------

/// The local workspace hub product (connect → bind → upload → recover) must
/// complete with durable file-backed state and no network calls.
#[test]
fn local_workspace_hub_product_completes_with_durable_state() {
    // arrange
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    // act
    let product = run_local_workspace_hub_product(root).expect("product");

    // assert — all four operations succeeded
    assert!(
        product.meets_contract(),
        "product must meet contract: {product:?}"
    );

    // state file exists and contains durable records
    let state_path = root.join(WORKSPACE_HUB_STATE_REL);
    assert!(state_path.is_file(), "state file must exist");

    let state_body = fs::read_to_string(&state_path).expect("read state");
    assert!(
        state_body.contains("connected"),
        "state must record connected"
    );
    assert!(
        state_body.contains("ws-primary"),
        "state must record bound workspace id"
    );
    assert!(
        state_body.contains("probe-artifact"),
        "state must record upload source hint"
    );
    assert!(
        state_body.contains("probe-session"),
        "state must record recovery session hint"
    );
}

/// Reopening a hub after a product run must reflect persisted state.
#[test]
fn local_workspace_hub_reopen_reflects_persisted_state() {
    // arrange
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    // act — run product, then reopen
    run_local_workspace_hub_product(root).expect("product");
    let reopened = LocalWorkspaceHub::open(root).expect("reopen");

    // assert
    assert!(reopened.is_connected());
    assert_eq!(
        reopened.state().bound_workspace_id.as_deref(),
        Some("ws-primary")
    );
    assert_eq!(reopened.state().uploads.len(), 1);
    assert_eq!(reopened.state().recoveries.len(), 1);
}

/// Upload artifact bytes must be durable on disk.
#[test]
fn local_workspace_hub_upload_bytes_are_durable() {
    // arrange
    let dir = tempdir().expect("tempdir");
    let root = dir.path();

    // act
    let product = run_local_workspace_hub_product(root).expect("product");

    // assert — uploaded bytes are readable from disk
    let stored_rel = match &product.upload {
        LocalHubOpResult::Uploaded { stored_rel, .. } => stored_rel.clone(),
        other => panic!("expected Uploaded, got {other:?}"),
    };
    let stored_path = root.join(WORKSPACE_HUB_ROOT_REL).join(&stored_rel);
    assert!(stored_path.is_file(), "upload file must exist");
    let bytes = fs::read(&stored_path).expect("read upload");
    assert_eq!(bytes, b"hub-upload-payload");
}

/// Bind without connect must fail closed.
#[test]
fn local_workspace_hub_bind_without_connect_fails_closed() {
    // arrange
    let dir = tempdir().expect("tempdir");
    let mut hub = LocalWorkspaceHub::open(dir.path()).expect("open");

    // act
    let err = hub.bind("ws").expect_err("must fail");

    // assert
    assert!(matches!(err, LocalWorkspaceHubError::NotConnected));
}

/// Upload without connect must fail closed.
#[test]
fn local_workspace_hub_upload_without_connect_fails_closed() {
    // arrange
    let dir = tempdir().expect("tempdir");
    let mut hub = LocalWorkspaceHub::open(dir.path()).expect("open");

    // act
    let err = hub
        .upload("artifact.bin", b"payload")
        .expect_err("must fail");

    // assert
    assert!(matches!(err, LocalWorkspaceHubError::NotConnected));
    assert!(!hub.is_connected());
}

/// Recover without connect must fail closed.
#[test]
fn local_workspace_hub_recover_without_connect_fails_closed() {
    // arrange
    let dir = tempdir().expect("tempdir");
    let mut hub = LocalWorkspaceHub::open(dir.path()).expect("open");

    // act
    let err = hub.recover("session").expect_err("must fail");

    // assert
    assert!(matches!(err, LocalWorkspaceHubError::NotConnected));
}

/// Connect with empty endpoint must fail closed.
#[test]
fn local_workspace_hub_connect_empty_endpoint_fails_closed() {
    // arrange
    let dir = tempdir().expect("tempdir");
    let mut hub = LocalWorkspaceHub::open(dir.path()).expect("open");

    // act
    let err = hub.connect("").expect_err("must fail");

    // assert
    assert!(matches!(
        err,
        LocalWorkspaceHubError::EmptyInput { field: "endpoint" }
    ));
}

/// Full local hub lifecycle: connect → bind → upload → recover with
/// explicit step-by-step verification.
#[test]
fn local_workspace_hub_full_lifecycle_step_by_step() {
    // arrange
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    let mut hub = LocalWorkspaceHub::open(root).expect("open");
    assert!(!hub.is_connected());

    // act + assert — connect
    let connect = hub.connect("local://test-hub").expect("connect");
    assert!(connect.is_success());
    assert!(hub.is_connected());
    assert_eq!(hub.state().endpoint.as_deref(), Some("local://test-hub"));

    // act + assert — bind
    let bind = hub.bind("ws-test-1").expect("bind");
    assert!(bind.is_success());
    assert_eq!(hub.state().bound_workspace_id.as_deref(), Some("ws-test-1"));

    // act + assert — upload
    let upload = hub
        .upload("test-artifact.bin", b"test-payload-bytes")
        .expect("upload");
    assert!(upload.is_success());
    assert_eq!(hub.state().uploads.len(), 1);

    // act + assert — recover
    let recover = hub.recover("test-session-42").expect("recover");
    assert!(recover.is_success());
    assert_eq!(hub.state().recoveries.len(), 1);

    // assert — state persisted with correct seq
    assert!(hub.state().seq >= 4, "seq must reflect 4 operations");
}

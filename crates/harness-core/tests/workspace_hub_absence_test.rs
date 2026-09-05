use harness_core::workspace_hub::{
    connect_workspace_hub, evaluate_workspace_hub, WorkspaceHubConnectResult,
};

#[test]
fn hosted_workspace_hub_remains_unavailable() {
    assert!(evaluate_workspace_hub().is_unavailable());
    assert!(matches!(
        connect_workspace_hub("https://example.invalid"),
        WorkspaceHubConnectResult::Unavailable { .. }
    ));
}

use harness_core::workspace_hub::{evaluate_workspace_hub, WorkspaceHubAvailability};

#[test]
fn hosted_workspace_hub_remains_unavailable() {
    let availability = evaluate_workspace_hub();
    assert!(availability.is_unavailable());
    assert!(!availability.is_available());
    assert_eq!(
        availability,
        WorkspaceHubAvailability::Unavailable {
            reason: "hosted workspace integration removed".to_string(),
        }
    );
}

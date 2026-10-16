use super::*;
use crate::{clock::FakeClock, redact::DefaultRedactor};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Automatic approvers, such as the desktop GUI's auto modes, get exactly YOLO's authority.
#[tokio::test]
async fn automatic_approval_never_exceeds_yolo_eligibility(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let count = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(super::tests::CountTool(Arc::clone(&count))));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.tool_registry = Arc::new(registry);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("auto", temp.path()).await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    for (path, completes) in [
        (temp.path().join("ordinary.txt"), true),
        (temp.path().join(".env"), false),
        (outside.path().join("outside.txt"), false),
    ] {
        let external = !path.starts_with(temp.path());
        let id = coordinator
            .request_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "count",
                json!({"path":path}),
            )
            .await?;
        let first = super::grant_tests::approval(&mut events, &id).await?;
        // The tool's own request is eligible unless it reads a secret file.
        let approved = coordinator
            .resolve_permission_if_yolo_eligible(first.clone(), "automatic approver")
            .await?;
        assert_eq!(approved, completes || external);
        let pending = if external {
            // Leaving the workspace asks again, and that request is never approved automatically.
            let request = super::grant_tests::approval(&mut events, &id).await?;
            let approved = coordinator
                .resolve_permission_if_yolo_eligible(request.clone(), "automatic approver")
                .await?;
            assert!(!approved);
            Some(request)
        } else if completes {
            None
        } else {
            Some(first)
        };
        if let Some(request) = pending {
            coordinator
                .resolve_permission(request, PermissionDecision::Deny, None)
                .await?;
        } else {
            super::history_tests::settled(&coordinator, &id).await?;
        }
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    coordinator.stop_run().await?;
    Ok(())
}

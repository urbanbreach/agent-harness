use super::*;

#[tokio::test]
async fn remembered_external_directory_allows_file_lifecycle_across_tools(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    let outside = temp.path().join("outside");
    fs::create_dir(&root)?;
    fs::create_dir(&outside)?;
    let file = outside.join("sample.txt");
    let denied = outside.join("denied.txt");
    let sibling = temp.path().join("unapproved.txt");
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.yolo_on_start = true;
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Ask,
        },
        PermissionRule {
            permission: "edit".into(),
            pattern: denied.to_string_lossy().into_owned().into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("external file lifecycle", &root)
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let patch = format!(
        "*** Begin Patch\n*** Delete File: {}\n*** End Patch",
        file.display()
    );
    for (tool, args, approval, succeeds) in [
        (
            "list",
            json!({"path":outside}),
            Some(PermissionDecision::Allow),
            true,
        ),
        (
            "write",
            json!({"filePath":file,"content":"created\n"}),
            None,
            true,
        ),
        ("read", json!({"filePath":file}), None, true),
        (
            "edit",
            json!({"filePath":file,"oldString":"created","newString":"edited"}),
            None,
            true,
        ),
        ("read", json!({"filePath":file}), None, true),
        ("apply_patch", json!({"patchText":patch}), None, true),
        (
            "write",
            json!({"filePath":denied,"content":"blocked"}),
            None,
            false,
        ),
        (
            "write",
            json!({"filePath":sibling,"content":"blocked"}),
            Some(PermissionDecision::Deny),
            false,
        ),
    ] {
        let task = coordinator
            .request_tool_call(EventActor::new(ActorKind::User, None), None, tool, args)
            .await?;
        let mut requested = false;
        loop {
            let event = tokio::time::timeout(std::time::Duration::from_secs(5), events.next())
                .await?
                .ok_or("missing tool completion")??;
            match event.payload {
                EventV1::PermissionRequested(permission)
                    if permission
                        .tool_call_id
                        .as_ref()
                        .is_some_and(|id| id.as_str() == task) =>
                {
                    assert_eq!(permission.kind, "external_directory");
                    assert!(!requested, "duplicate approval for {tool}");
                    requested = true;
                    let decision =
                        approval.ok_or("unexpected approval inside the remembered directory")?;
                    coordinator
                        .resolve_permission_with_grant_scope(
                            permission.permission_id,
                            decision,
                            None,
                            (decision == PermissionDecision::Allow)
                                .then_some(PermissionGrantScope::Session),
                        )
                        .await?;
                }
                EventV1::ToolCallFinished(result) if result.tool_call_id.as_str() == task => {
                    assert_eq!(
                        result.status,
                        if succeeds {
                            ToolCallStatus::Succeeded
                        } else {
                            ToolCallStatus::Failed
                        },
                        "{tool}: {:?}",
                        result.output_summary
                    );
                    assert_eq!(
                        requested,
                        approval.is_some(),
                        "{tool}: directory scope changed"
                    );
                    break;
                }
                _ => {}
            }
        }
        match tool {
            "write" if succeeds => assert_eq!(fs::read_to_string(&file)?, "created\n"),
            "edit" => assert_eq!(fs::read_to_string(&file)?, "edited\n"),
            "apply_patch" => assert!(!file.exists()),
            _ => {}
        }
    }
    assert!(!denied.exists() && !sibling.exists());
    coordinator.stop_run().await?;
    Ok(())
}

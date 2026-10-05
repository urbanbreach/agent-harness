#[tokio::test]
async fn ast_rewrites_preview_check_all_paths_and_use_normal_edit_receipts(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::{
        event::PermissionDecision,
        perm::{PermissionAction, PermissionRule},
    };
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real ast-grep checks".into());
    }
    let root = tempfile::tempdir()?;
    let source = "const face = '😀';\nconsole.log(face);\nconsole.log('second');\n";
    std::fs::write(root.path().join("a.js"), source)?;
    std::fs::write(root.path().join("b.js"), "console.log('other');\n")?;
    std::fs::write(
        root.path().join("secret.js"),
        "console.log('DENIED_CONTENT');\n",
    )?;
    std::fs::write(
        root.path().join("ignored.js"),
        "console.log('IGNORED_CONTENT');\n",
    )?;
    std::fs::write(root.path().join(".ignore"), "ignored.js\n")?;
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.formatter = Arc::new(harness_core::config::FormatterConfig {
        enabled: false,
        ..Default::default()
    });
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "read".into(),
            pattern: "secret.js".into(),
            action: PermissionAction::Deny,
        },
        PermissionRule {
            permission: "edit".into(),
            pattern: "b.js".into(),
            action: PermissionAction::Ask,
        },
        PermissionRule {
            permission: "edit".into(),
            pattern: "denied.js".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle.start_run("ast", root.path()).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    let search = handle
        .execute_agent_tool_call(
            actor(),
            None,
            "ast_grep_search",
            json!({"pattern":"console.log($X)","language":"javascript","limit":1}),
        )
        .await?;
    let metadata = search.structured_json.ok_or("missing search metadata")?;
    assert_eq!(metadata["total_count"], 3);
    assert_eq!(metadata["returned_count"], 1);
    assert_eq!(metadata["truncated"], true);
    assert_eq!(metadata["skipped_files"], 1);
    assert!(!metadata.to_string().contains("DENIED_CONTENT"));
    let mut args =
        json!({"pattern":"console.log($X)","rewrite":"console.warn($X)","include":["a.js","b.js"]});
    let preview = handle
        .execute_agent_tool_call(actor(), None, "ast_grep_replace", args.clone())
        .await?;
    let preview_artifact = preview
        .artifacts
        .first()
        .ok_or("missing TUI preview artifact")?;
    assert!(preview_artifact.path.ends_with(".diff"));
    assert!(
        std::fs::read_to_string(run.run_dir.join(&preview_artifact.path))?
            .contains("+console.warn(face)")
    );
    let preview = preview.structured_json.ok_or("missing rewrite preview")?;
    assert_eq!(preview["applied"], false);
    assert_eq!(preview["total_count"], 3);
    assert!(preview["diff_artifact"]["path"]
        .as_str()
        .is_some_and(|path| path.ends_with(".diff")));
    assert_eq!(std::fs::read_to_string(root.path().join("a.js"))?, source);
    args["mode"] = "apply".into();
    args["limit"] = 1.into();
    assert!(handle
        .execute_agent_tool_call(actor(), None, "ast_grep_replace", args.clone())
        .await
        .is_err_and(|e| e.contains("limit")));
    args["limit"] = 200.into();
    let mut events = handle.subscribe_new_events().await?;
    for outcome in ["deny", "cancel", "stale", "allow"] {
        let task = handle
            .request_tool_call(actor(), None, "ast_grep_replace", args.clone())
            .await?;
        let permission = ast_permission(&mut events).await?;
        assert!(permission.summary.contains("b.js"));
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.js"))?,
            source,
            "preflight must finish before any write"
        );
        if outcome == "cancel" {
            handle
                .cancel_task(&task, "cancel during path approval")
                .await?;
        } else {
            if outcome == "stale" {
                std::fs::write(root.path().join("b.js"), "editor changed this\n")?;
            }
            handle
                .resolve_permission_with_grant_scope(
                    permission.permission_id,
                    if outcome == "deny" {
                        PermissionDecision::Deny
                    } else {
                        PermissionDecision::Allow
                    },
                    None,
                    (outcome == "allow")
                        .then_some(harness_core::perm::PermissionGrantScope::Session),
                )
                .await?;
        }
        let status = ast_finished(&mut events, &task).await?;
        assert_eq!(
            status,
            if outcome == "allow" {
                ToolCallStatus::Succeeded
            } else {
                ToolCallStatus::Failed
            }
        );
        if outcome == "stale" {
            assert_eq!(
                std::fs::read_to_string(root.path().join("b.js"))?,
                "editor changed this\n"
            );
            std::fs::write(root.path().join("b.js"), "console.log('other');\n")?;
        }
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("a.js"))?,
        source.replace("console.log", "console.warn")
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("b.js"))?,
        "console.warn('other');\n"
    );
    std::fs::write(root.path().join("denied.js"), "console.log('never');\n")?;
    let granted_args = args.clone();
    args["paths"] = json!(["denied.js"]);
    assert!(handle
        .execute_agent_tool_call(actor(), None, "ast_grep_replace", args)
        .await
        .is_err_and(|e| e.contains("permission denied")));
    let journal = harness_core::store::read_events(&run.events_path)?;
    assert_eq!(
        journal
            .iter()
            .filter(|e| matches!(e.payload, EventV1::EditApplied(_)))
            .count(),
        2
    );
    for snapshot in journal.iter().filter_map(|e| match &e.payload {
        EventV1::WorkspaceSnapshot(s) => Some(s.request_id.to_string()),
        _ => None,
    }) {
        handle.revert_workspace(snapshot).await?;
    }
    assert_eq!(std::fs::read_to_string(root.path().join("a.js"))?, source);
    let approvals = journal
        .iter()
        .filter(|e| matches!(e.payload, EventV1::PermissionRequested(_)))
        .count();
    let repeated = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        handle.execute_agent_tool_call(actor(), None, "ast_grep_replace", granted_args),
    )
    .await??;
    assert!(!repeated.is_error());
    assert_eq!(
        harness_core::store::read_events(&run.events_path)?
            .iter()
            .filter(|e| matches!(e.payload, EventV1::PermissionRequested(_)))
            .count(),
        approvals,
        "the exact path grant is reusable"
    );
    handle.stop_run().await?;
    Ok(())
}

async fn ast_permission(
    events: &mut harness_core::store::EventStream,
) -> Result<harness_core::event::PermissionRequestedEvent, Box<dyn std::error::Error>> {
    use tokio_stream::StreamExt;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if let EventV1::PermissionRequested(p) = event?.payload {
                return Ok(p);
            }
        }
        Err("missing AST path approval".into())
    })
    .await?
}

async fn ast_finished(
    events: &mut harness_core::store::EventStream,
    task: &str,
) -> Result<ToolCallStatus, Box<dyn std::error::Error>> {
    use tokio_stream::StreamExt;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if let EventV1::ToolCallFinished(result) = event?.payload
                && result.tool_call_id.as_str() == task
            {
                return Ok(result.status);
            }
        }
        Err("missing AST completion".into())
    })
    .await?
}

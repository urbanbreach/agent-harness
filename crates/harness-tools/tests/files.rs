use harness_core::{
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1, ToolCallStatus},
    perm::{
        PermissionAction, PermissionDecision, PermissionGrantScope, PermissionPolicy,
        PermissionRule,
    },
    redact::DefaultRedactor,
};
use serde_json::json;
use std::{fs, sync::Arc};
use tokio_stream::StreamExt;

#[path = "files/finalized_state.rs"]
mod finalized_state;

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

#[tokio::test]
async fn file_edits_require_current_reads_and_obey_external_path_policy(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    fs::create_dir(&root)?;
    let file = root.join("sample.txt");
    fs::write(&file, "alpha\r\nbeta\r\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755))?;
    }
    let outside = temp.path().join("outside.txt");
    fs::write(&outside, "outside")?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
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
            pattern: "secret.txt".into(),
            action: PermissionAction::Deny,
        },
        PermissionRule {
            permission: "external_directory".into(),
            pattern: "*".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("files", &root).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    use base64::Engine;
    let encoded = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let png = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    fs::write(root.join("picture.png"), &png)?;
    let picture = coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"path":"picture.png"}))
        .await?;
    assert_eq!(picture.attachments.len(), 1);
    assert_eq!(picture.attachments[0].mime, "image/png");
    assert_eq!(picture.attachments[0].bytes()?, png);
    let pdf = b"%PDF-1.7\nPDF_BODY_NOT_JOURNALED\n%%EOF\n";
    fs::write(root.join("document.pdf"), pdf)?;
    let document = coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"path":"document.pdf"}))
        .await?;
    assert!(document.attachments.is_empty());
    assert!(document
        .display_text
        .contains("contents were not extracted"));
    let artifact = document.artifacts.first().ok_or("missing PDF artifact")?;
    assert!(artifact.path.ends_with(".pdf"));
    let retained = run.run_dir.join(&artifact.path);
    assert_eq!(fs::read(&retained)?, pdf);
    assert_eq!(artifact.digest, blake3::hash(pdf).to_hex().as_str());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&retained)?.permissions().mode() & 0o777, 0o600);
    }
    let before = fs::read_dir(&run.artifacts_dir)?.count();
    fs::write(
        root.join("document.pdf"),
        b"%PDF-1.7\nsk-private-pdf-credential\n%%EOF",
    )?;
    assert!(coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"path":"document.pdf"}))
        .await
        .is_err_and(|error| error.contains("credential")));
    assert_eq!(fs::read_dir(&run.artifacts_dir)?.count(), before);
    assert!(!fs::read_to_string(&run.events_path)?.contains("PDF_BODY_NOT_JOURNALED"));
    assert!(!std::fs::read_to_string(&run.events_path)?.contains(encoded));
    assert!(
        coordinator
            .execute_agent_tool_call(
                actor(),
                None,
                "write",
                json!({"path":".agent-harness/permission-grants.json","content":"{}"})
            )
            .await
            .is_err(),
        "tools must not rewrite their persistent permissions"
    );
    assert!(!root.join(".agent-harness/permission-grants.json").exists());
    let edit = || json!({"filePath":"sample.txt", "oldString":"alpha", "newString":"gamma"});
    assert!(coordinator
        .execute_agent_tool_call(actor(), None, "edit", edit())
        .await
        .is_err());
    let read = coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"filePath":"sample.txt"}))
        .await?;
    assert!(read.display_text.contains("alpha"));
    coordinator
        .execute_agent_tool_call(actor(), None, "edit", edit())
        .await?;
    assert_eq!(fs::read_to_string(&file)?, "gamma\r\nbeta\r\n");
    let scan = coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"path":"sample.txt"}))
        .await?;
    let anchors = scan
        .structured_json
        .as_ref()
        .and_then(|v| v.get("anchors"))
        .and_then(serde_json::Value::as_array)
        .ok_or("read did not include line anchors")?;
    let first = format!(
        "1#{}",
        anchors[0]["hash"].as_str().ok_or("missing anchor hash")?
    );
    let second = format!(
        "2#{}",
        anchors[1]["hash"].as_str().ok_or("missing anchor hash")?
    );
    assert!(coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"sample.txt", "edits":[
                {"op":"replace", "pos":first, "lines":["would change"]},
                {"op":"replace", "pos":"2#00000000", "lines":["stale"]}
            ]})
        )
        .await
        .is_err());
    assert_eq!(
        fs::read_to_string(&file)?,
        "gamma\r\nbeta\r\n",
        "all anchors must be checked before any change"
    );
    assert!(coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"sample.txt", "edits":[
                {"op":"replace", "pos":second, "end":first, "lines":["reversed"]}
            ]})
        )
        .await
        .is_err());
    coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"sample.txt", "edits":[
                {"op":"replace", "pos":first, "end":second, "lines":["delta", "epsilon"]}
            ]}),
        )
        .await?;
    assert_eq!(fs::read_to_string(&file)?, "delta\r\nepsilon\r\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&file)?.permissions().mode() & 0o777, 0o755);
    }
    fs::write(&file, "changed by editor\n")?;
    assert!(coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "write",
            json!({"filePath":"sample.txt", "content":"overwrite"})
        )
        .await
        .is_err());
    assert_eq!(fs::read_to_string(&file)?, "changed by editor\n");
    coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "write",
            json!({"filePath":"nested/new.txt", "content":"created\n"}),
        )
        .await?;
    assert_eq!(
        fs::read_to_string(root.join("nested/new.txt"))?,
        "created\n"
    );
    assert!(coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"filePath":"../outside.txt"}))
        .await
        .is_err());
    #[cfg(unix)]
    {
        fs::write(root.join("secret.txt"), "private data")?;
        std::os::unix::fs::symlink(root.join("secret.txt"), root.join("alias"))?;
        assert!(coordinator
            .execute_agent_tool_call(actor(), None, "read", json!({"filePath":"alias"}))
            .await
            .is_err());
        std::os::unix::fs::symlink(&outside, root.join("link"))?;
        assert!(coordinator
            .execute_agent_tool_call(
                actor(),
                None,
                "write",
                json!({"filePath":"link", "content":"escape"})
            )
            .await
            .is_err());
    }
    assert_eq!(fs::read_to_string(outside)?, "outside");
    coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"created-by-edit.txt", "oldString":"", "newString":"created\n"}),
        )
        .await?;
    assert_eq!(
        fs::read_to_string(root.join("created-by-edit.txt"))?,
        "created\n"
    );
    assert!(coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"created-by-edit.txt", "oldString":"", "newString":"overwrite"})
        )
        .await
        .is_err());
    let snapshots: Vec<_> = harness_core::store::read_events(&run.events_path)?
        .into_iter()
        .filter_map(|e| match e.payload {
            EventV1::WorkspaceSnapshot(snapshot) => Some(snapshot.request_id.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(snapshots.len(), 4);
    let removed = coordinator.revert_workspace(snapshots[3].clone()).await?;
    assert_eq!(removed.removed_paths, ["created-by-edit.txt"]);
    assert!(!root.join("created-by-edit.txt").exists());
    assert!(coordinator
        .revert_workspace(snapshots[1].clone())
        .await
        .is_err());
    assert_eq!(fs::read_to_string(&file)?, "changed by editor\n");
    fs::write(&file, "delta\r\nepsilon\r\n")?;
    let restored = coordinator.revert_workspace(snapshots[1].clone()).await?;
    assert_eq!(restored.restored_paths, ["sample.txt"]);
    assert_eq!(fs::read_to_string(&file)?, "gamma\r\nbeta\r\n");
    assert!(coordinator
        .revert_workspace(snapshots[1].clone())
        .await?
        .restored_paths
        .is_empty());
    coordinator.stop_run().await?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "files resumed")
        .await?;
    resumed.revert_workspace(snapshots[0].clone()).await?;
    assert_eq!(fs::read_to_string(&file)?, "alpha\r\nbeta\r\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&file)?.permissions().mode() & 0o777, 0o755);
    }
    resumed.stop_run().await?;
    let events = harness_core::store::read_events(&run.events_path)?;
    let edits: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::EditApplied(edit) => Some((event.seq, edit)),
            _ => None,
        })
        .collect();
    assert_eq!(edits.len(), 4, "only completed file writes are recorded");
    assert!(
        edits
            .iter()
            .all(|(_, edit)| edit.new_file_digest.len() == 12),
        "edit digests must match the TUI's 12-character content fingerprint"
    );
    let (seq, edit) = edits[0];
    assert!(events.iter().any(|e| e.seq < seq
        && matches!(&e.payload, EventV1::EditProposed(p) if p.edit_id == edit.edit_id)));
    let diff = fs::read_to_string(
        run.run_dir
            .join(edit.diff_rel_path.as_ref().ok_or("missing edit diff")?),
    )?;
    assert!(diff.contains("-alpha") && diff.contains("+gamma"));
    Ok(())
}

use super::*;
use crate::perm::{PermissionAction, PermissionRule};

struct FileTool(&'static str, ToolCapability);

#[async_trait]
impl Tool for FileTool {
    fn id(&self) -> &str {
        self.0
    }
    fn capability(&self) -> ToolCapability {
        self.1
    }
    async fn call(&self, _: ToolContext, _: serde_json::Value) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text("executed"))
    }
}

fn config(root: &Path, action: PermissionAction) -> CoordinatorConfig {
    let mut config = test_config(root);
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FileTool("read", ToolCapability::ReadFs)));
    registry.register(Arc::new(FileTool("edit", ToolCapability::EditFs)));
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let mut profile = test_agent_profile("default");
    profile.permission_ruleset = vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "read".into(),
            pattern: "secret/*".into(),
            action,
        },
        PermissionRule {
            permission: "edit".into(),
            pattern: "secret/*".into(),
            action,
        },
    ];
    config.agent_profiles.insert("default".into(), profile);
    config
}

pub(super) async fn permission_path_equivalents() {
    for action in [PermissionAction::Deny, PermissionAction::Ask] {
        let temp = tempfile::tempdir().unwrap_or_abort();
        fs::create_dir(temp.path().join("secret")).unwrap_or_abort();
        fs::write(temp.path().join("secret/data"), "unchanged").unwrap_or_abort();
        let mut paths = vec!["secret/data", "missing/../secret/data", "secret/new/leaf"];
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("secret", temp.path().join("alias")).unwrap_or_abort();
            paths.extend(["alias/data", "alias/new/leaf", "secret/link"]);
            fs::write(temp.path().join("allowed"), "control").unwrap_or_abort();
            std::os::unix::fs::symlink("../allowed", temp.path().join("secret/link"))
                .unwrap_or_abort();
        }
        let handle = spawn_coordinator(
            config(temp.path(), action),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = handle
            .start_run("permission_paths", temp.path())
            .await
            .unwrap_or_abort();
        let actor = EventActor::new(ActorKind::Supervisor, None);
        let mut blocked = Vec::new();
        for tool in ["read", "edit"] {
            for path in &paths {
                let result = handle
                    .request_tool_call(actor.clone(), None, tool, json!({"path": path}))
                    .await;
                let id = match (action, result) {
                    (PermissionAction::Deny, Err(CoordinatorError::PermissionDenied(id))) => id,
                    (PermissionAction::Ask, Ok(id)) => {
                        assert!(read_events(&run.events_path).iter().any(|event| matches!(&event.payload, EventV1::PermissionRequested(data) if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(id.as_str()))));
                        id
                    }
                    (_, result) => panic!("{tool} {path}: expected {action:?}, got {result:?}"),
                };
                blocked.push(id);
            }
            let allowed = handle
                .request_tool_call(actor.clone(), None, tool, json!({"path": "allowed"}))
                .await
                .unwrap_or_abort();
            wait_for_events(&handle, &run.events_path, "allowed file start", |event| matches!(&event.payload, EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == allowed)).await;
        }
        handle.stop_run().await.unwrap_or_abort();
        assert!(!read_events(&run.events_path).iter().any(|event| matches!(&event.payload, EventV1::ToolCallStarted(data) if blocked.iter().any(|id| id == data.tool_call_id.as_str()))));
        assert_eq!(
            fs::read_to_string(temp.path().join("secret/data")).unwrap_or_abort(),
            "unchanged"
        );
    }
}

pub(super) async fn permission_path_invalid_arguments_fail_closed() {
    let temp = tempfile::tempdir().unwrap_or_abort();
    fs::write(temp.path().join("file"), "unchanged").unwrap_or_abort();
    let mut arguments = vec![
        json!({"path": ""}),
        json!({"path": null}),
        json!({"paths": ["allowed", 7]}),
        json!({"filePath": "bad\u{0}path"}),
        json!({"path": "file/child"}),
        json!({"patchText": "*** Begin Patch\n*** Add File: \n+data\n*** End Patch"}),
    ];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("missing", temp.path().join("dangling")).unwrap_or_abort();
        arguments.extend([
            json!({"path": "dangling"}),
            json!({"path": "dangling/child"}),
        ]);
    }
    let handle = spawn_coordinator(
        config(temp.path(), PermissionAction::Allow),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("invalid_paths", temp.path())
        .await
        .unwrap_or_abort();
    for args in arguments {
        assert!(matches!(
            handle
                .request_tool_call(
                    EventActor::new(ActorKind::Supervisor, None),
                    None,
                    "read",
                    args
                )
                .await,
            Err(CoordinatorError::PolicyViolation(_))
        ));
    }
    handle.stop_run().await.unwrap_or_abort();
    assert!(!read_events(&run.events_path)
        .iter()
        .any(|event| matches!(event.payload, EventV1::ToolCallStarted(_))));
}

#[cfg(unix)]
pub(super) async fn permission_path_sensitive_alias_grants_and_pending_approval() {
    let temp = tempfile::tempdir().unwrap_or_abort();
    fs::create_dir(temp.path().join("secret")).unwrap_or_abort();
    for name in ["a.env", "b.env", "denied"] {
        fs::write(temp.path().join("secret").join(name), "unchanged").unwrap_or_abort();
    }
    let alias = temp.path().join("harmless");
    std::os::unix::fs::symlink("secret/a.env", &alias).unwrap_or_abort();
    let mut config = config(temp.path(), PermissionAction::Ask);
    config
        .agent_profiles
        .get_mut("default")
        .unwrap_or_abort()
        .permission_ruleset
        .push(PermissionRule {
            permission: "read".into(),
            pattern: "secret/denied".into(),
            action: PermissionAction::Deny,
        });
    config.always_approve_on_start = true;
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("alias_grants", temp.path())
        .await
        .unwrap_or_abort();
    let actor = EventActor::new(ActorKind::Supervisor, None);
    let args = json!({"path": "harmless"});
    let first = handle
        .request_tool_call(actor.clone(), None, "read", args.clone())
        .await
        .unwrap_or_abort();
    let permission = requested_permission(&read_events(&run.events_path), &first);
    handle.set_always_approve_mode(true).await.unwrap_or_abort();
    assert!(!read_events(&run.events_path).iter().any(|event| matches!(&event.payload, EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == first)));
    handle
        .resolve_permission_with_grant_scope(
            permission,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .unwrap_or_abort();
    let second = handle
        .request_tool_call(actor.clone(), None, "read", args.clone())
        .await
        .unwrap_or_abort();
    wait_for_events(&handle, &run.events_path, "unchanged alias grant", |event| matches!(&event.payload, EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == second)).await;

    // Identical arguments must not carry the grant onto a new effective target.
    fs::remove_file(&alias).unwrap_or_abort();
    std::os::unix::fs::symlink("secret/b.env", &alias).unwrap_or_abort();
    let third = handle
        .request_tool_call(actor.clone(), None, "read", args.clone())
        .await
        .unwrap_or_abort();
    let permission = requested_permission(&read_events(&run.events_path), &third);
    fs::remove_file(&alias).unwrap_or_abort();
    std::os::unix::fs::symlink("secret/denied", &alias).unwrap_or_abort();
    handle
        .resolve_permission_with_grant_scope(
            permission,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .unwrap_or_abort();
    assert!(matches!(
        handle.request_tool_call(actor, None, "read", args).await,
        Err(CoordinatorError::PermissionDenied(_))
    ));
    handle.stop_run().await.unwrap_or_abort();
    let events = read_events(&run.events_path);
    assert!(!events.iter().any(|event| matches!(&event.payload, EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == third)));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::PermissionGrantRecorded(_)))
            .count(),
        1
    );
}

#[cfg(unix)]
fn requested_permission(events: &[EventEnvelopeV1], call: &str) -> String {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(call) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort()
}

#[cfg(unix)]
pub(super) async fn permission_path_secondary_prompts_revalidate_targets() {
    for kind in ["external_directory", "doom_loop"] {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(workspace.join("secret")).unwrap_or_abort();
        fs::create_dir(temp.path().join("outside")).unwrap_or_abort();
        fs::write(temp.path().join("outside/data"), "outside").unwrap_or_abort();
        fs::write(workspace.join("allowed"), "inside").unwrap_or_abort();
        fs::write(workspace.join("secret/denied"), "restricted").unwrap_or_abort();
        let alias = if kind == "external_directory" {
            temp.path().join("outside/alias")
        } else {
            workspace.join("alias")
        };
        std::os::unix::fs::symlink(
            if kind == "external_directory" {
                "data"
            } else {
                "allowed"
            },
            &alias,
        )
        .unwrap_or_abort();
        let mut config = config(temp.path(), PermissionAction::Deny);
        config
            .agent_profiles
            .get_mut("default")
            .unwrap_or_abort()
            .permission_ruleset
            .push(PermissionRule {
                permission: kind.into(),
                pattern: "*".into(),
                action: PermissionAction::Ask,
            });
        let handle = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = handle
            .start_run("secondary_paths", &workspace)
            .await
            .unwrap_or_abort();
        let actor = EventActor::new(ActorKind::Supervisor, None);
        let args = json!({"path": alias});
        let controls = if kind == "doom_loop" { 2 } else { 1 };
        for _ in 0..controls {
            let id = handle
                .request_tool_call(actor.clone(), None, "read", args.clone())
                .await
                .unwrap_or_abort();
            if kind == "external_directory" {
                let permission = requested_permission(&read_events(&run.events_path), &id);
                handle
                    .resolve_permission(permission, PermissionDecision::Allow, None)
                    .await
                    .unwrap_or_abort();
            }
            wait_for_events(&handle, &run.events_path, "approved target started", |event| matches!(&event.payload, EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == id)).await;
        }
        let blocked = handle
            .request_tool_call(actor, None, "read", args)
            .await
            .unwrap_or_abort();
        let permission = requested_permission(&read_events(&run.events_path), &blocked);
        fs::remove_file(&alias).unwrap_or_abort();
        std::os::unix::fs::symlink(workspace.join("secret/denied"), &alias).unwrap_or_abort();
        handle
            .resolve_permission(permission, PermissionDecision::Allow, None)
            .await
            .unwrap_or_abort();
        handle.stop_run().await.unwrap_or_abort();
        assert!(!read_events(&run.events_path).iter().any(|event| matches!(&event.payload, EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == blocked)), "{kind} approval used a stale target");
    }
}

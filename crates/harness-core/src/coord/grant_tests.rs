use super::*;
use crate::{clock::FakeClock, perm::PermissionGrantScope, redact::DefaultRedactor};
use serde_json::json;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio_stream::StreamExt;

#[tokio::test]
async fn grants_match_exact_requests_and_respect_run_session_workspace_lifetimes(
) -> Result<(), Box<dyn std::error::Error>> {
    for scope in [
        PermissionGrantScope::Run,
        PermissionGrantScope::Session,
        PermissionGrantScope::Workspace,
    ] {
        let temp = tempfile::tempdir()?;
        let count = Arc::new(AtomicUsize::new(0));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(super::tests::CountTool(Arc::clone(&count))));
        config.tool_registry = Arc::new(registry);
        let mut profile = AgentProfile::fallback("default");
        profile.toolset = vec!["count".into()];
        config.agent_profiles.insert("default".into(), profile);
        let make = |config| {
            spawn_coordinator(
                config,
                Arc::new(FakeClock::new()),
                Arc::new(DefaultRedactor::default()),
            )
        };
        let coordinator = make(config.clone());
        let run = coordinator.start_run("scope", temp.path()).await?;
        let agent = coordinator
            .spawn_agent_idle(
                EventActor::new(ActorKind::Supervisor, None),
                "default",
                None,
            )
            .await?;
        let actor = EventActor::new(ActorKind::Worker, Some(agent.clone()));
        let mut events = coordinator.event_store().await?.subscribe(1)?;
        let first = coordinator
            .request_tool_call(actor.clone(), None, "count", json!({"value":1}))
            .await?;
        let permission = approval(&mut events, &first).await?;
        coordinator
            .resolve_permission_with_grant_scope(
                permission,
                PermissionDecision::Allow,
                None,
                Some(scope),
            )
            .await?;
        super::history_tests::settled(&coordinator, &first).await?;
        let second = coordinator
            .request_tool_call(actor.clone(), None, "count", json!({"value":1}))
            .await?;
        super::history_tests::settled(&coordinator, &second).await?;
        assert_eq!(count.load(Ordering::SeqCst), 2);
        let different = coordinator
            .request_tool_call(actor.clone(), None, "count", json!({"value":2}))
            .await?;
        let permission = approval(&mut events, &different).await?;
        coordinator
            .resolve_permission(permission, PermissionDecision::Deny, None)
            .await?;
        #[cfg(unix)]
        if scope == PermissionGrantScope::Run {
            let link = temp.path().join("link");
            for name in ["one", "two"] {
                std::fs::write(temp.path().join(name), name)?;
            }
            std::os::unix::fs::symlink(temp.path().join("one"), &link)?;
            let id = coordinator
                .request_tool_call(actor.clone(), None, "count", json!({"path":"link"}))
                .await?;
            let permission = approval(&mut events, &id).await?;
            coordinator
                .resolve_permission_with_grant_scope(
                    permission,
                    PermissionDecision::Allow,
                    None,
                    Some(scope),
                )
                .await?;
            super::history_tests::settled(&coordinator, &id).await?;
            assert_eq!(count.load(Ordering::SeqCst), 3);
            std::fs::remove_file(&link)?;
            std::os::unix::fs::symlink(temp.path().join("two"), &link)?;
            let id = coordinator
                .request_tool_call(actor.clone(), None, "count", json!({"path":"link"}))
                .await?;
            let permission = approval(&mut events, &id).await?;
            coordinator
                .resolve_permission(permission, PermissionDecision::Deny, None)
                .await?;
            assert_eq!(count.load(Ordering::SeqCst), 3);
            let pending_link = temp.path().join("pending-link");
            std::os::unix::fs::symlink(temp.path().join("one"), &pending_link)?;
            let id = coordinator
                .request_tool_call(actor.clone(), None, "count", json!({"path":"pending-link"}))
                .await?;
            let permission = approval(&mut events, &id).await?;
            std::fs::remove_file(&pending_link)?;
            std::os::unix::fs::symlink(temp.path().join("two"), &pending_link)?;
            coordinator
                .resolve_permission(permission, PermissionDecision::Allow, None)
                .await?;
            assert!(super::history_tests::settled(&coordinator, &id)
                .await
                .is_err());
            assert_eq!(count.load(Ordering::SeqCst), 3);
        }
        coordinator.stop_run().await?;
        let baseline_count = count.load(Ordering::SeqCst);
        assert_eq!(
            temp.path()
                .join(".agent-harness/permission-grants.json")
                .exists(),
            scope == PermissionGrantScope::Workspace
        );

        for resume in [true, false] {
            let next = make(config.clone());
            let actor = if resume {
                next.resume_run(run.run_id.to_string(), "continued").await?;
                actor.clone()
            } else {
                next.start_run("other", temp.path()).await?;
                let agent = next
                    .spawn_agent_idle(
                        EventActor::new(ActorKind::Supervisor, None),
                        "default",
                        None,
                    )
                    .await?;
                EventActor::new(ActorKind::Worker, Some(agent))
            };
            let store = next.event_store().await?;
            let mut events = store.subscribe(
                crate::store::read_events(&next.run_info().await?.events_path)?
                    .last()
                    .map_or(1, |e| e.seq + 1),
            )?;
            let id = next
                .request_tool_call(actor, None, "count", json!({"value":1}))
                .await?;
            if scope == PermissionGrantScope::Workspace
                || (resume && scope == PermissionGrantScope::Session)
            {
                super::history_tests::settled(&next, &id).await?;
            } else {
                let permission = approval(&mut events, &id).await?;
                next.resolve_permission(permission, PermissionDecision::Deny, None)
                    .await?;
            }
            next.stop_run().await?;
        }
        let expected = baseline_count
            + match scope {
                PermissionGrantScope::Run => 0,
                PermissionGrantScope::Session => 1,
                PermissionGrantScope::Workspace => 2,
            };
        assert_eq!(count.load(Ordering::SeqCst), expected);
        config.permission_policy =
            PermissionPolicy::from_rules(vec![crate::perm::PermissionRule {
                permission: "*".into(),
                pattern: "*".into(),
                action: crate::perm::PermissionAction::Deny,
            }])?;
        let denied = make(config);
        denied.resume_run(run.run_id.to_string(), "denied").await?;
        denied.set_always_approve_mode(true).await?;
        denied
            .request_tool_call(actor, None, "count", json!({"value":1}))
            .await?;
        denied.stop_run().await?;
        assert_eq!(count.load(Ordering::SeqCst), expected);
    }
    Ok(())
}

#[tokio::test]
async fn always_approve_releases_pending_tools_but_keeps_sensitive_requests_interactive(
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
    coordinator.start_run("toggle", temp.path()).await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    let mut completed = 0;
    for already_enabled in [false, true] {
        for (path, allowed) in [
            (temp.path().join("ordinary.txt"), true),
            (temp.path().join(".env"), false),
            (outside.path().join("outside.txt"), false),
        ] {
            let external = !path.starts_with(temp.path());
            coordinator.set_always_approve_mode(already_enabled).await?;
            let id = coordinator
                .request_tool_call(
                    EventActor::new(ActorKind::User, None),
                    None,
                    "count",
                    json!({"path":path}),
                )
                .await?;
            let mut permission = if !already_enabled || !allowed {
                Some(approval(&mut events, &id).await?)
            } else {
                None
            };
            coordinator.set_always_approve_mode(true).await?;
            if external && !already_enabled {
                permission = Some(approval(&mut events, &id).await?);
            }
            if allowed {
                super::history_tests::settled(&coordinator, &id).await?;
                completed += 1;
            } else {
                coordinator
                    .resolve_permission(
                        permission.ok_or("missing approval")?,
                        PermissionDecision::Deny,
                        None,
                    )
                    .await?;
            }
            assert_eq!(count.load(Ordering::SeqCst), completed);
        }
    }
    coordinator.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn repeated_calls_require_approval_and_a_remembered_grant_cannot_override_role_denial(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::perm::{PermissionAction, PermissionRule};
    let temp = tempfile::tempdir()?;
    let count = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(super::tests::CountTool(Arc::clone(&count))));
    let rule = |permission: &str, action| PermissionRule {
        permission: permission.into(),
        pattern: "*".into(),
        action,
    };
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::from_rules(vec![
        rule("*", PermissionAction::Allow),
        rule("doom_loop", PermissionAction::Ask),
    ])?;
    let mut profile = AgentProfile::fallback("blocked");
    profile.toolset = vec!["count".into()];
    profile.permission_ruleset = vec![rule("doom_loop", PermissionAction::Deny)];
    config.agent_profiles.insert("blocked".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("repeated", temp.path()).await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    let actor = EventActor::new(ActorKind::User, None);
    for call in 1..=9 {
        let id = coordinator
            .request_tool_call(actor.clone(), None, "count", json!({}))
            .await?;
        if call == 3 || call == 6 {
            let permission = approval(&mut events, &id).await?;
            coordinator.set_always_approve_mode(true).await?;
            assert_eq!(count.load(Ordering::SeqCst), call - 1);
            coordinator
                .resolve_permission_with_grant_scope(
                    permission,
                    PermissionDecision::Allow,
                    None,
                    (call == 6).then_some(PermissionGrantScope::Run),
                )
                .await?;
        }
        super::history_tests::settled(&coordinator, &id).await?;
        assert_eq!(count.load(Ordering::SeqCst), call);
    }
    let blocked = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "blocked",
            None,
        )
        .await?;
    let id = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(blocked)),
            None,
            "count",
            json!({}),
        )
        .await?;
    assert!(super::history_tests::settled(&coordinator, &id)
        .await
        .is_err());
    assert_eq!(count.load(Ordering::SeqCst), 9);
    coordinator.stop_run().await?;
    coordinator.start_run("new run", temp.path()).await?;
    let id = coordinator
        .request_tool_call(actor, None, "count", json!({}))
        .await?;
    super::history_tests::settled(&coordinator, &id).await?;
    assert_eq!(count.load(Ordering::SeqCst), 10);
    coordinator.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn remembered_path_grants_cover_the_approved_target_or_external_parent_only(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::perm::{PermissionAction, PermissionRule};
    for (external, root_grant) in [(false, false), (true, false), (true, true)] {
        let temp = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let count = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(super::tests::CountTool(Arc::clone(&count))));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.tool_registry = Arc::new(registry);
        config.permission_policy = PermissionPolicy::from_rules(vec![
            PermissionRule {
                permission: "*".into(),
                pattern: "*".into(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: if external {
                    "external_directory"
                } else {
                    "count"
                }
                .into(),
                pattern: "*".into(),
                action: PermissionAction::Ask,
            },
        ])?;
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator.start_run("paths", temp.path()).await?;
        let mut events = coordinator.event_store().await?.subscribe(1)?;
        let actor = EventActor::new(ActorKind::User, None);
        let first = if root_grant {
            PathBuf::from(std::path::MAIN_SEPARATOR_STR)
        } else if external {
            outside.path().join("approved/one")
        } else {
            temp.path().join("one")
        };
        std::fs::create_dir_all(first.parent().unwrap_or(&first))?;
        for step in 0..3 {
            let reused = step == 1 && !root_grant;
            let decision = if step == 0 {
                PermissionDecision::Allow
            } else {
                PermissionDecision::Deny
            };
            let kind = if external {
                "external_directory"
            } else {
                "count"
            };
            let path = match (external, step) {
                (true, 1) => outside.path().join("approved/two"),
                (true, 2) => outside.path().join("approved-other/file"),
                (false, 2) => temp.path().join("different"),
                _ => first.clone(),
            };
            let id = coordinator
                .request_tool_call(
                    actor.clone(),
                    None,
                    "count",
                    json!({"path":path,"offset":step+1}),
                )
                .await?;
            if !reused {
                let permission = approval(&mut events, &id).await?;
                let journal =
                    crate::store::read_events(&coordinator.run_info().await?.events_path)?;
                assert!(journal.iter().any(|e| matches!(&e.payload, EventV1::PermissionRequested(p) if p.permission_id == permission && p.kind == kind)));
                coordinator
                    .resolve_permission_with_grant_scope(
                        permission,
                        decision,
                        None,
                        (step == 0).then_some(PermissionGrantScope::Run),
                    )
                    .await?;
            }
            if step == 0 || reused {
                super::history_tests::settled(&coordinator, &id).await?;
            }
            assert_eq!(
                count.load(Ordering::SeqCst),
                if root_grant { 1 } else { (step + 1).min(2) }
            );
        }
        coordinator.stop_run().await?;
    }
    Ok(())
}

async fn approval(
    events: &mut crate::store::EventStream,
    task: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            if let EventV1::PermissionRequested(request) = event?.payload {
                if request
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| id.as_str() == task)
                {
                    return Ok(request.permission_id);
                }
            }
        }
        Err("approval missing".into())
    })
    .await?
}

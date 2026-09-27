use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use serde_json::json;
use std::sync::Arc;
use tokio_stream::StreamExt;

#[tokio::test]
async fn todos_commit_valid_lists_and_follow_resume_and_rewind(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::allow_all();
    config.secret_values = vec!["opaque-todo-credential".into()];
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec!["todowrite".into(), "todoread".into()];
    let mut read_only = profile.clone();
    read_only.name = "read_only".into();
    read_only.permission_ruleset = vec![PermissionRule {
        permission: "todowrite".into(),
        pattern: "*".into(),
        action: PermissionAction::Deny,
    }];
    config.agent_profiles.insert("read_only".into(), read_only);
    config.agent_profiles.insert("default".into(), profile);
    let handle = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle.start_run("todos", root.path()).await?;
    let agent = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(agent.clone()));
    let first = handle.execute_agent_tool_call(actor.clone(), None, "todowrite", json!({"todos":[
        {"content":"Review opaque-todo-credential","status":"in_progress","priority":"high"},
        {"text":"Verify"}
    ]})).await?;
    assert!(!first.display_text.contains("opaque-todo-credential"));
    let initial = first.structured_json.ok_or("todo result missing")?["todos"].clone();
    assert_eq!(initial[1]["status"], "pending");
    let read_only = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "read_only", None)
        .await?;
    assert!(handle
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(read_only)),
            None,
            "todowrite",
            json!({"todos":[]})
        )
        .await
        .is_err_and(|e| e.contains("permission denied")));
    for todos in [
        json!([{"content":"a","status":"in_progress"},{"content":"b","status":"in_progress"}]),
        json!([{"content":"a","status":"unknown"}]),
        json!([{"content":"a","priority":"unknown"}]),
        json!([{"content":" "}]),
        json!([{"content":"a".repeat(40_000)}]),
    ] {
        assert!(handle
            .execute_agent_tool_call(actor.clone(), None, "todowrite", json!({"todos":todos}))
            .await
            .is_err());
    }
    let output = handle
        .execute_agent_tool_call(actor.clone(), None, "todoread", json!({}))
        .await?;
    assert_eq!(
        output.structured_json.ok_or("todo result missing")?["todos"],
        initial
    );
    let mut events = handle.subscribe_new_events().await?;
    let request = handle
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "A new plan")
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(e) if e.task_id.as_str() == request => {
                    return Ok::<_, Box<dyn std::error::Error>>(())
                }
                EventV1::TaskCancelled(e) if e.task_id.as_str() == request => {
                    return Err(e.reason.into())
                }
                _ => {}
            }
        }
        Err("turn did not finish".into())
    })
    .await??;
    handle
        .execute_agent_tool_call(actor.clone(), None, "todowrite", json!({"todos":[]}))
        .await?;
    handle.stop_run().await?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed.resume_run(run.run_id.to_string(), "todos").await?;
    let empty = resumed
        .execute_agent_tool_call(actor.clone(), None, "todoread", json!({}))
        .await?;
    assert_eq!(
        empty.structured_json.ok_or("todo result missing")?["todos"],
        json!([])
    );
    resumed.rewind_conversation(request).await?;
    let restored = resumed
        .execute_agent_tool_call(actor, None, "todoread", json!({}))
        .await?;
    assert_eq!(
        restored.structured_json.ok_or("todo result missing")?["todos"],
        initial
    );
    resumed.stop_run().await?;
    assert!(!std::fs::read_to_string(&run.events_path)?.contains("opaque-todo-credential"));
    assert!(!run.run_dir.join("todos.json").exists());
    Ok(())
}

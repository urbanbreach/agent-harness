use super::*;

#[tokio::test]
async fn spawn_approval_precedes_child_work_and_resume_rechecks_policy(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([vec![
        Stream::TextDelta("approved report".into()),
        done(),
    ]]));
    let mut config = config(temp.path(), Arc::<MockProvider>::clone(&provider));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "task".into(),
            pattern: "general-purpose".into(),
            action: PermissionAction::Ask,
        },
    ])?;
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("spawn approval", temp.path()).await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    let mut events = coordinator.subscribe_new_events().await?;
    let call = {
        let (coordinator, actor) = (coordinator.clone(), actor.clone());
        tokio::spawn(async move {
            coordinator
                .execute_agent_tool_call(
                    actor,
                    None,
                    "spawn_subagent",
                    json!({"prompt":"Inspect the change.","description":"Review change","background":false}),
                )
                .await
        })
    };
    let permission = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            if let EventV1::PermissionRequested(permission) = event?.payload {
                return Ok::<_, harness_core::store::EventStoreError>(permission.permission_id);
            }
        }
        Err(harness_core::store::EventStoreError::Invalid(
            "missing approval",
        ))
    })
    .await??;
    assert_eq!(provider.call_count(), 0);
    assert!(coordinator.subagent_history().await?.records.is_empty());
    coordinator
        .resolve_permission(permission, PermissionDecision::Allow, None)
        .await?;
    let result = tokio::time::timeout(Duration::from_secs(3), call).await???;
    let child = child_id(result.structured_json.as_ref().ok_or("completion data")?)?;
    assert_eq!(provider.captured_requests().await[0].model_id, "chosen");
    coordinator.stop_run().await?;

    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "task".into(),
            pattern: "general-purpose".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "policy recheck")
        .await?;
    assert!(resumed
        .execute_agent_tool_call(
            actor,
            None,
            "spawn_subagent",
            json!({"resume_from":child,"prompt":"Forbidden resume.","description":"Resume change","background":false}),
        )
        .await
        .is_err());
    assert_eq!(provider.call_count(), 1);
    resumed.stop_run().await?;
    Ok(())
}

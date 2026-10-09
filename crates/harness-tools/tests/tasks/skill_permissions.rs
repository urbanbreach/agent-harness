use super::*;

#[tokio::test]
async fn skill_approval_and_metadata_cannot_grant_a_child_denied_write(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let skill = temp.path().join(".harness/skills/review");
    std::fs::create_dir_all(&skill)?;
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\nallowed_tools: [write]\n---\nfixture-skill-marker\n",
    )?;
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "denied-child-write".into(),
                function_name: "write".into(),
                arguments_json: json!({"filePath":"forbidden.txt","content":"must not be created"})
                    .to_string(),
            },
            done(),
        ],
        vec![Stream::TextDelta("permission report".into()), done()],
    ]));
    let mut config = config(temp.path(), Arc::<MockProvider>::clone(&provider));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "skill".into(),
            pattern: "review".into(),
            action: PermissionAction::Ask,
        },
        PermissionRule {
            permission: "edit".into(),
            pattern: "*".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("child skill policy", temp.path())
        .await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    let mut events = coordinator.subscribe_new_events().await?;
    let skill_call = {
        let (coordinator, actor) = (coordinator.clone(), actor.clone());
        tokio::spawn(async move {
            coordinator
                .execute_agent_tool_call(actor, None, "skill", json!({"name":"review"}))
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
            "skill approval missing",
        ))
    })
    .await??;
    assert_eq!(provider.call_count(), 0);
    coordinator
        .resolve_permission(permission, PermissionDecision::Allow, None)
        .await?;
    tokio::time::timeout(Duration::from_secs(3), skill_call).await???;
    coordinator
        .execute_agent_tool_call(
            actor,
            None,
            "spawn_subagent",
            json!({"prompt":"Write note.","description":"Check denied write","background":false,
            "load_skills":["review"],"capability_mode":"all","sender":"root"}),
        )
        .await?;
    assert!(!temp.path().join("forbidden.txt").exists());
    let requests = provider.captured_requests().await;
    assert!(requests[0]
        .messages
        .iter()
        .all(|message| { !message.content.contains("fixture-skill-marker") }));
    assert!(!requests[0]
        .tools
        .as_ref()
        .ok_or("child tools")?
        .iter()
        .any(|tool| tool.tool_id == "write"));
    let denied = tokio::time::timeout(Duration::from_secs(3), async {
        let mut write_id = None;
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::ToolCallRequested(tool) if tool.tool_id == "write" => {
                    write_id = Some(tool.tool_call_id);
                }
                EventV1::ToolCallFinished(tool)
                    if Some(&tool.tool_call_id) == write_id.as_ref() =>
                {
                    return Ok::<_, harness_core::store::EventStoreError>(tool.status);
                }
                _ => {}
            }
        }
        Err(harness_core::store::EventStoreError::Invalid(
            "denied write result missing",
        ))
    })
    .await??;
    assert_eq!(denied, harness_core::event::ToolCallStatus::Failed);
    coordinator.stop_run().await?;
    Ok(())
}

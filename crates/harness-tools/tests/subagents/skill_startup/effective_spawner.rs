use super::*;

#[tokio::test]
async fn skill_startup_uses_effective_spawner_snapshot_for_root_and_nested_requests(
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio_stream::StreamExt;
    let temp = tempfile::tempdir()?;
    let project = temp.path().join("project");
    let spawner_cwd = project.join("spawner");
    let leaf_cwd = project.join("leaf");
    fs::create_dir_all(&leaf_cwd)?;
    write_skill(
        &project.join("custom/rootonly"),
        "rootonly",
        "ROOT_ONLY_BODY",
    )?;
    write_skill(
        &spawner_cwd.join("custom/spawneronly"),
        "spawneronly",
        "SPAWNER_BODY",
    )?;
    write_skill(
        &spawner_cwd.join(".agent-harness/skills/local"),
        "local",
        "PARENT_LOCAL_BODY",
    )?;
    let secret = "skill-description-private-marker";
    fs::write(
        spawner_cwd.join("custom/spawneronly/SKILL.md"),
        format!("---\nname: spawneronly\ndescription: spawner {secret}\n---\nSPAWNER_BODY\n"),
    )?;
    let skills = SkillsConfig {
        project_roots: vec!["custom".into()],
        global_roots: vec![],
        walk_to_git_root: false,
        ..Default::default()
    };
    let mut definitions = SubagentDefinitionSnapshot::default();
    for (name, inherit, tools) in [
        (
            "skill-parent",
            false,
            vec!["Skill".into(), "Agent(skill-leaf)".into()],
        ),
        ("skill-leaf", true, vec!["Skill".into()]),
    ] {
        definitions.cli.insert(
            name.into(),
            SubagentDefinition {
                name: name.into(),
                description: name.into(),
                inherit_skills: inherit,
                tools,
                inject_default_tools: false,
                ..Default::default()
            },
        );
    }
    let provider = Arc::new(MockProvider::script((0..3).map(|_| {
        vec![
            Stream::TextDelta("done".into()),
            Stream::Done { usage: None },
        ]
    })));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.skills = skills.clone();
    config.skill_catalog_discovery = Some(Arc::new(harness_tools::NativeSkillCatalogDiscovery));
    config.subagents.max_depth = 2;
    config.secret_values = vec![secret.into()];
    let mut registry =
        harness_tools::coordinator_registry_with_skills(ShellAllowlist::default(), skills);
    harness_tools::register_subagent_tools(&mut registry, &config.subagents, &definitions, None);
    let mut root_profile = AgentProfile::fallback("default");
    root_profile.toolset = registry.tool_ids();
    config.agent_profiles.insert("default".into(), root_profile);
    config.tool_registry = Arc::new(registry);
    config.subagent_definitions = Some(definitions);
    config.permission_policy = PermissionPolicy::allow_all();
    config.provider = Arc::<MockProvider>::clone(&provider);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("effective spawner skills", &project)
        .await?;
    let root = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    coordinator
        .set_agent_execution_cwd(
            EventActor::new(ActorKind::System, None),
            root.clone(),
            spawner_cwd.clone(),
        )
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let request = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, Some(root.clone())),
            root.clone(),
            "inspect skills",
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            let event = event?;
            if matches!(event.payload, harness_core::event::EventV1::TaskCompleted(ref completed)
                if completed.task_id.as_str() == request)
            {
                return Ok::<_, Box<dyn std::error::Error>>(());
            }
        }
        Err("root completion event stream closed".into())
    })
    .await??;
    let parent = coordinator.execute_agent_tool_call(
        EventActor::new(ActorKind::Worker, Some(root)), None, "spawn_subagent",
        json!({"prompt":"inspect local skills","description":"Local catalog parent","subagent_type":"skill-parent","background":false,"cwd":spawner_cwd}),
    ).await?.structured_json.ok_or("parent result")?["subagent_id"]
        .as_str().ok_or("parent ID")?.to_owned();
    coordinator.execute_agent_tool_call(
        EventActor::new(ActorKind::Worker, Some(parent)), None, "spawn_subagent",
        json!({"prompt":"inspect inherited local skills","description":"Inherited local leaf","subagent_type":"skill-leaf","background":false,"cwd":leaf_cwd}),
    ).await?;
    let requests = provider.captured_requests().await;
    let run = coordinator.run_info().await?;
    coordinator.stop_run().await?;
    let root_metadata = available_skills(requests.first().ok_or("root request")?)?;
    assert!(root_metadata
        .iter()
        .any(|entry| entry["name"] == "spawneronly"));
    assert!(!root_metadata
        .iter()
        .any(|entry| entry["name"] == "rootonly"));
    for request in requests.iter().skip(1) {
        let metadata = available_skills(request)?;
        assert!(metadata.iter().any(|entry| entry["name"] == "local"));
        assert!(!metadata
            .iter()
            .any(|entry| entry["name"] == "spawneronly" || entry["name"] == "rootonly"));
    }
    assert_eq!(requests.len(), 3);
    assert!(!serde_json::to_string(&requests)?.contains(secret));
    assert!(!fs::read_to_string(run.events_path)?.contains(secret));
    Ok(())
}

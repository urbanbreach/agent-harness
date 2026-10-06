use super::*;
use harness_core::{event::EventV1, perm::PermissionDecision};
use tokio_stream::StreamExt;

#[tokio::test]
async fn native_skill_preload_waits_for_shared_permission_before_sampling(
) -> Result<(), Box<dyn std::error::Error>> {
    for decision in [PermissionDecision::Allow, PermissionDecision::Deny] {
        check_preload_permission(decision).await?;
    }
    Ok(())
}

async fn check_preload_permission(
    decision: PermissionDecision,
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    write_skill(
        &temp.path().join("skills/review"),
        "review",
        "APPROVED_PRELOAD_BODY",
    )?;
    let skills = SkillsConfig {
        project_roots: vec!["skills".into()],
        global_roots: vec![],
        walk_to_git_root: false,
        permissions: std::collections::BTreeMap::from([("review".into(), PermissionMode::Ask)]),
        ..Default::default()
    };
    let provider = Arc::new(MockProvider::default());
    let registry =
        harness_tools::coordinator_registry_with_skills(ShellAllowlist::default(), skills.clone());
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.skills = skills;
    config.skill_catalog_discovery = Some(Arc::new(CountedDiscovery::default()));
    config.permission_policy = PermissionPolicy::allow_all();
    config.provider = Arc::<MockProvider>::clone(&provider);
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = registry.tool_ids();
    config.tool_registry = Arc::new(registry);
    config.agent_profiles.insert("default".into(), parent);
    config.subagent_definitions = Some(SubagentDefinitionSnapshot {
        cli: std::collections::BTreeMap::from([(
            "preload-reviewer".into(),
            SubagentDefinition {
                name: "preload-reviewer".into(),
                description: "Reviewer".into(),
                tools: vec!["Skill".into()],
                skills: vec!["REVIEW".into()],
                inject_default_tools: false,
                ..Default::default()
            },
        )]),
        ..Default::default()
    });
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("preload permission", temp.path())
        .await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    let spawn = coordinator.execute_agent_tool_call(EventActor::new(ActorKind::Worker, Some(parent)), None,
        "spawn_subagent", json!({"prompt":"review","description":"Review","subagent_type":"preload-reviewer","background":false}));
    let approve = async {
        while let Some(event) = events.next().await {
            let EventV1::PermissionRequested(request) = event?.payload else {
                continue;
            };
            assert_eq!(request.kind, "skill");
            assert!(
                provider.captured_requests().await.is_empty(),
                "child sampled before preload approval"
            );
            coordinator
                .resolve_permission(request.permission_id, decision, None)
                .await?;
            return Ok::<_, Box<dyn std::error::Error>>(());
        }
        Err("missing preload approval".into())
    };
    let (spawned, approved) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(spawn, approve)
    })
    .await?;
    spawned?;
    approved?;
    let requests = provider.captured_requests().await;
    let first = requests.first().ok_or("missing post-approval sample")?;
    assert_eq!(
        serde_json::to_string(first)?.contains("APPROVED_PRELOAD_BODY"),
        decision == PermissionDecision::Allow
    );
    coordinator.stop_run().await?;
    Ok(())
}

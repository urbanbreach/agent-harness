#[tokio::test]
async fn coordinator_projects_lead_roles_bounds_and_delayed_member_activation() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = CoordinatorConfig::new(tempdir.path());
    config.agent_profiles = agent_profiles();
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle
        .start_run("team-run", tempdir.path())
        .await
        .expect("start run");
    let actor = EventActor::new(ActorKind::Supervisor, None);

    let created = handle
        .create_team(
            actor.clone(),
            TeamSpec {
                lead: Some(TeamMemberSelector::SubagentType {
                    subagent_type: "general".to_string(),
                }),
                bounds: TeamBounds {
                    max_members: 2,
                    max_parallel_members: 1,
                    ..TeamBounds::default()
                },
                ..team_spec()
            },
            Some("team_lead_bounds".to_string()),
        )
        .await
        .expect("create team");
    let lead = created.lead.as_ref().expect("lead projection");
    assert_eq!(lead.status, TeamMemberStatus::Running);
    assert_eq!(lead.profile.as_deref(), Some("general"));
    assert_eq!(created.members["alpha"].status, TeamMemberStatus::Running);
    assert_eq!(created.members["beta"].status, TeamMemberStatus::Pending);
    assert_eq!(created.bounds_consumption.running_members, 1);
    assert_eq!(created.bounds_consumption.pending_members, 1);

    let lead_actor = EventActor::new(ActorKind::Worker, lead.agent_id.clone());
    handle
        .send_team_message(
            lead_actor,
            "team_lead_bounds",
            TeamMessage {
                version: 1,
                message_id: "lead_broadcast".to_string(),
                from: "lead".to_string(),
                to: "*".to_string(),
                kind: TeamMessageKind::Announcement,
                body: "lead can coordinate".to_string(),
                summary: None,
                references: Vec::new(),
                correlation_id: None,
            },
        )
        .await
        .expect("projected lead can act as lead");

    handle
        .request_team_shutdown(actor.clone(), "team_lead_bounds", "alpha", "lead")
        .await
        .expect("request alpha shutdown");
    let activated = handle
        .approve_team_shutdown(actor, "team_lead_bounds", "alpha", "alpha")
        .await
        .expect("approving alpha activates beta");
    assert_eq!(
        activated.members["alpha"].status,
        TeamMemberStatus::ShutdownApproved
    );
    assert_eq!(activated.members["beta"].status, TeamMemberStatus::Running);
    assert!(activated.members["beta"].agent_id.is_some());
}
#[tokio::test]
async fn coordinator_allows_research_members_but_blocks_team_writes() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = CoordinatorConfig::new(tempdir.path());
    config.agent_profiles = agent_profiles();
    config
        .agent_profiles
        .insert("explore".to_string(), profile("explore"));
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle
        .start_run("team-run", tempdir.path())
        .await
        .expect("start run");
    let actor = EventActor::new(ActorKind::Supervisor, None);

    let created = handle
        .create_team(
            actor.clone(),
            TeamSpec {
                members: vec![TeamMemberSpec {
                    name: "research".to_string(),
                    role: TeamMemberRole::Research,
                    selector: TeamMemberSelector::SubagentType {
                        subagent_type: "explore".to_string(),
                    },
                    prompt: None,
                }],
                ..team_spec()
            },
            Some("team_research".to_string()),
        )
        .await
        .expect("research member can join team");
    let research_agent = created.members["research"]
        .agent_id
        .clone()
        .expect("research agent id");
    assert_eq!(created.members["research"].role, TeamMemberRole::Research);

    let research_actor = EventActor::new(ActorKind::Worker, Some(research_agent));
    let write = handle
        .send_team_message(
            research_actor.clone(),
            "team_research",
            TeamMessage {
                version: 1,
                message_id: "research_write".to_string(),
                from: "research".to_string(),
                to: "lead".to_string(),
                kind: TeamMessageKind::Message,
                body: "result".to_string(),
                summary: None,
                references: Vec::new(),
                correlation_id: None,
            },
        )
        .await
        .expect_err("research role cannot write mailbox");
    assert!(write.to_string().contains("research team member"));

    handle
        .request_team_shutdown(
            research_actor.clone(),
            "team_research",
            "research",
            "research",
        )
        .await
        .expect("research member can request own shutdown");
    handle
        .approve_team_shutdown(research_actor, "team_research", "research", "research")
        .await
        .expect("research member can acknowledge own shutdown");
}
#[tokio::test]
async fn coordinator_enforces_runtime_bounds_and_shutdown_approved_write_gate() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = CoordinatorConfig::new(tempdir.path());
    config.agent_profiles = agent_profiles();
    let clock = Arc::new(FakeClock::new());
    let handle = spawn_coordinator(config, clock.clone(), Arc::new(DefaultRedactor::default()));
    handle
        .start_run("team-run", tempdir.path())
        .await
        .expect("start run");
    let actor = EventActor::new(ActorKind::Supervisor, None);

    let created = handle
        .create_team(
            actor.clone(),
            TeamSpec {
                bounds: TeamBounds {
                    max_member_turns: 1,
                    max_wall_clock_minutes: 1,
                    ..TeamBounds::default()
                },
                ..team_spec()
            },
            Some("team_runtime_bounds".to_string()),
        )
        .await
        .expect("create team");
    let alpha_actor = EventActor::new(ActorKind::Worker, created.members["alpha"].agent_id.clone());

    handle
        .send_team_message(
            alpha_actor.clone(),
            "team_runtime_bounds",
            TeamMessage {
                version: 1,
                message_id: "turn_1".to_string(),
                from: "alpha".to_string(),
                to: "beta".to_string(),
                kind: TeamMessageKind::Message,
                body: "one turn".to_string(),
                summary: None,
                references: Vec::new(),
                correlation_id: None,
            },
        )
        .await
        .expect("first member turn allowed");
    let too_many_turns = handle
        .send_team_message(
            alpha_actor.clone(),
            "team_runtime_bounds",
            TeamMessage {
                version: 1,
                message_id: "turn_2".to_string(),
                from: "alpha".to_string(),
                to: "beta".to_string(),
                kind: TeamMessageKind::Message,
                body: "second turn".to_string(),
                summary: None,
                references: Vec::new(),
                correlation_id: None,
            },
        )
        .await
        .expect_err("member turn bound blocks extra work");
    assert!(too_many_turns.to_string().contains("max_member_turns"));

    handle
        .request_team_shutdown(actor.clone(), "team_runtime_bounds", "alpha", "lead")
        .await
        .expect("request alpha shutdown");
    handle
        .approve_team_shutdown(actor.clone(), "team_runtime_bounds", "alpha", "alpha")
        .await
        .expect("shutdown approval allowed after member turn bound");
    let approved_write = handle
        .send_team_message(
            alpha_actor,
            "team_runtime_bounds",
            TeamMessage {
                version: 1,
                message_id: "after_shutdown".to_string(),
                from: "alpha".to_string(),
                to: "beta".to_string(),
                kind: TeamMessageKind::Message,
                body: "should fail".to_string(),
                summary: None,
                references: Vec::new(),
                correlation_id: None,
            },
        )
        .await
        .expect_err("shutdown-approved member cannot write");
    assert!(approved_write.to_string().contains("shutdown-approved"));

    clock.advance(60_000);
    let deadline_write = handle
        .send_team_message(
            actor.clone(),
            "team_runtime_bounds",
            TeamMessage {
                version: 1,
                message_id: "after_deadline".to_string(),
                from: "lead".to_string(),
                to: "beta".to_string(),
                kind: TeamMessageKind::Message,
                body: "should fail".to_string(),
                summary: None,
                references: Vec::new(),
                correlation_id: None,
            },
        )
        .await
        .expect_err("wall clock bound blocks non-shutdown writes");
    assert!(deadline_write
        .to_string()
        .contains("max_wall_clock_minutes"));

    handle
        .request_team_shutdown(actor.clone(), "team_runtime_bounds", "beta", "lead")
        .await
        .expect("shutdown request allowed after deadline");
    handle
        .approve_team_shutdown(actor, "team_runtime_bounds", "beta", "beta")
        .await
        .expect("shutdown approval allowed after deadline");
}

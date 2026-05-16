use std::collections::BTreeMap;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::ToolFailureMode;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{
    ActorKind, EventActor, EventV1, TeamBounds, TeamMemberRole, TeamMemberSelector, TeamMemberSpec,
    TeamMessage, TeamMessageKind, TeamSpec, TeamTask, TeamTaskStatus,
};
use harness_core::proj::{project_team_state, TeamMemberStatus, TeamRunStatus};
use harness_core::redact::DefaultRedactor;

mod common;

use common::load_events;

#[tokio::test]
async fn coordinator_team_lifecycle_is_event_sourced_and_enforces_blockers() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = CoordinatorConfig::new(tempdir.path());
    config.run_id_override = Some("run_team".to_string());
    config.agent_profiles = agent_profiles();
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("team-run", tempdir.path())
        .await
        .expect("start run");
    let actor = EventActor::new(ActorKind::Supervisor, None);

    let created = handle
        .create_team(actor.clone(), team_spec(), Some("team_alpha".to_string()))
        .await
        .expect("create team");
    assert_eq!(created.members.len(), 2);
    assert!(created.members["alpha"].agent_id.is_some());

    handle
        .send_team_message(
            actor.clone(),
            "team_alpha",
            TeamMessage {
                version: 1,
                message_id: "msg_1".to_string(),
                from: "lead".to_string(),
                to: "alpha".to_string(),
                kind: TeamMessageKind::Message,
                body: "Please take task two after task one completes.".to_string(),
                summary: Some("task handoff".to_string()),
                references: Vec::new(),
                correlation_id: Some("corr_1".to_string()),
            },
        )
        .await
        .expect("send message");
    let duplicate_message = handle
        .send_team_message(
            actor.clone(),
            "team_alpha",
            TeamMessage {
                version: 1,
                message_id: "msg_1".to_string(),
                from: "lead".to_string(),
                to: "alpha".to_string(),
                kind: TeamMessageKind::Message,
                body: "duplicate".to_string(),
                summary: None,
                references: Vec::new(),
                correlation_id: None,
            },
        )
        .await
        .expect_err("duplicate message ids are rejected");
    assert!(duplicate_message.to_string().contains("already exists"));

    handle
        .create_team_task(actor.clone(), "team_alpha", team_task("task_1", Vec::new()))
        .await
        .expect("create first task");
    handle
        .create_team_task(
            actor.clone(),
            "team_alpha",
            team_task("task_2", vec!["task_1".to_string()]),
        )
        .await
        .expect("create blocked task");

    let blocked = handle
        .update_team_task(
            actor.clone(),
            "team_alpha",
            "task_2",
            TeamTaskStatus::Claimed,
            Some("alpha".to_string()),
            BTreeMap::new(),
        )
        .await
        .expect_err("blocked task cannot be claimed");
    assert!(blocked.to_string().contains("blocked by incomplete tasks"));

    handle
        .update_team_task(
            actor.clone(),
            "team_alpha",
            "task_1",
            TeamTaskStatus::Completed,
            Some("alpha".to_string()),
            BTreeMap::new(),
        )
        .await
        .expect("complete blocker");
    handle
        .update_team_task(
            actor.clone(),
            "team_alpha",
            "task_2",
            TeamTaskStatus::Claimed,
            Some("alpha".to_string()),
            BTreeMap::new(),
        )
        .await
        .expect("claim unblocked task");

    handle
        .request_team_shutdown(actor.clone(), "team_alpha", "beta", "lead")
        .await
        .expect("request beta shutdown");
    handle
        .reject_team_shutdown(actor.clone(), "team_alpha", "beta", "beta", "not done")
        .await
        .expect("reject beta shutdown");
    handle
        .request_team_shutdown(actor.clone(), "team_alpha", "alpha", "lead")
        .await
        .expect("request alpha shutdown");
    handle
        .approve_team_shutdown(actor.clone(), "team_alpha", "alpha", "alpha")
        .await
        .expect("approve alpha shutdown");
    handle
        .request_team_shutdown(actor.clone(), "team_alpha", "beta", "lead")
        .await
        .expect("request beta shutdown again");
    handle
        .approve_team_shutdown(actor.clone(), "team_alpha", "beta", "beta")
        .await
        .expect("approve beta shutdown");
    let deleted = handle
        .delete_team(actor, "team_alpha")
        .await
        .expect("delete team");
    assert_eq!(deleted.status, TeamRunStatus::Deleted);

    let events = load_events(&run.events_path);
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::TeamCreated(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::TeamMemberSpawned(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::TeamDeleted(_))));
    let projection = project_team_state(events.iter()).expect("project team state");
    assert_eq!(projection.teams["team_alpha"].messages.len(), 1);
    assert_eq!(
        projection.teams["team_alpha"].tasks["task_2"]
            .owner
            .as_deref(),
        Some("alpha")
    );
    assert_eq!(
        projection.teams["team_alpha"].tasks["task_1"].blocks,
        vec!["task_2".to_string()]
    );
}

#[tokio::test]
async fn coordinator_rejects_read_only_team_member_profiles() {
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

    let err = handle
        .create_team(
            EventActor::new(ActorKind::Supervisor, None),
            TeamSpec {
                members: vec![TeamMemberSpec {
                    name: "research".to_string(),
                    role: TeamMemberRole::Member,
                    selector: TeamMemberSelector::SubagentType {
                        subagent_type: "explore".to_string(),
                    },
                    prompt: None,
                }],
                ..team_spec()
            },
            Some("team_readonly".to_string()),
        )
        .await
        .expect_err("read-only profiles are not team members");
    assert!(err.to_string().contains("read-only"));
}

#[tokio::test]
async fn coordinator_team_create_preflights_members_before_events() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = CoordinatorConfig::new(tempdir.path());
    config.run_id_override = Some("run_team_preflight".to_string());
    config.agent_profiles = agent_profiles();
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("team-run", tempdir.path())
        .await
        .expect("start run");

    let err = handle
        .create_team(
            EventActor::new(ActorKind::Supervisor, None),
            TeamSpec {
                members: vec![
                    TeamMemberSpec {
                        name: "alpha".to_string(),
                        role: TeamMemberRole::Member,
                        selector: TeamMemberSelector::SubagentType {
                            subagent_type: "alpha".to_string(),
                        },
                        prompt: None,
                    },
                    TeamMemberSpec {
                        name: "missing".to_string(),
                        role: TeamMemberRole::Member,
                        selector: TeamMemberSelector::SubagentType {
                            subagent_type: "missing".to_string(),
                        },
                        prompt: None,
                    },
                ],
                ..team_spec()
            },
            Some("team_preflight".to_string()),
        )
        .await
        .expect_err("unknown member prevents team creation");
    assert!(err.to_string().contains("unknown agent"));

    let events = load_events(&run.events_path);
    assert!(!events
        .iter()
        .any(|event| matches!(event.payload, EventV1::TeamCreated(_))));
    let projection = project_team_state(events.iter()).expect("project team state");
    assert!(!projection.teams.contains_key("team_preflight"));
}

#[tokio::test]
async fn coordinator_team_category_resolution_rejects_read_only_profiles() {
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

    let err = handle
        .create_team(
            EventActor::new(ActorKind::Supervisor, None),
            TeamSpec {
                members: vec![TeamMemberSpec {
                    name: "research".to_string(),
                    role: TeamMemberRole::Member,
                    selector: TeamMemberSelector::Category {
                        category: "explore".to_string(),
                    },
                    prompt: None,
                }],
                ..team_spec()
            },
            Some("team_readonly_category".to_string()),
        )
        .await
        .expect_err("read-only category profiles are not team members");
    assert!(err.to_string().contains("read-only"));
}

#[tokio::test]
async fn coordinator_team_enforces_bounds_actor_identity_and_owner_patch_semantics() {
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

    let invalid_bounds = handle
        .create_team(
            actor.clone(),
            TeamSpec {
                bounds: TeamBounds {
                    max_members: 2,
                    max_parallel_members: 3,
                    ..TeamBounds::default()
                },
                ..team_spec()
            },
            Some("team_bad_bounds".to_string()),
        )
        .await
        .expect_err("invalid bounds are rejected");
    assert!(invalid_bounds.to_string().contains("max_parallel_members"));

    let created = handle
        .create_team(
            actor.clone(),
            TeamSpec {
                bounds: TeamBounds {
                    max_messages_per_run: 1,
                    ..TeamBounds::default()
                },
                ..team_spec()
            },
            Some("team_identity".to_string()),
        )
        .await
        .expect("create team");
    let alpha_agent = created.members["alpha"]
        .agent_id
        .clone()
        .expect("alpha agent id");
    let beta_agent = created.members["beta"]
        .agent_id
        .clone()
        .expect("beta agent id");
    let alpha_actor = EventActor::new(ActorKind::Worker, Some(alpha_agent));
    let beta_actor = EventActor::new(ActorKind::Worker, Some(beta_agent));

    handle
        .send_team_message(
            alpha_actor.clone(),
            "team_identity",
            TeamMessage {
                version: 1,
                message_id: "msg_1".to_string(),
                from: "alpha".to_string(),
                to: "beta".to_string(),
                kind: TeamMessageKind::Message,
                body: "hello".to_string(),
                summary: None,
                references: Vec::new(),
                correlation_id: None,
            },
        )
        .await
        .expect("alpha sends message");
    let too_many_messages = handle
        .send_team_message(
            alpha_actor.clone(),
            "team_identity",
            TeamMessage {
                version: 1,
                message_id: "msg_2".to_string(),
                from: "alpha".to_string(),
                to: "beta".to_string(),
                kind: TeamMessageKind::Message,
                body: "again".to_string(),
                summary: None,
                references: Vec::new(),
                correlation_id: None,
            },
        )
        .await
        .expect_err("message bound is enforced");
    assert!(too_many_messages
        .to_string()
        .contains("max_messages_per_run"));

    let forged = handle
        .request_team_shutdown(beta_actor, "team_identity", "alpha", "alpha")
        .await
        .expect_err("beta cannot act as alpha");
    assert!(forged.to_string().contains("cannot act as"));
    let lead_forgery = handle
        .request_team_shutdown(alpha_actor, "team_identity", "alpha", "lead")
        .await
        .expect_err("worker cannot act as lead");
    assert!(lead_forgery.to_string().contains("cannot act as lead"));

    handle
        .create_team_task(
            actor.clone(),
            "team_identity",
            TeamTask {
                owner: Some("alpha".to_string()),
                ..team_task("owned", Vec::new())
            },
        )
        .await
        .expect("create owned task");
    let updated = handle
        .update_team_task(
            actor,
            "team_identity",
            "owned",
            TeamTaskStatus::Completed,
            None,
            BTreeMap::new(),
        )
        .await
        .expect("status-only update keeps owner");
    assert_eq!(updated.tasks["owned"].owner.as_deref(), Some("alpha"));
}

#[tokio::test]
async fn coordinator_team_requires_pending_shutdown_request_before_decision() {
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

    handle
        .create_team(
            actor.clone(),
            team_spec(),
            Some("team_shutdown".to_string()),
        )
        .await
        .expect("create team");

    let missing_request = handle
        .approve_team_shutdown(actor.clone(), "team_shutdown", "alpha", "alpha")
        .await
        .expect_err("approval requires pending shutdown request");
    assert!(missing_request
        .to_string()
        .contains("no pending shutdown request"));

    handle
        .request_team_shutdown(actor.clone(), "team_shutdown", "alpha", "lead")
        .await
        .expect("request alpha shutdown");
    let rejected = handle
        .reject_team_shutdown(
            actor.clone(),
            "team_shutdown",
            "alpha",
            "alpha",
            "still busy",
        )
        .await
        .expect("reject alpha shutdown");
    assert_eq!(rejected.status, TeamRunStatus::Active);
    assert_eq!(rejected.members["alpha"].status, TeamMemberStatus::Running);

    let stale_decision = handle
        .reject_team_shutdown(actor.clone(), "team_shutdown", "alpha", "alpha", "again")
        .await
        .expect_err("stale rejection requires a fresh request");
    assert!(stale_decision
        .to_string()
        .contains("no pending shutdown request"));

    handle
        .request_team_shutdown(actor.clone(), "team_shutdown", "alpha", "lead")
        .await
        .expect("request alpha shutdown again");
    handle
        .approve_team_shutdown(actor.clone(), "team_shutdown", "alpha", "alpha")
        .await
        .expect("approve alpha shutdown");
    let duplicate_request = handle
        .request_team_shutdown(actor, "team_shutdown", "alpha", "lead")
        .await
        .expect_err("approved member cannot reopen shutdown");
    assert!(duplicate_request.to_string().contains("already approved"));
}

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

fn team_spec() -> TeamSpec {
    TeamSpec {
        version: 1,
        name: "alpha-team".to_string(),
        description: Some("test team".to_string()),
        lead: None,
        members: vec![
            TeamMemberSpec {
                name: "alpha".to_string(),
                role: TeamMemberRole::Member,
                selector: TeamMemberSelector::SubagentType {
                    subagent_type: "alpha".to_string(),
                },
                prompt: None,
            },
            TeamMemberSpec {
                name: "beta".to_string(),
                role: TeamMemberRole::Member,
                selector: TeamMemberSelector::SubagentType {
                    subagent_type: "beta".to_string(),
                },
                prompt: None,
            },
        ],
        bounds: TeamBounds::default(),
    }
}

fn team_task(task_id: &str, blocked_by: Vec<String>) -> TeamTask {
    TeamTask {
        version: 1,
        task_id: task_id.to_string(),
        subject: task_id.to_string(),
        description: format!("description for {task_id}"),
        status: TeamTaskStatus::Pending,
        owner: None,
        blocks: Vec::new(),
        blocked_by,
        metadata: BTreeMap::new(),
    }
}

fn agent_profiles() -> BTreeMap<String, AgentProfile> {
    BTreeMap::from([
        ("alpha".to_string(), profile("alpha")),
        ("beta".to_string(), profile("beta")),
        ("general".to_string(), profile("general")),
    ])
}

fn profile(name: &str) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: "deep".to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        fallback_model_refs: Vec::new(),
        fallback_model_settings: Vec::new(),
        system_prompt: format!("{name}-prompt"),
        max_iters: Some(1),
        temperature: Some(0.0),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: vec![],
    }
}

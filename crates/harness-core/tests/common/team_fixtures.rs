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

#[path = "mod.rs"]
mod common;

use common::load_events;

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
        system_prompt: format!("{name}-prompt"),
        max_iters: Some(1),
        temperature: Some(0.0),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: vec![],
    }
}

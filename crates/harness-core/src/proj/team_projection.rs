use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::{
    ActorKind, EventEnvelopeV1, EventV1, TeamBounds, TeamMemberRole, TeamMemberSelector,
    TeamMemberSpec, TeamMessage, TeamSpec, TeamTask,
};

use super::{enforce_seq, ProjectionError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRunStatus {
    Active,
    ShutdownRequested,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMemberStatus {
    Pending,
    Running,
    ShutdownRequested,
    ShutdownApproved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamLeadProjection {
    pub selector: TeamMemberSelector,
    pub status: TeamMemberStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMemberProjection {
    pub name: String,
    pub role: TeamMemberRole,
    pub spec: TeamMemberSpec,
    pub status: TeamMemberStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shutdown_requester: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shutdown_rejected_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamShutdownRequestProjection {
    pub member_name: String,
    pub requester: String,
    pub status: TeamMemberStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRunProjection {
    pub team_run_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: TeamRunStatus,
    pub bounds: TeamBounds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<TeamLeadProjection>,
    pub members: BTreeMap<String, TeamMemberProjection>,
    pub messages: Vec<TeamMessage>,
    pub tasks: BTreeMap<String, TeamTask>,
    pub shutdown_requests: BTreeMap<String, TeamShutdownRequestProjection>,
    pub bounds_consumption: TeamBoundsConsumption,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_mono_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_mono_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TeamBoundsConsumption {
    pub running_members: u32,
    pub pending_members: u32,
    pub shutdown_approved_members: u32,
    pub messages: u32,
    pub tasks: u32,
    pub member_turns: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_wall_clock_minutes: Option<u32>,
}

impl TeamRunProjection {
    fn from_spec(team_run_id: String, spec: TeamSpec, created_mono_ms: u64) -> Self {
        let lead = spec.lead.clone().map(|selector| TeamLeadProjection {
            selector,
            status: TeamMemberStatus::Pending,
            agent_id: None,
            profile: None,
        });
        let members = spec
            .members
            .iter()
            .cloned()
            .map(|member| {
                let name = member.name.clone();
                let role = member.role;
                (
                    name.clone(),
                    TeamMemberProjection {
                        name,
                        role,
                        spec: member,
                        status: TeamMemberStatus::Pending,
                        agent_id: None,
                        profile: None,
                        shutdown_requester: None,
                        shutdown_rejected_reason: None,
                    },
                )
            })
            .collect();

        Self {
            team_run_id,
            name: spec.name,
            description: spec.description,
            status: TeamRunStatus::Active,
            bounds: spec.bounds,
            lead,
            members,
            messages: Vec::new(),
            tasks: BTreeMap::new(),
            shutdown_requests: BTreeMap::new(),
            bounds_consumption: TeamBoundsConsumption::default(),
            created_mono_ms: Some(created_mono_ms),
            last_mono_ms: Some(created_mono_ms),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TeamProjection {
    pub teams: BTreeMap<String, TeamRunProjection>,
}

pub fn project_team_state<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
) -> Result<TeamProjection, ProjectionError> {
    let mut projection = TeamProjection::default();
    let mut last_seq = None;

    for event in events {
        enforce_seq(last_seq, event.seq)?;
        last_seq = Some(event.seq);
        apply_team_event(&mut projection, event);
    }

    Ok(projection)
}

fn apply_team_event(projection: &mut TeamProjection, event: &EventEnvelopeV1) {
    match &event.payload {
        EventV1::TeamCreated(payload) => {
            let mut team = TeamRunProjection::from_spec(
                payload.team_run_id.clone(),
                payload.spec.clone(),
                event.mono_ms,
            );
            refresh_team_derived_state(&mut team, event.mono_ms);
            projection.teams.insert(payload.team_run_id.clone(), team);
        }
        EventV1::TeamMemberSpawned(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                if payload.member_name == "lead" {
                    if let Some(lead) = team.lead.as_mut() {
                        if lead.agent_id.is_none() {
                            lead.status = TeamMemberStatus::Running;
                            lead.agent_id = Some(payload.agent_id.clone());
                            lead.profile = Some(payload.profile.clone());
                        }
                    }
                } else if let Some(member) = team.members.get_mut(&payload.member_name) {
                    if member.agent_id.is_none() {
                        member.status = TeamMemberStatus::Running;
                        member.agent_id = Some(payload.agent_id.clone());
                        member.profile = Some(payload.profile.clone());
                    }
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamMessageSent(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                if team
                    .messages
                    .iter()
                    .all(|message| message.message_id != payload.message.message_id)
                {
                    if member_write_participant_for_event(team, event, Some(&payload.message.from))
                        .is_some()
                    {
                        team.bounds_consumption.member_turns =
                            team.bounds_consumption.member_turns.saturating_add(1);
                    }
                    team.messages.push(payload.message.clone());
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamTaskCreated(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                if !team.tasks.contains_key(&payload.task.task_id) {
                    let mut task = payload.task.clone();
                    task.blocks.clear();
                    if member_write_participant_for_event(team, event, task.owner.as_deref())
                        .is_some()
                    {
                        team.bounds_consumption.member_turns =
                            team.bounds_consumption.member_turns.saturating_add(1);
                    }
                    team.tasks.insert(task.task_id.clone(), task);
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamTaskUpdated(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                if member_write_participant_for_event(team, event, payload.owner.as_deref())
                    .is_some()
                {
                    team.bounds_consumption.member_turns =
                        team.bounds_consumption.member_turns.saturating_add(1);
                }
                if let Some(task) = team.tasks.get_mut(&payload.task_id) {
                    task.status = payload.status;
                    if payload.owner.is_some() {
                        task.owner = payload.owner.clone();
                    }
                    if !payload.metadata.is_empty() {
                        task.metadata.extend(payload.metadata.clone());
                    }
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamShutdownRequested(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                team.status = TeamRunStatus::ShutdownRequested;
                if let Some(member) = team.members.get_mut(&payload.member_name) {
                    member.status = TeamMemberStatus::ShutdownRequested;
                    member.shutdown_requester = Some(payload.requester.clone());
                    member.shutdown_rejected_reason = None;
                }
                team.shutdown_requests.insert(
                    payload.member_name.clone(),
                    TeamShutdownRequestProjection {
                        member_name: payload.member_name.clone(),
                        requester: payload.requester.clone(),
                        status: TeamMemberStatus::ShutdownRequested,
                        rejected_reason: None,
                    },
                );
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamShutdownApproved(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                team.status = TeamRunStatus::ShutdownRequested;
                if let Some(member) = team.members.get_mut(&payload.member_name) {
                    member.status = TeamMemberStatus::ShutdownApproved;
                    member.shutdown_rejected_reason = None;
                }
                if let Some(request) = team.shutdown_requests.get_mut(&payload.member_name) {
                    request.status = TeamMemberStatus::ShutdownApproved;
                    request.rejected_reason = None;
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamShutdownRejected(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                if let Some(member) = team.members.get_mut(&payload.member_name) {
                    member.status = TeamMemberStatus::Running;
                    member.shutdown_rejected_reason = Some(payload.reason.clone());
                }
                if let Some(request) = team.shutdown_requests.get_mut(&payload.member_name) {
                    request.status = TeamMemberStatus::Running;
                    request.rejected_reason = Some(payload.reason.clone());
                }
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        EventV1::TeamDeleted(payload) => {
            if let Some(team) = projection.teams.get_mut(&payload.team_run_id) {
                team.status = TeamRunStatus::Deleted;
                refresh_team_derived_state(team, event.mono_ms);
            }
        }
        _ => {}
    }
}

fn refresh_team_derived_state(team: &mut TeamRunProjection, mono_ms: u64) {
    team.last_mono_ms = Some(mono_ms);
    refresh_team_shutdown_status(team);
    refresh_team_task_blocks(team);
    team.bounds_consumption.running_members = team
        .members
        .values()
        .filter(|member| {
            matches!(
                member.status,
                TeamMemberStatus::Running | TeamMemberStatus::ShutdownRequested
            )
        })
        .count() as u32;
    team.bounds_consumption.pending_members = team
        .members
        .values()
        .filter(|member| member.status == TeamMemberStatus::Pending)
        .count() as u32;
    team.bounds_consumption.shutdown_approved_members = team
        .members
        .values()
        .filter(|member| member.status == TeamMemberStatus::ShutdownApproved)
        .count() as u32;
    team.bounds_consumption.messages = team.messages.len() as u32;
    team.bounds_consumption.tasks = team.tasks.len() as u32;
    team.bounds_consumption.elapsed_wall_clock_minutes = team
        .created_mono_ms
        .map(|created| mono_ms.saturating_sub(created) / 60_000)
        .map(|minutes| minutes.min(u64::from(u32::MAX)) as u32);
}

fn refresh_team_task_blocks(team: &mut TeamRunProjection) {
    for task in team.tasks.values_mut() {
        task.blocks.clear();
    }
    let edges = team
        .tasks
        .iter()
        .flat_map(|(task_id, task)| {
            task.blocked_by
                .iter()
                .cloned()
                .map(|blocked_by| (blocked_by, task_id.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (blocked_by, task_id) in edges {
        if let Some(blocker) = team.tasks.get_mut(&blocked_by) {
            if !blocker.blocks.contains(&task_id) {
                blocker.blocks.push(task_id);
            }
        }
    }
}

fn member_write_participant<'a>(
    team: &'a TeamRunProjection,
    participant: &str,
) -> Option<&'a TeamMemberProjection> {
    team.members
        .get(participant)
        .filter(|member| member.role == TeamMemberRole::Member)
}

fn member_write_participant_for_event<'a>(
    team: &'a TeamRunProjection,
    event: &EventEnvelopeV1,
    explicit_participant: Option<&str>,
) -> Option<&'a TeamMemberProjection> {
    if event.actor.kind == ActorKind::Worker {
        if let Some(agent_id) = event.actor.agent_id.as_deref() {
            return team
                .members
                .values()
                .find(|member| member.agent_id.as_deref() == Some(agent_id))
                .filter(|member| member.role == TeamMemberRole::Member);
        }
    }
    explicit_participant.and_then(|participant| member_write_participant(team, participant))
}

fn refresh_team_shutdown_status(team: &mut TeamRunProjection) {
    if team.status == TeamRunStatus::Deleted {
        return;
    }
    team.status = if team.members.values().any(|member| {
        matches!(
            member.status,
            TeamMemberStatus::ShutdownRequested | TeamMemberStatus::ShutdownApproved
        )
    }) {
        TeamRunStatus::ShutdownRequested
    } else {
        TeamRunStatus::Active
    };
}

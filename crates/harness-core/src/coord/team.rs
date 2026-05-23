use super::*;

pub(super) fn reject_nested_team_create(
    actor: &EventActor,
    projection: &TeamProjection,
) -> Result<(), CoordinatorError> {
    let Some(agent_id) = actor.agent_id.as_deref() else {
        return Ok(());
    };
    let is_team_member = projection.teams.values().any(|team| {
        team.members
            .values()
            .any(|member| member.agent_id.as_deref() == Some(agent_id))
    });
    if is_team_member {
        return Err(CoordinatorError::PolicyViolation(
            "team members cannot create nested teams".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_team_spec(spec: &TeamSpec) -> Result<(), CoordinatorError> {
    if spec.version != 1 {
        return Err(CoordinatorError::PolicyViolation(
            "team spec version must be 1".to_string(),
        ));
    }
    if non_empty_trimmed(&spec.name).is_none() {
        return Err(CoordinatorError::PolicyViolation(
            "team name cannot be empty".to_string(),
        ));
    }
    validate_team_text_field("team name", &spec.name)?;
    if let Some(description) = spec.description.as_deref() {
        validate_team_text_field("team description", description)?;
    }
    if spec.members.is_empty() || spec.members.len() > TEAM_MAX_MEMBERS {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team must have between 1 and {TEAM_MAX_MEMBERS} members"
        )));
    }
    if spec.bounds.max_members == 0 || spec.bounds.max_members as usize > TEAM_MAX_MEMBERS {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team max_members must be between 1 and {TEAM_MAX_MEMBERS}"
        )));
    }
    if spec.bounds.max_parallel_members == 0
        || spec.bounds.max_parallel_members > spec.bounds.max_members
    {
        return Err(CoordinatorError::PolicyViolation(
            "team max_parallel_members must be between 1 and max_members".to_string(),
        ));
    }
    if spec.bounds.max_messages_per_run == 0 {
        return Err(CoordinatorError::PolicyViolation(
            "team max_messages_per_run must be greater than zero".to_string(),
        ));
    }
    if spec.bounds.max_wall_clock_minutes == 0 || spec.bounds.max_member_turns == 0 {
        return Err(CoordinatorError::PolicyViolation(
            "team wall-clock and member-turn bounds must be greater than zero".to_string(),
        ));
    }
    if spec.members.len() > spec.bounds.max_members as usize {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team member count exceeds max_members bound {}",
            spec.bounds.max_members
        )));
    }
    let mut names = BTreeSet::new();
    for member in spec.members.iter() {
        if non_empty_trimmed(&member.name).is_none() {
            return Err(CoordinatorError::PolicyViolation(
                "team member name cannot be empty".to_string(),
            ));
        }
        if matches!(member.name.as_str(), "lead" | "*") {
            return Err(CoordinatorError::PolicyViolation(format!(
                "team member name `{}` is reserved",
                member.name
            )));
        }
        validate_team_text_field("team member name", &member.name)?;
        if let Some(prompt) = member.prompt.as_deref() {
            validate_team_text_field("team member prompt", prompt)?;
        }
        if !names.insert(member.name.clone()) {
            return Err(CoordinatorError::PolicyViolation(format!(
                "duplicate team member `{}`",
                member.name
            )));
        }
    }
    Ok(())
}

fn validate_team_text_field(label: &str, value: &str) -> Result<(), CoordinatorError> {
    if value.chars().count() > TEAM_TEXT_FIELD_MAX_CHARS {
        return Err(CoordinatorError::PolicyViolation(format!(
            "{label} exceeds {TEAM_TEXT_FIELD_MAX_CHARS} characters"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(super) enum TeamParticipantRole {
    Lead,
    Member(TeamMemberRole),
}

pub(super) fn validate_team_profile_role(
    profile: &str,
    profile_config: Option<&AgentProfile>,
    role: TeamParticipantRole,
) -> Result<(), CoordinatorError> {
    let read_only = is_read_only_team_profile(profile, profile_config);
    match role {
        TeamParticipantRole::Lead if read_only => Err(CoordinatorError::PolicyViolation(format!(
            "team lead profile `{profile}` is read-only or planning-only"
        ))),
        TeamParticipantRole::Member(TeamMemberRole::Member) if read_only => {
            Err(CoordinatorError::PolicyViolation(format!(
                "team member profile `{profile}` is read-only or planning-only; mark the member role as research or use task delegation for ad hoc research"
            )))
        }
        TeamParticipantRole::Member(TeamMemberRole::Research) if !read_only => {
            Err(CoordinatorError::PolicyViolation(format!(
                "research team member profile `{profile}` must be read-only or planning-only"
            )))
        }
        _ => Ok(()),
    }
}

fn is_read_only_team_profile(profile: &str, profile_config: Option<&AgentProfile>) -> bool {
    if matches!(
        profile,
        "oracle"
            | "librarian"
            | "explore"
            | "metis"
            | "momus"
            | "multimodal-looker"
            | "prometheus"
            | "plan"
    ) {
        return true;
    }
    profile_config.is_some_and(|profile| {
        matches!(
            profile.category.as_str(),
            "explore" | "oracle" | "librarian" | "plan" | "research" | "read_only"
        )
    })
}

pub(super) fn require_active_team<'a>(
    projection: &'a TeamProjection,
    team_run_id: &str,
) -> Result<&'a TeamRunProjection, CoordinatorError> {
    let team = projection
        .teams
        .get(team_run_id)
        .ok_or_else(|| CoordinatorError::UnknownTask(format!("team:{team_run_id}")))?;
    if team.status == crate::proj::TeamRunStatus::Deleted {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team `{team_run_id}` is deleted"
        )));
    }
    Ok(team)
}

pub(super) fn require_active_team_or_shutdown<'a>(
    projection: &'a TeamProjection,
    team_run_id: &str,
) -> Result<&'a TeamRunProjection, CoordinatorError> {
    require_active_team(projection, team_run_id)
}

pub(super) fn validate_team_member(
    team: &TeamRunProjection,
    member_name: &str,
) -> Result<(), CoordinatorError> {
    if team.members.contains_key(member_name) {
        Ok(())
    } else {
        Err(CoordinatorError::PolicyViolation(format!(
            "unknown team member `{member_name}`"
        )))
    }
}

pub(super) fn validate_team_participant(
    team: &TeamRunProjection,
    participant: &str,
) -> Result<(), CoordinatorError> {
    if participant == "lead" || team.members.contains_key(participant) {
        Ok(())
    } else {
        Err(CoordinatorError::PolicyViolation(format!(
            "unknown team participant `{participant}`"
        )))
    }
}

fn validate_team_actor_can_act_as(
    actor: &EventActor,
    team: &TeamRunProjection,
    participant: &str,
) -> Result<(), CoordinatorError> {
    if actor.kind != ActorKind::Worker {
        return Ok(());
    }
    let Some(actor_agent_id) = actor.agent_id.as_deref() else {
        return Err(CoordinatorError::PolicyViolation(
            "worker team action missing agent_id".to_string(),
        ));
    };
    if participant == "lead" {
        if team.lead.as_ref().and_then(|lead| lead.agent_id.as_deref()) == Some(actor_agent_id) {
            return Ok(());
        }
        return Err(CoordinatorError::PolicyViolation(
            "worker team members cannot act as lead".to_string(),
        ));
    }
    let Some(member) = team.members.get(participant) else {
        return Err(CoordinatorError::PolicyViolation(format!(
            "unknown team participant `{participant}`"
        )));
    };
    if member.agent_id.as_deref() != Some(actor_agent_id) {
        return Err(CoordinatorError::PolicyViolation(format!(
            "worker `{actor_agent_id}` cannot act as team participant `{participant}`"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TeamActionKind {
    TeamWrite,
    Shutdown,
}

pub(super) fn validate_team_action(
    actor: &EventActor,
    team: &TeamRunProjection,
    action: TeamActionKind,
    participant: &str,
    now_mono_ms: u64,
) -> Result<(), CoordinatorError> {
    validate_team_participant(team, participant)?;
    validate_team_participant_can_perform(team, action, participant, now_mono_ms)?;
    validate_team_actor_can_act_as(actor, team, participant)
}

pub(super) fn validate_team_actor_can_make_unowned_team_write(
    actor: &EventActor,
    team: &TeamRunProjection,
    now_mono_ms: u64,
) -> Result<(), CoordinatorError> {
    validate_team_wall_clock(team, now_mono_ms)?;
    if actor.kind != ActorKind::Worker {
        return Ok(());
    }
    let participant = team_participant_for_worker_actor(actor, team)?;
    validate_team_participant_can_perform(
        team,
        TeamActionKind::TeamWrite,
        &participant,
        now_mono_ms,
    )
}

fn validate_team_participant_can_perform(
    team: &TeamRunProjection,
    action: TeamActionKind,
    participant: &str,
    now_mono_ms: u64,
) -> Result<(), CoordinatorError> {
    if action == TeamActionKind::TeamWrite {
        validate_team_wall_clock(team, now_mono_ms)?;
    }
    if participant == "lead" {
        return Ok(());
    }
    let member = team.members.get(participant).ok_or_else(|| {
        CoordinatorError::PolicyViolation(format!("unknown team participant `{participant}`"))
    })?;
    match action {
        TeamActionKind::TeamWrite => {
            if member.role == TeamMemberRole::Research {
                return Err(CoordinatorError::PolicyViolation(format!(
                    "research team member `{participant}` cannot mutate team messages or tasks"
                )));
            }
            match member.status {
                crate::proj::TeamMemberStatus::Pending => {
                    return Err(CoordinatorError::PolicyViolation(format!(
                        "team member `{participant}` is not active"
                    )));
                }
                crate::proj::TeamMemberStatus::ShutdownApproved => {
                    return Err(CoordinatorError::PolicyViolation(format!(
                        "team member `{participant}` is shutdown-approved and cannot mutate team state"
                    )));
                }
                crate::proj::TeamMemberStatus::Running
                | crate::proj::TeamMemberStatus::ShutdownRequested => {}
            }
            if team.bounds_consumption.member_turns >= team.bounds.max_member_turns {
                return Err(CoordinatorError::PolicyViolation(format!(
                    "team `{}` has reached max_member_turns {}",
                    team.team_run_id, team.bounds.max_member_turns
                )));
            }
        }
        TeamActionKind::Shutdown => {
            if member.status == crate::proj::TeamMemberStatus::ShutdownApproved {
                return Err(CoordinatorError::PolicyViolation(format!(
                    "team member `{participant}` is shutdown-approved and cannot make further shutdown decisions"
                )));
            }
        }
    }
    Ok(())
}

fn validate_team_wall_clock(
    team: &TeamRunProjection,
    now_mono_ms: u64,
) -> Result<(), CoordinatorError> {
    let Some(created_mono_ms) = team.created_mono_ms else {
        return Ok(());
    };
    let limit_ms = u64::from(team.bounds.max_wall_clock_minutes).saturating_mul(60_000);
    if now_mono_ms.saturating_sub(created_mono_ms) >= limit_ms {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team `{}` has exceeded max_wall_clock_minutes {}",
            team.team_run_id, team.bounds.max_wall_clock_minutes
        )));
    }
    Ok(())
}

fn team_participant_for_worker_actor(
    actor: &EventActor,
    team: &TeamRunProjection,
) -> Result<String, CoordinatorError> {
    let Some(actor_agent_id) = actor.agent_id.as_deref() else {
        return Err(CoordinatorError::PolicyViolation(
            "worker team action missing agent_id".to_string(),
        ));
    };
    if team.lead.as_ref().and_then(|lead| lead.agent_id.as_deref()) == Some(actor_agent_id) {
        return Ok("lead".to_string());
    }
    team.members
        .values()
        .find(|member| member.agent_id.as_deref() == Some(actor_agent_id))
        .map(|member| member.name.clone())
        .ok_or_else(|| {
            CoordinatorError::PolicyViolation(format!(
                "worker `{actor_agent_id}` is not a participant in team `{}`",
                team.team_run_id
            ))
        })
}

pub(super) fn validate_team_shutdown_request_can_open(
    team: &TeamRunProjection,
    member_name: &str,
) -> Result<(), CoordinatorError> {
    match team
        .shutdown_requests
        .get(member_name)
        .map(|request| request.status)
    {
        Some(crate::proj::TeamMemberStatus::ShutdownRequested) => {
            Err(CoordinatorError::PolicyViolation(format!(
                "shutdown request for team member `{member_name}` is already pending"
            )))
        }
        Some(crate::proj::TeamMemberStatus::ShutdownApproved) => {
            Err(CoordinatorError::PolicyViolation(format!(
                "shutdown for team member `{member_name}` is already approved"
            )))
        }
        _ => Ok(()),
    }
}

pub(super) fn validate_team_shutdown_request_pending(
    team: &TeamRunProjection,
    member_name: &str,
) -> Result<(), CoordinatorError> {
    match team
        .shutdown_requests
        .get(member_name)
        .map(|request| request.status)
    {
        Some(crate::proj::TeamMemberStatus::ShutdownRequested) => Ok(()),
        _ => Err(CoordinatorError::PolicyViolation(format!(
            "team member `{member_name}` has no pending shutdown request"
        ))),
    }
}

pub(super) fn validate_team_message(
    team: &TeamRunProjection,
    message: &TeamMessage,
) -> Result<(), CoordinatorError> {
    if message.version != 1 {
        return Err(CoordinatorError::PolicyViolation(
            "team message version must be 1".to_string(),
        ));
    }
    if non_empty_trimmed(&message.message_id).is_none() {
        return Err(CoordinatorError::PolicyViolation(
            "team message id cannot be empty".to_string(),
        ));
    }
    if team.messages.len() >= team.bounds.max_messages_per_run as usize {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team `{}` has reached max_messages_per_run {}",
            team.team_run_id, team.bounds.max_messages_per_run
        )));
    }
    validate_team_text_field("team message id", &message.message_id)?;
    if team
        .messages
        .iter()
        .any(|existing| existing.message_id == message.message_id)
    {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team message `{}` already exists",
            message.message_id
        )));
    }
    validate_team_text_field("team message sender", &message.from)?;
    validate_team_text_field("team message recipient", &message.to)?;
    if let Some(summary) = message.summary.as_deref() {
        validate_team_text_field("team message summary", summary)?;
    }
    if message.body.len() > TEAM_MESSAGE_BODY_MAX_BYTES {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team message body exceeds {TEAM_MESSAGE_BODY_MAX_BYTES} bytes"
        )));
    }
    if message.references.len() > TEAM_REFERENCE_LIMIT {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team message references exceed {TEAM_REFERENCE_LIMIT} entries"
        )));
    }
    for reference in &message.references {
        validate_team_text_field("team reference path", &reference.path)?;
        if reference.path.starts_with('/') || reference.path.contains("..") {
            return Err(CoordinatorError::PolicyViolation(
                "team reference path must be workspace-relative and must not contain traversal"
                    .to_string(),
            ));
        }
        if let Some(description) = reference.description.as_deref() {
            validate_team_text_field("team reference description", description)?;
        }
    }
    validate_team_participant(team, &message.from)?;
    if message.to == "*" {
        if message.from != "lead" || message.kind != TeamMessageKind::Announcement {
            return Err(CoordinatorError::PolicyViolation(
                "only lead may broadcast announcements".to_string(),
            ));
        }
    } else {
        validate_team_participant(team, &message.to)?;
    }
    Ok(())
}

pub(super) fn validate_team_task_create(
    team: &TeamRunProjection,
    task: &TeamTask,
) -> Result<(), CoordinatorError> {
    if task.version != 1 {
        return Err(CoordinatorError::PolicyViolation(
            "team task version must be 1".to_string(),
        ));
    }
    if non_empty_trimmed(&task.task_id).is_none() {
        return Err(CoordinatorError::PolicyViolation(
            "team task id cannot be empty".to_string(),
        ));
    }
    validate_team_text_field("team task id", &task.task_id)?;
    validate_team_text_field("team task subject", &task.subject)?;
    validate_team_text_field("team task description", &task.description)?;
    if team.tasks.contains_key(&task.task_id) {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team task `{}` already exists",
            task.task_id
        )));
    }
    validate_team_metadata(&task.metadata)?;
    if let Some(owner) = task.owner.as_deref() {
        validate_team_participant(team, owner)?;
    }
    for blocker in task.blocked_by.iter() {
        if !team.tasks.contains_key(blocker) {
            return Err(CoordinatorError::PolicyViolation(format!(
                "team task `{}` depends on unknown task `{blocker}`",
                task.task_id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_team_task_update(
    team: &TeamRunProjection,
    task_id: &str,
    status: TeamTaskStatus,
    owner: Option<&str>,
    metadata: &BTreeMap<String, String>,
) -> Result<(), CoordinatorError> {
    let task = team.tasks.get(task_id).ok_or_else(|| {
        CoordinatorError::UnknownTask(format!("team:{}/task:{task_id}", team.team_run_id))
    })?;
    if let Some(owner) = owner {
        validate_team_participant(team, owner)?;
    }
    validate_team_metadata(metadata)?;
    if matches!(
        status,
        TeamTaskStatus::Claimed | TeamTaskStatus::InProgress | TeamTaskStatus::Completed
    ) {
        let incomplete = task
            .blocked_by
            .iter()
            .filter(|blocked_by| {
                team.tasks
                    .get(*blocked_by)
                    .is_none_or(|candidate| candidate.status != TeamTaskStatus::Completed)
            })
            .cloned()
            .collect::<Vec<_>>();
        if !incomplete.is_empty() {
            return Err(CoordinatorError::PolicyViolation(format!(
                "team task `{task_id}` is blocked by incomplete tasks: {}",
                incomplete.join(", ")
            )));
        }
    }
    Ok(())
}

fn validate_team_metadata(metadata: &BTreeMap<String, String>) -> Result<(), CoordinatorError> {
    if metadata.len() > TEAM_TASK_METADATA_MAX_ENTRIES {
        return Err(CoordinatorError::PolicyViolation(format!(
            "team task metadata exceeds {TEAM_TASK_METADATA_MAX_ENTRIES} entries"
        )));
    }
    for (key, value) in metadata {
        validate_team_metadata_field("team task metadata key", key)?;
        validate_team_metadata_field("team task metadata value", value)?;
    }
    Ok(())
}

fn validate_team_metadata_field(label: &str, value: &str) -> Result<(), CoordinatorError> {
    if value.chars().count() > TEAM_TASK_METADATA_MAX_CHARS {
        return Err(CoordinatorError::PolicyViolation(format!(
            "{label} exceeds {TEAM_TASK_METADATA_MAX_CHARS} characters"
        )));
    }
    Ok(())
}

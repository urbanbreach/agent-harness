use super::team::{
    reject_nested_team_create, require_active_team, require_active_team_or_shutdown,
    validate_team_action, validate_team_actor_can_make_unowned_team_write, validate_team_member,
    validate_team_message, validate_team_participant, validate_team_profile_role,
    validate_team_shutdown_request_can_open, validate_team_shutdown_request_pending,
    validate_team_task_create, validate_team_task_update, TeamActionKind, TeamParticipantRole,
};
use super::*;

impl Coordinator {
    pub(in crate::coord) async fn team_projection_internal(
        &self,
    ) -> Result<TeamProjection, CoordinatorError> {
        let events = self.replay_current_run_events().await?;
        project_team_state(events.iter())
            .map_err(|err| CoordinatorError::PolicyViolation(err.to_string()))
    }

    pub(in crate::coord) async fn create_team_internal(
        &mut self,
        actor: EventActor,
        mut spec: TeamSpec,
        team_run_id: Option<String>,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        reject_nested_team_create(&actor, &self.team_projection_internal().await?)?;
        if spec.bounds.max_members == 0 {
            spec.bounds.max_members = TeamBounds::default().max_members;
        }
        team::validate_team_spec(&spec)?;

        let team_run_id = team_run_id
            .and_then(|value| non_empty_trimmed(&value).map(str::to_string))
            .unwrap_or_else(|| {
                self.run_state
                    .as_ref()
                    .map(|run_state| format!("team_{:06}", run_state.next_event_seq))
                    .unwrap_or_else(|| "team_000001".to_string())
            });

        let existing = self.team_projection_internal().await?;
        if existing.teams.contains_key(&team_run_id) {
            return Err(CoordinatorError::PolicyViolation(format!(
                "team `{team_run_id}` already exists"
            )));
        }

        let resolved_lead = spec
            .lead
            .as_ref()
            .map(|selector| self.resolve_team_selector_profile(selector, TeamParticipantRole::Lead))
            .transpose()?;

        let resolved_members = spec
            .members
            .iter()
            .map(|member| {
                self.resolve_team_member_profile(member)
                    .map(|profile| (member, profile))
            })
            .collect::<Result<Vec<_>, _>>()?;

        {
            let run_state = self
                .run_state
                .as_mut()
                .ok_or(CoordinatorError::RunNotStarted)?;
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("team:{team_run_id}")),
                EventV1::TeamCreated(TeamCreatedEvent {
                    team_run_id: team_run_id.clone(),
                    spec: spec.clone(),
                }),
            )?;
        }

        if let Some(profile) = resolved_lead {
            let agent_id = self
                .spawn_agent_internal(
                    EventActor::new(ActorKind::Supervisor, None),
                    profile.clone(),
                    actor.agent_id.clone(),
                    Some(format!("{} (@lead team lead)", spec.name)),
                    false,
                )
                .await?;
            let run_state = self
                .run_state
                .as_mut()
                .ok_or(CoordinatorError::RunNotStarted)?;
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("team:{team_run_id}:lead")),
                EventV1::TeamMemberSpawned(TeamMemberSpawnedEvent {
                    team_run_id: team_run_id.clone(),
                    member_name: "lead".to_string(),
                    agent_id,
                    profile,
                }),
            )?;
        }

        let activation_limit = spec.bounds.max_parallel_members as usize;
        for (member, profile) in resolved_members.into_iter().take(activation_limit) {
            let agent_id = self
                .spawn_agent_internal(
                    EventActor::new(ActorKind::Supervisor, None),
                    profile.clone(),
                    actor.agent_id.clone(),
                    Some(format!("{} (@{} team member)", spec.name, member.name)),
                    false,
                )
                .await?;
            let run_state = self
                .run_state
                .as_mut()
                .ok_or(CoordinatorError::RunNotStarted)?;
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("team:{team_run_id}:member:{}", member.name)),
                EventV1::TeamMemberSpawned(TeamMemberSpawnedEvent {
                    team_run_id: team_run_id.clone(),
                    member_name: member.name.clone(),
                    agent_id,
                    profile,
                }),
            )?;
        }

        self.project_single_team(&team_run_id).await
    }

    pub(in crate::coord) fn resolve_team_selector_profile(
        &self,
        selector: &TeamMemberSelector,
        role: TeamParticipantRole,
    ) -> Result<String, CoordinatorError> {
        let profile = match selector {
            TeamMemberSelector::SubagentType { subagent_type } => {
                let profile = non_empty_trimmed(subagent_type).ok_or_else(|| {
                    CoordinatorError::PolicyViolation(
                        "team participant subagent_type cannot be empty".to_string(),
                    )
                })?;
                if !self.config.agent_profiles.contains_key(profile) {
                    return Err(CoordinatorError::UnknownAgent(profile.to_string()));
                }
                profile.to_string()
            }
            TeamMemberSelector::Category { category } => {
                let category = non_empty_trimmed(category).ok_or_else(|| {
                    CoordinatorError::PolicyViolation(
                        "team participant category cannot be empty".to_string(),
                    )
                })?;
                if self.config.agent_profiles.contains_key(category) {
                    category.to_string()
                } else {
                    self.config
                        .agent_profiles
                        .iter()
                        .find_map(|(name, profile)| {
                            (profile.category == category).then(|| name.clone())
                        })
                        .ok_or_else(|| CoordinatorError::UnknownAgent(category.to_string()))?
                }
            }
        };
        let profile_config = self.config.agent_profiles.get(&profile);
        validate_team_profile_role(&profile, profile_config, role)?;
        Ok(profile)
    }

    pub(in crate::coord) fn resolve_team_member_profile(
        &self,
        member: &TeamMemberSpec,
    ) -> Result<String, CoordinatorError> {
        self.resolve_team_selector_profile(
            &member.selector,
            TeamParticipantRole::Member(member.role),
        )
    }

    pub(in crate::coord) async fn send_team_message_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        message: TeamMessage,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team(&projection, &team_run_id)?;
        validate_team_message(team, &message)?;
        validate_team_action(
            &actor,
            team,
            TeamActionKind::TeamWrite,
            &message.from,
            self.clock.mono_ms(),
        )?;
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}:message:{}", message.message_id)),
            EventV1::TeamMessageSent(TeamMessageSentEvent {
                team_run_id: team_run_id.clone(),
                message,
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    pub(in crate::coord) async fn create_team_task_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        task: TeamTask,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team(&projection, &team_run_id)?;
        validate_team_task_create(team, &task)?;
        if let Some(owner) = task.owner.as_deref() {
            validate_team_action(
                &actor,
                team,
                TeamActionKind::TeamWrite,
                owner,
                self.clock.mono_ms(),
            )?;
        } else {
            validate_team_actor_can_make_unowned_team_write(&actor, team, self.clock.mono_ms())?;
        }
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}:task:{}", task.task_id)),
            EventV1::TeamTaskCreated(TeamTaskCreatedEvent {
                team_run_id: team_run_id.clone(),
                task,
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    pub(in crate::coord) async fn update_team_task_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        task_id: String,
        status: TeamTaskStatus,
        owner: Option<String>,
        metadata: BTreeMap<String, String>,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team(&projection, &team_run_id)?;
        validate_team_task_update(team, &task_id, status, owner.as_deref(), &metadata)?;
        if let Some(owner) = owner.as_deref() {
            validate_team_action(
                &actor,
                team,
                TeamActionKind::TeamWrite,
                owner,
                self.clock.mono_ms(),
            )?;
        } else {
            validate_team_actor_can_make_unowned_team_write(&actor, team, self.clock.mono_ms())?;
        }
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}:task:{task_id}")),
            EventV1::TeamTaskUpdated(TeamTaskUpdatedEvent {
                team_run_id: team_run_id.clone(),
                task_id,
                status,
                owner,
                metadata,
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    pub(in crate::coord) async fn request_team_shutdown_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        member_name: String,
        requester: String,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team(&projection, &team_run_id)?;
        validate_team_member(team, &member_name)?;
        validate_team_shutdown_request_can_open(team, &member_name)?;
        validate_team_participant(team, &requester)?;
        validate_team_action(
            &actor,
            team,
            TeamActionKind::Shutdown,
            &requester,
            self.clock.mono_ms(),
        )?;
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}:shutdown:{member_name}")),
            EventV1::TeamShutdownRequested(TeamShutdownRequestedEvent {
                team_run_id: team_run_id.clone(),
                member_name,
                requester,
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    pub(in crate::coord) async fn approve_team_shutdown_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        member_name: String,
        approver: String,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team_or_shutdown(&projection, &team_run_id)?;
        validate_team_member(team, &member_name)?;
        validate_team_shutdown_request_pending(team, &member_name)?;
        validate_team_participant(team, &approver)?;
        validate_team_action(
            &actor,
            team,
            TeamActionKind::Shutdown,
            &approver,
            self.clock.mono_ms(),
        )?;
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor.clone(),
            Some(format!("team:{team_run_id}:shutdown:{member_name}")),
            EventV1::TeamShutdownApproved(TeamShutdownApprovedEvent {
                team_run_id: team_run_id.clone(),
                member_name,
                approver,
            }),
        )?;
        self.activate_pending_team_members(&actor, &team_run_id)
            .await?;
        self.project_single_team(&team_run_id).await
    }

    pub(in crate::coord) async fn reject_team_shutdown_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
        member_name: String,
        rejecter: String,
        reason: String,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team_or_shutdown(&projection, &team_run_id)?;
        validate_team_member(team, &member_name)?;
        validate_team_shutdown_request_pending(team, &member_name)?;
        validate_team_participant(team, &rejecter)?;
        validate_team_action(
            &actor,
            team,
            TeamActionKind::Shutdown,
            &rejecter,
            self.clock.mono_ms(),
        )?;
        if non_empty_trimmed(&reason).is_none() {
            return Err(CoordinatorError::PolicyViolation(
                "shutdown rejection reason cannot be empty".to_string(),
            ));
        }
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}:shutdown:{member_name}")),
            EventV1::TeamShutdownRejected(TeamShutdownRejectedEvent {
                team_run_id: team_run_id.clone(),
                member_name,
                rejecter,
                reason,
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    pub(in crate::coord) async fn delete_team_internal(
        &mut self,
        actor: EventActor,
        team_run_id: String,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let team = require_active_team_or_shutdown(&projection, &team_run_id)?;
        let unapproved = team
            .members
            .values()
            .filter(|member| member.status != crate::proj::TeamMemberStatus::ShutdownApproved)
            .map(|member| member.name.clone())
            .collect::<Vec<_>>();
        if !unapproved.is_empty() {
            return Err(CoordinatorError::PolicyViolation(format!(
                "cannot delete team `{team_run_id}` before shutdown approval from: {}",
                unapproved.join(", ")
            )));
        }
        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;
        append_payload_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            actor,
            Some(format!("team:{team_run_id}")),
            EventV1::TeamDeleted(TeamDeletedEvent {
                team_run_id: team_run_id.clone(),
            }),
        )?;
        self.project_single_team(&team_run_id).await
    }

    pub(in crate::coord) async fn activate_pending_team_members(
        &mut self,
        actor: &EventActor,
        team_run_id: &str,
    ) -> Result<(), CoordinatorError> {
        let projection = self.team_projection_internal().await?;
        let Some(team) = projection.teams.get(team_run_id) else {
            return Ok(());
        };
        if team.status == crate::proj::TeamRunStatus::Deleted {
            return Ok(());
        }
        let running = team
            .members
            .values()
            .filter(|member| {
                matches!(
                    member.status,
                    crate::proj::TeamMemberStatus::Running
                        | crate::proj::TeamMemberStatus::ShutdownRequested
                )
            })
            .count();
        let capacity = (team.bounds.max_parallel_members as usize).saturating_sub(running);
        if capacity == 0 {
            return Ok(());
        }
        let team_name = team.name.clone();
        let pending = team
            .members
            .values()
            .filter(|member| member.status == crate::proj::TeamMemberStatus::Pending)
            .take(capacity)
            .map(|member| member.spec.clone())
            .collect::<Vec<_>>();

        for member in pending {
            let profile = self.resolve_team_member_profile(&member)?;
            let agent_id = self
                .spawn_agent_internal(
                    EventActor::new(ActorKind::Supervisor, None),
                    profile.clone(),
                    actor.agent_id.clone(),
                    Some(format!("{} (@{} team member)", team_name, member.name)),
                    false,
                )
                .await?;
            let run_state = self
                .run_state
                .as_mut()
                .ok_or(CoordinatorError::RunNotStarted)?;
            append_payload_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                actor.clone(),
                Some(format!("team:{team_run_id}:member:{}", member.name)),
                EventV1::TeamMemberSpawned(TeamMemberSpawnedEvent {
                    team_run_id: team_run_id.to_string(),
                    member_name: member.name,
                    agent_id,
                    profile,
                }),
            )?;
        }
        Ok(())
    }

    pub(in crate::coord) async fn project_single_team(
        &self,
        team_run_id: &str,
    ) -> Result<TeamRunProjection, CoordinatorError> {
        let mut projection = self.team_projection_internal().await?;
        projection
            .teams
            .remove(team_run_id)
            .ok_or_else(|| CoordinatorError::UnknownTask(format!("team:{team_run_id}")))
    }
}

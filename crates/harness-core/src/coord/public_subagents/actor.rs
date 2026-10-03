use super::*;

impl Runtime {
    pub(super) fn authenticated_native_tool(
        &self,
        actor: &EventActor,
        tool: &str,
        spawn: bool,
    ) -> Result<&Job, CoordinatorError> {
        self.accepting()?;
        self.check_task(tool)?;
        let job = &self.running[tool];
        if job.actor != *actor
            || job.join_id.is_none()
            || !matches!(job.kind, JobKind::Tool { .. })
            || spawn
                && !matches!(
                    job.kind,
                    JobKind::Tool {
                        capability: ToolCapability::SpawnAgent,
                        ..
                    }
                )
        {
            return Err(CoordinatorError::PermissionDenied(
                "native subagent operation requires the authenticated running tool".into(),
            ));
        }
        Ok(job)
    }

    pub(super) fn native_root(&self, agent: &str) -> Result<String, CoordinatorError> {
        if let Some(child) = self.native_subagents.get(agent) {
            return Ok(child.registration.root_agent.clone());
        }
        let mut current = agent;
        loop {
            let state = self
                .agents
                .get(current)
                .ok_or_else(|| CoordinatorError::UnknownAgent(current.into()))?;
            match state.info.parent_agent_id.as_deref() {
                Some(parent) => current = parent,
                None => return Ok(current.to_owned()),
            }
        }
    }

    pub(super) fn native_depth(&self, agent: &str) -> u32 {
        self.subagent_history
            .records
            .get(agent)
            .and_then(|record| record.metadata.as_ref())
            .map_or_else(
                || {
                    if self.native_subagents.contains_key(agent) {
                        1
                    } else {
                        0
                    }
                },
                |metadata| metadata.injected_depth.0,
            )
    }

    pub(in crate::coord) fn native_transition_metadata(
        &self,
        agent: &str,
    ) -> Option<SubagentTransitionMetadataV1> {
        let child = self.native_subagents.get(agent)?;
        let run = self.info.as_ref()?.run_id.to_string();
        let spawner = if child.registration.spawner == child.registration.root_agent {
            run.clone()
        } else {
            child.registration.spawner.clone()
        };
        let injected_depth = self
            .subagent_history
            .records
            .get(agent)
            .and_then(|record| record.metadata.as_ref())
            .map_or_else(
                || {
                    self.native_depth(&child.registration.spawner)
                        .saturating_add(1)
                        .max(1)
                },
                |metadata| metadata.injected_depth.0,
            );
        Some(SubagentTransitionMetadataV1 {
            ancestry: SubagentAncestry::new(
                Some(SubagentId(spawner.clone())),
                Some(SubagentId(run.clone())),
            ),
            execution_owner: SubagentExecutionOwner::RootSession {
                session_id: run.clone(),
            },
            display_route: SubagentDisplayRoute {
                root_session_id: run,
                parent_session_id: if child.routed_to_root {
                    None
                } else {
                    Some(spawner)
                },
                child_session_id: agent.into(),
            },
            notification_route: SubagentNotificationRoute {
                parent_prompt_id: if child.routed_to_root {
                    None
                } else {
                    child.registration.parent_request.clone()
                },
                background: child.registration.background,
                await_to_completion: child.foreground_attached,
                surface_completion: !child.consumed
                    && !child.routed_to_root
                    && child.registration.spawner == child.registration.root_agent,
            },
            // Execution ownership is root-scoped; nesting depth follows the spawner.
            injected_depth: InjectedSubagentDepth(injected_depth),
            isolation_requested: child.registration.isolation,
            context: self.agents.get(agent)?.execution.clone(),
        })
    }

    pub(in crate::coord) fn detach_native_spawn_waiter(
        &mut self,
        id: &str,
        waiter: &str,
    ) -> Result<(), CoordinatorError> {
        if let Some(child) = self.native_subagents.get_mut(id) {
            child.foreground_attached = false;
            child.waiters.remove(waiter);
        }
        for job in self.running.values_mut() {
            if job.parent.as_deref() == Some(waiter) && job.actor.agent_id.as_deref() == Some(id) {
                job.parent = None;
            }
        }
        Ok(())
    }

    pub(in crate::coord) fn native_foreground_children(&self) -> Vec<String> {
        self.native_subagents
            .iter()
            .filter(|(_, child)| child.phase != NativePhase::Terminal && child.foreground_attached)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(in crate::coord) fn native_foreground_handle(
        &self,
        handle: &str,
    ) -> Option<(String, String)> {
        self.native_subagents
            .iter()
            .find(|(id, child)| {
                (id.as_str() == handle || child.request.as_deref() == Some(handle))
                    && child.phase != NativePhase::Terminal
                    && child.foreground_attached
            })
            .map(|(id, child)| {
                (
                    id.clone(),
                    child.request.clone().unwrap_or_else(|| id.clone()),
                )
            })
    }

    pub(in crate::coord) fn native_notification_reservations(&self, agent: &str) -> usize {
        self.native_subagents
            .values()
            .filter(|child| {
                child.registration.spawner == agent
                    && child.registration.background
                    && child.registration.parent_request.is_some()
                    && child.phase != NativePhase::Terminal
            })
            .count()
    }

    pub(in crate::coord) fn native_lineage(
        &self,
        id: &str,
    ) -> Option<(String, Option<String>, String, String)> {
        let child = self.native_subagents.get(id)?;
        let parent = if child.registration.spawner == child.registration.root_agent {
            self.info.as_ref()?.run_id.to_string()
        } else {
            child.registration.spawner.clone()
        };
        Some((
            child.registration.parent_tool.clone(),
            child.registration.parent_request.clone(),
            parent,
            child
                .request
                .clone()
                .or_else(|| self.agents.get(id).and_then(|a| a.attempt.clone()))
                .unwrap_or_default(),
        ))
    }

    pub(in crate::coord) fn fail_native_waiters(&mut self, message: &str) {
        for child in self.native_subagents.values_mut() {
            child.updates.send_modify(|snapshot| {
                snapshot.error = Some(message.into());
                snapshot.completed = None;
                snapshot.terminal = true;
            });
            for pending in child.parked.drain(..) {
                self.native_message_ingress = self.native_message_ingress.saturating_sub(1);
                let _ = pending.reply.send(SendSubagentMessageResult::ChannelClosed);
            }
        }
    }

    pub(in crate::coord) fn detach_native_waiter(&mut self, waiter: &str, _reason: &str) {
        let ids: Vec<_> = self
            .native_subagents
            .iter()
            .filter(|(_, child)| child.registration.parent_tool == waiter)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if self
                .native_subagents
                .get(&id)
                .is_some_and(|child| child.foreground_attached)
            {
                let _ = self.cancel_native_subagent(&id, "foreground waiter cancelled", false);
            }
            let _ = self.detach_native_spawn_waiter(&id, waiter);
        }
        for child in self.native_subagents.values_mut() {
            child.waiters.remove(waiter);
        }
    }

    pub(in crate::coord) fn native_consumed_notifications(
        &self,
        job: &Job,
        result: &Result<ToolResult, CoordinatorError>,
    ) -> Vec<String> {
        let (JobKind::Tool { tool_id, .. }, Ok(output), Some(parent)) =
            (&job.kind, result, job.actor.agent_id.as_deref())
        else {
            return Vec::new();
        };
        if !matches!(
            tool_id.as_str(),
            "spawn_subagent"
                | "get_command_or_subagent_output"
                | "wait_commands_or_subagents"
                | "task"
                | "get_task_output"
                | "wait_tasks"
        ) {
            return Vec::new();
        }
        let Some(value) = &output.structured_json else {
            return Vec::new();
        };
        let reports: Vec<_> = std::iter::once(value)
            .chain(value.get("Result"))
            .chain(
                value
                    .get("MultiResult")
                    .and_then(|v| v.get("results"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten(),
            )
            .collect();
        let ids: Vec<_> = reports
            .iter()
            .filter_map(|report| {
                let id = report
                    .get("task_id")
                    .or_else(|| report.get("subagent_id"))?
                    .as_str()?;
                let child = self.native_subagents.get(id)?;
                (child.registration.spawner == parent && child.phase == NativePhase::Terminal)
                    .then_some(child.request.as_deref())
                    .flatten()
            })
            .collect();
        self.agents
            .get(parent)
            .into_iter()
            .flat_map(|state| &state.queue)
            .filter(|turn| {
                turn.prompt
                    .child_completion
                    .as_deref()
                    .is_some_and(|id| ids.contains(&id))
            })
            .map(|turn| turn.id.clone())
            .collect()
    }

    pub(in crate::coord) fn native_owned_descendants(&self, session: &str) -> Vec<SubagentId> {
        self.native_subagents
            .iter()
            .filter(|(id, child)| {
                if id.as_str() == session || child.registration.root_agent == session {
                    return true;
                }
                let mut current = child.registration.spawner.as_str();
                loop {
                    if current == session {
                        return true;
                    }
                    let Some(parent) = self.native_subagents.get(current) else {
                        return false;
                    };
                    current = &parent.registration.spawner;
                }
            })
            .map(|(id, _)| SubagentId(id.clone()))
            .collect()
    }

    /// Called only after the shared typed intent has been validated and appended.
    /// Display/root routes never become cancellation ancestry.
    pub(in crate::coord) fn native_cancel_scope(
        &mut self,
        command: &SubagentCommandRequest,
    ) -> Result<Vec<SubagentId>, CoordinatorError> {
        let targets: Vec<_> = match command {
            SubagentCommandRequest::ExplicitChildKill { child_id } => self
                .native_subagents
                .contains_key(&child_id.0)
                .then(|| child_id.clone())
                .into_iter()
                .collect(),
            SubagentCommandRequest::ChildSessionCancel { session_id, .. } => {
                self.native_owned_descendants(session_id)
            }
            SubagentCommandRequest::ParentPromptCancel { prompt_id } => self
                .native_subagents
                .iter()
                .filter(|(_, child)| {
                    child.registration.parent_request.as_deref() == Some(prompt_id)
                })
                .map(|(id, _)| SubagentId(id.clone()))
                .collect(),
            SubagentCommandRequest::ParentSessionStop { session_id } => {
                if self.info()?.run_id.as_str() == session_id {
                    self.native_subagents
                        .keys()
                        .cloned()
                        .map(SubagentId)
                        .collect()
                } else {
                    self.native_owned_descendants(session_id)
                }
            }
            SubagentCommandRequest::WaiterCancel { waiter_id } => self
                .native_subagents
                .iter()
                .filter(|(_, child)| {
                    child.registration.parent_tool == *waiter_id && child.foreground_attached
                })
                .map(|(id, _)| SubagentId(id.clone()))
                .collect(),
            SubagentCommandRequest::RootShutdown => self
                .native_subagents
                .keys()
                .cloned()
                .map(SubagentId)
                .collect(),
        };
        for target in &targets {
            self.cancel_native_subagent(
                &target.0,
                "subagent cancellation requested",
                matches!(
                    command,
                    SubagentCommandRequest::ExplicitChildKill { .. }
                        | SubagentCommandRequest::ChildSessionCancel { .. }
                ),
            )?;
        }
        Ok(targets)
    }
}

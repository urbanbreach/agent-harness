use super::*;

impl Runtime {
    pub(in crate::coord) fn pump_native_subagents(&mut self) -> Result<(), CoordinatorError> {
        if self.stopping.is_some() || self.fault.is_some() || self.rewind.is_some() {
            return Ok(());
        }
        if self
            .info
            .as_ref()
            .is_some_and(|info| self.stopped_sessions.contains(info.run_id.as_str()))
        {
            return Ok(());
        }
        let queued = self.native_subagent_queue.len();
        let mut blocked = BTreeSet::new();
        for _ in 0..queued {
            let Some(id) = self.native_subagent_queue.pop_front() else {
                break;
            };
            let Some(child) = self.native_subagents.get(&id) else {
                continue;
            };
            if child.phase != NativePhase::Queued {
                continue;
            }
            if !self.eval_pool_has_capacity(&id) {
                self.native_subagent_queue.push_back(id);
                continue;
            }
            let root = child.registration.root_agent.clone();
            let active = self
                .native_subagents
                .values()
                .filter(|other| {
                    other.registration.root_agent == root
                        && matches!(
                            other.phase,
                            NativePhase::Preparing | NativePhase::Running | NativePhase::Finalizing
                        )
                })
                .count();
            if blocked.contains(&root) || active >= self.config.subagents.max_concurrent {
                blocked.insert(root);
                self.native_subagent_queue.push_back(id);
                continue;
            }
            let preparation = self.id("subagent-prepare")?;
            let child = self
                .native_subagents
                .get_mut(&id)
                .ok_or_else(|| CoordinatorError::UnknownAgent(id.clone()))?;
            child.phase = NativePhase::Preparing;
            child.preparation = Some(preparation.clone());
            let cancellation = child.cancellation.clone();
            let requested = child.registration.isolation;
            let cwd = child.cwd.clone();
            let source = child
                .source_state
                .as_ref()
                .map(|state| state.execution_context.clone());
            let snapshot = child.snapshot_ref.clone();
            let checkpoint = child.creation_checkpoint.clone();
            let parent = child
                .foreground_attached
                .then(|| child.registration.parent_tool.clone());
            let run_dir = self.info()?.run_dir.clone();
            let workspace_root = self.info()?.workspace_root.clone();
            let actor = EventActor::new(ActorKind::Worker, Some(id.clone()));
            let completion_id = preparation.clone();
            let worker_id = id.clone();
            let job_cancellation = cancellation.clone();
            let join = self.jobs.spawn(async move {
                let result = workspace::prepare(
                    &worker_id,
                    &cwd,
                    &workspace_root,
                    &run_dir,
                    requested,
                    source,
                    snapshot,
                    cancellation,
                    checkpoint,
                )
                .await;
                Completion::SubagentPrepared {
                    id: completion_id,
                    result,
                }
            });
            self.running.insert(
                preparation,
                Job {
                    join_id: Some(join.id()),
                    actor,
                    kind: JobKind::SubagentPreparation { agent: id },
                    parent,
                    cancellation: job_cancellation,
                    reason: None,
                    hooks: Vec::new(),
                },
            );
        }
        Ok(())
    }

    pub(in crate::coord) fn finish_native_subagent_preparation(
        &mut self,
        id: String,
        result: Result<PreparedSubagent, CoordinatorError>,
    ) -> Result<(), CoordinatorError> {
        let Some(job) = self.running.remove(&id) else {
            return Ok(());
        };
        let Some(agent) = job.actor.agent_id else {
            return Ok(());
        };
        if self
            .native_subagents
            .get(&agent)
            .is_some_and(|child| child.cleanup_pending)
            && let Some(child) = self.native_subagents.get_mut(&agent)
        {
            child.cleanup_pending = false;
            // Successful cleanup returns the original startup/cancellation
            // error. A cleanup failure retains ownership for diagnosis.
            if result
                .as_ref()
                .is_ok_and(|prepared| prepared.ownership.is_none())
            {
                child.ownership = None;
                child.worktree = None;
            }
        }
        if self
            .native_subagents
            .get(&agent)
            .is_some_and(|child| child.phase == NativePhase::Finalizing)
        {
            if let Ok(prepared) = result
                && let Some(child) = self.native_subagents.get_mut(&agent)
            {
                child.cwd = prepared.cwd;
                child.worktree = prepared.worktree;
                child.snapshot_ref = prepared.snapshot_ref;
                child.ownership = prepared.ownership;
            }
            return self.publish_native_terminal(&agent);
        }
        let result = if job.cancellation.is_cancelled() {
            match result {
                Ok(prepared) if prepared.ownership.is_some() => {
                    return self.cleanup_native_preparation(
                        &agent,
                        prepared,
                        CoordinatorError::Cancelled(
                            job.reason
                                .unwrap_or_else(|| "Subagent was cancelled".into()),
                        ),
                    );
                }
                _ => Err(CoordinatorError::Cancelled(
                    job.reason
                        .unwrap_or_else(|| "Subagent was cancelled".into()),
                )),
            }
        } else {
            result
        };
        match result {
            Ok(mut prepared) => {
                if let Some(error) = prepared.failure.take() {
                    if prepared.ownership.is_some() {
                        return self.cleanup_native_preparation(&agent, prepared, error);
                    }
                    return self.finish_native_before_start(&agent, error);
                }
                if let Some(child) = self.native_subagents.get_mut(&agent) {
                    child.ownership = prepared.ownership.clone();
                }
                let result = self.initialize_native_execution(&agent, prepared);
                if let Err(error) = result {
                    if let Some(ownership) = self
                        .native_subagents
                        .get_mut(&agent)
                        .and_then(|child| child.ownership.take())
                    {
                        let prepared = PreparedSubagent {
                            cwd: self.native_subagents[&agent].cwd.clone(),
                            worktree: Some(ownership.path.clone()),
                            snapshot_ref: None,
                            ownership: Some(ownership),
                            failure: None,
                        };
                        return self.cleanup_native_preparation(&agent, prepared, error);
                    }
                    self.finish_native_before_start(&agent, error)?;
                }
            }
            Err(error) => self.finish_native_before_start(&agent, error)?,
        }
        Ok(())
    }
}

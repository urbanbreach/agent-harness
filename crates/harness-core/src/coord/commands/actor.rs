use super::*;

impl Runtime {
    /// A storage fault settles command observations without claiming a process exit.
    /// Actual process groups remain owned by jobs and are drained by shutdown.
    pub(in crate::coord) fn fail_command_waiters(&mut self, message: &str) {
        let now = self.clock.mono_ms();
        let ended = self.clock.system_time_rfc3339();
        for command in self
            .commands
            .values_mut()
            .filter(|command| !command.snapshot.is_terminal())
        {
            command.snapshot.result.status = "failed".into();
            command.snapshot.result.output = message.into();
            command.snapshot.result.exit_code = None;
            command.snapshot.result.ended = ended.clone();
            command.snapshot.finished_mono_ms = Some(now);
            command.snapshot.result.duration_secs =
                Duration::from_millis(now.saturating_sub(command.snapshot.started_mono_ms))
                    .as_secs_f64();
            command.finished = Some(Instant::now());
            command.updates.send_replace(command.snapshot.clone());
        }
    }

    pub(super) fn authorize_command(
        &self,
        actor: &EventActor,
        command: &CommandRun,
    ) -> Result<(), CoordinatorError> {
        if matches!(actor.kind, ActorKind::System | ActorKind::Supervisor)
            && actor.agent_id.is_none()
            || actor.agent_id == command.snapshot.owner_agent_id
        {
            return Ok(());
        }
        Err(CoordinatorError::PermissionDenied(
            "command is not owned by this actor".into(),
        ))
    }

    pub(super) fn admit_command(
        &mut self,
        context: ToolContext,
        mut prepared: PreparedCommand,
    ) -> Result<CommandSnapshot, CoordinatorError> {
        self.accepting()?;
        let tool = context.tool_call_id.as_str();
        self.check_task(tool)?;
        let job = self
            .running
            .get(tool)
            .ok_or_else(|| CoordinatorError::UnknownTask(tool.into()))?;
        if job.actor != context.actor
            || !matches!(
                job.kind,
                JobKind::Tool {
                    capability: ToolCapability::Shell,
                    ..
                }
            )
            || context.run_id != self.info()?.run_id.as_str()
            || context.workspace_root != self.execution_cwd(&job.actor)?
        {
            return Err(CoordinatorError::PermissionDenied(
                "command admission requires the authenticated running shell tool".into(),
            ));
        }
        let parent = job.parent.clone();
        // Actor dependency edges, not an irreversible parent-token link, own
        // cancellation so surviving child commands can be reparented.
        let cancellation = CancellationToken::new();
        let id = uuid::Uuid::now_v7().to_string();
        crate::store::create_private_dir(&self.info()?.artifacts_dir)?;
        let path = self.info()?.artifacts_dir.join(format!("command-{id}.log"));
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        prepared
            .command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let cwd = prepared
            .command
            .as_std()
            .get_current_dir()
            .unwrap_or(&context.workspace_root)
            .to_string_lossy()
            .into_owned();
        #[cfg(not(unix))]
        return Err(CoordinatorError::Invalid(
            "process-tree control is unavailable on this platform".into(),
        ));
        #[cfg(unix)]
        let (child, group) = crate::process::spawn_group(prepared.command)
            .map_err(|error| CoordinatorError::Invalid(error.to_string()))?;
        #[cfg(unix)]
        {
            let snapshot = CommandSnapshot {
                result: GetCommandOrSubagentOutputResult {
                    task_id: id.clone(),
                    command: self.redactor.redact_text(&prepared.label),
                    status: "running".into(),
                    started: self.clock.system_time_rfc3339().unwrap_or_default(),
                    output_file: path.to_string_lossy().into_owned(),
                    ..Default::default()
                },
                owner_agent_id: context.actor.agent_id.clone(),
                original_owner_agent_id: context.actor.agent_id.clone(),
                parent_tool_call_id: tool.into(),
                parent_task_id: parent.clone(),
                description: prepared
                    .description
                    .map(|description| self.redactor.redact_text(&description)),
                cwd,
                pid: child.id(),
                started_mono_ms: self.clock.mono_ms(),
                finished_mono_ms: None,
                stdout: String::new(),
                stderr: String::new(),
            };
            self.emit(
                context.actor.clone(),
                Some(id.clone()),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: id.clone().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("command".into()),
                    metadata: Some(TaskScheduleMetadata {
                        lineage: Some(TaskLineageMetadata {
                            parent_tool_call_id: Some(tool.into()),
                            parent_task_id: parent.clone(),
                            parent_session_id: Some(context.run_id.clone()),
                            ..Default::default()
                        }),
                    }),
                }),
            )?;
            let (updates, _) = watch::channel(snapshot.clone());
            self.commands.insert(
                id.clone(),
                CommandRun {
                    output_tail: self
                        .config
                        .behavior
                        .command_notifications
                        .enabled
                        .then(String::new),
                    snapshot: snapshot.clone(),
                    updates,
                    finished: None,
                    output: Some(output),
                    file_bytes: 0,
                    pending: [Vec::new(), Vec::new()],
                    discarded: [false, false],
                },
            );
            let handle = self.handle()?;
            let worker_id = id.clone();
            let worker_token = cancellation.clone();
            let worker = self.jobs.spawn(async move {
                let _scratch = prepared.scratch;
                let result = run_command(
                    handle,
                    worker_id.clone(),
                    child,
                    group,
                    prepared.timeout,
                    worker_token,
                )
                .await;
                Completion::Command {
                    id: worker_id,
                    result,
                }
            });
            self.running.insert(
                id,
                Job {
                    join_id: Some(worker.id()),
                    actor: context.actor,
                    kind: JobKind::Command,
                    parent,
                    cancellation,
                    reason: None,
                    hooks: Vec::new(),
                },
            );
            Ok(snapshot)
        }
    }

    /// Surviving native commands move to the parent's session, not its prompt.
    pub(in crate::coord) fn reparent_commands(&mut self, child: &str, parent: Option<&str>) {
        for (id, command) in &mut self.commands {
            if command.snapshot.owner_agent_id.as_deref() != Some(child) {
                continue;
            }
            command.snapshot.owner_agent_id = parent.map(str::to_owned);
            if let Some(job) = self.running.get_mut(id) {
                job.actor.agent_id = parent.map(str::to_owned);
                job.parent = None;
            }
            command.updates.send_replace(command.snapshot.clone());
        }
    }

    pub(super) fn command_bytes(
        &mut self,
        id: &str,
        stream: usize,
        bytes: &[u8],
        ended: bool,
    ) -> Result<(), CoordinatorError> {
        let redactor = Arc::clone(&self.redactor);
        let command = self
            .commands
            .get_mut(id)
            .ok_or_else(|| CoordinatorError::UnknownTask(id.into()))?;
        command.snapshot.result.raw_output_bytes = command
            .snapshot
            .result
            .raw_output_bytes
            .saturating_add(bytes.len());
        if !command.discarded[stream] {
            if bytes.len() > PENDING_LIMIT.saturating_sub(command.pending[stream].len()) {
                // Never publish a possibly secret-bearing partial token.
                command.pending[stream].clear();
                command.discarded[stream] = true;
                command.snapshot.result.truncated = true;
            } else {
                command.pending[stream].extend_from_slice(bytes);
                let valid = if ended {
                    command.pending[stream].len()
                } else {
                    match std::str::from_utf8(&command.pending[stream]) {
                        Ok(text) => text.len(),
                        Err(error) => error.valid_up_to(),
                    }
                };
                let text = String::from_utf8_lossy(&command.pending[stream][..valid]);
                let safe = if ended {
                    text.len()
                } else {
                    redactor.streaming_prefix(&text)
                };
                if safe != 0 {
                    let safe_text = redactor.redact_text(&text[..safe]);
                    command.pending[stream].drain(..safe.min(valid));
                    command.append(stream, &safe_text)?;
                }
            }
        }
        command.snapshot.result.duration_secs = Duration::from_millis(
            self.clock
                .mono_ms()
                .saturating_sub(command.snapshot.started_mono_ms),
        )
        .as_secs_f64();
        command.updates.send_replace(command.snapshot.clone());
        Ok(())
    }

    pub(in crate::coord) fn finish_command(
        &mut self,
        id: String,
        result: Result<CommandExit, CoordinatorError>,
    ) -> Result<(), CoordinatorError> {
        let Some(job) = self.running.remove(&id) else {
            return Ok(());
        };
        for stream in 0..2 {
            self.command_bytes(&id, stream, &[], true)?;
        }
        let command = self
            .commands
            .get(&id)
            .ok_or_else(|| CoordinatorError::UnknownTask(id.clone()))?;
        let mut snapshot = command.snapshot(self.clock.mono_ms());
        snapshot.result.ended = self.clock.system_time_rfc3339();
        snapshot.result.duration_secs = Duration::from_millis(
            self.clock
                .mono_ms()
                .saturating_sub(snapshot.started_mono_ms),
        )
        .as_secs_f64();
        snapshot.finished_mono_ms = Some(self.clock.mono_ms());
        snapshot.result.exit_code = result.as_ref().ok().and_then(|exit| exit.exit_code);
        snapshot.result.status = if job.cancellation.is_cancelled() {
            "cancelled"
        } else if result.as_ref().is_ok_and(|exit| exit.timed_out) {
            "timed_out"
        } else if result.as_ref().is_ok_and(|exit| exit.exit_code == Some(0)) {
            "completed"
        } else {
            "failed"
        }
        .into();
        let summary = match &result {
            Err(error) => self.redactor.redact_text(&error.to_string()),
            Ok(_) => format!(
                "Command {} {} (exit {})",
                snapshot.result.task_id,
                snapshot.result.status,
                snapshot
                    .result
                    .exit_code
                    .map_or_else(|| "signal".into(), |code| code.to_string())
            ),
        };
        let payload = if snapshot.result.status == "completed" {
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: id.clone().into(),
                result_digest: digest(&summary),
                result_summary: summary,
                metadata: Some(TaskCompletionMetadata {
                    timing: Some(ExecutionTimingMetadata {
                        started_mono_ms: Some(snapshot.started_mono_ms),
                        finished_mono_ms: snapshot.finished_mono_ms,
                        elapsed_ms: snapshot
                            .finished_mono_ms
                            .map(|end| end.saturating_sub(snapshot.started_mono_ms)),
                    }),
                    lineage: Some(TaskLineageMetadata {
                        parent_tool_call_id: Some(snapshot.parent_tool_call_id.clone()),
                        parent_task_id: snapshot.parent_task_id.clone(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            })
        } else {
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: id.clone().into(),
                reason: job.reason.unwrap_or(summary),
                failure: snapshot.result.status != "cancelled",
                task_scope: None,
            })
        };
        if let Some(command) = self.commands.get_mut(&id)
            && let Some(mut file) = command.output.take()
        {
            file.flush()?;
            file.sync_data()?;
        }
        // Terminal waiters observe only a durably accepted terminal transition.
        let completed_id = id.clone();
        self.emit_applied(job.actor, Some(id.clone()), payload, move |runtime, _| {
            if let Some(command) = runtime.commands.get_mut(&id) {
                command.snapshot = snapshot;
                command.finished = Some(Instant::now());
                command.updates.send_replace(command.snapshot.clone());
            }
        })?;
        self.notify_command_completed(&completed_id)?;
        // The pinned native terminal retains at most 100 completed snapshots.
        while self
            .commands
            .values()
            .filter(|command| command.finished.is_some())
            .count()
            > 100
        {
            let oldest = self
                .commands
                .iter()
                .filter_map(|(id, command)| command.finished.map(|finished| (id.clone(), finished)))
                .min_by_key(|(_, finished)| *finished)
                .map(|(id, _)| id);
            if let Some(oldest) = oldest {
                self.commands.remove(&oldest);
            }
        }
        Ok(())
    }
}

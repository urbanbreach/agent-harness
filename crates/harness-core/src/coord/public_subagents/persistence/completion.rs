use super::*;

impl Runtime {
    pub(in crate::coord::public_subagents) fn finish_native_before_start(
        &mut self,
        agent: &str,
        error: CoordinatorError,
    ) -> Result<(), CoordinatorError> {
        if self
            .native_subagents
            .get(agent)
            .is_some_and(|child| child.displaced.is_some())
        {
            let child = self
                .native_subagents
                .get_mut(agent)
                .ok_or_else(|| CoordinatorError::UnknownAgent(agent.into()))?;
            if let Some(previous) = child.displaced.take() {
                child.registration = previous.registration;
                child.request = previous.request;
                child.started_ms = previous.started_ms;
                child.started = previous.started;
                child.ended = previous.ended;
                child.phase = NativePhase::Terminal;
                child.terminal_published = true;
                child.cancellation = CancellationToken::new();
                child.updates.send_replace(previous.snapshot);
                for pending in child.parked.drain(..) {
                    self.native_message_ingress = self.native_message_ingress.saturating_sub(1);
                    let _ = pending
                        .reply
                        .send(SendSubagentMessageResult::NotActiveOrFinalizing);
                }
            }
            return self.pump_native_subagents();
        }
        let request = self.id("subagent-rejected")?;
        self.begin_agent_attempt(agent, &request)?;
        if let Some(child) = self.native_subagents.get_mut(agent) {
            child.request = Some(request.clone());
        }
        let cancelled = matches!(error, CoordinatorError::Cancelled(_));
        self.finish_agent_state(
            agent,
            &request,
            &Context::default(),
            false,
            false,
            cancelled,
        )?;
        let terminal = self.emit(
            EventActor::new(ActorKind::Worker, Some(agent.into())),
            Some(request.clone()),
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: request.into(),
                reason: error.to_string(),
                failure: !cancelled,
                task_scope: Some(TaskTerminalScope::AgentTurn),
            }),
        )?;
        self.native_subagent_finished(agent, &terminal)
    }

    pub(in crate::coord) fn native_subagent_finished(
        &mut self,
        agent: &str,
        terminal: &EventEnvelopeV1,
    ) -> Result<(), CoordinatorError> {
        if !self.native_subagents.contains_key(agent) {
            return Ok(());
        }
        if !self.agents[agent].queue.is_empty() {
            // Protected queue messages are later turns of the same public
            // child execution, not an early public terminal result.
            return Ok(());
        }
        let pending = self
            .native_subagents
            .get_mut(agent)
            .map(|child| std::mem::take(&mut child.messages))
            .unwrap_or_default();
        if !pending.is_empty() {
            for receipt in pending {
                let text = format!(
                    "<agent_message sender=\"{}\">\n{}\n</agent_message>",
                    receipt.sender, receipt.text
                );
                self.queue_turn(
                    EventActor::new(ActorKind::Worker, Some(agent.into())),
                    agent,
                    super::super::prompt::Prompt {
                        text,
                        native_subagent: true,
                        ..Default::default()
                    },
                    None,
                    None,
                    None,
                )?;
            }
            return Ok(());
        }
        let (status, output, error, cancelled) = match &terminal.payload {
            EventV1::TaskCompleted(event) => {
                ("completed", event.result_summary.clone(), None, false)
            }
            EventV1::TaskCancelled(event) => (
                if event.failure { "failed" } else { "cancelled" },
                String::new(),
                Some(event.reason.clone()),
                !event.failure,
            ),
            _ => return Ok(()),
        };
        let accounting = self
            .subagent_history
            .records
            .get(agent)
            .and_then(|record| record.accounting);
        let child = self
            .native_subagents
            .get_mut(agent)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent.into()))?;
        child.phase = NativePhase::Finalizing;
        child.ended = Some(native_timestamp(self.clock.as_ref()));
        child.completion_age = terminal.seq;
        child.displaced = None;
        let duration = accounting.as_ref().map_or_else(
            || self.clock.mono_ms().saturating_sub(child.started_ms),
            |a| a.duration_ms,
        );
        let (structured_output, output_errors) = if status == "completed" {
            crate::subagent::output_contract::validate(child.output_contract.as_deref(), &output)
        } else {
            (None, Vec::new())
        };
        let completed = (status == "completed").then(|| SpawnSubagentOutput {
            output: output.clone(),
            structured_output: structured_output.clone(),
            output_errors: output_errors.clone(),
            subagent_id: agent.into(),
            subagent_type: child.registration.subagent_type.clone(),
            tool_calls: accounting.as_ref().map_or(0, |a| a.tool_calls),
            turns: accounting.as_ref().map_or(0, |a| a.turns),
            duration_ms: duration,
            worktree_path: child
                .worktree
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            persona: None,
            resume_from_hint: agent.into(),
            persona_hint: None,
        });
        let body = if let Some(completed) = &completed {
            completed_body(completed)
        } else {
            error
                .clone()
                .unwrap_or_else(|| "Subagent was cancelled".into())
        };
        let snapshot = NativeSnapshot {
            result: GetCommandOrSubagentOutputResult {
                structured_output,
                output_errors,
                task_id: agent.into(),
                command: format!(
                    "[subagent:{}] {}",
                    child.registration.subagent_type, child.registration.description
                ),
                status: status.into(),
                exit_code: if cancelled {
                    None
                } else {
                    Some(if error.is_some() { 1 } else { 0 })
                },
                started: child.started.clone(),
                ended: child.ended.clone(),
                duration_secs: Duration::from_millis(duration).as_secs_f64(),
                raw_output_bytes: body.len(),
                output: body,
                output_file: String::new(),
                truncated: false,
                truncation_hint: String::new(),
            },
            completed,
            error,
            terminal: true,
            demoted: false,
        };
        child.pending_terminal = Some(snapshot);
        child.terminal_event = Some(terminal.clone());
        for pending in child.parked.drain(..) {
            self.native_message_ingress = self.native_message_ingress.saturating_sub(1);
            let _ = pending
                .reply
                .send(SendSubagentMessageResult::NotActiveOrFinalizing);
        }
        self.write_native_projection(agent, true)?;
        let parent = self.native_subagents[agent].registration.root_agent.clone();
        self.reparent_commands(agent, Some(&parent));
        self.transfer_command_notices(agent, &parent)?;
        if self.config.subagents.worktree_snapshot
            && self.native_subagents[agent].worktree.is_some()
        {
            self.start_native_disposal(agent)
        } else {
            self.publish_native_terminal(agent)
        }
    }

    pub(in crate::coord::public_subagents) fn publish_native_terminal(
        &mut self,
        agent: &str,
    ) -> Result<(), CoordinatorError> {
        self.write_native_projection(agent, true)?;
        let child = self
            .native_subagents
            .get_mut(agent)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent.into()))?;
        let terminal = child.terminal_event.take().ok_or_else(|| {
            native_invalid("native terminal publication has no committed event".into())
        })?;
        let mut snapshot = child
            .pending_terminal
            .take()
            .ok_or_else(|| native_invalid("native terminal publication has no result".into()))?;
        if let Some(output) = &mut snapshot.completed {
            output.worktree_path = child
                .worktree
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            snapshot.result.output = completed_body(output);
            snapshot.result.raw_output_bytes = snapshot.result.output.len();
        }
        child.phase = NativePhase::Terminal;
        child.preparation = None;
        child.updates.send_replace(snapshot);
        if self.eval_pool_completed_child(agent)?
            && let Some(child) = self.native_subagents.get_mut(agent)
        {
            child.consumed = true;
        }
        self.native_terminal_publication(agent, &terminal)?;
        self.route_native_survivors(agent)?;
        self.evict_native_completed();
        self.pump_native_subagents()?;
        self.publish_eval_pool_notifications()
    }
}

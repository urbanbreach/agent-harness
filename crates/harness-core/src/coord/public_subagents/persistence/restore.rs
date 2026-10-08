use super::*;

impl Runtime {
    pub(in crate::coord) fn restore_native_profiles(
        &mut self,
        events: &[EventEnvelopeV1],
    ) -> Result<(), CoordinatorError> {
        for event in events {
            if let EventV1::NativeSubagentRegistered(registration) = &event.payload {
                if registration.payload_version != 1 {
                    return Err(native_invalid(
                        "unsupported native registration version".into(),
                    ));
                }
                let profile = registration.profile();
                self.profiles.insert(
                    format!("native:{}", registration.child_id),
                    Arc::clone(&profile),
                );
                self.profiles
                    .entry(registration.subagent_type.clone())
                    .or_insert(profile);
            }
        }
        Ok(())
    }

    /// Read-only reconstruction. Only explicit live reconciliation may repair
    /// subordinate files or append interruption transitions afterward.
    pub(in crate::coord) fn restore_native_subagents(
        &mut self,
        events: &[EventEnvelopeV1],
    ) -> Result<(), CoordinatorError> {
        let registrations: BTreeMap<_, _> = events
            .iter()
            .filter_map(|event| {
                if let EventV1::NativeSubagentRegistered(registration) = &event.payload {
                    Some((
                        registration.child_id.clone(),
                        (registration.as_ref().clone(), event),
                    ))
                } else {
                    None
                }
            })
            .collect();
        for (id, (registration, event)) in registrations {
            let Some(agent) = self.agents.get(&id) else {
                continue;
            };
            let record = self.subagent_history.records.get(&id);
            let terminal = record.is_some_and(|record| record.lifecycle.is_finished());
            let result = NativeSnapshot {
                result: GetCommandOrSubagentOutputResult {
                    task_id: id.clone(),
                    command: format!(
                        "[subagent:{}] {}",
                        registration.subagent_type, registration.description
                    ),
                    status: if terminal {
                        "cancelled"
                    } else {
                        "initializing"
                    }
                    .into(),
                    started: event.ts.clone().unwrap_or_default(),
                    output: if terminal {
                        "Subagent output is unavailable in this observation".into()
                    } else {
                        "Subagent interrupted by process restart".into()
                    },
                    ..Default::default()
                },
                completed: None,
                error: None,
                terminal,
                demoted: false,
            };
            let request = agent.attempt.clone();
            let mut child = NativeSubagent {
                output_contract: registration
                    .output_schema
                    .as_ref()
                    .map(crate::subagent::output_contract::compile)
                    .transpose()
                    .map_err(native_invalid)?,
                registration,
                resolved: None,
                phase: if terminal {
                    NativePhase::Terminal
                } else {
                    NativePhase::Queued
                },
                request,
                cancellation: CancellationToken::new(),
                foreground_attached: false,
                explicitly_killed: self.killed_agents.contains(&id),
                terminal_published: terminal,
                updates: watch::channel(result).0,
                started_ms: event.mono_ms,
                started: event.ts.clone().unwrap_or_default(),
                ended: None,
                cwd: agent.cwd.clone(),
                worktree: match &agent.execution.isolation {
                    ResolvedSubagentIsolation::Worktree { path } => Some(PathBuf::from(path)),
                    _ => None,
                },
                snapshot_ref: None,
                preparation: None,
                source_state: None,
                source_reference: None,
                fork_context: None,
                fork_read_state: None,
                messages: VecDeque::new(),
                outbound: 0,
                sender_generation: agent.generation,
                waiters: BTreeSet::new(),
                completion_age: event.seq,
                consumed: false,
                wait_interrupt: watch::channel(0).0,
                parked: VecDeque::new(),
                displaced: None,
                terminal_event: None,
                pending_terminal: None,
                buffered_for: None,
                routed_to_root: record
                    .and_then(|r| r.metadata.as_ref())
                    .is_some_and(|m| m.display_route.parent_session_id.is_none()),
                creation_checkpoint: None,
                ownership: None,
                cleanup_pending: false,
                progress: None,
            };
            let mut snapshot = child.updates.borrow().clone();
            for event in events.iter().filter(|event| {
                event.actor.agent_id.as_deref() == Some(&id)
                    || matches!(&event.payload, EventV1::NativeSubagentReceipt(receipt)
                            if receipt.child_id == id)
            }) {
                let cancellation_status = if matches!(&event.payload, EventV1::TaskCancelled(terminal) if terminal.failure)
                {
                    "failed"
                } else {
                    "cancelled"
                };
                match &event.payload {
                    EventV1::TaskCompleted(terminal)
                        if Some(terminal.task_id.as_str()) == child.request.as_deref() =>
                    {
                        let accounting = record.and_then(|record| record.accounting);
                        let duration = accounting.as_ref().map_or(0, |a| a.duration_ms);
                        let (structured_output, output_errors) =
                            crate::subagent::output_contract::validate(
                                child.output_contract.as_deref(),
                                &terminal.result_summary,
                            );
                        let output = SpawnSubagentOutput {
                            output: terminal.result_summary.clone(),
                            structured_output,
                            output_errors,
                            subagent_id: id.clone(),
                            subagent_type: child.registration.subagent_type.clone(),
                            tool_calls: accounting.as_ref().map_or(0, |a| a.tool_calls),
                            turns: accounting.as_ref().map_or(0, |a| a.turns),
                            duration_ms: duration,
                            worktree_path: child
                                .worktree
                                .as_ref()
                                .map(|p| p.to_string_lossy().into_owned()),
                            persona: None,
                            resume_from_hint: id.clone(),
                            persona_hint: None,
                        };
                        snapshot.result.output = completed_body(&output);
                        snapshot.result.structured_output = output.structured_output.clone();
                        snapshot.result.output_errors = output.output_errors.clone();
                        snapshot.result.raw_output_bytes = snapshot.result.output.len();
                        snapshot.completed = Some(output);
                        snapshot.result.status = "completed".into();
                        snapshot.result.exit_code = Some(0);
                        snapshot.result.ended = event.ts.clone();
                        snapshot.result.duration_secs =
                            Duration::from_millis(duration).as_secs_f64();
                        snapshot.terminal = true;
                        child.ended = event.ts.clone();
                        child.phase = NativePhase::Terminal;
                    }
                    EventV1::TaskCancelled(terminal)
                        if Some(terminal.task_id.as_str()) == child.request.as_deref() =>
                    {
                        snapshot.error = Some(terminal.reason.clone());
                        snapshot.result.output = terminal.reason.clone();
                        snapshot.result.status = cancellation_status.into();
                        snapshot.result.exit_code = terminal.failure.then_some(1);
                        snapshot.result.ended = event.ts.clone();
                        snapshot.terminal = true;
                        child.ended = event.ts.clone();
                        child.phase = NativePhase::Terminal;
                    }
                    EventV1::NativeSubagentReceipt(receipt)
                        if matches!(
                            receipt.kind.as_str(),
                            "notification_consumed" | "completion_reminder_delivered"
                        ) =>
                    {
                        child.consumed = true
                    }
                    EventV1::NativeSubagentReceipt(receipt)
                        if receipt.kind == "terminal_published" =>
                    {
                        child.terminal_published = true
                    }
                    _ => {}
                }
            }
            for event in events {
                let EventV1::NativeSubagentWorkspace(receipt) = &event.payload else {
                    continue;
                };
                if receipt.child_id != id {
                    continue;
                }
                child.snapshot_ref = Some(receipt.snapshot_ref.clone());
                if receipt.removed {
                    child.worktree = None;
                }
            }
            if let Some(output) = &mut snapshot.completed {
                output.worktree_path = child
                    .worktree
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned());
                snapshot.result.output = completed_body(output);
                snapshot.result.raw_output_bytes = snapshot.result.output.len();
            }
            child.updates.send_replace(snapshot);
            self.native_subagents.insert(id, child);
        }
        self.evict_native_completed();
        Ok(())
    }
}

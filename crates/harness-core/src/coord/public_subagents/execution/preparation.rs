use super::*;

mod queue;

impl Runtime {
    fn cleanup_native_preparation(
        &mut self,
        agent: &str,
        prepared: PreparedSubagent,
        error: CoordinatorError,
    ) -> Result<(), CoordinatorError> {
        let id = self.id("subagent-workspace-cleanup")?;
        let completion_id = id.clone();
        let actor = EventActor::new(ActorKind::Worker, Some(agent.into()));
        if let Some(child) = self.native_subagents.get_mut(agent) {
            child.ownership = prepared.ownership.clone();
            child.worktree = prepared.worktree.clone();
            child.preparation = Some(id.clone());
            child.cleanup_pending = true;
        }
        let join = self.jobs.spawn(async move {
            let cwd = prepared.cwd.clone();
            let result = workspace::cleanup(prepared)
                .await
                .map(|()| PreparedSubagent {
                    cwd,
                    worktree: None,
                    snapshot_ref: None,
                    ownership: None,
                    failure: Some(error),
                });
            Completion::SubagentPrepared {
                id: completion_id,
                result,
            }
        });
        // Cleanup is deliberately not cancelled by the child token. Runtime
        // shutdown drains this existing actor job before releasing ownership.
        self.running.insert(
            id,
            Job {
                join_id: Some(join.id()),
                actor,
                kind: JobKind::SubagentPreparation {
                    agent: agent.into(),
                },
                parent: None,
                cancellation: CancellationToken::new(),
                reason: None,
                hooks: Vec::new(),
            },
        );
        Ok(())
    }

    fn initialize_native_execution(
        &mut self,
        agent: &str,
        prepared: PreparedSubagent,
    ) -> Result<(), CoordinatorError> {
        let cwd = prepared.cwd.canonicalize()?;
        let root = self.info()?.workspace_root.clone();
        let worktree = prepared.worktree.clone();
        if !cwd.is_dir() || (!cwd.starts_with(&root) && worktree.as_ref() != Some(&cwd)) {
            return Err(CoordinatorError::PermissionDenied(
                "child cwd is outside its accepted policy roots".into(),
            ));
        }
        let skill_startup = self.native_skill_startup(agent, &cwd)?;
        let system_prompt = self
            .native_system_prompt(agent, &cwd)?
            .unwrap_or_else(|| self.agents[agent].profile.system_prompt.clone());
        let child = self
            .native_subagents
            .get(agent)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent.into()))?;
        let mut roots = vec![root.to_string_lossy().into_owned()];
        if let Some(worktree) = &worktree {
            roots.push(worktree.to_string_lossy().into_owned());
        }
        let execution = ResolvedSubagentContext {
            effective_cwd: cwd.to_string_lossy().into_owned(),
            policy_roots: roots,
            isolation: worktree.as_ref().map_or(
                ResolvedSubagentIsolation::SharedWorkspace,
                |path| ResolvedSubagentIsolation::Worktree {
                    path: path.to_string_lossy().into_owned(),
                },
            ),
        };
        let mut context = Context::new(&system_prompt);
        let mut copied = None;
        if let Some(source) = &child.source_state {
            let mut source = source.clone();
            context = super::super::subagents::restored_context(
                &mut source,
                &system_prompt,
                &self.info()?.run_dir,
            )?;
            let window = self.native_model_window(&source.source_model);
            let percent = self
                .config
                .compaction
                .threshold_percent
                .map_or(95, |p| u64::from(p.get()).min(95));
            let limit = window.saturating_mul(percent) / 100;
            if source.conversation_items.is_empty()
                || window == 0
                || u64::from(context.tokens()) > limit
            {
                return Err(native_invalid(format!(
                    "Cannot resume from subagent '{}': source transcript (~{} tokens) exceeds the resume limit ({limit} of {window} tokens). Compact the source first, or resume on a model with a larger context window.",
                    child.registration.source.as_deref().unwrap_or(agent), context.tokens(),
                )));
            }
            copied = Some(source);
        } else if let Some(fork) = &child.fork_context {
            context = fork_context(
                fork,
                &system_prompt,
                self.native_model_window(&child.registration.model),
            );
        }
        if child.registration.source.as_deref() != Some(agent) {
            context.native_context_usage =
                super::super::super::context::native_tokens(&context.entries).map(|total_tokens| {
                    crate::subagent::SubagentContextUsage {
                        total_tokens,
                        estimate_at_last_response: Some(total_tokens),
                        estimate_after_last_response: Some(total_tokens),
                    }
                });
        }
        let fork_reads = child.fork_read_state.clone();
        let actor = EventActor::new(ActorKind::Worker, Some(agent.into()));
        let owner = agent.to_owned();
        let next_context = execution.clone();
        let next_cwd = cwd.clone();
        self.emit_applied(
            actor.clone(),
            None,
            EventV1::AgentExecutionContextChanged(AgentExecutionContextChangedV1 {
                payload_version: 1,
                agent_id: SubagentId(agent.into()),
                context: execution,
                system_prompt: Some(system_prompt.clone()),
            }),
            move |runtime, _| {
                if let Some(state) = runtime.agents.get_mut(&owner) {
                    state.execution = next_context;
                    state.cwd = next_cwd;
                    state.messages = context;
                    state.skill_startup = skill_startup;
                    Arc::make_mut(&mut state.profile).system_prompt = system_prompt;
                }
            },
        )?;
        if let Some(reads) = fork_reads {
            let tool_state = self
                .tool_state
                .with_read_snapshot(reads)
                .map_err(CoordinatorError::from)?;
            if let Some(state) = self.agents.get_mut(agent) {
                state.tool_state = tool_state;
            }
        }
        if let Some(source) = copied {
            let reads = self
                .tool_state
                .with_read_snapshot(source.read_state.clone())
                .map_err(|error| native_invalid(error.to_string()))?;
            let reference = self.native_subagents[agent]
                .source_reference
                .clone()
                .ok_or_else(|| native_invalid("source reference is unavailable".into()))?;
            self.emit(
                actor.clone(),
                None,
                EventV1::AgentContextInitialized(AgentContextInitializedV1 {
                    payload_version: 1,
                    agent_id: SubagentId(agent.into()),
                    mode: if self.native_subagents[agent].registration.source.as_deref()
                        == Some(agent)
                    {
                        FinalizedContextCopy::Wake
                    } else {
                        FinalizedContextCopy::Resume
                    },
                    source: reference.clone(),
                }),
            )?;
            if let Some(state) = self.agents.get_mut(agent) {
                state.tool_state = reads;
                state.info.model_ref = source.source_model;
                state.settings = source.model_settings;
                state.target = source.model_target;
                state.source_reference = Some(Box::new(reference));
            }
        } else if let Some(resolved) = self.native_subagents[agent].resolved.as_ref() {
            let settings = resolved.reasoning_effort.clone();
            let target = self.native_model_target(&resolved.model).or_else(|| {
                let parent = &self.agents[&self.native_subagents[agent].registration.root_agent];
                (parent.info.model_ref == resolved.model)
                    .then(|| parent.target.clone())
                    .flatten()
            });
            if let Some(state) = self.agents.get_mut(agent) {
                state.settings.reasoning_effort = settings;
                state.target = target;
            }
        }
        if let Some(child) = self.native_subagents.get_mut(agent) {
            child.cwd = cwd;
            child.worktree = prepared.worktree;
            child.snapshot_ref = prepared.snapshot_ref;
            child.ownership = prepared.ownership;
            child.preparation = None;
        }
        self.write_native_projection(agent, false)?;
        let prompt = self.native_subagents[agent].registration.prompt.clone();
        let request = self.queue_turn(
            actor,
            agent,
            super::super::prompt::Prompt {
                text: prompt,
                native_subagent: true,
                ..Default::default()
            },
            None,
            None,
            None,
        )?;
        if let Some(child) = self.native_subagents.get_mut(agent) {
            child.request = Some(request.clone());
            if child.foreground_attached {
                if let Some(job) = self.running.get_mut(&request) {
                    job.parent = Some(child.registration.parent_tool.clone());
                }
            }
        }
        Ok(())
    }

    pub(in crate::coord) fn native_subagent_started(
        &mut self,
        agent: &str,
        request: &str,
    ) -> Result<(), CoordinatorError> {
        let Some(child) = self.native_subagents.get_mut(agent) else {
            return Ok(());
        };
        let continuing = child.phase == NativePhase::Running;
        child.phase = NativePhase::Running;
        child.request = Some(request.into());
        if !continuing {
            child.outbound = 0;
            child.progress = Some(super::super::progress::Publisher::new());
        }
        child.sender_generation = self.agents[agent].generation;
        child.terminal_published = false;
        child.updates.send_modify(|snapshot| {
            snapshot.terminal = false;
            snapshot.demoted = false;
            snapshot.completed = None;
            snapshot.error = None;
            snapshot.result.status = "running".into();
            snapshot.result.output = "Subagent running".into();
            snapshot.result.ended = None;
        });
        self.promote_native_message_admissions(agent)?;
        if let Some(child) = self.native_subagents.get_mut(agent) {
            child.displaced = None;
        }
        self.write_native_projection(agent, false)?;
        Ok(())
    }
}

fn fork_context(source: &Context, system: &str, window: u64) -> Context {
    let coherent = source.entries.last().is_some_and(|entry| {
        entry.message.role == MessageRole::Assistant
            && entry
                .message
                .assistant_tool_calls
                .as_ref()
                .is_none_or(Vec::is_empty)
    });
    let mut result = source.clone();
    if coherent && window > 0 && u64::from(source.tokens()) <= window.saturating_mul(80) / 100 {
        result
            .entries
            .retain(|entry| entry.message.role != MessageRole::System);
        let mut head = Context::new(system);
        head.entries.append(&mut result.entries);
        head.usage = result.usage;
        return head;
    }
    // Never invent a tool result to complete a dangling logical request.
    let complete_end = source
        .entries
        .iter()
        .rposition(|entry| {
            entry.message.role == MessageRole::Assistant
                && entry
                    .message
                    .assistant_tool_calls
                    .as_ref()
                    .is_none_or(Vec::is_empty)
        })
        .map_or(0, |index| index + 1);
    let complete = &source.entries[..complete_end];
    let starts: Vec<_> = complete
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.message.role == MessageRole::User)
        .map(|(index, _)| index)
        .collect();
    let keep_from = starts
        .get(starts.len().saturating_sub(3))
        .copied()
        .unwrap_or(0);
    let older = complete[..keep_from]
        .iter()
        .filter(|entry| entry.message.role != MessageRole::System)
        .map(|entry| {
            format!(
                "{:?}: {}",
                entry.message.role,
                strip_wrappers(&entry.message.content)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let recent = complete[keep_from..]
        .iter()
        .filter(|entry| entry.message.role != MessageRole::System)
        .chain(
            source.entries[complete_end..]
                .iter()
                .filter(|entry| entry.message.role == MessageRole::User),
        )
        .map(|entry| {
            format!(
                "{:?}: {}",
                entry.message.role,
                strip_wrappers(&entry.message.content)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut context = Context::new(system);
    if !older.is_empty() || !recent.is_empty() {
        context.push(
            harness_providers::CompletionMessage::text(
                MessageRole::User,
                format!("<background_context>\n{older}\n{recent}\n</background_context>"),
            ),
            0,
            None,
        );
    }
    context
}

fn strip_wrappers(value: &str) -> String {
    let mut text = value.to_owned();
    for tag in [
        "system-reminder",
        "system_reminder",
        "user_info",
        "git_status",
        "project_layout",
        "attached_files",
    ] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        while let Some(start) = text.find(&open) {
            let Some(end) = text[start..].find(&close) else {
                break;
            };
            text.replace_range(start..start + end + close.len(), "");
        }
    }
    text
}

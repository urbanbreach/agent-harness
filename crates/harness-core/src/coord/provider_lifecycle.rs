// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use super::agent_turn_runtime::{
    provider_request_finished_metadata, provider_request_started_metadata,
};
use super::*;

impl Coordinator {
    pub(in crate::coord) async fn agent_provider_request_started_internal(
        &mut self,
        args: AgentProviderRequestStartedArgs,
    ) -> Result<(), CoordinatorError> {
        let AgentProviderRequestStartedArgs {
            task_id,
            agent_id,
            request_id,
            provider_id,
            model_id,
            prompt_summary,
            request_digest,
            metadata,
            model_target,
        } = args;
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(running) = run_state.running_agent_turns.get(&task_id) else {
            return Ok(());
        };
        let turn_request_id = running.request_id.clone();
        let profile = running.profile.clone();
        let cancellation_token = running.cancellation_token.clone();
        let parent_agent_id = run_state.subagent_parent_by_id.get(&agent_id).cloned();
        let context_budget = metadata
            .as_ref()
            .and_then(|metadata| metadata.context_budget);
        if parent_agent_id.is_none() {
            if let Some(target) = model_target.as_ref() {
                let mut context = RecordedRuntimeContext::from_model_target(
                    profile.as_deref().unwrap_or("default"),
                    target,
                );
                context.last_request_budget = context_budget;
                run_state.recorded_runtime_context = Some(context);
                write_run_metadata(run_state, &self.config, self.clock.as_ref())?;
            } else if let (Some(context), Some(snapshot)) =
                (run_state.recorded_runtime_context.as_mut(), context_budget)
            {
                context.last_request_budget = Some(snapshot);
                write_run_metadata(run_state, &self.config, self.clock.as_ref())?;
            }
        }
        let provider_id_for_state = provider_id.clone();
        let model_id_for_state = model_id.clone();
        let metadata = provider_request_started_metadata(metadata, &turn_request_id, &request_id);

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(turn_request_id.clone()),
            EventV1::ProviderRequestStarted(crate::event::ProviderRequestStartedEvent {
                request_id: request_id.clone().into(),
                provider_id: provider_id.clone(),
                model_id: model_id.clone(),
                prompt_summary: prompt_summary.clone(),
                request_digest,
                metadata,
            }),
        )?;

        let hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::ProviderRequestStarted,
                run_id: run_state.info.run_id.to_string(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(agent_actor(&agent_id)),
                agent_id: Some(agent_id.clone()),
                request_id: Some(turn_request_id.clone()),
                permission_id: None,
                task_id: Some(task_id.clone()),
                tool_call_id: None,
                tool_id: None,
                provider_id: Some(provider_id),
                model_id: Some(model_id),
                parent_agent_id,
                profile,
                outcome: Some("started".to_string()),
                output_summary: Some(prompt_summary),
                failure_reason: None,
            },
        )
        .await;
        if let Some(running) = run_state.running_agent_turns.get_mut(&task_id) {
            running.latest_provider_request_id = Some(request_id.clone());
            running.latest_provider_id = Some(provider_id_for_state);
            running.latest_model_id = Some(model_id_for_state);
            running
                .hook_executions
                .extend(hook_batch.hook_executions.clone());
        }
        if let Some(reason) = hook_batch.critical_failure {
            cancellation_token.cancel();
            if run_state.cancelled_running_tasks.insert(task_id.clone()) {
                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    agent_actor(&agent_id),
                    Some(format!("task:{task_id}")),
                    Some(turn_request_id),
                    EventV1::TaskCancelled(TaskCancelledEvent {
                        task_id: task_id.into(),
                        reason,
                        task_scope: Some(TaskTerminalScope::AgentTurn),
                    }),
                )?;
            }
        }

        Ok(())
    }

    pub(in crate::coord) fn agent_provider_live_event_internal(
        &mut self,
        task_id: String,
        agent_id: String,
        payload: LiveEventV1,
    ) -> Result<(), CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(turn_request_id) = run_state
            .running_agent_turns
            .get(&task_id)
            .map(|running| running.request_id.clone())
        else {
            return Ok(());
        };

        let builder = crate::event::EventBuilder::new(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state.info.run_id.to_string(),
        );
        publish_live_event(
            &builder,
            run_state,
            LiveEventPublishArgs {
                actor: agent_actor(&agent_id),
                stream_key: Some(format!("agent:{agent_id}")),
                correlation_id: Some(turn_request_id),
                payload,
            },
        )?;

        Ok(())
    }

    pub(in crate::coord) async fn agent_provider_request_finished_internal(
        &mut self,
        args: AgentProviderRequestFinishedArgs,
    ) -> Result<(), CoordinatorError> {
        let AgentProviderRequestFinishedArgs {
            task_id,
            agent_id,
            request_id,
            finish_reason,
            output_digest,
            usage,
            metadata,
        } = args;

        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(());
        };

        let Some(running) = run_state.running_agent_turns.get(&task_id) else {
            return Ok(());
        };
        let turn_request_id = running.request_id.clone();
        let profile = running.profile.clone();
        let cancellation_token = running.cancellation_token.clone();
        let parent_agent_id = run_state.subagent_parent_by_id.get(&agent_id).cloned();
        let usage_for_state = usage.clone();
        let metadata = provider_request_finished_metadata(metadata, &turn_request_id, &request_id);

        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(turn_request_id.clone()),
            EventV1::ProviderRequestFinished(crate::event::ProviderRequestFinishedEvent {
                request_id: request_id.clone().into(),
                finish_reason: finish_reason.clone(),
                output_digest: output_digest.clone(),
                usage: usage.clone(),
                metadata,
            }),
        )?;

        let hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::ProviderRequestFinished,
                run_id: run_state.info.run_id.to_string(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(agent_actor(&agent_id)),
                agent_id: Some(agent_id.clone()),
                request_id: Some(turn_request_id.clone()),
                permission_id: None,
                task_id: Some(task_id.clone()),
                tool_call_id: None,
                tool_id: None,
                provider_id: None,
                model_id: None,
                parent_agent_id,
                profile,
                outcome: Some(finish_reason.clone()),
                output_summary: output_digest,
                failure_reason: None,
            },
        )
        .await;
        if let Some(running) = run_state.running_agent_turns.get_mut(&task_id) {
            running.latest_provider_usage = usage_for_state;
            running
                .hook_executions
                .extend(hook_batch.hook_executions.clone());
        }
        if let Some(reason) = hook_batch.critical_failure {
            cancellation_token.cancel();
            if run_state.cancelled_running_tasks.insert(task_id.clone()) {
                append_payload_event_with_correlation(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    agent_actor(&agent_id),
                    Some(format!("task:{task_id}")),
                    Some(turn_request_id),
                    EventV1::TaskCancelled(TaskCancelledEvent {
                        task_id: task_id.into(),
                        reason,
                        task_scope: Some(TaskTerminalScope::AgentTurn),
                    }),
                )?;
            }
        }

        Ok(())
    }

    pub(in crate::coord) async fn agent_assistant_message_finished_internal(
        &mut self,
        task_id: String,
        agent_id: String,
        response: Box<AssistantResponse>,
    ) -> Result<Vec<crate::ids::ToolCallId>, CoordinatorError> {
        let Some(run_state) = self.run_state.as_mut() else {
            return Ok(Vec::new());
        };

        let Some(turn_request_id) = run_state
            .running_agent_turns
            .get(&task_id)
            .map(|running| running.request_id.clone())
        else {
            return Ok(Vec::new());
        };

        let tool_call_ids = response
            .tool_intents
            .iter()
            .map(|_| {
                let id = format!("toolcall_{:06}", run_state.next_tool_call_id);
                run_state.next_tool_call_id += 1;
                crate::ids::ToolCallId::new(id)
            })
            .collect::<Vec<_>>();
        let builder = crate::event::EventBuilder::new(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state.info.run_id.to_string(),
        );
        let finished =
            semantic_history::assistant_message_finished_event(&builder, &response, &tool_call_ids);
        append_payload_event_with_correlation(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            agent_actor(&agent_id),
            Some(format!("agent:{agent_id}")),
            Some(turn_request_id),
            EventV1::AssistantMessageFinished(finished),
        )?;

        if let Some(running) = run_state.running_agent_turns.get_mut(&task_id) {
            running.latest_assistant_output = Some(response.text.clone());
        }

        if !response.tool_intents.is_empty() {
            let request_id = response.request_id.to_string();
            if let Err(err) = self.snapshot_workspace_internal(request_id.clone()).await {
                tracing::warn!(error = %err, request_id, "failed to snapshot workspace before tool batch");
            }
        }

        Ok(tool_call_ids)
    }
}

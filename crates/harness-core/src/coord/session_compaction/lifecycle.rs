use std::sync::Arc;

use super::super::*;
use super::pipeline::generate_session_compaction;
use super::prepared::{prepare_session_compaction, SessionCompactionPreparationRequest};
use super::request_context::compaction_start_context;

impl Coordinator {
    pub(in crate::coord) async fn prepare_idle_compaction(
        &mut self,
        agent_id: &str,
        warm_when_idle: bool,
    ) {
        if warm_when_idle
            && matches!(
                self.config.session_mode_source,
                Some(
                    crate::proj::SessionModeSource::InteractiveLive
                        | crate::proj::SessionModeSource::InteractiveMock
                )
            )
        {
            self.start_compaction_generation(
                CompactAgentContextRequest {
                    task_id: None,
                    agent_id: agent_id.to_string(),
                    through_request_id: None,
                    trigger_reason: "idle".to_string(),
                    evidence: CompactionRequestEvidence::default(),
                },
                PendingCompactionResponse::Internal {
                    trigger_reason: "idle".to_string(),
                },
            )
            .await;
        }
    }

    pub(in crate::coord) async fn start_compaction_generation(
        &mut self,
        request: CompactAgentContextRequest,
        response: PendingCompactionResponse,
    ) {
        let Some(run_state) = self.run_state.as_ref() else {
            response.finish(Err(CoordinatorError::RunNotStarted));
            return;
        };
        let start = match compaction_start_context(run_state, &request) {
            Ok(start) => start,
            Err(error) => {
                response.finish(Err(error));
                return;
            }
        };

        let prepared = {
            let Some(run_state) = self.run_state.as_ref() else {
                response.finish(Err(CoordinatorError::RunNotStarted));
                return;
            };
            prepare_session_compaction(SessionCompactionPreparationRequest {
                run_state,
                agent_id: &request.agent_id,
                trigger_reason: &request.trigger_reason,
                settings: &self.config.compaction,
                prepared_budget: request.evidence.context_budget,
            })
            .await
        };
        let mut prepared = match prepared {
            Ok(Some(prepared)) => prepared,
            Ok(None) if request.trigger_reason == "overflow" => {
                let reason = "overflow requested compaction, but no cut point reduced the active session context"
                    .to_string();
                response.finish(Err(CoordinatorError::CompactionFailed(reason)));
                return;
            }
            Ok(None) => {
                response.finish(Ok(CompactAgentContextResult::NoOp {
                    context: start.existing_context,
                }));
                return;
            }
            Err(error) => {
                response.finish(Err(error));
                return;
            }
        };

        if let Some(instructions) = request
            .evidence
            .custom_instructions
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            prepared.summary_prompt.push_str("\n\nAdditional focus: ");
            prepared.summary_prompt.push_str(instructions.trim());
        }
        let Some(run_state) = self.run_state.as_mut() else {
            response.finish(Err(CoordinatorError::RunNotStarted));
            return;
        };
        if request.evidence.custom_instructions.is_some()
            && run_state
                .pending_compactions
                .get(&request.agent_id)
                .is_some_and(|pending| pending.background)
        {
            if let Some(pending) = run_state.pending_compactions.remove(&request.agent_id) {
                pending.cancellation_token.cancel();
            }
        }
        if let Some(pending) = run_state.pending_compactions.get_mut(&request.agent_id) {
            if pending.background && prepared.required && !prepared.within_grace {
                pending.response = response;
                pending.background = false;
                pending.task_id = request.task_id;
                pending.trigger = start.trigger;
                pending.request_budget = prepared.request_budget;
            } else if pending.background {
                response.finish(Ok(CompactAgentContextResult::NoOp {
                    context: start.existing_context,
                }));
            } else {
                response.finish(Err(CoordinatorError::CompactionInProgress {
                    agent_id: request.agent_id,
                }));
            }
            return;
        }
        let state = run_state
            .compaction_state
            .entry(request.agent_id.clone())
            .or_default();
        let now = self.clock.mono_ms();
        if request.trigger_reason != "manual" && state.tripped(now) {
            response.finish(if request.trigger_reason == "overflow" {
                Err(CoordinatorError::CompactionFailed(
                    "automatic compaction cooling down after repeated failures".to_string(),
                ))
            } else {
                Ok(CompactAgentContextResult::NoOp {
                    context: start.existing_context,
                })
            });
            return;
        }
        let mut warm = state
            .warm
            .take()
            .filter(|_| request.evidence.custom_instructions.is_none());
        if warm.as_mut().is_some_and(|generated| {
            generated
                .rebase(
                    &prepared.committed_events,
                    prepared.request_budget,
                    &prepared.model,
                )
                .is_err()
        }) {
            warm = None;
        }
        if !prepared.required
            && (warm.is_some()
                || state
                    .last_started
                    .is_some_and(|started| now < started.saturating_add(30_000)))
        {
            state.warm = warm;
            response.finish(Ok(CompactAgentContextResult::NoOp {
                context: start.existing_context,
            }));
            return;
        }
        let requested_hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
            &self.config.hook_runtime_config,
            start.hook_context,
        )
        .await;
        if let Some(reason) = requested_hook_batch.critical_failure {
            response.finish(Err(CoordinatorError::LifecycleHookFailed(reason)));
            return;
        }

        let Some(run_state) = self.run_state.as_mut() else {
            response.finish(Err(CoordinatorError::RunNotStarted));
            return;
        };
        let profile = &run_state.agents[&request.agent_id];
        prepared.summary_tools = match crate::agent::build_provider_tool_defs_for_model(
            profile,
            self.config.tool_registry.as_ref(),
            &format!("{}:{}", prepared.model.provider_id, prepared.model.model_id),
        ) {
            Ok(tools) => Some(tools),
            Err(error) => {
                response.finish(Err(CoordinatorError::CompactionFailed(error)));
                return;
            }
        };
        let generation = run_state.next_compaction_generation();
        let cancellation_token = run_state.shutdown_token.child_token();
        let base = CompactionGenerationBase::capture(run_state, prepared.durable_agent_tail_seq);
        let background = !prepared.required;
        let response = if background {
            response.finish(Ok(CompactAgentContextResult::NoOp {
                context: start.existing_context,
            }));
            PendingCompactionResponse::Internal {
                trigger_reason: request.trigger_reason.clone(),
            }
        } else {
            response
        };
        let pending = PendingCompactionState {
            agent_id: request.agent_id.clone(),
            task_id: if background { None } else { request.task_id },
            generation,
            base,
            cancellation_token: cancellation_token.clone(),
            trigger: start.trigger,
            response,
            background,
            allow_appended: background,
            request_budget: prepared.request_budget,
        };
        run_state
            .compaction_state
            .entry(request.agent_id.clone())
            .or_default()
            .last_started = Some(now);
        let _ = run_state
            .pending_compactions
            .insert(request.agent_id.clone(), pending);
        if let Some(generated) = warm {
            self.compaction_generated_internal(
                request.agent_id,
                generation,
                Box::new(Ok(generated)),
            )
            .await;
            return;
        }

        self.publish_compaction_progress(&request.agent_id, generation.0, Some(String::new()));

        let provider = Arc::clone(&self.config.provider);
        let job_tx = self.job_tx.clone();
        let progress = super::summary::SummaryProgress {
            job_tx: job_tx.clone(),
            agent_id: request.agent_id.clone(),
            generation: generation.0,
        };
        tokio::spawn(async move {
            let result =
                generate_session_compaction(provider, prepared, cancellation_token, Some(progress))
                    .await;
            warn_command_send_failure(
                job_tx
                    .send(Command::CompactionGenerated(CompactionGeneratedCommand {
                        agent_id: request.agent_id,
                        generation,
                        result: Box::new(result),
                    }))
                    .await,
                "compaction_generated",
            );
        });
    }

    pub(in crate::coord) fn publish_compaction_progress(
        &mut self,
        agent_id: &str,
        generation: u64,
        preview: Option<String>,
    ) {
        let Some(run_state) = self.run_state.as_mut() else {
            return;
        };
        let pending = run_state.pending_compactions.get(agent_id);
        if preview.is_some()
            && !pending.is_some_and(|pending| {
                pending.generation.0 == generation && !pending.cancellation_token.is_cancelled()
            })
        {
            return;
        }
        let trigger_reason = pending
            .map(|pending| pending.trigger.trigger_reason.clone())
            .unwrap_or_default();
        let builder = crate::event::EventBuilder::new(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state.info.run_id.to_string(),
        );
        if let Err(error) = publish_live_event(
            &builder,
            run_state,
            LiveEventPublishArgs {
                actor: agent_actor(agent_id),
                stream_key: Some(format!("agent:{agent_id}")),
                correlation_id: None,
                payload: LiveEventV1::CompactionProgress {
                    agent_id: agent_id.to_string(),
                    generation,
                    trigger_reason,
                    preview,
                },
            },
        ) {
            tracing::warn!(%error, "compaction feedback unavailable");
        }
    }
}

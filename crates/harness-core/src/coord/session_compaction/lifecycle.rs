use std::sync::Arc;

use super::super::*;
use super::pipeline::generate_session_compaction;
use super::prepared::{prepare_session_compaction, SessionCompactionPreparationRequest};
use super::request_context::compaction_start_context;

impl Coordinator {
    pub(in crate::coord) async fn start_compaction_generation(
        &mut self,
        request: CompactAgentContextRequest,
        response: PendingCompactionResponse,
    ) {
        let Some(run_state) = self.run_state.as_ref() else {
            response.finish(Err(CoordinatorError::RunNotStarted));
            return;
        };
        if run_state
            .pending_compactions
            .contains_key(&request.agent_id)
        {
            response.finish(Err(CoordinatorError::CompactionInProgress {
                agent_id: request.agent_id,
            }));
            return;
        }
        let start = match compaction_start_context(run_state, &request) {
            Ok(start) => start,
            Err(error) => {
                response.finish(Err(error));
                return;
            }
        };

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
        let prepared = match prepared {
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

        let Some(run_state) = self.run_state.as_mut() else {
            response.finish(Err(CoordinatorError::RunNotStarted));
            return;
        };
        let generation = run_state.next_compaction_generation();
        let cancellation_token = run_state.shutdown_token.child_token();
        let base = CompactionGenerationBase::capture(run_state, prepared.durable_agent_tail_seq);
        let pending = PendingCompactionState {
            agent_id: request.agent_id.clone(),
            task_id: request.task_id,
            generation,
            base,
            cancellation_token: cancellation_token.clone(),
            trigger: start.trigger,
            response,
        };
        let _ = run_state
            .pending_compactions
            .insert(request.agent_id.clone(), pending);

        let provider = Arc::clone(&self.config.provider);
        let job_tx = self.job_tx.clone();
        tokio::spawn(async move {
            let result = generate_session_compaction(provider, prepared, cancellation_token).await;
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
}

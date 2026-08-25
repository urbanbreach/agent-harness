use std::sync::Arc;

use harness_providers::{CompletionUsage, Provider};
use tokio_util::sync::CancellationToken;

use crate::clock::Clock;
use crate::event::{EventV1, SessionCompactionEvent};
use crate::redact::Redactor;

use super::super::compaction::format_file_operations;
use super::super::provider_context::reconstruct_provider_context_from_events;
use super::super::{append_payload_event, system_actor, CoordinatorError, RunState};
use super::prepared::PreparedSessionCompaction;
use super::summary::{generate_summary, SummaryGenerationRequest};
use super::validation::{
    post_compaction_history_tokens, validate_post_compaction_request, PostCompactionRequest,
};
use super::AppliedCompaction;

#[derive(Debug)]
pub(in crate::coord) struct GeneratedSessionCompaction {
    prepared: PreparedSessionCompaction,
    summary: String,
    tokens_after: u32,
    summary_usage: Option<CompletionUsage>,
    summary_provider_id: String,
    summary_model_id: String,
}

impl GeneratedSessionCompaction {
    pub(in crate::coord) fn agent_id(&self) -> &str {
        &self.prepared.agent_id
    }

    pub(in crate::coord) fn refresh_committed_events(
        &mut self,
        committed_events: Vec<crate::event::EventEnvelopeV1>,
    ) {
        self.prepared.committed_events = committed_events;
    }

    pub(in crate::coord) fn commit<C, R>(
        self,
        clock: &C,
        redactor: &R,
        run_state: &mut RunState,
    ) -> Result<AppliedCompaction, CoordinatorError>
    where
        C: Clock + ?Sized,
        R: Redactor + ?Sized,
    {
        let mut prepared = self.prepared;
        let current_model = super::preparation::determine_model_ref(run_state, &prepared.agent_id);
        validate_post_compaction_request(PostCompactionRequest {
            agent_id: &prepared.agent_id,
            prepared_model: &prepared.model,
            current_model_ref: &current_model,
            generated_provider_id: &self.summary_provider_id,
            generated_model_id: &self.summary_model_id,
            request_budget: prepared.request_budget,
            tokens_before: prepared.tokens_before,
            retained_history_tokens: prepared.preserved_message_tokens,
            summary: &self.summary,
        })?;
        reconstruct_provider_context_from_events(&prepared.committed_events, &prepared.agent_id)
            .map_err(|error| CoordinatorError::CompactionFailed(error.to_string()))?;
        let agent_id = prepared.agent_id.clone();
        let committed = append_payload_event(
            clock,
            redactor,
            run_state,
            system_actor(),
            Some(format!("compaction:{agent_id}")),
            EventV1::SessionCompaction(SessionCompactionEvent {
                agent_id: agent_id.clone(),
                summary: self.summary.clone(),
                first_kept_event_seq: prepared.first_kept_event_seq,
                first_kept_request_id: prepared.first_kept_request_id,
                first_kept_entry_id: prepared.first_kept_entry_id,
                tokens_before: prepared.tokens_before,
                tokens_after: Some(self.tokens_after),
                summary_usage: self.summary_usage,
                summary_provider_id: Some(self.summary_provider_id),
                summary_model_id: Some(self.summary_model_id),
                read_files: prepared.read_files,
                modified_files: prepared.modified_files,
                current_intent: prepared.current_intent,
                trigger_reason: prepared.trigger_reason,
                from_hook: false,
            }),
        )?;
        prepared.committed_events.push(committed);
        let context =
            reconstruct_provider_context_from_events(&prepared.committed_events, &agent_id)
                .map_err(|error| CoordinatorError::CompactionFailed(error.to_string()))?;
        run_state
            .provider_context_by_agent
            .insert(agent_id, context);
        run_state.advance_compaction_boundary();
        Ok(AppliedCompaction {
            summary: self.summary,
            first_kept_event_seq: prepared.first_kept_event_seq,
            tokens_before: prepared.tokens_before,
            tokens_after: self.tokens_after,
        })
    }
}

pub(in crate::coord) async fn generate_session_compaction(
    provider: Arc<dyn Provider>,
    prepared: PreparedSessionCompaction,
    cancellation: CancellationToken,
) -> Result<GeneratedSessionCompaction, CoordinatorError> {
    let generated = generate_summary(
        &provider,
        SummaryGenerationRequest {
            provider_id: &prepared.model.provider_id,
            model_id: &prepared.model.model_id,
            user_prompt: &prepared.summary_prompt,
            max_tokens: prepared.summary_max_tokens,
        },
        &cancellation,
    )
    .await
    .map_err(|error| CoordinatorError::CompactionFailed(error.to_string()))?;
    let mut summary = generated.text.into_string();
    let mut summary_usage = generated.usage;
    let summary_provider_id = generated.provider_id;
    let summary_model_id = generated.model_id;

    if let Some(turn_prefix_prompt) = prepared.turn_prefix_prompt.as_deref() {
        let turn_prefix = generate_summary(
            &provider,
            SummaryGenerationRequest {
                provider_id: &prepared.model.provider_id,
                model_id: &prepared.model.model_id,
                user_prompt: turn_prefix_prompt,
                max_tokens: prepared.summary_max_tokens,
            },
            &cancellation,
        )
        .await
        .map_err(|error| CoordinatorError::CompactionFailed(error.to_string()))?;
        summary_usage = combined_usage(summary_usage, turn_prefix.usage);
        summary = format!(
            "{summary}\n\n---\n\n**Turn Context (split turn):**\n\n{}",
            turn_prefix.text.as_str()
        );
    }
    summary.push_str(&format_file_operations(
        &prepared.read_files,
        &prepared.modified_files,
    ));
    let tokens_after = post_compaction_history_tokens(&summary, prepared.preserved_message_tokens);

    Ok(GeneratedSessionCompaction {
        prepared,
        summary,
        tokens_after,
        summary_usage,
        summary_provider_id,
        summary_model_id,
    })
}

fn combined_usage(
    summary: Option<CompletionUsage>,
    prefix: Option<CompletionUsage>,
) -> Option<CompletionUsage> {
    match (summary, prefix) {
        (Some(summary), Some(prefix)) => Some(CompletionUsage {
            prompt_tokens: summary.prompt_tokens.saturating_add(prefix.prompt_tokens),
            completion_tokens: summary
                .completion_tokens
                .saturating_add(prefix.completion_tokens),
            total_tokens: summary.total_tokens.saturating_add(prefix.total_tokens),
        }),
        (None, _) | (_, None) => None,
    }
}

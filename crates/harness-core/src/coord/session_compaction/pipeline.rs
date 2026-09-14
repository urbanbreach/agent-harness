use std::collections::BTreeMap;
use std::sync::Arc;

use harness_providers::{CompletionUsage, Provider};
use tokio_util::sync::CancellationToken;

use crate::clock::Clock;
use crate::event::{EventV1, SessionCompactionEvent};
use crate::redact::Redactor;

use super::super::compaction::format_file_operations;
use super::super::provider_context::recover_canonical_provider_context_from_events;
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
    task_intent: Option<String>,
    deterministic: bool,
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

    pub(in crate::coord) fn rebase(
        &mut self,
        events: &[crate::event::EventEnvelopeV1],
        request_budget: crate::context_budget::RequestBudgetSnapshot,
        model: &crate::agent::AgentModelRef,
    ) -> Result<(), CoordinatorError> {
        let original = &self.prepared.committed_events;
        if *model != self.prepared.model
            || !events.starts_with(original)
            || events[original.len()..]
                .iter()
                .any(|event| match &event.payload {
                    EventV1::SessionCompaction(data) => data.agent_id == self.prepared.agent_id,
                    EventV1::BranchSummary(data) => data.agent_id == self.prepared.agent_id,
                    _ => false,
                })
        {
            return Err(CoordinatorError::CompactionStale {
                agent_id: self.prepared.agent_id.clone(),
            });
        }
        let mut messages =
            super::preparation::build_agent_conversation_messages(events, &self.prepared.agent_id);
        messages.retain(|message| {
            !matches!(
                message,
                crate::conversation::ConversationMessage::Checkpoint(_)
            )
        });
        let added_tokens = messages
            .iter()
            .filter(|message| {
                super::preparation::message_seq(message)
                    > self.prepared.durable_agent_tail_seq.unwrap_or(0)
            })
            .map(super::super::compaction::estimate_message_tokens)
            .fold(0, u32::saturating_add);
        if added_tokens > self.prepared.warm_max_growth {
            return Err(CoordinatorError::CompactionStale {
                agent_id: self.prepared.agent_id.clone(),
            });
        }
        self.prepared.preserved_message_tokens = messages
            .iter()
            .filter(|message| {
                super::preparation::message_seq(message) >= self.prepared.first_kept_event_seq
            })
            .map(|message| {
                super::super::compaction::estimate_admitted_message_tokens(
                    message,
                    self.prepared.context_window,
                )
            })
            .fold(0, u32::saturating_add);
        self.prepared.tokens_before = self
            .prepared
            .tokens_before
            .max(request_budget.occupied_input_tokens);
        self.prepared.request_budget = request_budget;
        self.tokens_after =
            post_compaction_history_tokens(&self.summary, self.prepared.preserved_message_tokens);
        Ok(())
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
        let runtime_fallbacks = compaction_runtime_fallbacks(&prepared);
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
                summary_provider_id: (!self.deterministic).then_some(self.summary_provider_id),
                summary_model_id: (!self.deterministic).then_some(self.summary_model_id),
                read_files: prepared.read_files,
                modified_files: prepared.modified_files,
                task_intent: self.task_intent,
                current_intent: prepared.current_intent,
                trigger_reason: prepared.trigger_reason,
                from_hook: false,
            }),
        )?;
        prepared.committed_events.push(committed);
        let mut recovery = recover_canonical_provider_context_from_events(
            &prepared.committed_events,
            Vec::new(),
            run_state.info.run_id.as_ref(),
            &runtime_fallbacks,
        )
        .map_err(|error| CoordinatorError::CompactionFailed(error.to_string()))?;
        let recovered = recovery.by_agent.remove(&agent_id).ok_or_else(|| {
            CoordinatorError::CompactionFailed(format!(
                "canonical provider recovery omitted compacted agent `{agent_id}`"
            ))
        })?;
        run_state.install_canonical_provider_recovery(recovered.view, recovered.context);
        run_state.advance_compaction_boundary();
        Ok(AppliedCompaction {
            summary: self.summary,
            first_kept_event_seq: prepared.first_kept_event_seq,
            tokens_before: prepared.tokens_before,
            tokens_after: self.tokens_after,
        })
    }
}

fn compaction_runtime_fallbacks(
    prepared: &PreparedSessionCompaction,
) -> BTreeMap<String, crate::session::CanonicalRuntimeSelection> {
    let selection = prepared.committed_events.iter().rev().find_map(|event| {
        if event.actor.agent_id.as_deref() != Some(prepared.agent_id.as_str()) {
            return None;
        }
        let EventV1::ProviderRequestStarted(started) = &event.payload else {
            return None;
        };
        started
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.runtime_selection.as_deref())
            .cloned()
    });
    selection
        .map(|selection| BTreeMap::from([(prepared.agent_id.clone(), selection)]))
        .unwrap_or_default()
}

pub(in crate::coord) async fn generate_session_compaction(
    provider: Arc<dyn Provider>,
    mut prepared: PreparedSessionCompaction,
    cancellation: CancellationToken,
    progress: Option<super::summary::SummaryProgress>,
) -> Result<GeneratedSessionCompaction, CoordinatorError> {
    let generated = generate_summary(
        &provider,
        SummaryGenerationRequest {
            provider_id: &prepared.model.provider_id,
            model_id: &prepared.model.model_id,
            user_prompt: &prepared.summary_prompt,
            max_tokens: prepared.summary_max_tokens,
            progress: progress.as_ref(),
            messages: &prepared.summary_messages,
            tools: prepared.summary_tools.as_deref(),
            context_window: prepared.context_window,
        },
        &cancellation,
    )
    .await;
    let inherited_intent =
        prepared
            .committed_events
            .iter()
            .rev()
            .find_map(|event| match &event.payload {
                EventV1::SessionCompaction(data) if data.agent_id == prepared.agent_id => {
                    data.task_intent.clone()
                }
                _ => None,
            });
    let (mut summary, task_intent, summary_usage, deterministic) = match generated {
        Ok(generated) => (
            generated.text.into_string(),
            generated.task_intent.or(inherited_intent),
            generated.usage,
            false,
        ),
        Err(super::summary_reducer::SummaryGenerationError::Cancelled) => {
            return Err(CoordinatorError::CompactionCancelled {
                agent_id: prepared.agent_id.clone(),
                reason: "generation cancelled".to_string(),
            })
        }
        Err(error)
            if prepared.required
                && matches!(
                    error,
                    super::summary_reducer::SummaryGenerationError::IdleTimeout
                        | super::summary_reducer::SummaryGenerationError::DurationBudget
                        | super::summary_reducer::SummaryGenerationError::MissingTerminal
                        | super::summary_reducer::SummaryGenerationError::EmptyOutput
                        | super::summary_reducer::SummaryGenerationError::IncompleteOutput(_)
                        | super::summary_reducer::SummaryGenerationError::Provider {
                            category: Some(
                                harness_providers::ProviderErrorCategory::ContextWindowExceeded
                            ),
                            ..
                        }
                ) =>
        {
            let mut summary = "[Deterministic compaction recovery checkpoint]\nGenerated summarization did not complete, so older context was reduced without another provider request.\nContinue from the retained messages after this checkpoint. Treat omitted transcript details as unknown.".to_string();
            if let Some(intent) = &inherited_intent {
                summary.push_str(&format!("\n\nTask intent:\n{intent}"));
            }
            if let Some(previous) = super::preparation::find_previous_summary(
                &prepared.committed_events,
                &prepared.agent_id,
            ) {
                let budget = usize::try_from(prepared.context_window.saturating_mul(2) / 5)
                    .unwrap_or(usize::MAX)
                    .max(1024)
                    .saturating_sub(summary.len() + 64);
                summary.push_str("\n\nPrevious checkpoint:\n");
                summary.push_str(&previous[..previous.floor_char_boundary(budget)]);
                if previous.len() > budget {
                    summary.push_str("\n[Older checkpoint truncated]");
                }
            }
            (summary, inherited_intent, None, true)
        }
        Err(error) => return Err(CoordinatorError::CompactionFailed(error.to_string())),
    };
    let summary_provider_id = prepared.model.provider_id.clone();
    let summary_model_id = prepared.model.model_id.clone();

    if deterministic {
        preserve_fallback_user_turn(&mut prepared, &summary);
    }
    summary.push_str(&format_file_operations(
        &prepared.read_files,
        &prepared.modified_files,
    ));
    summary.push_str(&super::preparation::restoration_context(
        &prepared, &summary,
    ));
    let tokens_after = post_compaction_history_tokens(&summary, prepared.preserved_message_tokens);

    Ok(GeneratedSessionCompaction {
        prepared,
        summary,
        task_intent,
        deterministic,
        tokens_after,
        summary_usage,
        summary_provider_id,
        summary_model_id,
    })
}

fn preserve_fallback_user_turn(prepared: &mut PreparedSessionCompaction, summary: &str) {
    use crate::conversation::ConversationMessage;
    let messages = super::preparation::build_agent_conversation_messages(
        &prepared.committed_events,
        &prepared.agent_id,
    );
    let Some(user) = messages
        .iter()
        .rev()
        .find_map(|message| match message {
            ConversationMessage::User(user) => Some(user),
            _ => None,
        })
        .filter(|user| {
            user.seq
                .is_some_and(|seq| seq < prepared.first_kept_event_seq)
        })
    else {
        return;
    };
    let Some(seq) = user.seq else {
        return;
    };
    let retained = messages
        .iter()
        .filter(|message| {
            !matches!(message, ConversationMessage::Checkpoint(_))
                && super::preparation::message_seq(message) >= seq
        })
        .map(|message| {
            super::super::compaction::estimate_admitted_message_tokens(
                message,
                prepared.context_window,
            )
        })
        .fold(0_u32, u32::saturating_add);
    if validate_post_compaction_request(PostCompactionRequest {
        agent_id: &prepared.agent_id,
        prepared_model: &prepared.model,
        current_model_ref: &format!("{}:{}", prepared.model.provider_id, prepared.model.model_id),
        generated_provider_id: &prepared.model.provider_id,
        generated_model_id: &prepared.model.model_id,
        request_budget: prepared.request_budget,
        tokens_before: prepared.tokens_before,
        retained_history_tokens: retained,
        summary,
    })
    .is_err()
    {
        return;
    }
    let Some(event) = prepared
        .committed_events
        .iter()
        .find(|event| event.seq == seq)
    else {
        return;
    };
    let Some(entry_id) =
        crate::session::EventIdentityNamespace::new(&event.run_id).source_entry_id(event)
    else {
        return;
    };
    prepared.first_kept_event_seq = seq;
    prepared.first_kept_request_id = Some(user.request_id.to_string());
    prepared.first_kept_entry_id = Some(entry_id);
    prepared.preserved_message_tokens = retained;
}

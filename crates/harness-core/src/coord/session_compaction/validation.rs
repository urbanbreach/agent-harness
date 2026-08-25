use crate::agent::AgentModelRef;
use crate::context_budget::RequestBudgetSnapshot;
use crate::coord::compaction::estimate_text_tokens;

use super::super::CoordinatorError;

pub(super) struct PostCompactionRequest<'a> {
    pub(super) agent_id: &'a str,
    pub(super) prepared_model: &'a AgentModelRef,
    pub(super) current_model_ref: &'a str,
    pub(super) generated_provider_id: &'a str,
    pub(super) generated_model_id: &'a str,
    pub(super) request_budget: RequestBudgetSnapshot,
    pub(super) tokens_before: u32,
    pub(super) retained_history_tokens: u32,
    pub(super) summary: &'a str,
}

pub(super) fn post_compaction_history_tokens(summary: &str, retained_history_tokens: u32) -> u32 {
    estimate_text_tokens(summary).saturating_add(retained_history_tokens)
}

pub(super) fn validate_post_compaction_request(
    request: PostCompactionRequest<'_>,
) -> Result<(), CoordinatorError> {
    let current_model = AgentModelRef::parse(request.current_model_ref);
    let generation_matches_preparation = request.generated_provider_id
        == request.prepared_model.provider_id
        && request.generated_model_id == request.prepared_model.model_id;
    if !generation_matches_preparation || current_model != *request.prepared_model {
        return Err(CoordinatorError::CompactionStale {
            agent_id: request.agent_id.to_string(),
        });
    }

    let components = request.request_budget.components;
    let fixed_input_tokens = [
        components.system_tokens,
        components.tools_tokens,
        components.attachments_tokens,
        components.framing_tokens,
        components.pending_prompt_tokens,
    ]
    .into_iter()
    .fold(0_u32, u32::saturating_add);
    let post_input_tokens = fixed_input_tokens.saturating_add(post_compaction_history_tokens(
        request.summary,
        request.retained_history_tokens,
    ));
    match request.request_budget.compaction_threshold_tokens {
        Some(threshold) if post_input_tokens >= threshold => {
            return Err(CoordinatorError::CompactionFailed(format!(
                "generated summary does not fit current-model request: occupied input {post_input_tokens} meets or exceeds threshold {threshold}"
            )));
        }
        None if post_input_tokens >= request.tokens_before => {
            return Err(CoordinatorError::CompactionFailed(format!(
                "generated summary does not reduce active history: occupied input {post_input_tokens} is not below prior input {}",
                request.tokens_before
            )));
        }
        Some(_) | None => {}
    }
    Ok(())
}

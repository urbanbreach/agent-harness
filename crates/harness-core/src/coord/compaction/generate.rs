//! Model calls for compaction: candidate selection, retries, folded summaries and handoffs.
use super::super::{context::Context, turn::Worker, *};
use super::progress::Progress;
use crate::event::CompactionPhase;
use harness_compaction::{
    prepare::Preparation,
    prompts::{self, SummaryPrompt},
    request, serialize, CompactionMethod, Item,
};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, ProviderErrorCategory,
    ProviderRequestContext, ProviderRequestInitiator, ToolChoice,
};

mod candidates;
pub(in crate::coord) mod extract;
mod handoff;
mod summary;
mod transport;
mod walk;

pub(in crate::coord::compaction) use candidates::{Candidate, NativeBoundary, WalkMode};
pub(in crate::coord::compaction) use extract::{into_coordinator, is_cancelled};
pub(in crate::coord::compaction) use handoff::HandoffDocument;
pub(in crate::coord::compaction) use summary::SummaryInputs;

/// One summary-side response. The provider-abort flag and the endpoint's own display summary
/// and extra fields ride along for callers; generation itself uses the text only, because a
/// remote `shortSummary` never replaces the independently generated display summary.
#[derive(Default)]
#[allow(
    dead_code,
    reason = "Carried for callers that own abort classification or endpoint extras"
)]
pub(super) struct Side {
    pub text: String,
    pub usage: Option<CompletionUsage>,
    pub aborted: bool,
    pub short_summary: Option<String>,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

pub(super) struct SoftOutput {
    pub summary: String,
    /// The display summary, exactly as generated; `None` only when generation was skipped.
    pub short: Option<String>,
    pub usage: Option<CompletionUsage>,
    pub provider_id: String,
    pub model_id: String,
}

/// Where summary text comes from: a model, or a configured remote summary endpoint.
#[derive(Clone, Copy)]
pub(super) enum Transport<'a> {
    Model,
    Endpoint(&'a str),
}

fn usage_sum(total: &mut Option<CompletionUsage>, next: Option<CompletionUsage>) {
    if let Some(next) = next {
        let total = total.get_or_insert_with(CompletionUsage::default);
        for (sum, add) in [
            (&mut total.prompt_tokens, next.prompt_tokens),
            (&mut total.completion_tokens, next.completion_tokens),
            (&mut total.total_tokens, next.total_tokens),
            (&mut total.input_tokens, next.input_tokens),
            (&mut total.output_tokens, next.output_tokens),
            (&mut total.cache_read_tokens, next.cache_read_tokens),
            (&mut total.cache_write_tokens, next.cache_write_tokens),
        ] {
            *sum = sum.saturating_add(add);
        }
    }
}

impl Worker {
    /// Resolves a `provider:model` reference against configured targets and the catalog.
    pub(super) async fn resolve_target(
        &self,
        reference: &str,
    ) -> Result<Option<ResolvedModelTarget>, CoordinatorError> {
        let parsed = crate::agent::AgentModelRef::parse(reference);
        self.handle
            .call(move |s| {
                let configured = s
                    .config
                    .agent_model_targets
                    .values()
                    .chain(s.config.agent_model_fallbacks.values().flatten())
                    .find(|t| t.provider == parsed.provider_id && t.model == parsed.model_id)
                    .cloned();
                Ok(configured.or_else(|| {
                    s.config
                        .model_catalog
                        .iter()
                        .find(|e| e.provider == parsed.provider_id && e.model == parsed.model_id)
                        .map(catalog_target)
                }))
            })
            .await
    }

    pub(super) async fn live_target(&self) -> Result<ResolvedModelTarget, CoordinatorError> {
        if let Some(target) = self.turn.target.clone() {
            return Ok(target);
        }
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        self.handle
            .call(move |s| {
                let selection = crate::session::CanonicalRuntimeSelection {
                    provider_id: model.provider_id,
                    model_id: model.model_id,
                    ..Default::default()
                };
                Ok(crate::coord::resume::restored_target(
                    &selection, None, &s.config,
                ))
            })
            .await
    }
}

fn catalog_target(entry: &crate::config::ResolvedModelCatalogEntry) -> ResolvedModelTarget {
    ResolvedModelTarget {
        model_ref: format!("{}:{}", entry.provider, entry.model),
        provider: entry.provider.clone(),
        model: entry.model.clone(),
        variant: entry.variant.clone(),
        reasoning_effort: entry.reasoning_effort.clone(),
        text_verbosity: entry.text_verbosity.clone(),
        reasoning_summary: None,
        thinking: entry.thinking.clone(),
        limits: entry.limits.clone(),
        resolution: entry.resolution.clone(),
        catalog_entry: Some(Box::new(entry.clone())),
    }
}

/// A native lane failure as a coordinator failure that keeps its classification.
pub(super) fn native_error(error: harness_providers::NativeCompactionError) -> CoordinatorError {
    CoordinatorError::Compaction(error.into())
}

#[cfg(test)]
mod tests;

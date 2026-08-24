use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use harness_core::UnwrapOrAbort;
use harness_providers::{
    CompletionRequest, Provider, ProviderBudgetSemantics, ProviderEventStream,
    ProviderRequestCostError, ProviderStreamEvent,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BudgetObservation {
    pub(super) model_id: String,
    pub(super) message_digest: String,
    pub(super) max_tokens: Option<u32>,
}

#[derive(Clone)]
pub(super) struct BudgetObservingProvider {
    requests: Arc<Mutex<Vec<CompletionRequest>>>,
    observations: Arc<Mutex<Vec<BudgetObservation>>>,
    scripted_events: Arc<Mutex<VecDeque<Vec<ProviderStreamEvent>>>>,
}

impl BudgetObservingProvider {
    pub(super) fn new(scripted_events: Vec<Vec<ProviderStreamEvent>>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            observations: Arc::new(Mutex::new(Vec::new())),
            scripted_events: Arc::new(Mutex::new(scripted_events.into())),
        }
    }

    pub(super) fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().unwrap_or_abort().clone()
    }

    pub(super) fn observations(&self) -> Vec<BudgetObservation> {
        self.observations.lock().unwrap_or_abort().clone()
    }

    pub(super) fn clear_observations(&self) {
        self.observations.lock().unwrap_or_abort().clear();
    }
}

#[async_trait]
impl Provider for BudgetObservingProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
        let message_bytes = serde_json::to_vec(&request.messages).unwrap_or_abort();
        let message_digest = blake3::hash(&message_bytes)
            .to_hex()
            .chars()
            .take(12)
            .collect();
        self.observations
            .lock()
            .unwrap_or_abort()
            .push(BudgetObservation {
                model_id: request.model_id.clone(),
                message_digest,
                max_tokens: request.max_tokens,
            });
        harness_providers::generic_request_budget_semantics(request, pending_prompt_index)
    }

    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        self.requests.lock().unwrap_or_abort().push(request);
        let events = self
            .scripted_events
            .lock()
            .unwrap_or_abort()
            .pop_front()
            .unwrap_or_else(|| vec![ProviderStreamEvent::error("missing scripted response")]);
        Box::pin(tokio_stream::iter(events))
    }
}

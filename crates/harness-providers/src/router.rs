use crate::{
    CompletionRequest, Provider, ProviderBudgetSemantics, ProviderEventStream,
    ProviderRequestCostError, ProviderSessionEvent, ProviderStreamEvent,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, PoisonError, RwLock},
};

pub struct ProviderRouter {
    providers: RwLock<BTreeMap<String, Arc<dyn Provider>>>,
}
impl ProviderRouter {
    pub fn new(providers: BTreeMap<String, Arc<dyn Provider>>) -> Self {
        Self {
            providers: RwLock::new(providers),
        }
    }
    fn get(&self, id: Option<&str>) -> Option<Arc<dyn Provider>> {
        let providers = self
            .providers
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        match id {
            Some(id) => providers.get(id),
            None => providers.get("default").or_else(|| {
                if providers.len() == 1 {
                    providers.values().next()
                } else {
                    None
                }
            }),
        }
        .cloned()
    }
}
#[async_trait::async_trait]
impl Provider for ProviderRouter {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending: usize,
    ) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
        self.get(request.provider_id.as_deref())
            .ok_or(ProviderRequestCostError::ProviderUnavailable)?
            .request_budget_semantics(request, pending)
    }
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        match self.get(request.provider_id.as_deref()) {
            Some(provider) => provider.stream_completion(request).await,
            None => unknown_provider(),
        }
    }
    async fn stream_completion_abortable(
        &self,
        request: CompletionRequest,
        abort: tokio_util::sync::CancellationToken,
    ) -> ProviderEventStream {
        match self.get(request.provider_id.as_deref()) {
            Some(provider) => provider.stream_completion_abortable(request, abort).await,
            None => unknown_provider(),
        }
    }
    fn manages_context(&self, request: &CompletionRequest) -> bool {
        self.get(request.provider_id.as_deref())
            .is_some_and(|provider| provider.manages_context(request))
    }
    /// Every provider hears every session event: one that lost the session must learn it.
    fn session_event(&self, event: &ProviderSessionEvent) {
        let providers: Vec<_> = self
            .providers
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        for provider in providers {
            provider.session_event(event);
        }
    }
    /// Replaces same-named providers: a fresh login can carry credentials the old one lacked.
    fn add_providers(&self, providers: BTreeMap<String, Arc<dyn Provider>>) {
        self.providers
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(providers);
    }
}

fn unknown_provider() -> ProviderEventStream {
    Box::pin(tokio_stream::iter([ProviderStreamEvent::error(
        "provider selection is missing or unknown",
    )]))
}

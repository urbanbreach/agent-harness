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
    fn native_compaction_lanes(
        &self,
        request: &CompletionRequest,
        settings: &crate::NativeLaneSettings,
    ) -> Vec<crate::NativeCompactionLane> {
        self.get(request.provider_id.as_deref())
            .map_or_else(Vec::new, |p| p.native_compaction_lanes(request, settings))
    }
    async fn compaction_credential(
        &self,
        request: &CompletionRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Option<crate::CompactionCredential>, crate::NativeCompactionError> {
        match self.get(request.provider_id.as_deref()) {
            Some(provider) => provider.compaction_credential(request, cancel).await,
            None => Ok(None),
        }
    }
    fn cache_lookback_blocks(&self, request: &CompletionRequest) -> Option<u32> {
        self.get(request.provider_id.as_deref())
            .and_then(|p| p.cache_lookback_blocks(request))
    }
    fn endpoint_host(&self, request: &crate::CompletionRequest) -> Option<String> {
        self.get(request.provider_id.as_deref())
            .and_then(|p| p.endpoint_host(request))
    }
    fn token_reader(&self, request: &CompletionRequest) -> Option<harness_tokenizer::Reader> {
        self.get(request.provider_id.as_deref())
            .and_then(|p| p.token_reader(request))
    }
    async fn native_compact(
        &self,
        request: crate::NativeCompactionRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<crate::NativeCompactionResult, crate::NativeCompactionError> {
        let provider = self
            .get(request.request.provider_id.as_deref())
            .ok_or_else(|| crate::NativeCompactionError::new("provider unavailable"))?;
        provider.native_compact(request, cancel).await
    }
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

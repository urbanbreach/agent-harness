use crate::{
    CompletionRequest, Provider, ProviderBudgetSemantics, ProviderEventStream,
    ProviderRequestCostError, ProviderStreamEvent,
};
use std::{collections::BTreeMap, sync::Arc};

pub struct ProviderRouter {
    providers: BTreeMap<String, Arc<dyn Provider>>,
}
impl ProviderRouter {
    pub fn new(providers: BTreeMap<String, Arc<dyn Provider>>) -> Self {
        Self { providers }
    }
    fn get(&self, id: Option<&str>) -> Option<&Arc<dyn Provider>> {
        match id {
            Some(id) => self.providers.get(id),
            None => self.providers.get("default").or_else(|| {
                if self.providers.len() == 1 {
                    self.providers.values().next()
                } else {
                    None
                }
            }),
        }
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
            None => Box::pin(tokio_stream::iter([ProviderStreamEvent::error(
                "provider selection is missing or unknown",
            )])),
        }
    }
}

use crate::{CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct MockProvider {
    scripted: BTreeMap<String, Vec<ProviderStreamEvent>>,
    turns: Mutex<Option<VecDeque<Vec<ProviderStreamEvent>>>>,
    captured: Mutex<VecDeque<CompletionRequest>>,
    calls: AtomicUsize,
}

impl MockProvider {
    pub fn new(scripted_events: BTreeMap<String, Vec<ProviderStreamEvent>>) -> Self {
        Self {
            scripted: scripted_events,
            ..Self::default()
        }
    }
    pub fn script(turns: impl IntoIterator<Item = Vec<ProviderStreamEvent>>) -> Self {
        Self {
            turns: Mutex::new(Some(turns.into_iter().collect())),
            ..Self::default()
        }
    }
    pub fn call_count(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
    pub async fn captured_requests(&self) -> Vec<CompletionRequest> {
        self.captured.lock().await.iter().cloned().collect()
    }
}

#[async_trait::async_trait]
impl Provider for MockProvider {
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        let events = if let Some(turns) = self.turns.lock().await.as_mut() {
            turns
                .pop_front()
                .unwrap_or_else(|| vec![ProviderStreamEvent::error("mock script exhausted")])
        } else if !self.scripted.is_empty() {
            self.scripted
                .get(&request_digest(&request))
                .cloned()
                .unwrap_or_else(|| {
                    vec![ProviderStreamEvent::error(
                        "no mock response matches the request",
                    )]
                })
        } else {
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("Hello world".into()),
                ProviderStreamEvent::Done { usage: None },
            ]
        };
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut captured = self.captured.lock().await;
        // ponytail: keep eight diagnostic requests; explicit fixtures cover longer conversations.
        if captured.len() == 8 {
            captured.pop_front();
        }
        captured.push_back(request);
        Box::pin(tokio_stream::iter(events))
    }
}

pub fn request_digest(request: &CompletionRequest) -> String {
    let mut stable = request.clone();
    stable.context.request_id = None;
    match serde_json::to_vec(&stable) {
        Ok(bytes) => blake3::hash(&bytes).to_hex().to_string(),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn router_never_substitutes_an_unknown_provider() {
        let provider = Arc::new(MockProvider::default());
        let router = crate::ProviderRouter::new(BTreeMap::from([(
            "local".into(),
            Arc::clone(&provider) as Arc<dyn Provider>,
        )]));
        let request = CompletionRequest {
            model_id: "fixture".into(),
            provider_id: Some("missing".into()),
            ..Default::default()
        };
        assert!(matches!(
            router.stream_completion(request.clone()).await.next().await,
            Some(ProviderStreamEvent::Error { .. })
        ));
        assert_eq!(provider.call_count(), 0);
        let mut request = request;
        request.provider_id = None;
        let events = router
            .stream_completion(request)
            .await
            .collect::<Vec<_>>()
            .await;
        assert!(matches!(
            events.last(),
            Some(ProviderStreamEvent::Done { .. })
        ));
        assert_eq!(provider.call_count(), 1);
        assert_eq!(provider.captured_requests().await.len(), 1);
    }
}

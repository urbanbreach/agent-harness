use super::*;

#[derive(Clone)]
pub(crate) struct BlockingSummaryProvider {
    requests: Arc<Mutex<Vec<CompletionRequest>>>,
    scripts: Arc<[Vec<ProviderStreamEvent>]>,
    next_call: Arc<AtomicUsize>,
    block_call: usize,
    entered: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    release: Arc<Notify>,
}

impl BlockingSummaryProvider {
    pub(crate) fn new(
        scripts: Vec<Vec<ProviderStreamEvent>>,
        block_call: usize,
    ) -> (Self, tokio::sync::oneshot::Receiver<()>, Arc<Notify>) {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let release = Arc::new(Notify::new());
        (
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
                scripts: Arc::from(scripts),
                next_call: Arc::new(AtomicUsize::new(0)),
                block_call,
                entered: Arc::new(Mutex::new(Some(entered_tx))),
                release: Arc::clone(&release),
            },
            entered_rx,
            release,
        )
    }

    pub(crate) fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().unwrap_or_abort().clone()
    }
}

#[async_trait]
impl Provider for BlockingSummaryProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<
        harness_providers::ProviderBudgetSemantics,
        harness_providers::ProviderRequestCostError,
    > {
        harness_providers::generic_request_budget_semantics(request, pending_prompt_index)
    }

    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        self.requests.lock().unwrap_or_abort().push(request);
        let call = self.next_call.fetch_add(1, Ordering::SeqCst);
        if call == self.block_call {
            if let Some(entered) = self.entered.lock().unwrap_or_abort().take() {
                let _ = entered.send(());
            }
            self.release.notified().await;
        }
        let events = self.scripts.get(call).cloned().unwrap_or_else(|| {
            vec![ProviderStreamEvent::error(
                "missing blocking-provider script",
            )]
        });
        Box::pin(tokio_stream::iter(events))
    }
}

//! Provider requests and streamed responses. Wire formats stay in this crate.
mod error;
mod retry_failure;
pub use retry_failure::ProviderRetryFailure;
pub mod mock;
mod router;
pub use router::ProviderRouter;
pub mod attachment_protocol;
mod sse;
mod types;
mod wire;
pub use wire::Protocol;
mod http;
pub use http::HttpProvider;
pub mod anthropic_subscription;
mod subscription;
pub use subscription::{codex_model_allowed, ProviderAuthProfile};
pub type ProviderEventStream =
    std::pin::Pin<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send>>;
pub mod request_budget;
pub use request_budget::*;

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
        generic_request_budget_semantics(request, pending_prompt_index)
    }
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream;
    /// Streams with an abort signal. An aborted stream settles and ends with `Aborted`;
    /// by default it ends at once.
    async fn stream_completion_abortable(
        &self,
        request: CompletionRequest,
        abort: tokio_util::sync::CancellationToken,
    ) -> ProviderEventStream {
        end_on_abort(self.stream_completion(request).await, abort)
    }
    fn session_event(&self, _event: &ProviderSessionEvent) {}
    /// True when the backend keeps and compacts this request's conversation itself, so the
    /// harness stands its automatic compaction and input-budget stop down for it.
    fn manages_context(&self, _request: &CompletionRequest) -> bool {
        false
    }
    /// Providers signed in after startup. A router routes to them from the next request.
    fn add_providers(
        &self,
        _providers: std::collections::BTreeMap<String, std::sync::Arc<dyn Provider>>,
    ) {
    }
}

/// Ends `stream` with `Aborted { usage: None }` as soon as `abort` fires.
pub fn end_on_abort(
    mut stream: ProviderEventStream,
    abort: tokio_util::sync::CancellationToken,
) -> ProviderEventStream {
    use tokio_stream::StreamExt;
    Box::pin(async_stream::stream! {
        loop {
            tokio::select! {
                biased;
                () = abort.cancelled() => {
                    yield ProviderStreamEvent::Aborted { usage: None };
                    return;
                }
                event = stream.next() => match event {
                    Some(event) => yield event,
                    None => return,
                },
            }
        }
    })
}

#[async_trait::async_trait]
impl Provider for HttpProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
        self.budget(request, pending_prompt_index)
    }
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        self.stream(request)
    }
}
pub use error::*;
pub use types::*;
/// A shared ceiling keeps tool catalogs portable across supported wire protocols.
pub const MAX_TOOL_DEFINITIONS: usize = 128;

// The unchanged TUI test suite imports this historical helper.
pub trait UnwrapOrAbort<T> {
    fn unwrap_or_abort(self) -> T;
}

#[allow(
    clippy::panic,
    reason = "Compatibility helper for the preserved TUI tests."
)]
fn failed_test_value() -> ! {
    panic!("required test value is absent")
}

impl<T> UnwrapOrAbort<T> for Option<T> {
    fn unwrap_or_abort(self) -> T {
        self.unwrap_or_else(|| failed_test_value())
    }
}

impl<T, E> UnwrapOrAbort<T> for Result<T, E> {
    fn unwrap_or_abort(self) -> T {
        self.unwrap_or_else(|_| failed_test_value())
    }
}

//! Provider requests and streamed responses. Wire formats stay in this crate.
mod envelope;
pub use envelope::{ProviderErrorEnvelope, ERROR_BODY_LIMIT};
mod error;
mod native_compaction;
pub use native_compaction::*;
mod retry_failure;
pub use retry_failure::ProviderRetryFailure;
pub mod mock;
mod router;
pub use router::ProviderRouter;
pub mod attachment_protocol;
mod sse;
mod token_reader;
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
    fn native_compaction_lanes(
        &self,
        _request: &CompletionRequest,
        _settings: &NativeLaneSettings,
    ) -> Vec<NativeCompactionLane> {
        Vec::new()
    }
    /// Credentials for a compaction side request to a configured endpoint.
    ///
    /// `None` means credentials are unavailable and the candidate must be skipped;
    /// `Some(CompactionCredential::default())` means the provider needs no authentication.
    /// A provider that does not implement lookup reads as keyless.
    async fn compaction_credential(
        &self,
        _request: &CompletionRequest,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Option<CompactionCredential>, NativeCompactionError> {
        Ok(Some(CompactionCredential::default()))
    }
    /// How many trailing content blocks a provider-side prompt cache looks back over.
    fn cache_lookback_blocks(&self, _request: &CompletionRequest) -> Option<u32> {
        None
    }
    async fn native_compact(
        &self,
        _request: NativeCompactionRequest,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> Result<NativeCompactionResult, NativeCompactionError> {
        Err(NativeCompactionError::new("native compaction unsupported"))
    }
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
    /// The model identity and wire facts that price this request's tokens, or `None` when the
    /// provider cannot tell (the caller then infers the reader from the model name).
    fn token_reader(&self, _request: &CompletionRequest) -> Option<harness_tokenizer::Reader> {
        None
    }
    /// Host of the endpoint this request reaches, or `None` for the provider's built-in
    /// endpoint. Deployment image caps depend on it.
    fn endpoint_host(&self, _request: &CompletionRequest) -> Option<String> {
        None
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

/// Anthropic's prompt cache matches a prefix that ends within this many blocks of a breakpoint.
const ANTHROPIC_CACHE_LOOKBACK_BLOCKS: u32 = 20;

#[async_trait::async_trait]
impl Provider for HttpProvider {
    fn endpoint_host(&self, _request: &CompletionRequest) -> Option<String> {
        self.endpoint.host_str().map(str::to_owned)
    }
    fn token_reader(&self, request: &CompletionRequest) -> Option<harness_tokenizer::Reader> {
        Some(token_reader::reader_for(
            request,
            &request.model_id,
            self.protocol(request),
            self.endpoint.host_str(),
        ))
    }
    fn native_compaction_lanes(
        &self,
        request: &CompletionRequest,
        settings: &NativeLaneSettings,
    ) -> Vec<NativeCompactionLane> {
        self.compaction_lanes(request, settings)
    }
    async fn native_compact(
        &self,
        request: NativeCompactionRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<NativeCompactionResult, NativeCompactionError> {
        self.compact_native(request, cancel).await
    }
    async fn compaction_credential(
        &self,
        _request: &CompletionRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Option<CompactionCredential>, NativeCompactionError> {
        tokio::select! {
            biased;
            () = cancel.cancelled() => Err(NativeCompactionError::new("Remote compaction cancelled")),
            credential = self.resolve_compaction_credential() => credential,
        }
    }
    fn cache_lookback_blocks(&self, request: &CompletionRequest) -> Option<u32> {
        (self.protocol(request) == Protocol::Anthropic).then_some(ANTHROPIC_CACHE_LOOKBACK_BLOCKS)
    }
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

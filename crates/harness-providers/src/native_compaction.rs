//! Transient native context replacement, using the provider's existing transport identity.
//! Dispatch owns protocol attempts and retry backoff; fitting, retention, and streaming
//! remain separate from continuation metadata and endpoint policy.
use crate::{
    CompactionCredential, CompletionMessage, CompletionRequest, CompletionUsage, HttpProvider,
    NativeLaneSettings, Protocol, ProviderContinuation, ProviderErrorCategory,
};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
mod anthropic;
mod custom;
mod dispatch;
mod endpoint;
mod error;
mod fit;
mod input;
mod openai;
mod retention;
mod stream;
#[cfg(test)]
mod tests;

pub(crate) use anthropic::{
    add_beta, anthropic_capable, route_anthropic_beta, supported_anthropic,
};
pub use anthropic::{FilesDue, RetainedFile};
pub use custom::{custom_endpoint_summary, CustomSummary};
pub use endpoint::{resolve_native_endpoints, NativeEndpointInput, NativeEndpoints};
pub use error::NativeCompactionError;

/// Native replacement protocols, in their preferred attempt order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCompactionLane {
    OpenAiV2,
    OpenAiV1,
    Anthropic,
}
/// Live provider context plus explicit user-authored V2 retention candidates.
#[derive(Default)]
pub struct NativeCompactionRequest {
    pub request: CompletionRequest,
    pub lanes: Vec<NativeCompactionLane>,
    pub instructions: String,
    pub retained_message_budget: u32,
    /// Unknown or zero windows bypass fit rejection.
    pub context_window: Option<u32>,
    pub max_tokens: u32,
    pub retained: Vec<CompletionMessage>,
    /// Resolved remote gates the lanes were listed under.
    pub settings: NativeLaneSettings,
    /// Native payload of the boundary being replaced, when it is reusable.
    pub previous_replay: Option<Box<ProviderContinuation>>,
    /// The last request the live turn sent. Its controls (tools, thinking, effort) are the
    /// ones a signed summary keeps, so compaction introduces no roster or effort change.
    pub saved_request: Option<Box<CompletionRequest>>,
    /// Explicit prompt cache identity; the request's session identity is used when absent.
    pub cache_key: Option<String>,
}
/// Attach `replay` to the summary user message; its contents live only in the private
/// native payload store.
pub struct NativeCompactionResult {
    pub lane: NativeCompactionLane,
    pub summary: Option<String>,
    pub replay: Box<ProviderContinuation>,
    pub used_tokens: u32,
    pub usage: Option<CompletionUsage>,
    /// Last journal sequence the snapshot included; the caller sets it from request metadata.
    pub snapshot_through_seq: Option<u64>,
    pub short_summary: Option<String>,
}

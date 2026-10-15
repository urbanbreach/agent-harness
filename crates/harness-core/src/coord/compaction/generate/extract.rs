//! Summary-side response extraction, error shaping and the shared retry classifier.
//!
//! Summary, short summary, prefix, handoff and branch requests read provider streams
//! here instead of through main-turn enforcement: only text is kept, tool calls are
//! ignored whatever their shape or size, and only a resolved provider error fails the
//! request. A provider abort is reported to the caller, which owns the cancellation
//! decision.
use crate::coord::{
    compaction::{CompactionError, CompactionErrorKind, Progress},
    CoordinatorError, CoordinatorHandle,
};
use crate::event::{EventActor, LiveEventV1};
use harness_compaction::retry;
use harness_providers::{
    CompletionRequest, CompletionUsage, Provider, ProviderErrorCategory, ProviderStreamEvent,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

/// Cause text marking a [`CompactionError`] that stands for cancellation.
const CANCELLED_CAUSE: &str = "cancelled";

/// How a summary request retries its own transient failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum RetryMode {
    /// The summarizer's short default budget.
    Default,
    /// No nested retry: an outer loop owns it and repeats the whole operation.
    Disabled,
}

/// A cancellation carried through the compaction error type.
pub(in crate::coord) fn cancelled_error(kind: CompactionErrorKind) -> CompactionError {
    let mut error = CompactionError::new(kind, "Compaction cancelled");
    error.cause = Some(CANCELLED_CAUSE.into());
    error
}

/// Whether `error` is the cancellation marker rather than a provider failure.
pub(in crate::coord) fn is_cancelled(error: &CompactionError) -> bool {
    error.cause.as_deref() == Some(CANCELLED_CAUSE)
}

/// The generator error prefix for a request kind.
fn prefix(kind: CompactionErrorKind) -> Option<&'static str> {
    match kind {
        CompactionErrorKind::Summary => Some("Summarization failed"),
        CompactionErrorKind::TurnPrefix => Some("Turn prefix summarization failed"),
        CompactionErrorKind::ShortSummary => Some("Short summary failed"),
        CompactionErrorKind::Handoff => Some("Handoff generation failed"),
        CompactionErrorKind::Branch
        | CompactionErrorKind::Native
        | CompactionErrorKind::Endpoint
        | CompactionErrorKind::Hook => None,
    }
}

/// Shapes a resolved provider error into the typed compaction failure. Generator kinds
/// get their exact prefix (`Unknown error` when the provider sent no text; a branch summary
/// keeps the bare text, falling back to `Summarization failed`); the status is top level
/// only when the provider supplied one, and the bounded response body rides in `cause` so
/// classifiers can read it without it reaching user-facing text.
pub(in crate::coord) fn provider_failure(
    kind: CompactionErrorKind,
    message: String,
    category: Option<ProviderErrorCategory>,
    retry_after_ms: Option<u64>,
    envelope: Option<harness_providers::ProviderErrorEnvelope>,
) -> CompactionError {
    let text = if !message.is_empty() {
        message
    } else if kind == CompactionErrorKind::Branch {
        "Summarization failed".to_owned()
    } else {
        "Unknown error".to_owned()
    };
    let message = match prefix(kind) {
        Some(prefix) => format!("{prefix}: {text}"),
        None => text,
    };
    let mut error = CompactionError::new(kind, message);
    error.category = category;
    if let Some(envelope) = envelope {
        error.status = envelope.status;
        error.headers = envelope.headers;
        error.cause = envelope.body;
    }
    if let Some(hint) = retry_after_ms
        && header_hint(&error.headers).is_none_or(|existing| hint > existing)
    {
        error
            .headers
            .insert("retry-after-ms".into(), hint.to_string());
    }
    error.auth = matches!(
        category,
        Some(ProviderErrorCategory::MissingCredentials | ProviderErrorCategory::InvalidCredentials)
    ) || matches!(error.status, Some(401 | 403));
    error
}

/// A coordinator failure as a compaction failure; cancellation keeps its marker.
pub(in crate::coord) fn from_coordinator(
    kind: CompactionErrorKind,
    error: CoordinatorError,
) -> CompactionError {
    match error {
        CoordinatorError::Compaction(error) => error,
        CoordinatorError::Cancelled(_) | CoordinatorError::CompactionCancelled { .. } => {
            cancelled_error(kind)
        }
        CoordinatorError::Provider {
            message,
            category,
            retry_after_ms,
            ..
        } => provider_failure(kind, message, category, retry_after_ms, None),
        other => CompactionError::new(kind, other.to_string()),
    }
}

/// A compaction failure as a coordinator failure for `task`.
pub(in crate::coord) fn into_coordinator(error: CompactionError, task: &str) -> CoordinatorError {
    if is_cancelled(&error) {
        CoordinatorError::Cancelled(task.to_owned())
    } else {
        CoordinatorError::Compaction(error)
    }
}

/// What a failure means for retry decisions. Exclusions (abort, content block, overflow,
/// rejected payload, deterministic local parse failure) win over any status or hint.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) struct Class {
    pub overflow: bool,
    pub timeout: bool,
    pub transient: bool,
    pub usage_limit: bool,
    /// Largest of the header, provider and `retry-after-ms=` hints; zero is a real hint.
    pub hint_ms: Option<u64>,
    excluded: bool,
}

impl Class {
    /// A retry is allowed when nothing excludes it and the failure is transient, a usage
    /// limit, or carries a retry-after hint.
    pub(in crate::coord) fn retryable(&self) -> bool {
        !self.excluded
            && !self.overflow
            && (self.hint_ms.is_some() || self.transient || self.usage_limit)
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn parse_decimal(value: &str) -> Option<f64> {
    let value: f64 = value.trim().parse().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Finite non-negative millisecond hints round upward and saturate at the integer boundary"
)]
fn ceil_ms(value: f64) -> u64 {
    value.ceil() as u64
}

/// The largest retry hint carried by response headers, in milliseconds.
fn header_hint(headers: &BTreeMap<String, String>) -> Option<u64> {
    let get = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    let millis = get("retry-after-ms").and_then(parse_decimal).map(ceil_ms);
    let seconds = get("retry-after")
        .and_then(parse_decimal)
        .map(|seconds| ceil_ms(seconds * 1000.0));
    millis.into_iter().chain(seconds).max()
}

/// Classifies a failure for retry and candidate decisions.
pub(in crate::coord) fn classify(error: &CompactionError) -> Class {
    let text = format!(
        "{} {}",
        error.message,
        error.cause.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase();
    let overflow = error.category == Some(ProviderErrorCategory::ContextWindowExceeded);
    let excluded = is_cancelled(error)
        || error.status == Some(413)
        || contains_any(
            &text,
            &[
                "content_filter",
                "content filter",
                "content policy",
                "blocked by",
                "payload too large",
                "request entity too large",
                "request too large",
            ],
        )
        || (error.category == Some(ProviderErrorCategory::MalformedStream)
            && contains_any(&text, &["tool", "function"])
            && contains_any(&text, &["json", "arguments", "parse"]));
    let timeout =
        text.contains("timed out") || text.contains("timeout") || error.status == Some(408);
    let usage_limit = contains_any(&text, &["usage limit", "usage_limit", "quota"]);
    let transient = matches!(
        error.category,
        Some(ProviderErrorCategory::RateLimited | ProviderErrorCategory::TransportFailure)
    ) || matches!(
        error.status,
        Some(408 | 409 | 425 | 429 | 500 | 502 | 503 | 504 | 520..=524 | 529)
    ) || contains_any(
        &text,
        &[
            "overloaded",
            "rate limit",
            "rate_limit",
            "too many requests",
            "try again",
            "temporarily",
            "service unavailable",
            "connection reset",
            "connection closed",
            "econnreset",
            "server error",
            "internal error",
        ],
    ) || timeout;
    let hint = header_hint(&error.headers)
        .into_iter()
        .chain(retry::parse_retry_after_suffix(&error.message))
        .chain(
            error
                .cause
                .as_deref()
                .and_then(retry::parse_retry_after_suffix),
        )
        .max();
    Class {
        overflow,
        timeout,
        transient,
        usage_limit,
        hint_ms: hint,
        excluded,
    }
}

/// A uniform value in `[0, 1)` for retry jitter.
pub(in crate::coord) fn jitter_unit() -> f64 {
    let mut bytes = [0_u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        return 0.5;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "A 53-bit integer is exactly representable"
    )]
    let unit = (u64::from_le_bytes(bytes) >> 11) as f64 / (1_u64 << 53) as f64;
    unit
}

/// Text and billing facts of one summary-side response.
#[derive(Debug, Default)]
pub(in crate::coord) struct Extracted {
    pub text: String,
    pub usage: Option<CompletionUsage>,
    /// The provider reported its own abort; the caller decides whether that is a cancel.
    pub aborted: bool,
}

/// One summary-side request context: where to read, how to gate and whom to show text to.
pub(in crate::coord) struct Reader<'a> {
    pub handle: &'a CoordinatorHandle,
    pub provider: &'a dyn Provider,
    pub permits: &'a Arc<tokio::sync::Semaphore>,
    pub actor: &'a EventActor,
    pub task: &'a str,
    pub cancel: &'a CancellationToken,
    /// Receives redacted text as it streams, for compaction progress.
    pub preview: Option<&'a Progress>,
}

impl Reader<'_> {
    /// Reads one response, retrying transient failures per `mode`.
    pub(in crate::coord) async fn run(
        &self,
        request: CompletionRequest,
        kind: CompactionErrorKind,
        mode: RetryMode,
    ) -> Result<Extracted, CompactionError> {
        let policy = match mode {
            RetryMode::Disabled => None,
            RetryMode::Default => Some(retry::ONESHOT),
        };
        let mut attempt = 1;
        loop {
            let error = match self.once(request.clone(), kind).await {
                Ok(extracted) => return Ok(extracted),
                Err(error) => error,
            };
            let Some(policy) = policy.filter(|policy| attempt < policy.attempts) else {
                return Err(error);
            };
            if self.cancel.is_cancelled() || is_cancelled(&error) {
                return Err(error);
            }
            let class = classify(&error);
            if !class.retryable() {
                return Err(error);
            }
            let Some(delay) = retry::backoff_ms(&policy, attempt, jitter_unit(), class.hint_ms)
            else {
                return Err(error);
            };
            self.wait(delay, kind).await?;
            attempt += 1;
        }
    }

    /// An abortable wait; cancellation during it is the cancellation marker.
    pub(in crate::coord) async fn wait(
        &self,
        delay_ms: u64,
        kind: CompactionErrorKind,
    ) -> Result<(), CompactionError> {
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(cancelled_error(kind)),
            () = tokio::time::sleep(Duration::from_millis(delay_ms)) => Ok(()),
        }
    }

    async fn once(
        &self,
        request: CompletionRequest,
        kind: CompactionErrorKind,
    ) -> Result<Extracted, CompactionError> {
        let permit = tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(cancelled_error(kind)),
            permit = Arc::clone(self.permits).acquire_owned() => permit
                .map_err(|_| CompactionError::new(kind, "coordinator is closed"))?,
        };
        let result = self.stream(request, kind).await;
        drop(permit);
        result
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        kind: CompactionErrorKind,
    ) -> Result<Extracted, CompactionError> {
        let redactor = match self.preview {
            Some(_) => Some(
                self.handle
                    .call(|s| Ok(Arc::clone(&s.redactor)))
                    .await
                    .map_err(|error| from_coordinator(kind, error))?,
            ),
            None => None,
        };
        let request_abort = self.cancel.child_token();
        let mut stream = tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(cancelled_error(kind)),
            stream = self.provider.stream_completion_abortable(request, request_abort.clone()) => stream,
        };
        let mut out = Extracted::default();
        // Set when a non-text event followed text, so the next text starts a new block.
        let mut gap = false;
        loop {
            let event = tokio::select! {
                biased;
                () = self.cancel.cancelled() => {
                    request_abort.cancel();
                    return Err(cancelled_error(kind));
                }
                event = stream.next() => event,
            };
            let Some(event) = event else {
                return Err(provider_failure(
                    kind,
                    "provider stream ended before completion".into(),
                    Some(ProviderErrorCategory::MalformedStream),
                    None,
                    None,
                ));
            };
            match event {
                ProviderStreamEvent::Start | ProviderStreamEvent::Started { .. } => {}
                ProviderStreamEvent::TextDelta(delta) => {
                    if delta.is_empty() {
                        continue;
                    }
                    let mut shown = String::new();
                    if gap && !out.text.is_empty() {
                        out.text.push('\n');
                        shown.push('\n');
                    }
                    gap = false;
                    out.text.push_str(&delta);
                    shown.push_str(&delta);
                    if let (Some(progress), Some(redactor)) = (self.preview, &redactor) {
                        progress
                            .preview(&redactor.redact_text(&shown))
                            .await
                            .map_err(|error| from_coordinator(kind, error))?;
                    }
                }
                ProviderStreamEvent::ReasoningDelta(_)
                | ProviderStreamEvent::ToolCallDelta { .. }
                | ProviderStreamEvent::ToolCallComplete { .. }
                | ProviderStreamEvent::TextBlockEnd => gap = true,
                ProviderStreamEvent::Done { usage }
                | ProviderStreamEvent::DoneWithMetadata { usage, .. } => {
                    out.usage = usage;
                    return Ok(out);
                }
                ProviderStreamEvent::Error {
                    message,
                    category,
                    retry_after_ms,
                    envelope,
                    ..
                } => {
                    return Err(provider_failure(
                        kind,
                        message,
                        category,
                        retry_after_ms,
                        envelope.map(|envelope| *envelope),
                    ));
                }
                ProviderStreamEvent::Notice(message) => {
                    let (actor, task) = (self.actor.clone(), self.task.to_owned());
                    self.handle
                        .call(move |s| s.live(actor, task, LiveEventV1::RuntimeWarning { message }))
                        .await
                        .map_err(|error| from_coordinator(kind, error))?;
                }
                ProviderStreamEvent::Aborted { usage } => {
                    if self.cancel.is_cancelled() {
                        return Err(cancelled_error(kind));
                    }
                    out.usage = usage;
                    out.aborted = true;
                    return Ok(out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

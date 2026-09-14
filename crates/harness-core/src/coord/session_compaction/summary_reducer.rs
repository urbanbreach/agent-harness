use harness_providers::{CompletionUsage, ProviderEventStream, ProviderStreamEvent};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(super) enum SummaryGenerationError {
    #[error("summary generation was cancelled")]
    Cancelled,
    #[error("summary provider failed: {message}")]
    Provider {
        message: String,
        category: Option<harness_providers::ProviderErrorCategory>,
        retry_after_ms: Option<u64>,
    },
    #[error("summary stream stalled without provider events")]
    IdleTimeout,
    #[error("summary generation exceeded its duration budget")]
    DurationBudget,
    #[error("summary is incomplete: {0}")]
    IncompleteOutput(String),
    #[error("summary stream ended without completion")]
    MissingTerminal,
    #[error("summary stream contained duplicate completion")]
    DuplicateTerminal,
    #[error("summary stream contained text after completion")]
    PostTerminalDelta,
    #[error("summary stream contained an event after completion")]
    PostTerminalEvent,
    #[error("summary stream requested a tool")]
    ToolCall,
    #[error("summary completion was empty")]
    EmptyOutput,
}

pub(super) async fn reduce_summary_stream(
    stream: ProviderEventStream,
    cancellation: &CancellationToken,
) -> Result<ReducedSummary, SummaryGenerationError> {
    reduce_summary_stream_with_progress(stream, cancellation, None).await
}

pub(super) async fn reduce_summary_stream_with_progress(
    mut stream: ProviderEventStream,
    cancellation: &CancellationToken,
    progress: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> Result<ReducedSummary, SummaryGenerationError> {
    let mut reducer = SummaryStreamReducer::new();
    loop {
        let event = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(SummaryGenerationError::Cancelled),
            event = tokio::time::timeout(std::time::Duration::from_millis(300_000), stream.next()) => event.map_err(|_| SummaryGenerationError::IdleTimeout)?,
        };
        let Some(event) = event else {
            break;
        };
        let text_changed =
            matches!(&event, ProviderStreamEvent::TextDelta(delta) if !delta.is_empty());
        reducer.apply(event)?;
        if text_changed {
            if let Some(progress) = progress {
                progress(&reducer.text);
            }
        }
    }
    let reduced = reducer.finish()?;
    if cancellation.is_cancelled() {
        return Err(SummaryGenerationError::Cancelled);
    }
    Ok(reduced)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Terminal {
    Pending,
    Completed(Option<CompletionUsage>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReducedSummary {
    pub(super) text: String,
    pub(super) usage: Option<CompletionUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SummaryStreamReducer {
    text: String,
    terminal: Terminal,
}

impl SummaryStreamReducer {
    pub(super) const fn new() -> Self {
        Self {
            text: String::new(),
            terminal: Terminal::Pending,
        }
    }

    pub(super) fn apply(
        &mut self,
        event: ProviderStreamEvent,
    ) -> Result<(), SummaryGenerationError> {
        if matches!(self.terminal, Terminal::Completed(_)) {
            return match event {
                ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. } => {
                    Err(SummaryGenerationError::DuplicateTerminal)
                }
                ProviderStreamEvent::TextDelta(_) => Err(SummaryGenerationError::PostTerminalDelta),
                ProviderStreamEvent::Error {
                    message,
                    category,
                    retry_after_ms,
                    ..
                } => Err(SummaryGenerationError::Provider {
                    message,
                    category,
                    retry_after_ms,
                }),
                ProviderStreamEvent::Start
                | ProviderStreamEvent::Started { .. }
                | ProviderStreamEvent::ReasoningDelta(_)
                | ProviderStreamEvent::ToolCallDelta { .. }
                | ProviderStreamEvent::ToolCallComplete { .. } => {
                    Err(SummaryGenerationError::PostTerminalEvent)
                }
            };
        }

        match event {
            ProviderStreamEvent::TextDelta(delta) => self.text.push_str(&delta),
            ProviderStreamEvent::Done { usage } => self.terminal = Terminal::Completed(usage),
            ProviderStreamEvent::DoneWithMetadata { usage, metadata } => {
                if let Some(reason) = metadata
                    .as_ref()
                    .and_then(|metadata| metadata.provider_stop_reason.as_deref())
                {
                    if matches!(
                        reason,
                        "length"
                            | "max_tokens"
                            | "max_output_tokens"
                            | "incomplete"
                            | "aborted"
                            | "cancelled"
                            | "refusal"
                    ) {
                        return Err(SummaryGenerationError::IncompleteOutput(reason.to_string()));
                    }
                }
                self.terminal = Terminal::Completed(usage);
            }
            ProviderStreamEvent::Error {
                message,
                category,
                retry_after_ms,
                ..
            } => {
                return Err(SummaryGenerationError::Provider {
                    message,
                    category,
                    retry_after_ms,
                });
            }
            ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. } => {
                return Err(SummaryGenerationError::ToolCall);
            }
            ProviderStreamEvent::Start
            | ProviderStreamEvent::Started { .. }
            | ProviderStreamEvent::ReasoningDelta(_) => {}
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<ReducedSummary, SummaryGenerationError> {
        let usage = match self.terminal {
            Terminal::Pending => return Err(SummaryGenerationError::MissingTerminal),
            Terminal::Completed(usage) => usage,
        };
        let text = self.text.trim();
        if text.is_empty() {
            return Err(SummaryGenerationError::EmptyOutput);
        }
        Ok(ReducedSummary {
            text: text.to_string(),
            usage,
        })
    }
}

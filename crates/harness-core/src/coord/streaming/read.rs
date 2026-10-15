use super::*;
use std::collections::HashSet;
use tokio_stream::StreamExt;

/// Where streamed text goes while a request runs.
pub(in crate::coord) enum Live<'a> {
    /// Nothing is shown live.
    Off,
    /// A main-turn response: text, reasoning and tool input stream to the UI.
    Turn,
    /// A compaction request: text streams into the compaction preview only.
    Preview(&'a crate::coord::compaction::Progress),
}

pub(in crate::coord) async fn read(
    handle: &CoordinatorHandle,
    provider: &dyn Provider,
    actor: &EventActor,
    task: &str,
    request_id: &str,
    request: CompletionRequest,
    cancel: &CancellationToken,
    live_mode: Live<'_>,
    stream_guard_enabled: bool,
    progress: &mut bool,
    aborted_usage: &mut Option<CompletionUsage>,
) -> Result<Response, CoordinatorError> {
    let emit_live = matches!(live_mode, Live::Turn);
    let redactor = handle.call(|s| Ok(Arc::clone(&s.redactor))).await?;
    let mut display = LiveOutput::default();
    let aliases: BTreeMap<_, _> = request
        .tools
        .iter()
        .flatten()
        .map(|tool| (tool.function_name.clone(), tool.tool_id.clone()))
        .collect();
    let request_abort = cancel.child_token();
    let mut stream = tokio::select! {
        biased;
        () = cancel.cancelled() => return Err(CoordinatorError::Cancelled(task.into())),
        stream = provider.stream_completion_abortable(request, request_abort.clone()) => stream,
    };
    let mut response = Response::default();
    let mut completed = HashSet::new();
    let mut unfinished = HashSet::new();
    let mut bytes = 0usize;
    let mut guard = stream_guard_enabled.then(guard::StreamGuard::default);
    loop {
        let event = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                *aborted_usage = settle_abort(&mut stream).await.0;
                return Err(CoordinatorError::Cancelled(task.into()));
            }
            event = stream.next() => event.ok_or_else(|| CoordinatorError::Invalid("provider stream ended before completion".into()))?,
        };
        if let Some(reason) = guard.as_mut().and_then(|guard| guard.observe(&event)) {
            request_abort.cancel();
            let (usage, usage_complete) = match event {
                Stream::Done { usage } => {
                    let complete = usage.is_some();
                    (usage, complete)
                }
                Stream::DoneWithMetadata { usage, metadata } => {
                    let complete = usage.is_some()
                        && metadata.as_ref().and_then(|m| m.usage_complete) == Some(true);
                    (usage, complete)
                }
                _ => settle_abort(&mut stream).await,
            };
            *aborted_usage = usage.clone();
            return Err(CoordinatorError::StreamGuard(super::StreamGuardFailure {
                reason,
                request_id: request_id.into(),
                usage,
                usage_complete,
            }));
        }
        let live = match event {
            Stream::Start | Stream::Started { .. } => None,
            Stream::TextDelta(delta) => {
                *progress |= !delta.is_empty();
                bytes = bytes.saturating_add(delta.len());
                response.push_text(&delta);
                if let Live::Preview(preview) = &live_mode {
                    preview.preview(&redactor.redact_text(&delta)).await?;
                }
                Some(LiveEventV1::ProviderTextDelta {
                    request_id: request_id.into(),
                    delta,
                })
            }
            Stream::ReasoningDelta(delta) => {
                *progress |= !delta.is_empty();
                response.push_reasoning(&delta);
                bytes = bytes.saturating_add(delta.len());
                Some(LiveEventV1::ProviderReasoningDelta {
                    request_id: request_id.into(),
                    delta,
                })
            }
            Stream::ToolCallDelta {
                tool_call_id,
                arguments_delta,
                function_name,
            } => {
                *progress = true;
                if tool_call_id.is_empty() || tool_call_id.len() > 256 {
                    return Err(CoordinatorError::Invalid(
                        "invalid provider tool call id".into(),
                    ));
                }
                unfinished.insert(tool_call_id.clone());
                bytes = bytes.saturating_add(arguments_delta.len());
                Some(LiveEventV1::ProviderToolInputDelta {
                    request_id: request_id.into(),
                    tool_call_id: tool_call_id.into(),
                    tool_name: function_name
                        .filter(|name| name.len() <= 256)
                        .map(|name| redactor.redact_text(aliases.get(&name).unwrap_or(&name))),
                    delta: arguments_delta,
                })
            }
            Stream::ToolCallComplete {
                tool_call_id,
                function_name,
                arguments_json,
            } => {
                *progress = true;
                bytes = bytes.saturating_add(arguments_json.len());
                if tool_call_id.is_empty()
                    || tool_call_id.len() > 256
                    || function_name.is_empty()
                    || function_name.len() > 256
                    || !completed.insert(tool_call_id.clone())
                    || response.calls.len() >= 128
                {
                    return Err(CoordinatorError::Invalid(
                        "invalid or duplicate provider tool call".into(),
                    ));
                }
                let args: serde_json::Value = serde_json::from_str(&arguments_json)?;
                if !args.is_object() {
                    return Err(CoordinatorError::Invalid(
                        "provider tool arguments must be an object".into(),
                    ));
                }
                unfinished.remove(&tool_call_id);
                response.push_call(AssistantToolCall {
                    tool_call_id,
                    function_name: aliases
                        .get(&function_name)
                        .cloned()
                        .unwrap_or(function_name),
                    arguments_json,
                });
                None
            }
            Stream::Done { usage } => {
                response.usage_complete = usage.is_some();
                response.usage = usage;
                break;
            }
            Stream::DoneWithMetadata { usage, metadata } => {
                response.usage_complete = usage.is_some()
                    && metadata.as_ref().and_then(|m| m.usage_complete) == Some(true);
                response.usage = usage;
                response.set_settled_reasoning(
                    metadata.as_ref().and_then(|m| m.settled_reasoning.clone()),
                );
                response.thinking = metadata.as_ref().and_then(|m| m.thinking.clone());
                response.continuation = metadata.as_ref().and_then(|m| m.continuation.clone());
                response.metadata = metadata;
                break;
            }
            Stream::Error {
                message,
                category,
                remediation,
                retry_after_ms,
                envelope,
            } => {
                return Err(CoordinatorError::Provider {
                    message,
                    category,
                    remediation,
                    retry_after_ms,
                    envelope,
                })
            }
            Stream::TextBlockEnd => None,
            Stream::Notice(message) => {
                // Shown even for requests that stream nothing live (compaction summaries).
                let (actor, task) = (actor.clone(), task.to_owned());
                handle
                    .call(move |s| s.live(actor, task, LiveEventV1::RuntimeWarning { message }))
                    .await?;
                None
            }
            Stream::Aborted { usage } => {
                *aborted_usage = usage;
                return Err(CoordinatorError::Cancelled(task.into()));
            }
        };
        if bytes > 4 * 1024 * 1024 || unfinished.len() > 128 {
            return Err(CoordinatorError::Invalid(
                "provider response exceeds the runtime limit".into(),
            ));
        }
        if let Some(payload) = live
            .filter(|_| emit_live)
            .and_then(|payload| display.push(payload, redactor.as_ref()))
        {
            let (actor, task) = (actor.clone(), task.to_owned());
            handle.call(move |s| s.live(actor, task, payload)).await?;
        }
    }
    if response
        .metadata
        .as_ref()
        .is_some_and(|m| m.output_limit_reached)
    {
        response.calls.clear();
    } else if !unfinished.is_empty() {
        return Err(CoordinatorError::Invalid(
            "provider ended with unfinished tool arguments".into(),
        ));
    }
    let settled_bytes = response
        .settled_reasoning
        .iter()
        .flatten()
        .map(String::len)
        .sum::<usize>();
    let continuation_bytes = response.continuation.as_ref().map_or(0, |s| s.byte_len());
    if bytes.saturating_add(settled_bytes).max(continuation_bytes) > 4 * 1024 * 1024 {
        return Err(CoordinatorError::Invalid(
            "settled provider response exceeds the runtime limit".into(),
        ));
    }
    for payload in display.finish(redactor.as_ref()) {
        let (actor, task) = (actor.clone(), task.to_owned());
        handle.call(move |s| s.live(actor, task, payload)).await?;
    }
    Ok(response)
}

/// Upper bound on a provider's abort handshake (interrupt, then its own settle grace).
const ABORT_SETTLE: std::time::Duration = std::time::Duration::from_secs(5);

/// Reads a cancelled stream until the provider settles, for the usage it billed.
async fn settle_abort(stream: &mut ProviderEventStream) -> (Option<CompletionUsage>, bool) {
    let settled = async {
        while let Some(event) = stream.next().await {
            match event {
                Stream::Aborted { usage } | Stream::Done { usage } => {
                    let complete = usage.is_some();
                    return (usage, complete);
                }
                Stream::DoneWithMetadata { usage, metadata } => {
                    let complete = usage.is_some()
                        && metadata.as_ref().and_then(|m| m.usage_complete) == Some(true);
                    return (usage, complete);
                }
                Stream::Error { .. } => return (None, false),
                _ => {}
            }
        }
        (None, false)
    };
    tokio::time::timeout(ABORT_SETTLE, settled)
        .await
        .unwrap_or_default()
}

use super::{runtime::digest, *};
use harness_providers::{
    AssistantToolCall, CompletionRequest, CompletionUsage, Provider, ProviderStreamEvent as Stream,
    ProviderStreamFinishedMetadata,
};
use std::collections::HashSet;
use tokio_stream::StreamExt;

#[derive(Default)]
pub(super) struct Response {
    pub text: String,
    pub calls: Vec<AssistantToolCall>,
    pub event_seq: u64,
    pub usage: Option<CompletionUsage>,
    pub usage_complete: bool,
    pub settled_reasoning: Option<Vec<String>>,
    pub thinking: Option<harness_providers::ProviderStreamThinkingMetadata>,
    pub logical_request: Option<CompletionRequest>,
    metadata: Option<ProviderStreamFinishedMetadata>,
}

pub(super) async fn complete(
    handle: &CoordinatorHandle,
    provider: &dyn Provider,
    actor: &EventActor,
    task: &str,
    mut request: CompletionRequest,
    selection: Box<crate::session::CanonicalRuntimeSelection>,
    budget: crate::RequestBudget,
    cancel: &CancellationToken,
    retry: ProviderRequestRetryMetadata,
    progress: &mut bool,
) -> Result<(String, Response), CoordinatorError> {
    let (provider_id, model_id) = (
        request.provider_id.clone().unwrap_or_default(),
        request.model_id.clone(),
    );
    let prompt_summary: String = request
        .messages
        .last()
        .map_or("", |m| m.content.as_str())
        .chars()
        .take(512)
        .collect();
    let request_digest = digest(&serde_json::to_string(&request)?);
    let (owner, task_id, selected) = (actor.clone(), task.to_owned(), selection.clone());
    let (provider_name, model_name) = (provider_id.clone(), model_id.clone());
    let id = handle
        .call(move |s| {
            s.check_task(&task_id)?;
            let id = s.id("provider")?;
            let agent_id = owner.agent_id.clone();
            s.emit_hooked(
                owner,
                Some(task_id.clone()),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: id.clone().into(),
                    provider_id: provider_name,
                    model_id: model_name,
                    prompt_summary,
                    request_digest,
                    metadata: Some(ProviderRequestStartedMetadata {
                        turn_id: Some(task_id),
                        runtime_selection: Some(selected.clone()),
                        context_budget: Some(budget),
                        retry: Some(retry),
                        ..Default::default()
                    }),
                }),
            )?;
            if let Some(agent_id) = agent_id {
                if let Some(agent) = s.agents.get_mut(&agent_id) {
                    let target =
                        super::resume::restored_target(&selected, agent.target.as_ref(), &s.config);
                    agent.info.model_ref = target.model_ref.clone();
                    agent.settings = (&target).into();
                    agent.target = Some(target);
                }
                s.record_selection(&agent_id)?;
            }
            Ok(id)
        })
        .await?;
    request.context.request_id = Some(id.clone());
    let mut logical_request = request.clone();
    let mut attachments = BTreeMap::new();
    let mut messages = Vec::new();
    for (index, message) in logical_request.messages.into_iter().enumerate() {
        if message.role != harness_providers::MessageRole::System {
            if let Some(attachment) = logical_request.attachments.remove(&index) {
                attachments.insert(messages.len(), attachment);
            }
            messages.push(message);
        }
    }
    logical_request.messages = messages;
    logical_request.attachments = attachments;
    logical_request.tools = None;
    let response = read(
        handle, provider, actor, task, &id, request, cancel, true, progress,
    )
    .await;
    let response = response.map(|mut response| {
        response.logical_request = Some(logical_request);
        response
    });
    let (request_id, actor, task) = (id.clone(), actor.clone(), task.to_owned());
    let response = handle
        .call(move |s| {
            // Terminal bookkeeping is accepted after cancellation, but never after task removal.
            if !s.running.contains_key(&task) {
                return Err(CoordinatorError::UnknownTask(task));
            }
            let usage = response.as_ref().ok().and_then(|r| r.usage.clone());
            let metadata = response.as_ref().ok().and_then(|r| r.metadata.as_ref());
            let finish_reason = match &response {
                Ok(_) => metadata
                    .and_then(|m| m.provider_stop_reason.clone())
                    .unwrap_or_else(|| "stop".into()),
                Err(CoordinatorError::Cancelled(_)) => "cancelled".into(),
                Err(_) => "error".into(),
            };
            let response_id = metadata.and_then(|m| m.provider_response_id.clone());
            let (provider_error_category, provider_error_remediation) = match &response {
                Err(CoordinatorError::Provider {
                    category,
                    remediation,
                    ..
                }) => (*category, remediation.clone()),
                _ => (None, None),
            };
            let finished = ProviderRequestFinishedMetadata {
                turn_id: Some(task.clone()),
                provider_response_id: response_id.clone(),
                provider_session_id: metadata.and_then(|m| m.provider_session_id.clone()),
                provider_cache_id: metadata.and_then(|m| m.provider_cache_id.clone()),
                cache_read_tokens: metadata.and_then(|m| m.cache_read_tokens),
                cache_write_tokens: metadata.and_then(|m| m.cache_write_tokens),
                provider_stop_reason: Some(finish_reason.clone()),
                provider_error_category,
                provider_error_remediation,
                ..Default::default()
            };
            s.emit_hooked(
                actor.clone(),
                Some(task.clone()),
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: request_id.clone().into(),
                    finish_reason: finish_reason.clone(),
                    output_digest: response.as_ref().ok().map(|r| digest(&r.text)),
                    usage: usage.clone(),
                    metadata: Some(finished),
                }),
            )?;
            let mut response = response?;
            let mut parts = Vec::new();
            if !response.text.is_empty() {
                parts.push(crate::session::AssistantPart::Text {
                    text: response.text.clone(),
                });
            }
            for (index, call) in response.calls.iter().enumerate() {
                let args: serde_json::Value = serde_json::from_str(&call.arguments_json)?;
                parts.push(crate::session::AssistantPart::ToolCall(
                    crate::session::AssistantToolCall {
                        tool_call_id: format!("{request_id}-tool-{index}").into(),
                        provider_tool_call_id: Some(call.tool_call_id.clone()),
                        tool_id: call.function_name.clone(),
                        args_summary: crate::redact::redact_value(s.redactor.as_ref(), &args)
                            .to_string(),
                        args_digest: digest(&call.arguments_json),
                        provider_call_id: Some(request_id.clone()),
                    },
                ));
            }
            let event = s.emit(
                actor,
                Some(task),
                EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                    request_id: request_id.clone().into(),
                    tool_call_count: response.calls.len(),
                    parts,
                    provenance: Some(crate::session::ProviderProvenance {
                        provider_id,
                        model_id,
                        request_id: request_id.into(),
                        response_id,
                        stop_reason: Some(finish_reason),
                        usage,
                        runtime_selection: Some(selection),
                    }),
                    assistant_message: None,
                }),
            )?;
            response.event_seq = event.seq;
            Ok(response)
        })
        .await?;
    Ok((id, response))
}

pub(super) async fn read(
    handle: &CoordinatorHandle,
    provider: &dyn Provider,
    actor: &EventActor,
    task: &str,
    request_id: &str,
    request: CompletionRequest,
    cancel: &CancellationToken,
    emit_live: bool,
    progress: &mut bool,
) -> Result<Response, CoordinatorError> {
    let redactor = handle.call(|s| Ok(Arc::clone(&s.redactor))).await?;
    let mut display = LiveOutput::default();
    let aliases: BTreeMap<_, _> = request
        .tools
        .iter()
        .flatten()
        .map(|tool| (tool.function_name.clone(), tool.tool_id.clone()))
        .collect();
    let mut stream = tokio::select! {
        biased;
        () = cancel.cancelled() => return Err(CoordinatorError::Cancelled(task.into())),
        stream = provider.stream_completion(request) => stream,
    };
    let mut response = Response::default();
    let mut completed = HashSet::new();
    let mut unfinished = HashSet::new();
    let mut bytes = 0usize;
    loop {
        let event = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(CoordinatorError::Cancelled(task.into())),
            event = stream.next() => event.ok_or_else(|| CoordinatorError::Invalid("provider stream ended before completion".into()))?,
        };
        let live = match event {
            Stream::Start | Stream::Started { .. } => None,
            Stream::TextDelta(delta) => {
                *progress |= !delta.is_empty();
                bytes = bytes.saturating_add(delta.len());
                response.text.push_str(&delta);
                Some(LiveEventV1::ProviderTextDelta {
                    request_id: request_id.into(),
                    delta,
                })
            }
            Stream::ReasoningDelta(delta) => {
                *progress |= !delta.is_empty();
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
                response.calls.push(AssistantToolCall {
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
                response.settled_reasoning =
                    metadata.as_ref().and_then(|m| m.settled_reasoning.clone());
                response.thinking = metadata.as_ref().and_then(|m| m.thinking.clone());
                response.metadata = metadata;
                break;
            }
            Stream::Error {
                message,
                category,
                remediation,
                retry_after_ms,
            } => {
                return Err(CoordinatorError::Provider {
                    message,
                    category,
                    remediation,
                    retry_after_ms,
                })
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
    if !unfinished.is_empty() {
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
    if bytes.saturating_add(settled_bytes) > 4 * 1024 * 1024 {
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

#[derive(Default)]
struct LiveOutput {
    pending: BTreeMap<(u8, String), (LiveEventV1, String, bool)>,
}
impl LiveOutput {
    fn push(
        &mut self,
        mut payload: LiveEventV1,
        redactor: &dyn crate::redact::Redactor,
    ) -> Option<LiveEventV1> {
        let (key, delta) = match &mut payload {
            LiveEventV1::ProviderTextDelta { delta, .. } => ((0, String::new()), delta),
            LiveEventV1::ProviderReasoningDelta { delta, .. } => ((1, String::new()), delta),
            LiveEventV1::ProviderToolInputDelta {
                tool_call_id,
                delta,
                ..
            } => ((2, tool_call_id.to_string()), delta),
            _ => return Some(payload),
        };
        let delta = std::mem::take(delta);
        let tool_name = match &payload {
            LiveEventV1::ProviderToolInputDelta { tool_name, .. } => tool_name.clone(),
            _ => None,
        };
        let (template, pending, held) =
            self.pending
                .entry(key)
                .or_insert((payload, String::new(), false));
        if let LiveEventV1::ProviderToolInputDelta {
            tool_name: current, ..
        } = template
        {
            if tool_name.is_some() {
                current.clone_from(&tool_name);
            }
        }
        pending.push_str(&delta);
        if *held {
            return None;
        }
        let prefix = redactor.streaming_prefix(pending);
        let safe: String = pending.drain(..prefix).collect();
        // ponytail: defer a suspicious tail over 4 KiB instead of rescanning it per delta.
        // The response-wide 4 MiB budget also bounds all retained tails together.
        *held = pending.len() > 4096;
        (!safe.is_empty() || tool_name.is_some()).then(|| with_delta(template.clone(), safe))
    }
    fn finish(self, redactor: &dyn crate::redact::Redactor) -> Vec<LiveEventV1> {
        self.pending
            .into_values()
            .filter_map(|(payload, pending, _)| {
                (!pending.is_empty()).then(|| with_delta(payload, redactor.redact_text(&pending)))
            })
            .collect()
    }
}
fn with_delta(mut payload: LiveEventV1, text: String) -> LiveEventV1 {
    match &mut payload {
        LiveEventV1::ProviderTextDelta { delta, .. }
        | LiveEventV1::ProviderReasoningDelta { delta, .. }
        | LiveEventV1::ProviderToolInputDelta { delta, .. } => *delta = text,
        _ => {}
    }
    payload
}

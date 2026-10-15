use super::{runtime::digest, *};
use harness_providers::{
    AssistantToolCall, CompletionRequest, CompletionUsage, Provider, ProviderEventStream,
    ProviderSessionEvent, ProviderStreamEvent as Stream, ProviderStreamFinishedMetadata,
};

/// One ordered block of a streamed response. Reasoning stays live-only: the journal event
/// skips it, so a restored entry has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ResponsePart {
    Text(String),
    Reasoning(String),
    /// Index into the response's calls.
    Call(usize),
}

#[derive(Default)]
pub(super) struct Response {
    /// Text and call blocks in arrival order; adjacent text deltas are merged.
    pub parts: Vec<ResponsePart>,
    pub text: String,
    pub continuation: Option<Box<harness_providers::ProviderContinuation>>,
    pub calls: Vec<AssistantToolCall>,
    pub event_seq: u64,
    pub usage: Option<CompletionUsage>,
    pub usage_complete: bool,
    pub settled_reasoning: Option<Vec<String>>,
    pub thinking: Option<harness_providers::ProviderStreamThinkingMetadata>,
    pub logical_request: Option<CompletionRequest>,
    metadata: Option<ProviderStreamFinishedMetadata>,
}

impl Response {
    /// Appends streamed text, merging it into a trailing text block.
    pub fn push_text(&mut self, delta: &str) {
        self.text.push_str(delta);
        match self.parts.last_mut() {
            Some(ResponsePart::Text(text)) => text.push_str(delta),
            _ if delta.is_empty() => {}
            _ => self.parts.push(ResponsePart::Text(delta.to_owned())),
        }
    }

    pub fn push_reasoning(&mut self, delta: &str) {
        match self.parts.last_mut() {
            Some(ResponsePart::Reasoning(text)) => text.push_str(delta),
            _ if delta.is_empty() => {}
            _ => self.parts.push(ResponsePart::Reasoning(delta.to_owned())),
        }
    }

    /// Records the settled reasoning blocks and makes the reasoning parts carry exactly
    /// them. With as many blocks as streamed reasoning parts each replaces its own; with a
    /// different count all blocks sit at the first reasoning position, or at the start when
    /// none streamed. Unsettled reasoning leaves no reasoning parts.
    pub fn set_settled_reasoning(&mut self, settled: Option<Vec<String>>) {
        let streamed = self
            .parts
            .iter()
            .filter(|part| matches!(part, ResponsePart::Reasoning(_)))
            .count();
        let blocks = settled.clone().unwrap_or_default();
        let first = self
            .parts
            .iter()
            .position(|part| matches!(part, ResponsePart::Reasoning(_)))
            .unwrap_or(0);
        if streamed == blocks.len() {
            let mut blocks = blocks.into_iter();
            for part in &mut self.parts {
                if let (ResponsePart::Reasoning(text), Some(block)) = (part, blocks.next()) {
                    *text = block;
                }
            }
        } else {
            self.parts
                .retain(|part| !matches!(part, ResponsePart::Reasoning(_)));
            let at = first.min(self.parts.len());
            self.parts
                .splice(at..at, blocks.into_iter().map(ResponsePart::Reasoning));
        }
        self.settled_reasoning = settled;
    }

    /// The blocks as an entry carries them.
    pub fn item_parts(&self) -> Vec<harness_compaction::ItemPart<'static>> {
        // Reasoning that never settled is not part of the entry, matching its empty
        // `settled_reasoning`.
        let settled = self.settled_reasoning.is_some();
        self.parts
            .iter()
            .filter_map(|part| match part {
                ResponsePart::Text(text) => Some(harness_compaction::ItemPart::text(text.clone())),
                ResponsePart::Reasoning(text) if settled => {
                    Some(harness_compaction::ItemPart::reasoning(text.clone()))
                }
                ResponsePart::Reasoning(_) => None,
                ResponsePart::Call(index) => Some(harness_compaction::ItemPart::Call(*index)),
            })
            .collect()
    }

    pub fn push_call(&mut self, call: AssistantToolCall) {
        self.parts.push(ResponsePart::Call(self.calls.len()));
        self.calls.push(call);
    }

    pub fn ensure_complete(&self) -> Result<(), CoordinatorError> {
        if self.output_limited() {
            let category = harness_providers::ProviderErrorCategory::OutputLimitExceeded;
            return Err(CoordinatorError::Provider {
                message:
                    "The response is incomplete because the provider reached its output limit."
                        .into(),
                category: Some(category),
                remediation: Some(category.remediation().into()),
                retry_after_ms: None,
                envelope: None,
            });
        }
        Ok(())
    }
    /// The provider stopped because it exhausted the output budget.
    pub fn output_limited(&self) -> bool {
        self.metadata
            .as_ref()
            .is_some_and(|m| m.output_limit_reached)
    }
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
    stream_guard_enabled: bool,
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
    let (id, started_at_ms) = handle
        .call(move |s| {
            s.check_task(&task_id)?;
            let id = s.id("provider")?;
            let agent_id = owner.agent_id.clone();
            let started = s.emit_hooked(
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
            let started_at_ms = started
                .ts
                .as_deref()
                .and_then(crate::clock::rfc3339_unix_ms);
            Ok((id, started_at_ms))
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
    if let Some(session_id) = request.context.session_id.clone() {
        provider.session_event(&ProviderSessionEvent::Routed {
            session_id,
            provider_id: provider_id.clone(),
            model_id: model_id.clone(),
        });
    }
    let mut aborted_usage = None;
    let response = read(
        handle,
        provider,
        actor,
        task,
        &id,
        request,
        cancel,
        read::Live::Turn,
        stream_guard_enabled,
        progress,
        &mut aborted_usage,
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
            let usage = match &response {
                Ok(response) => response.usage.clone(),
                Err(_) => aborted_usage,
            };
            let metadata = response.as_ref().ok().and_then(|r| r.metadata.as_ref());
            let finish_reason = match &response {
                Ok(_) => metadata
                    .and_then(|m| m.provider_stop_reason.clone())
                    .unwrap_or_else(|| "stop".into()),
                Err(CoordinatorError::Cancelled(_)) => "cancelled".into(),
                Err(CoordinatorError::StreamGuard(_)) => "stream_guard".into(),
                Err(_) => "error".into(),
            };
            let response_id = metadata.and_then(|m| m.provider_response_id.clone());
            let output_limit_reached = metadata.is_some_and(|m| m.output_limit_reached);
            let (provider_error_category, provider_error_remediation) = match &response {
                Ok(_) if output_limit_reached => {
                    let category = harness_providers::ProviderErrorCategory::OutputLimitExceeded;
                    (Some(category), Some(category.remediation().into()))
                }
                Err(CoordinatorError::Provider {
                    category,
                    remediation,
                    ..
                }) => (*category, remediation.clone()),
                _ => (None, None),
            };
            let cache_write_1h_tokens =
                metadata.map(|m| m.cache_write_1h_tokens).filter(|n| *n > 0);
            let cost_picodollars = usage.as_ref().and_then(|usage| {
                let tokens = crate::config::BilledTokens::new(
                    usage,
                    metadata.and_then(|m| m.cache_read_tokens).unwrap_or(0),
                    metadata.and_then(|m| m.cache_write_tokens).unwrap_or(0),
                    cache_write_1h_tokens.unwrap_or(0),
                );
                let picodollars = s
                    .config
                    .model_catalog
                    .iter()
                    .find(|entry| entry.provider == provider_id && entry.model == model_id)?
                    .cost
                    .as_ref()?
                    .picodollars(&tokens, started_at_ms)?;
                u64::try_from(picodollars).ok()
            });
            let finished = ProviderRequestFinishedMetadata {
                turn_id: Some(task.clone()),
                provider_response_id: response_id.clone(),
                provider_session_id: metadata.and_then(|m| m.provider_session_id.clone()),
                provider_cache_id: metadata.and_then(|m| m.provider_cache_id.clone()),
                cache_read_tokens: metadata.and_then(|m| m.cache_read_tokens),
                cache_write_tokens: metadata.and_then(|m| m.cache_write_tokens),
                cache_write_1h_tokens,
                cost_picodollars,
                provider_stop_reason: Some(finish_reason.clone()),
                provider_error_category,
                provider_error_remediation,
                account: metadata
                    .and_then(|m| m.session_report.as_ref())
                    .and_then(|r| r.account.clone()),
                native_compactions: metadata
                    .and_then(|m| m.session_report.as_ref())
                    .map(|r| r.native_compactions.clone())
                    .unwrap_or_default(),
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
            for part in &response.parts {
                match part {
                    ResponsePart::Text(text) if !text.is_empty() => {
                        parts.push(crate::session::AssistantPart::Text { text: text.clone() });
                    }
                    ResponsePart::Text(_) | ResponsePart::Reasoning(_) => {}
                    ResponsePart::Call(index) => {
                        let call = &response.calls[*index];
                        let args: serde_json::Value = serde_json::from_str(&call.arguments_json)?;
                        parts.push(crate::session::AssistantPart::ToolCall(
                            crate::session::AssistantToolCall {
                                tool_call_id: format!("{request_id}-tool-{index}").into(),
                                provider_tool_call_id: Some(call.tool_call_id.clone()),
                                tool_id: call.function_name.clone(),
                                args_summary: crate::redact::redact_value(
                                    s.redactor.as_ref(),
                                    &args,
                                )
                                .to_string(),
                                args_digest: digest(&call.arguments_json),
                                provider_call_id: Some(request_id.clone()),
                            },
                        ));
                    }
                }
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

mod read;
pub(super) use read::{read, Live};
mod guard;
mod live_output;
#[cfg(test)]
mod tests;
use live_output::LiveOutput;

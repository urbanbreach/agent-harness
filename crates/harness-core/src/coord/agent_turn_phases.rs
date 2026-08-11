// allow: SIZE_OK — coordinator turn phase state machine (scheduling + dispatch)
use super::*;
use crate::UnwrapOrAbort;
use std::time::Duration;
use tokio_stream::StreamExt;

pub(in crate::coord) struct AgentTurnPhaseLoopRequest<'a> {
    pub(in crate::coord) provider: Arc<dyn Provider>,
    pub(in crate::coord) tool_registry: Arc<ToolRegistry>,
    pub(in crate::coord) task: &'a QueuedAgentTurn,
    pub(in crate::coord) prior_context: &'a ProviderContext,
    pub(in crate::coord) job_tx: mpsc::Sender<Command>,
    pub(in crate::coord) cancellation_token: CancellationToken,
    pub(in crate::coord) provider_retry: ProviderRetryRuntimeConfig,
}

struct AgentProviderTurnState {
    model: AgentModelRef,
    tool_defs: Vec<ToolDef>,
    messages: Vec<CompletionMessage>,
    total_tool_calls: usize,
}

enum AgentToolPhaseDecision {
    RunTools(Vec<AssistantToolIntent>),
    TurnEnd { output: String },
}

struct ProviderStreamPhaseRequest<'a> {
    provider: Arc<dyn Provider>,
    profile: &'a AgentProfile,
    request: &'a AgentRequest,
    turn_request_id: &'a str,
    provider_request_id: String,
    model: AgentModelRef,
    messages: &'a [CompletionMessage],
    tool_defs: &'a [ToolDef],
    retry_metadata: ProviderRequestRetryMetadata,
    job_tx: mpsc::Sender<Command>,
    task_id: &'a str,
    agent_id: &'a str,
    session_id: &'a str,
}

pub(in crate::coord) async fn run_agent_turn_phase_loop(
    request: AgentTurnPhaseLoopRequest<'_>,
) -> AgentTurnOutcome {
    let AgentTurnPhaseLoopRequest {
        provider,
        tool_registry,
        task,
        prior_context,
        job_tx,
        cancellation_token,
        provider_retry,
    } = request;

    let mut turn_state = match prepare_provider_transform_phase(
        &task.profile,
        &task.request,
        prior_context,
        tool_registry.as_ref(),
    ) {
        Ok(turn_state) => turn_state,
        Err(reason) => return AgentTurnOutcome::failed(reason),
    };
    let current_turn_start_index = turn_state.messages.len().saturating_sub(1);

    loop {
        if cancellation_token.is_cancelled() {
            return AgentTurnOutcome::failed_with_memory(
                "job cancelled",
                AgentTurnFailure::new(
                    ProviderConversationTurnStatus::Aborted,
                    "cancelled",
                    "job cancelled",
                    "",
                    None,
                ),
            );
        }

        let assistant_response = match run_provider_with_retry_phase(
            &provider,
            &turn_state,
            task,
            &job_tx,
            &cancellation_token,
            &provider_retry,
        )
        .await
        {
            Ok(response) => response,
            Err(mut failure) => {
                let reason = normalize_provider_phase_error(failure.to_string());
                failure.reason = reason.clone();
                return AgentTurnOutcome::Failed {
                    reason,
                    memory: (failure.failure_stage == "provider_error").then_some(failure),
                };
            }
        };
        if let Err(reason) = append_assistant_message_end_phase(
            &job_tx,
            &task.task_id,
            &task.agent_id,
            &mut turn_state.messages,
            &assistant_response,
        )
        .await
        {
            return AgentTurnOutcome::failed(reason);
        }
        if cancellation_token.is_cancelled() {
            return AgentTurnOutcome::failed_with_memory(
                "job cancelled",
                AgentTurnFailure::new(
                    ProviderConversationTurnStatus::Aborted,
                    "cancelled",
                    "job cancelled",
                    assistant_response.text.clone(),
                    Some(assistant_response.request_id.to_string()),
                ),
            );
        }

        match decide_tool_phase(&assistant_response, &mut turn_state.total_tool_calls) {
            Ok(AgentToolPhaseDecision::TurnEnd { output }) => {
                return AgentTurnOutcome::Succeeded {
                    output,
                    messages: completion_messages_to_conversation_messages(
                        &task.profile,
                        &task.request_id,
                        &task.agent_id,
                        &turn_state.messages[current_turn_start_index..],
                    ),
                };
            }
            Ok(AgentToolPhaseDecision::RunTools(tool_intents)) => {
                if let Err(reason) = run_tool_phase(
                    &job_tx,
                    &task.agent_id,
                    &task.profile,
                    &mut turn_state.messages,
                    tool_intents,
                )
                .await
                {
                    return AgentTurnOutcome::failed_with_memory(
                        reason.clone(),
                        AgentTurnFailure::new(
                            ProviderConversationTurnStatus::Failed,
                            "tool_failure",
                            reason,
                            assistant_response.text.clone(),
                            Some(assistant_response.request_id.to_string()),
                        ),
                    );
                }
            }
            Err(reason) => return AgentTurnOutcome::failed(reason),
        }

        if cancellation_token.is_cancelled() {
            return AgentTurnOutcome::failed_with_memory(
                "job cancelled",
                AgentTurnFailure::new(
                    ProviderConversationTurnStatus::Aborted,
                    "cancelled",
                    "job cancelled",
                    assistant_response.text.clone(),
                    Some(assistant_response.request_id.to_string()),
                ),
            );
        }
    }
}

fn normalize_provider_phase_error(reason: String) -> String {
    if reason.contains("empty tool_call_id") && !reason.contains("invalid") {
        format!("invalid provider tool_call_id: {reason}")
    } else {
        reason
    }
}

pub(in crate::coord) async fn execute_session_title_operation(
    provider: Arc<dyn Provider>,
    operation: SessionTitleOperationSpec,
    prompt: &str,
) -> Result<Option<String>, String> {
    let model = AgentModelRef::parse(&operation.model_ref);
    let mut stream = provider
        .stream_completion(CompletionRequest {
            provider_id: Some(model.provider_id),
            model_id: model.model_id,
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: TITLE_OPERATION_SYSTEM_PROMPT.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: TITLE_GENERATION_USER_PROMPT.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: prompt.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
            ],
            temperature: Some(operation.temperature),
            max_tokens: None,
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            tools: None,
            tool_choice: None,
            context: Default::default(),
            stream: true,
        })
        .await;

    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderStreamEvent::Error { message, .. } => return Err(message),
            ProviderStreamEvent::Start
            | ProviderStreamEvent::Started { .. }
            | ProviderStreamEvent::ReasoningDelta(_)
            | ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. }
            | ProviderStreamEvent::Done { .. }
            | ProviderStreamEvent::DoneWithMetadata { .. } => {}
        }
    }

    Ok(clean_generated_title(&text))
}

fn prepare_provider_transform_phase(
    profile: &AgentProfile,
    request: &AgentRequest,
    prior_context: &ProviderContext,
    tool_registry: &ToolRegistry,
) -> Result<AgentProviderTurnState, String> {
    let model = AgentModelRef::parse(&request.model_ref);
    let tool_defs = build_provider_tool_defs_for_model(profile, tool_registry, &request.model_ref)?;
    let provider_prompt = request.provider_prompt();
    let messages = build_provider_context_messages(profile, prior_context, &provider_prompt);

    Ok(AgentProviderTurnState {
        model,
        tool_defs,
        messages,
        total_tool_calls: 0,
    })
}

async fn allocate_provider_request_id_phase(
    job_tx: &mpsc::Sender<Command>,
) -> Result<String, String> {
    let (respond_to, response_rx) = oneshot::channel();
    job_tx
        .send(Command::AllocateProviderRequestId { respond_to })
        .await
        .map_err(|_| "provider request id channel closed".to_string())?;
    response_rx
        .await
        .map_err(|_| "provider request id response channel closed".to_string())?
        .map_err(|err| err.to_string())
}

async fn run_provider_with_retry_phase(
    provider: &Arc<dyn Provider>,
    turn_state: &AgentProviderTurnState,
    task: &QueuedAgentTurn,
    job_tx: &mpsc::Sender<Command>,
    cancellation_token: &CancellationToken,
    provider_retry: &ProviderRetryRuntimeConfig,
) -> Result<AssistantResponse, AgentTurnFailure> {
    let max_attempts = provider_retry.max_retries.saturating_add(1);
    let mut attempt = 0_u32;
    let mut prior_retry_category = None;
    let mut prior_retry_delay_ms = 0_u64;
    let mut last_provider_request_id = None;

    loop {
        wait_provider_retry_backoff(
            prior_retry_delay_ms,
            cancellation_token,
            last_provider_request_id.clone(),
        )
        .await?;

        let provider_request_id = allocate_provider_request_id_phase(job_tx)
            .await
            .map_err(AgentTurnFailure::message)?;
        last_provider_request_id = Some(provider_request_id.clone());
        let retry_metadata = ProviderRequestRetryMetadata {
            attempt,
            max_attempts,
            delay_ms: Some(prior_retry_delay_ms),
            category: prior_retry_category,
        };
        let result = run_provider_stream_phase(ProviderStreamPhaseRequest {
            provider: Arc::clone(provider),
            profile: &task.profile,
            request: &task.request,
            turn_request_id: &task.request_id,
            provider_request_id,
            model: turn_state.model.clone(),
            messages: &turn_state.messages,
            tool_defs: &turn_state.tool_defs,
            retry_metadata,
            job_tx: job_tx.clone(),
            task_id: &task.task_id,
            agent_id: &task.agent_id,
            session_id: task.session_id.as_str(),
        })
        .await;

        match result {
            Ok(response) => return Ok(response),
            Err(failure) if should_retry_provider_failure(&failure, provider_retry, attempt) => {
                attempt = attempt.saturating_add(1);
                prior_retry_category = failure.provider_error_category;
                prior_retry_delay_ms =
                    provider_retry_delay_ms(provider_retry, attempt, failure.retry_after_ms);
            }
            Err(failure) => return Err(failure),
        }
    }
}

async fn wait_provider_retry_backoff(
    delay_ms: u64,
    cancellation_token: &CancellationToken,
    provider_request_id: Option<String>,
) -> Result<(), AgentTurnFailure> {
    if delay_ms == 0 {
        return Ok(());
    }

    tokio::select! {
        _ = cancellation_token.cancelled() => Err(AgentTurnFailure::new(
            ProviderConversationTurnStatus::Aborted,
            "cancelled",
            "job cancelled",
            "",
            provider_request_id,
        )),
        _ = tokio::time::sleep(Duration::from_millis(delay_ms)) => Ok(()),
    }
}

fn should_retry_provider_failure(
    failure: &AgentTurnFailure,
    provider_retry: &ProviderRetryRuntimeConfig,
    attempt: u32,
) -> bool {
    failure.failure_stage == "provider_error"
        && failure.partial_assistant_output.is_empty()
        && attempt < provider_retry.max_retries
        && matches!(
            failure.provider_error_category,
            Some(ProviderErrorCategory::RateLimited | ProviderErrorCategory::TransportFailure)
        )
}

fn provider_retry_delay_ms(
    provider_retry: &ProviderRetryRuntimeConfig,
    retry_attempt: u32,
    retry_after_ms: Option<u64>,
) -> u64 {
    let max_delay = provider_retry.max_delay_ms;
    if let Some(delay) = retry_after_ms {
        return delay.min(max_delay);
    }

    let exponent = retry_attempt.saturating_sub(1).min(31);
    let multiplier = 1_u64 << exponent;
    provider_retry
        .base_delay_ms
        .saturating_mul(multiplier)
        .min(max_delay)
}

async fn run_provider_stream_phase(
    request: ProviderStreamPhaseRequest<'_>,
) -> Result<AssistantResponse, AgentTurnFailure> {
    let ProviderStreamPhaseRequest {
        provider,
        profile,
        request,
        turn_request_id,
        provider_request_id,
        model,
        messages,
        tool_defs,
        retry_metadata,
        job_tx,
        task_id,
        agent_id,
        session_id,
    } = request;
    let task_id = task_id.to_string();
    let agent_id = agent_id.to_string();

    stream_assistant_response_once(
        StreamAssistantResponseOnceRequest {
            provider,
            profile,
            model,
            model_settings: request.model_settings.clone(),
            turn_request_id: turn_request_id.to_string(),
            provider_request_id,
            session_id: Some(session_id.to_string()),
            prompt_summary: &request.prompt,
            retry_metadata: Some(retry_metadata),
            context: ProviderBoundaryContext::ProviderMessages { messages },
            tool_defs,
        },
        |event| {
            let job_tx = job_tx.clone();
            let task_id = task_id.clone();
            let agent_id = agent_id.clone();
            async move {
                if let Err(reason) =
                    emit_agent_runtime_event_phase(job_tx, task_id, agent_id, event).await
                {
                    tracing::warn!(reason, "failed to emit agent runtime phase event");
                }
            }
        },
    )
    .await
}

async fn emit_agent_runtime_event_phase(
    job_tx: mpsc::Sender<Command>,
    task_id: String,
    agent_id: String,
    event: AgentRuntimeEvent,
) -> Result<(), String> {
    match event {
        AgentRuntimeEvent::ProviderRequestStarted(started) => job_tx
            .send(Command::AgentProviderRequestStarted {
                task_id,
                agent_id,
                request_id: started.request_id.to_string(),
                provider_id: started.provider_id,
                model_id: started.model_id,
                prompt_summary: started.prompt_summary,
                request_digest: started.request_digest,
                metadata: started.metadata,
            })
            .await
            .map_err(|_| "provider request start channel closed".to_string()),
        AgentRuntimeEvent::ProviderStreamDelta { request_id, delta } => job_tx
            .send(Command::AgentProviderStreamDelta {
                task_id,
                agent_id,
                request_id,
                delta,
            })
            .await
            .map_err(|_| "provider stream delta channel closed".to_string()),
        AgentRuntimeEvent::ProviderReasoningDelta { request_id, delta } => job_tx
            .send(Command::AgentProviderReasoningDelta {
                task_id,
                agent_id,
                request_id,
                delta,
            })
            .await
            .map_err(|_| "provider reasoning delta channel closed".to_string()),
        AgentRuntimeEvent::ProviderRequestFinished(finished) => {
            let (respond_to, response_rx) = oneshot::channel();
            job_tx
                .send(Command::AgentProviderRequestFinished {
                    task_id,
                    agent_id,
                    request_id: finished.request_id.to_string(),
                    finish_reason: finished.finish_reason,
                    output_digest: finished.output_digest,
                    usage: finished.usage,
                    metadata: finished.metadata,
                    respond_to: Some(respond_to),
                })
                .await
                .map_err(|_| "provider request finish channel closed".to_string())?;
            response_rx
                .await
                .map_err(|_| "provider request finish response channel closed".to_string())?
                .map_err(|err| err.to_string())
        }
    }
}

async fn append_assistant_message_end_phase(
    job_tx: &mpsc::Sender<Command>,
    task_id: &str,
    agent_id: &str,
    messages: &mut Vec<CompletionMessage>,
    response: &AssistantResponse,
) -> Result<(), String> {
    let assistant_tool_calls = (!response.tool_intents.is_empty()).then(|| {
        response
            .tool_intents
            .iter()
            .map(|tool_call| AssistantToolCall {
                tool_call_id: tool_call.tool_call_id.to_string(),
                function_name: tool_call.function_name.clone(),
                arguments_json: tool_call.arguments_json.clone(),
            })
            .collect::<Vec<_>>()
    });

    messages.push(CompletionMessage {
        role: MessageRole::Assistant,
        content: response.text.clone(),
        name: None,
        tool_call_id: None,
        assistant_tool_calls,
    });

    let (respond_to, response_rx) = oneshot::channel();
    job_tx
        .send(Command::AgentAssistantMessageFinished {
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            request_id: response.request_id.to_string(),
            assistant_output: response.text.clone(),
            tool_call_count: response.tool_intents.len(),
            assistant_message: response.finished_metadata.assistant_message.clone(),
            respond_to,
        })
        .await
        .map_err(|_| "assistant message finish channel closed".to_string())?;
    response_rx
        .await
        .map_err(|_| "assistant message finish response channel closed".to_string())?
        .map_err(|err| err.to_string())
}

fn decide_tool_phase(
    response: &AssistantResponse,
    total_tool_calls: &mut usize,
) -> Result<AgentToolPhaseDecision, String> {
    if response.tool_intents.is_empty() {
        return Ok(AgentToolPhaseDecision::TurnEnd {
            output: response.text.clone(),
        });
    }

    *total_tool_calls += response.tool_intents.len();
    if *total_tool_calls > MAX_TOOL_CALLS_TOTAL {
        return Err(format!(
            "agent turn exceeded MAX_TOOL_CALLS_TOTAL={MAX_TOOL_CALLS_TOTAL}"
        ));
    }

    Ok(AgentToolPhaseDecision::RunTools(
        response.tool_intents.clone(),
    ))
}

async fn run_tool_phase(
    job_tx: &mpsc::Sender<Command>,
    agent_id: &str,
    profile: &AgentProfile,
    messages: &mut Vec<CompletionMessage>,
    tool_intents: Vec<AssistantToolIntent>,
) -> Result<(), String> {
    let mut tool_phase_tasks = tokio::task::JoinSet::new();
    let tool_count = tool_intents.len();

    for (source_index, tool_call) in tool_intents.into_iter().enumerate() {
        let job_tx = job_tx.clone();
        let agent_id = agent_id.to_string();
        let tool_id = tool_call.tool_id.clone();
        let args_json = tool_call.arguments.clone();

        tool_phase_tasks.spawn(async move {
            let result = execute_agent_tool_phase(&job_tx, &agent_id, tool_id, args_json).await;
            AgentToolPhaseResult {
                source_index,
                tool_call,
                result,
            }
        });
    }

    let mut source_ordered_results = (0..tool_count).map(|_| None).collect::<Vec<_>>();
    while let Some(joined) = tool_phase_tasks.join_next().await {
        let phase_result = joined.map_err(|err| format!("tool phase task failed: {err}"))?;
        let source_index = phase_result.source_index;
        source_ordered_results[source_index] = Some(phase_result);
    }

    for phase_result in source_ordered_results {
        let AgentToolPhaseResult {
            tool_call, result, ..
        } = phase_result.unwrap_or_abort();
        let tool_result = match result {
            Ok(result) => result,
            Err(reason)
                if matches!(
                    profile.tool_failure_mode,
                    ToolFailureMode::ContinueAsToolMessage
                ) =>
            {
                ToolResult::structured(
                    format!("tool call `{}` failed: {reason}", tool_call.function_name),
                    json!({
                        "error": reason,
                        "status": "failed"
                    }),
                )
            }
            Err(reason) => {
                return Err(format!(
                    "tool call `{}` failed closed: {reason}",
                    tool_call.function_name
                ));
            }
        };

        append_tool_result_message_phase(messages, &tool_call, &tool_result);
    }

    Ok(())
}

struct AgentToolPhaseResult {
    source_index: usize,
    tool_call: AssistantToolIntent,
    result: Result<ToolResult, String>,
}

async fn execute_agent_tool_phase(
    job_tx: &mpsc::Sender<Command>,
    agent_id: &str,
    tool_id: String,
    args_json: Value,
) -> Result<ToolResult, String> {
    let (respond_to, response_rx) = oneshot::channel();
    job_tx
        .send(Command::ExecuteAgentToolCall {
            actor: EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
            legacy_profile_hint: None,
            tool_id,
            args_json,
            respond_to,
        })
        .await
        .map_err(|_| "tool call channel closed".to_string())?;
    response_rx
        .await
        .map_err(|_| "tool call response channel closed".to_string())?
}

fn append_tool_result_message_phase(
    messages: &mut Vec<CompletionMessage>,
    tool_call: &AssistantToolIntent,
    tool_result: &ToolResult,
) {
    messages.push(CompletionMessage {
        role: MessageRole::Tool,
        content: tool_result_to_message_content(tool_result),
        name: Some(tool_call.function_name.clone()),
        tool_call_id: Some(tool_call.tool_call_id.to_string()),
        assistant_tool_calls: None,
    });
}

pub(in crate::coord) fn completion_messages_to_conversation_messages(
    profile: &AgentProfile,
    request_id: &str,
    agent_id: &str,
    messages: &[CompletionMessage],
) -> Vec<ConversationMessage> {
    let mapping =
        crate::tool::build_tool_function_name_mapping(profile.toolset.iter().map(String::as_str));
    let mut tool_ids_by_call_id = BTreeMap::new();
    let mut conversation_messages = Vec::new();

    for message in messages {
        match message.role {
            MessageRole::System => {}
            MessageRole::User => {
                conversation_messages.push(ConversationMessage::User(ConversationUserMessage {
                    request_id: request_id.into(),
                    text: message.content.clone(),
                    seq: None,
                    agent_id: Some(agent_id.to_string()),
                }))
            }
            MessageRole::Assistant => {
                let tool_calls = message
                    .assistant_tool_calls
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|tool_call| {
                        let tool_id = mapping
                            .tool_id_for_function_name(&tool_call.function_name)
                            .unwrap_or(&tool_call.function_name)
                            .to_string();
                        tool_ids_by_call_id.insert(tool_call.tool_call_id.clone(), tool_id.clone());
                        ConversationToolCall {
                            tool_call_id: tool_call.tool_call_id.clone().into(),
                            tool_id,
                            args_summary: provider_tool_arguments_json(&tool_call.arguments_json),
                            args_digest: digest12(tool_call.arguments_json.as_bytes()),
                            seq: None,
                            metadata: None,
                        }
                    })
                    .collect();
                conversation_messages.push(ConversationMessage::Assistant(
                    ConversationAssistantMessage {
                        request_id: request_id.into(),
                        agent_id: Some(agent_id.to_string()),
                        text: message.content.clone(),
                        tool_calls,
                        stop_reason: None,
                        first_seq: None,
                        last_seq: None,
                        provider_id: None,
                        model_id: None,
                        output_digest: None,
                    },
                ));
            }
            MessageRole::Tool => {
                let tool_call_id = message.tool_call_id.clone().unwrap_or_default();
                let content = provider_tool_result_display_content(&message.content);
                let tool_id = message
                    .name
                    .as_deref()
                    .and_then(|name| mapping.tool_id_for_function_name(name))
                    .map(str::to_string)
                    .or_else(|| tool_ids_by_call_id.get(&tool_call_id).cloned())
                    .or_else(|| message.name.clone());
                conversation_messages.push(ConversationMessage::ToolResult(Box::new(
                    ConversationToolResultMessage {
                        request_id: request_id.into(),
                        tool_call_id: tool_call_id.into(),
                        tool_id,
                        status: provider_tool_message_status(&content),
                        output_summary: non_empty_trimmed(&content).map(|_| content.clone()),
                        output_digest: (!content.is_empty()).then(|| digest12(content.as_bytes())),
                        output_json: None,
                        seq: None,
                        metadata: None,
                    },
                )));
            }
        }
    }

    conversation_messages
}

fn provider_tool_result_display_content(content: &str) -> String {
    serde_json::from_str::<Value>(content)
        .ok()
        .and_then(|value| {
            value
                .get("_harness_tool_result")
                .and_then(|payload| payload.get("text"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| content.to_string())
}

pub(in crate::coord) fn provider_tool_message_status(content: &str) -> ToolCallStatus {
    let trimmed = content.trim_start();
    if trimmed.starts_with("tool call `") && trimmed.contains("` failed: ") {
        ToolCallStatus::Failed
    } else {
        ToolCallStatus::Succeeded
    }
}

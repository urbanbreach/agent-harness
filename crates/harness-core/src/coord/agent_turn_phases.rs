// allow: SIZE_OK — coordinator turn phase state machine (scheduling + dispatch)
use super::*;
use crate::agent::canonical_provider_messages;
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
    pub(in crate::coord) compaction: CompactionSettings,
    pub(in crate::coord) committed_prompt_request_id: Option<crate::ids::RequestId>,
    pub(in crate::coord) canonical_view: Option<&'a crate::session::CanonicalProviderView>,
    pub(in crate::coord) transient_operational_turns: &'a [ProviderConversationTurn],
}

pub(in crate::coord) struct AgentProviderTurnState {
    model: AgentModelRef,
    model_settings: AgentModelSettings,
    canonical_view: Option<crate::session::CanonicalProviderView>,
    transient_operational_turns: Vec<ProviderConversationTurn>,
    tool_defs: Vec<ToolDef>,
    messages: Vec<CompletionMessage>,
    request_budget: ProviderRequestBudgetContext,
    pub(in crate::coord) budget_snapshot: RequestBudgetSnapshot,
    total_tool_calls: usize,
    current_turn_start_index: usize,
}

pub(in crate::coord) struct ProviderTurnPreparationRequest<'a> {
    pub(in crate::coord) provider: &'a dyn Provider,
    pub(in crate::coord) task: &'a QueuedAgentTurn,
    pub(in crate::coord) prior_context: &'a ProviderContext,
    pub(in crate::coord) tool_registry: &'a ToolRegistry,
    pub(in crate::coord) compaction: &'a CompactionSettings,
    pub(in crate::coord) committed_prompt_request_id: Option<&'a crate::ids::RequestId>,
    pub(in crate::coord) canonical_view: Option<&'a crate::session::CanonicalProviderView>,
    pub(in crate::coord) transient_operational_turns: &'a [ProviderConversationTurn],
}

#[derive(Debug, thiserror::Error)]
pub(in crate::coord) enum ProviderTurnPreparationError {
    #[error("provider tool preparation failed: {0}")]
    ToolDefinitions(String),
    #[error(transparent)]
    RequestPreflight(#[from] ProviderRequestPreflightError),
    #[error(transparent)]
    CommittedPrompt(#[from] CommittedPromptLookupError),
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
    model_settings: AgentModelSettings,
    canonical_view: Option<&'a crate::session::CanonicalProviderView>,
    transient_operational_turns: &'a [ProviderConversationTurn],
    messages: &'a [CompletionMessage],
    tool_defs: &'a [ToolDef],
    request_budget: ProviderRequestBudgetContext,
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
        compaction,
        committed_prompt_request_id,
        canonical_view,
        transient_operational_turns,
    } = request;

    let mut turn_state = match prepare_provider_transform_phase(ProviderTurnPreparationRequest {
        provider: provider.as_ref(),
        task,
        prior_context,
        tool_registry: tool_registry.as_ref(),
        compaction: &compaction,
        committed_prompt_request_id: committed_prompt_request_id.as_ref(),
        canonical_view,
        transient_operational_turns,
    }) {
        Ok(turn_state) => turn_state,
        Err(failure) => {
            return AgentTurnOutcome::Failed {
                reason: failure.to_string(),
                memory: None,
            }
        }
    };
    if !compaction.suppress_auto_compaction {
        if let Err(error) = reject_compaction_pressure(turn_state.budget_snapshot) {
            return AgentTurnOutcome::failed(error.to_string());
        }
    }
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
        turn_state.canonical_view = None;
        let reserved_tool_call_ids = match append_assistant_message_end_phase(
            &job_tx,
            &task.task_id,
            &task.agent_id,
            &mut turn_state.messages,
            &assistant_response,
        )
        .await
        {
            Ok(tool_call_ids) => tool_call_ids,
            Err(reason) => return AgentTurnOutcome::failed(reason),
        };
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
                        &turn_state.messages[turn_state.current_turn_start_index..],
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
                    reserved_tool_call_ids,
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

pub(in crate::coord) fn prepare_provider_transform_phase(
    preparation: ProviderTurnPreparationRequest<'_>,
) -> Result<AgentProviderTurnState, ProviderTurnPreparationError> {
    let ProviderTurnPreparationRequest {
        provider,
        task,
        prior_context,
        tool_registry,
        compaction,
        committed_prompt_request_id,
        canonical_view,
        transient_operational_turns,
    } = preparation;
    let profile = &task.profile;
    let request = &task.request;
    let tool_defs = build_provider_tool_defs_for_model(profile, tool_registry, &request.model_ref)
        .map_err(ProviderTurnPreparationError::ToolDefinitions)?;
    let provider_prompt = request.provider_prompt();
    let (model, model_settings, model_limits, messages, pending_prompt_index) = if let Some(view) =
        canonical_view
    {
        let messages = if transient_operational_turns.is_empty() {
            canonical_provider_messages(view, profile)
        } else {
            build_provider_context_messages(profile, prior_context, &provider_prompt)
        };
        let model = AgentModelRef {
            provider_id: view.runtime_selection.provider_id.clone(),
            model_id: view.runtime_selection.model_id.clone(),
        };
        let settings = AgentModelSettings {
            variant: view.runtime_selection.variant.clone(),
            reasoning_effort: view.runtime_selection.reasoning_effort.clone(),
            text_verbosity: view.runtime_selection.text_verbosity.clone(),
            reasoning_summary: view.runtime_selection.reasoning_summary.clone(),
            thinking: view.runtime_selection.thinking.clone(),
        };
        let pending_prompt_index = messages
            .iter()
            .rposition(|message| {
                message.role == harness_providers::MessageRole::User
                    && message.content == provider_prompt
            })
            .unwrap_or_else(|| messages.len().saturating_sub(1));
        (
            model,
            settings,
            view.runtime_selection.resolved_limits.clone(),
            messages,
            pending_prompt_index,
        )
    } else if let Some(request_id) = committed_prompt_request_id {
        let committed =
            build_committed_provider_context_messages(profile, prior_context, request_id)?;
        (
            AgentModelRef::parse(&request.model_ref),
            request.model_settings.clone(),
            request
                .model_target
                .as_ref()
                .map(|target| target.limits.clone())
                .unwrap_or_default(),
            committed.messages,
            committed.pending_prompt_index,
        )
    } else {
        let messages = build_provider_context_messages(profile, prior_context, &provider_prompt);
        let pending_prompt_index = messages.len().saturating_sub(1);
        (
            AgentModelRef::parse(&request.model_ref),
            request.model_settings.clone(),
            request
                .model_target
                .as_ref()
                .map(|target| target.limits.clone())
                .unwrap_or_default(),
            messages,
            pending_prompt_index,
        )
    };
    let historical_attachment_tokens = match canonical_view {
        Some(view) => crate::agent::canonical_historical_attachment_tokens(view),
        None => crate::attachment_transport::historical_attachment_tokens(
            prior_context
                .preserved_turns
                .iter()
                .flat_map(|turn| turn.attachments.iter()),
        ),
    }
    .map_err(ProviderRequestPreflightError::Cost)?;
    let request_budget = ProviderRequestBudgetContext {
        model_limits,
        requested_output_tokens: None,
        safety_margin_tokens: compaction.reserve_tokens,
        estimated_token_triggers: compaction.estimated_token_triggers
            && !compaction.suppress_auto_compaction,
        fallback_input_tokens: compaction.fallback_input_tokens,
        pending_prompt_index,
        historical_attachment_tokens,
        has_media: canonical_view.is_some_and(|view| {
            !view.attachments.is_empty()
                || view
                    .pending_prompt
                    .as_ref()
                    .is_some_and(|prompt| !prompt.attachments.is_empty())
        }) || prior_context
            .preserved_turns
            .iter()
            .any(|turn| !turn.attachments.is_empty())
            || !task.request.attachments.is_empty(),
    };
    let provider_boundary = transform_context_for_provider(ProviderBoundaryInput {
        profile,
        model: model.clone(),
        model_settings: model_settings.clone(),
        context: ProviderBoundaryContext::ProviderMessages {
            messages: &messages,
        },
        tools: (!tool_defs.is_empty()).then(|| tool_defs.clone()),
        tool_choice: (!tool_defs.is_empty()).then_some(ToolChoice::Auto),
    });
    let mut completion_request = provider_boundary.request;
    let budget_snapshot =
        apply_provider_request_budget(provider, &mut completion_request, &request_budget)?;

    Ok(AgentProviderTurnState {
        model,
        model_settings,
        canonical_view: canonical_view.cloned(),
        transient_operational_turns: transient_operational_turns.to_vec(),
        tool_defs,
        messages,
        request_budget,
        budget_snapshot,
        total_tool_calls: 0,
        current_turn_start_index: pending_prompt_index,
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
            model_settings: turn_state.model_settings.clone(),
            canonical_view: turn_state.canonical_view.as_ref(),
            transient_operational_turns: &turn_state.transient_operational_turns,
            messages: &turn_state.messages,
            tool_defs: &turn_state.tool_defs,
            request_budget: turn_state.request_budget.clone(),
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
        model_settings,
        canonical_view,
        transient_operational_turns,
        messages,
        tool_defs,
        request_budget: turn_state_request_budget,
        retry_metadata,
        job_tx,
        task_id,
        agent_id,
        session_id,
    } = request;
    let task_id = task_id.to_string();
    let agent_id = agent_id.to_string();
    let model_target = request.model_target.clone();

    stream_assistant_response_once_with_budget(
        StreamAssistantResponseOnceRequest {
            provider,
            profile,
            model,
            model_settings,
            turn_request_id: turn_request_id.to_string(),
            provider_request_id,
            session_id: Some(session_id.to_string()),
            prompt_summary: &request.prompt,
            retry_metadata: Some(retry_metadata),
            canonical_view,
            transient_operational_turns,
            context: ProviderBoundaryContext::ProviderMessages { messages },
            tool_defs,
        },
        Some(turn_state_request_budget),
        |event| {
            let job_tx = job_tx.clone();
            let task_id = task_id.clone();
            let agent_id = agent_id.clone();
            let model_target = model_target.clone();
            async move {
                if let Err(reason) =
                    emit_agent_runtime_event_phase(job_tx, task_id, agent_id, model_target, event)
                        .await
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
    model_target: Option<crate::config::ResolvedModelTarget>,
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
                model_target: Box::new(model_target),
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
        AgentRuntimeEvent::ProviderToolInputDelta {
            request_id,
            tool_call_id,
            delta,
        } => job_tx
            .send(Command::AgentProviderToolInputDelta {
                task_id,
                agent_id,
                request_id,
                tool_call_id,
                delta,
            })
            .await
            .map_err(|_| "provider tool input delta channel closed".to_string()),
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
) -> Result<Vec<crate::ids::ToolCallId>, String> {
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
            response: Box::new(response.clone()),
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
    reserved_tool_call_ids: Vec<crate::ids::ToolCallId>,
) -> Result<(), String> {
    if tool_intents.len() != reserved_tool_call_ids.len() {
        return Err("assistant tool commit did not reserve every tool call".to_string());
    }

    let mut tool_phase_tasks = tokio::task::JoinSet::new();
    let tool_count = tool_intents.len();

    for (source_index, (tool_call, reserved_tool_call_id)) in tool_intents
        .into_iter()
        .zip(reserved_tool_call_ids)
        .enumerate()
    {
        let job_tx = job_tx.clone();
        let agent_id = agent_id.to_string();
        let tool_id = tool_call.tool_id.clone();
        let args_json = tool_call.arguments.clone();

        tool_phase_tasks.spawn(async move {
            let result = execute_agent_tool_phase(
                &job_tx,
                &agent_id,
                tool_id,
                args_json,
                reserved_tool_call_id,
            )
            .await;
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
    reserved_tool_call_id: crate::ids::ToolCallId,
) -> Result<ToolResult, String> {
    let (respond_to, response_rx) = oneshot::channel();
    job_tx
        .send(Command::ExecuteAgentToolCall {
            actor: EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
            legacy_profile_hint: None,
            tool_id,
            args_json,
            reserved_tool_call_id: Some(reserved_tool_call_id),
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

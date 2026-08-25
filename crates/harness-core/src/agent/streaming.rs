// allow: SIZE_OK — provider streaming pipeline (single-turn + multi-turn streaming + tool intent parsing + metadata assembly)
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use harness_providers::{
    CompletionRequest, CompletionUsage, MessageRole, Provider, ProviderErrorCategory,
    ProviderEventStream, ProviderRequestCostError, ProviderStreamEvent,
    ProviderStreamFinishedMetadata, ProviderStreamStartMetadata, ProviderStreamThinkingMetadata,
    ToolChoice, ToolDef,
};
use serde_json::Value;
use tokio_stream::StreamExt;

use super::provider_boundary::{
    apply_provider_request_context, build_provider_tool_defs_for_model,
    canonical_runtime_selection, project_provider_context_for_prompt,
    transform_context_for_provider, CanonicalRuntimeSelectionInput, LowerProviderContinuationInput,
    ProviderBoundaryContext, ProviderBoundaryInput,
};
use super::{
    AgentModelSettings, AgentProfile, AgentRequest, ProviderContext, ProviderConversationTurn,
    ProviderConversationTurnStatus,
};
use crate::config::{registered_profile_model_metadata, ResolvedModelLimits};
use crate::context_budget::{
    compute_request_budget, RequestBudgetError, RequestBudgetInput, RequestBudgetSnapshot,
};
use crate::conversation::ConversationMessage;
use crate::digest::{digest12, digest12_json};
use crate::event::{
    ProviderAssistantMessageMetadata, ProviderRequestFinishedMetadata,
    ProviderRequestRetryMetadata, ProviderRequestStartedMetadata, ProviderThinkingMetadata,
};
use crate::text::{non_empty_trimmed, truncate_with_ellipsis};
use crate::tool::{ToolRegistry, ToolResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentModelRef {
    pub provider_id: String,
    pub model_id: String,
}

impl AgentModelRef {
    pub fn parse(model_ref: &str) -> Self {
        let (provider_id, model_id) = model_ref
            .split_once(':')
            .or_else(|| model_ref.split_once('/'))
            .map(|(provider_id, model_id)| {
                let provider_id = if provider_id.trim().is_empty() {
                    "default"
                } else {
                    provider_id
                };
                let model_id = if model_id.trim().is_empty() {
                    "default"
                } else {
                    model_id
                };
                (provider_id.to_string(), model_id.to_string())
            })
            .unwrap_or_else(|| ("default".to_string(), model_ref.to_string()));

        Self {
            provider_id,
            model_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequestStarted {
    pub request_id: crate::ids::RequestId,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_summary: String,
    pub request_digest: String,
    pub metadata: Option<ProviderRequestStartedMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequestFinished {
    pub request_id: crate::ids::RequestId,
    pub finish_reason: String,
    pub output_digest: Option<String>,
    pub usage: Option<CompletionUsage>,
    pub metadata: Option<ProviderRequestFinishedMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRuntimeEvent {
    ProviderRequestStarted(Box<ProviderRequestStarted>),
    ProviderStreamDelta {
        request_id: String,
        delta: String,
    },
    ProviderReasoningDelta {
        request_id: String,
        delta: String,
    },
    ProviderToolInputDelta {
        request_id: String,
        tool_call_id: crate::ids::ToolCallId,
        delta: String,
    },
    ProviderRequestFinished(Box<ProviderRequestFinished>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTurnOutcome {
    Succeeded {
        output: String,
        messages: Vec<ConversationMessage>,
    },
    Failed {
        reason: String,
        memory: Option<AgentTurnFailure>,
    },
}

impl AgentTurnOutcome {
    pub fn failed(reason: impl Into<String>) -> Self {
        Self::Failed {
            reason: reason.into(),
            memory: None,
        }
    }

    pub fn failed_with_memory(reason: impl Into<String>, memory: AgentTurnFailure) -> Self {
        Self::Failed {
            reason: reason.into(),
            memory: Some(memory),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnFailure {
    pub status: ProviderConversationTurnStatus,
    pub failure_stage: String,
    pub reason: String,
    pub partial_assistant_output: String,
    pub provider_request_id: Option<String>,
    pub provider_error_category: Option<ProviderErrorCategory>,
    pub provider_error_remediation: Option<String>,
    pub retry_after_ms: Option<u64>,
}

impl AgentTurnFailure {
    pub fn new(
        status: ProviderConversationTurnStatus,
        failure_stage: impl Into<String>,
        reason: impl Into<String>,
        partial_assistant_output: impl Into<String>,
        provider_request_id: Option<String>,
    ) -> Self {
        Self {
            status,
            failure_stage: failure_stage.into(),
            reason: reason.into(),
            partial_assistant_output: partial_assistant_output.into(),
            provider_request_id,
            provider_error_category: None,
            provider_error_remediation: None,
            retry_after_ms: None,
        }
    }

    pub fn provider_error(
        reason: impl Into<String>,
        partial_assistant_output: impl Into<String>,
        provider_request_id: String,
        category: Option<ProviderErrorCategory>,
        remediation: Option<String>,
        retry_after_ms: Option<u64>,
    ) -> Self {
        let mut failure = Self::new(
            ProviderConversationTurnStatus::Failed,
            "provider_error",
            reason,
            partial_assistant_output,
            Some(provider_request_id),
        );
        failure.provider_error_category = category;
        failure.provider_error_remediation = remediation;
        failure.retry_after_ms = retry_after_ms;
        failure
    }

    pub fn message(reason: impl Into<String>) -> Self {
        Self::new(
            ProviderConversationTurnStatus::Failed,
            "unknown",
            reason,
            String::new(),
            None,
        )
    }
}

impl fmt::Display for AgentTurnFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderRequestBudgetContext {
    pub(crate) model_limits: ResolvedModelLimits,
    pub(crate) requested_output_tokens: Option<u32>,
    pub(crate) safety_margin_tokens: u32,
    pub(crate) estimated_token_triggers: bool,
    pub(crate) fallback_input_tokens: u32,
    pub(crate) pending_prompt_index: usize,
    pub(crate) historical_attachment_tokens: u32,
    pub(crate) has_media: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProviderRequestPreflightError {
    #[error("provider request costing failed: {0}")]
    Cost(#[from] ProviderRequestCostError),
    #[error("request budget calculation failed: {0}")]
    Budget(#[from] RequestBudgetError),
    #[error(
        "request budget exceeded: occupied input {occupied_input_tokens} meets or exceeds threshold {compaction_threshold_tokens}"
    )]
    InputBudgetExceeded {
        occupied_input_tokens: u32,
        compaction_threshold_tokens: u32,
    },
}

impl AgentTurnFailure {
    fn request_preflight(error: ProviderRequestPreflightError) -> Self {
        Self::new(
            ProviderConversationTurnStatus::Failed,
            "request_preflight",
            error.to_string(),
            String::new(),
            None,
        )
    }
}

pub(crate) fn apply_provider_request_budget(
    provider: &dyn Provider,
    request: &mut CompletionRequest,
    context: &ProviderRequestBudgetContext,
) -> Result<RequestBudgetSnapshot, ProviderRequestPreflightError> {
    let attachment_tokens = context.historical_attachment_tokens;
    let mut provisional =
        provider.request_budget_semantics(request, context.pending_prompt_index)?;
    provisional.request_cost.attachments_tokens = provisional
        .request_cost
        .attachments_tokens
        .checked_add(attachment_tokens)
        .ok_or(ProviderRequestCostError::ArithmeticOverflow)?;
    let provisional_budget = compute_request_budget(RequestBudgetInput {
        model_limits: &context.model_limits,
        request_cost: provisional.request_cost,
        requested_output_tokens: context.requested_output_tokens,
        safety_margin_tokens: context.safety_margin_tokens,
        estimated_token_triggers: context.estimated_token_triggers,
        fallback_input_tokens: context.fallback_input_tokens,
        output_cap_disposition: provisional.output_cap_disposition,
    })?;
    request.max_tokens = provisional_budget.reserved_output_tokens;

    let mut current = provider.request_budget_semantics(request, context.pending_prompt_index)?;
    current.request_cost.attachments_tokens = current
        .request_cost
        .attachments_tokens
        .checked_add(attachment_tokens)
        .ok_or(ProviderRequestCostError::ArithmeticOverflow)?;
    Ok(compute_request_budget(RequestBudgetInput {
        model_limits: &context.model_limits,
        request_cost: current.request_cost,
        requested_output_tokens: context.requested_output_tokens,
        safety_margin_tokens: context.safety_margin_tokens,
        estimated_token_triggers: context.estimated_token_triggers,
        fallback_input_tokens: context.fallback_input_tokens,
        output_cap_disposition: current.output_cap_disposition,
    })?
    .snapshot())
}

pub(crate) fn reject_compaction_pressure(
    snapshot: RequestBudgetSnapshot,
) -> Result<(), ProviderRequestPreflightError> {
    match (
        snapshot.requires_compaction,
        snapshot.compaction_threshold_tokens,
    ) {
        (Some(true), Some(compaction_threshold_tokens)) => {
            Err(ProviderRequestPreflightError::InputBudgetExceeded {
                occupied_input_tokens: snapshot.occupied_input_tokens,
                compaction_threshold_tokens,
            })
        }
        (Some(false) | None, Some(_) | None) => Ok(()),
        (Some(true), None) => Ok(()),
    }
}

pub fn default_provider() -> Arc<dyn Provider> {
    Arc::new(NullProvider)
}

pub(crate) const MAX_TOOL_CALLS_TOTAL: usize = 1000;

pub struct MultiTurnStreamingRequest<'a> {
    pub provider: Arc<dyn Provider>,
    pub tool_registry: Arc<ToolRegistry>,
    pub profile: &'a AgentProfile,
    pub request_id: crate::ids::RequestId,
    pub request: AgentRequest,
    pub prior_context: &'a ProviderContext,
}

pub struct StreamAssistantResponseOnceRequest<'a> {
    pub provider: Arc<dyn Provider>,
    pub profile: &'a AgentProfile,
    pub model: AgentModelRef,
    pub model_settings: AgentModelSettings,
    pub turn_request_id: String,
    pub provider_request_id: String,
    pub session_id: Option<String>,
    pub prompt_summary: &'a str,
    pub retry_metadata: Option<ProviderRequestRetryMetadata>,
    pub canonical_view: Option<&'a crate::session::CanonicalProviderView>,
    pub transient_operational_turns: &'a [ProviderConversationTurn],
    pub context: ProviderBoundaryContext<'a>,
    pub tool_defs: &'a [ToolDef],
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantResponse {
    pub request_id: crate::ids::RequestId,
    pub provider_id: String,
    pub model_id: String,
    pub text: String,
    pub reasoning: String,
    pub reasoning_deltas: Vec<String>,
    pub tool_call_deltas: Vec<AssistantToolCallDelta>,
    pub tool_intents: Vec<AssistantToolIntent>,
    pub stop_reason: String,
    pub usage: Option<CompletionUsage>,
    pub started_metadata: ProviderRequestStartedMetadata,
    pub finished_metadata: ProviderRequestFinishedMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantToolCallDelta {
    pub tool_call_id: crate::ids::ToolCallId,
    pub function_name: Option<String>,
    pub arguments_delta: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantToolIntent {
    pub tool_call_id: crate::ids::ToolCallId,
    pub function_name: String,
    pub tool_id: String,
    pub arguments_json: String,
    pub arguments: Value,
}

pub async fn run_single_turn_streaming<F, Fut>(
    provider: Arc<dyn Provider>,
    profile: &AgentProfile,
    request_id: String,
    request: AgentRequest,
    prior_context: &ProviderContext,
    mut emit: F,
) -> AgentTurnOutcome
where
    F: FnMut(AgentRuntimeEvent) -> Fut,
    Fut: Future<Output = ()>,
{
    let model = AgentModelRef::parse(&request.model_ref);
    let provider_prompt = request.provider_prompt();
    let projected_context = project_provider_context_for_prompt(prior_context, &provider_prompt);
    let provider_boundary = transform_context_for_provider(ProviderBoundaryInput {
        profile,
        model: model.clone(),
        model_settings: request.model_settings.clone(),
        context: ProviderBoundaryContext::ProjectedHarness {
            messages: &projected_context,
            checkpoint: prior_context.checkpoint.as_ref(),
        },
        tools: None,
        tool_choice: None,
    });
    let mut completion_request = provider_boundary.request;
    apply_provider_request_context(&mut completion_request, None, Some(request_id.as_str()));
    let request_digest = digest12_json(&completion_request);

    let mut stream = provider.stream_completion(completion_request).await;
    let (provider_start_metadata, mut pending_event) =
        consume_provider_start_event(&mut stream).await;
    emit(AgentRuntimeEvent::ProviderRequestStarted(Box::new(
        ProviderRequestStarted {
            request_id: request_id.clone().into(),
            provider_id: model.provider_id,
            model_id: model.model_id,
            prompt_summary: truncate_with_ellipsis(&request.prompt, 256),
            request_digest,
            metadata: Some(provider_request_started_metadata(
                &request_id,
                &request_id,
                provider_start_metadata.as_ref(),
                None,
                None,
                None,
            )),
        },
    )))
    .await;
    let mut output = String::new();

    loop {
        let Some(event) = next_provider_event(&mut pending_event, &mut stream).await else {
            break;
        };
        match event {
            ProviderStreamEvent::Start | ProviderStreamEvent::Started { .. } => {}
            ProviderStreamEvent::TextDelta(delta) => {
                if !delta.is_empty() {
                    output.push_str(&delta);
                    emit(AgentRuntimeEvent::ProviderStreamDelta {
                        request_id: request_id.clone(),
                        delta,
                    })
                    .await;
                }
            }
            ProviderStreamEvent::ReasoningDelta(delta) => {
                if !delta.is_empty() {
                    emit(AgentRuntimeEvent::ProviderReasoningDelta {
                        request_id: request_id.clone(),
                        delta,
                    })
                    .await;
                }
            }
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id,
                arguments_delta,
                ..
            } => {
                if !arguments_delta.is_empty() {
                    emit(AgentRuntimeEvent::ProviderToolInputDelta {
                        request_id: request_id.clone(),
                        tool_call_id: tool_call_id.into(),
                        delta: arguments_delta,
                    })
                    .await;
                }
            }
            ProviderStreamEvent::ToolCallComplete { .. } => {}
            ProviderStreamEvent::Done { usage } => {
                emit(AgentRuntimeEvent::ProviderRequestFinished(Box::new(
                    ProviderRequestFinished {
                        request_id: request_id.clone().into(),
                        finish_reason: "done".to_string(),
                        output_digest: Some(digest12(output.as_bytes())),
                        usage,
                        metadata: Some(provider_finished_metadata(
                            &request_id,
                            &request_id,
                            "done",
                            &output,
                            "",
                            None,
                        )),
                    },
                )))
                .await;

                return AgentTurnOutcome::Succeeded {
                    output,
                    messages: Vec::new(),
                };
            }
            ProviderStreamEvent::DoneWithMetadata {
                usage,
                metadata: provider_metadata,
            } => {
                emit(AgentRuntimeEvent::ProviderRequestFinished(Box::new(
                    ProviderRequestFinished {
                        request_id: request_id.clone().into(),
                        finish_reason: "done".to_string(),
                        output_digest: Some(digest12(output.as_bytes())),
                        usage,
                        metadata: Some(provider_finished_metadata(
                            &request_id,
                            &request_id,
                            "done",
                            &output,
                            "",
                            provider_metadata.as_ref(),
                        )),
                    },
                )))
                .await;

                return AgentTurnOutcome::Succeeded {
                    output,
                    messages: Vec::new(),
                };
            }
            ProviderStreamEvent::Error {
                message,
                category,
                remediation,
                retry_after_ms,
            } => {
                let mut metadata = provider_finished_metadata(
                    &request_id,
                    &request_id,
                    "error",
                    &output,
                    "",
                    None,
                );
                metadata.provider_error_category = category;
                metadata.provider_error_remediation = remediation.clone();
                emit(AgentRuntimeEvent::ProviderRequestFinished(Box::new(
                    ProviderRequestFinished {
                        request_id: request_id.clone().into(),
                        finish_reason: "error".to_string(),
                        output_digest: None,
                        usage: None,
                        metadata: Some(metadata),
                    },
                )))
                .await;

                return AgentTurnOutcome::failed_with_memory(
                    message.clone(),
                    AgentTurnFailure::provider_error(
                        message,
                        output,
                        request_id.clone(),
                        category,
                        remediation,
                        retry_after_ms,
                    ),
                );
            }
        }
    }

    emit(AgentRuntimeEvent::ProviderRequestFinished(Box::new(
        ProviderRequestFinished {
            request_id: request_id.clone().into(),
            finish_reason: "stream_ended".to_string(),
            output_digest: Some(digest12(output.as_bytes())),
            usage: None,
            metadata: Some(provider_finished_metadata(
                &request_id,
                &request_id,
                "stream_ended",
                &output,
                "",
                None,
            )),
        },
    )))
    .await;

    AgentTurnOutcome::Succeeded {
        output,
        messages: Vec::new(),
    }
}

pub async fn stream_assistant_response_once<F, Fut>(
    request: StreamAssistantResponseOnceRequest<'_>,
    emit: F,
) -> Result<AssistantResponse, AgentTurnFailure>
where
    F: FnMut(AgentRuntimeEvent) -> Fut,
    Fut: Future<Output = ()>,
{
    stream_assistant_response_once_with_budget(request, None, emit).await
}

pub(crate) async fn stream_assistant_response_once_with_budget<F, Fut>(
    request: StreamAssistantResponseOnceRequest<'_>,
    request_budget: Option<ProviderRequestBudgetContext>,
    mut emit: F,
) -> Result<AssistantResponse, AgentTurnFailure>
where
    F: FnMut(AgentRuntimeEvent) -> Fut,
    Fut: Future<Output = ()>,
{
    let StreamAssistantResponseOnceRequest {
        provider,
        profile,
        model,
        model_settings,
        turn_request_id,
        provider_request_id,
        session_id,
        prompt_summary,
        retry_metadata,
        canonical_view,
        transient_operational_turns,
        context,
        tool_defs,
    } = request;

    let function_to_tool_id = tool_defs
        .iter()
        .map(|tool| (tool.function_name.clone(), tool.tool_id.clone()))
        .collect::<BTreeMap<_, _>>();

    let tools = (!tool_defs.is_empty()).then(|| tool_defs.to_vec());
    let tool_choice = (!tool_defs.is_empty()).then_some(ToolChoice::Auto);
    let (provider_boundary, runtime_selection) = if let Some(view) = canonical_view {
        let boundary =
            super::provider_boundary::lower_provider_continuation(LowerProviderContinuationInput {
                view,
                transient_operational_turns,
                profile,
                tools,
                tool_choice,
                fresh_request_id: &provider_request_id,
            })
            .map_err(runtime_selection_failure)?;
        (boundary, Some(view.runtime_selection.clone()))
    } else {
        let runtime_selection = request_budget
            .as_ref()
            .map(|budget| {
                canonical_runtime_selection(CanonicalRuntimeSelectionInput {
                    profile,
                    model: &model,
                    settings: model_settings.clone(),
                    resolved_limits: budget.model_limits.clone(),
                    tools: tool_defs,
                })
            })
            .transpose()
            .map_err(runtime_selection_failure)?;
        let boundary = transform_context_for_provider(ProviderBoundaryInput {
            profile,
            model: model.clone(),
            model_settings,
            context,
            tools,
            tool_choice,
        });
        (boundary, runtime_selection)
    };
    let mut completion_request = provider_boundary.request;
    let request_budget = request_budget.map(|mut budget| {
        if let Some(prompt) = canonical_view.and_then(|view| view.pending_prompt.as_ref()) {
            let pending_text = crate::attachment_transport::lower_provider_attachments(
                &prompt.text,
                &prompt.attachments,
            );
            if let Some(index) = completion_request.messages.iter().rposition(|message| {
                message.role == MessageRole::User && message.content == pending_text
            }) {
                budget.pending_prompt_index = index;
            }
        }
        budget
    });
    if request_budget
        .as_ref()
        .is_some_and(|budget| budget.has_media)
    {
        completion_request.context.has_media = true;
    }
    let context_budget = request_budget
        .as_ref()
        .map(|budget| {
            apply_provider_request_budget(provider.as_ref(), &mut completion_request, budget)
        })
        .transpose()
        .map_err(AgentTurnFailure::request_preflight)?;
    if let Some(snapshot) = context_budget {
        reject_compaction_pressure(snapshot).map_err(AgentTurnFailure::request_preflight)?;
    }
    apply_provider_request_context(
        &mut completion_request,
        session_id.as_deref(),
        Some(provider_request_id.as_str()),
    );
    let request_digest = digest12_json(&completion_request);

    let mut stream = provider.stream_completion(completion_request).await;
    let (provider_start_metadata, mut pending_event) =
        consume_provider_start_event(&mut stream).await;
    let started_metadata = provider_request_started_metadata(
        &turn_request_id,
        &provider_request_id,
        provider_start_metadata.as_ref(),
        retry_metadata,
        context_budget,
        runtime_selection,
    );

    emit(AgentRuntimeEvent::ProviderRequestStarted(Box::new(
        ProviderRequestStarted {
            request_id: provider_request_id.clone().into(),
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
            prompt_summary: truncate_with_ellipsis(prompt_summary, 256),
            request_digest,
            metadata: Some(started_metadata.clone()),
        },
    )))
    .await;

    let mut output = String::new();
    let mut reasoning = String::new();
    let mut reasoning_deltas = Vec::new();
    let mut tool_call_deltas = Vec::new();
    let mut tool_calls = Vec::new();
    let mut stop_reason = "stream_ended".to_string();
    let mut usage = None;
    let mut provider_error = None;
    let mut finished_provider_metadata = None;
    let mut finished_provider_error_category = None;
    let mut finished_provider_error_remediation = None;
    let mut provider_retry_after_ms = None;

    loop {
        let Some(event) = next_provider_event(&mut pending_event, &mut stream).await else {
            break;
        };
        match event {
            ProviderStreamEvent::Start | ProviderStreamEvent::Started { .. } => {}
            ProviderStreamEvent::TextDelta(delta) => {
                if !delta.is_empty() {
                    output.push_str(&delta);
                    emit(AgentRuntimeEvent::ProviderStreamDelta {
                        request_id: provider_request_id.clone(),
                        delta,
                    })
                    .await;
                }
            }
            ProviderStreamEvent::ReasoningDelta(delta) => {
                if !delta.is_empty() {
                    reasoning.push_str(&delta);
                    reasoning_deltas.push(delta.clone());
                    emit(AgentRuntimeEvent::ProviderReasoningDelta {
                        request_id: provider_request_id.clone(),
                        delta,
                    })
                    .await;
                }
            }
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id,
                function_name,
                arguments_delta,
            } => {
                let tool_call_id = crate::ids::ToolCallId::from(tool_call_id);
                tool_call_deltas.push(AssistantToolCallDelta {
                    tool_call_id: tool_call_id.clone(),
                    function_name,
                    arguments_delta: arguments_delta.clone(),
                });
                if !arguments_delta.is_empty() {
                    emit(AgentRuntimeEvent::ProviderToolInputDelta {
                        request_id: provider_request_id.clone(),
                        tool_call_id,
                        delta: arguments_delta,
                    })
                    .await;
                }
            }
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id,
                function_name,
                arguments_json,
            } => {
                tool_calls.push(CollectedToolCall {
                    tool_call_id,
                    function_name,
                    arguments_json,
                });
            }
            ProviderStreamEvent::Done {
                usage: finished_usage,
            } => {
                stop_reason = "done".to_string();
                usage = finished_usage;
                break;
            }
            ProviderStreamEvent::DoneWithMetadata {
                usage: finished_usage,
                metadata,
            } => {
                stop_reason = "done".to_string();
                usage = finished_usage;
                finished_provider_metadata = metadata;
                break;
            }
            ProviderStreamEvent::Error {
                message,
                category,
                remediation,
                retry_after_ms,
            } => {
                stop_reason = "error".to_string();
                provider_error = Some(message);
                finished_provider_error_category = category;
                finished_provider_error_remediation = remediation;
                provider_retry_after_ms = retry_after_ms;
                break;
            }
        }
    }

    let finished_metadata = provider_finished_metadata(
        &turn_request_id,
        &provider_request_id,
        &stop_reason,
        &output,
        &reasoning,
        finished_provider_metadata.as_ref(),
    );
    let mut finished_metadata = finished_metadata;
    finished_metadata.provider_error_category = finished_provider_error_category;
    finished_metadata.provider_error_remediation = finished_provider_error_remediation.clone();
    let output_digest = if stop_reason == "error" {
        None
    } else {
        Some(digest12(output.as_bytes()))
    };
    emit(AgentRuntimeEvent::ProviderRequestFinished(Box::new(
        ProviderRequestFinished {
            request_id: provider_request_id.clone().into(),
            finish_reason: stop_reason.clone(),
            output_digest,
            usage: usage.clone(),
            metadata: Some(finished_metadata.clone()),
        },
    )))
    .await;

    if let Some(reason) = provider_error {
        return Err(AgentTurnFailure::provider_error(
            reason,
            output,
            provider_request_id,
            finished_provider_error_category,
            finished_provider_error_remediation,
            provider_retry_after_ms,
        ));
    }

    let tool_intents =
        parse_tool_intents(tool_calls, &function_to_tool_id).map_err(AgentTurnFailure::message)?;
    Ok(AssistantResponse {
        request_id: provider_request_id.into(),
        provider_id: model.provider_id,
        model_id: model.model_id,
        text: output,
        reasoning,
        reasoning_deltas,
        tool_call_deltas,
        tool_intents,
        stop_reason,
        usage,
        started_metadata,
        finished_metadata,
    })
}

fn runtime_selection_failure(error: impl ToString) -> AgentTurnFailure {
    AgentTurnFailure::new(
        ProviderConversationTurnStatus::Failed,
        "runtime_selection",
        error.to_string(),
        String::new(),
        None,
    )
}

/// Compatibility runner for tests and legacy callers that still need a provider
/// response through the old entry point.
///
/// Production coordinator turns use the explicit phase loop in `coord.rs` and
/// call [`stream_assistant_response_once`] for provider streaming. New runtime
/// paths must not use this wrapper. Tool execution is coordinator-owned; if the
/// provider emits tool intents here, this wrapper fails without invoking the
/// compatibility tool callback.
pub async fn run_multi_turn_streaming<F, Fut, T, TFut, P, PFut>(
    request: MultiTurnStreamingRequest<'_>,
    mut next_provider_request_id: P,
    _call_tool_and_wait: T,
    mut emit: F,
) -> AgentTurnOutcome
where
    F: FnMut(AgentRuntimeEvent) -> Fut,
    Fut: Future<Output = ()>,
    T: FnMut(String, Value) -> TFut,
    TFut: Future<Output = Result<ToolResult, String>>,
    P: FnMut() -> PFut,
    PFut: Future<Output = Result<String, String>>,
{
    let MultiTurnStreamingRequest {
        provider,
        tool_registry,
        profile,
        request_id,
        request,
        prior_context,
    } = request;

    let model = AgentModelRef::parse(&request.model_ref);
    let tool_defs = match build_provider_tool_defs_for_model(
        profile,
        tool_registry.as_ref(),
        &request.model_ref,
    ) {
        Ok(tool_defs) => tool_defs,
        Err(reason) => return AgentTurnOutcome::failed(reason),
    };

    let provider_prompt = request.provider_prompt();
    let projected_context = project_provider_context_for_prompt(prior_context, &provider_prompt);
    let provider_request_id = match next_provider_request_id().await {
        Ok(request_id) => request_id,
        Err(reason) => return AgentTurnOutcome::failed(reason),
    };

    let assistant_response = match stream_assistant_response_once(
        StreamAssistantResponseOnceRequest {
            provider,
            profile,
            model,
            model_settings: request.model_settings.clone(),
            turn_request_id: request_id.to_string(),
            provider_request_id,
            session_id: None,
            prompt_summary: &request.prompt,
            retry_metadata: None,
            canonical_view: None,
            transient_operational_turns: &[],
            context: ProviderBoundaryContext::ProjectedHarness {
                messages: &projected_context,
                checkpoint: prior_context.checkpoint.as_ref(),
            },
            tool_defs: &tool_defs,
        },
        &mut emit,
    )
    .await
    {
        Ok(assistant_response) => assistant_response,
        Err(reason) => {
            return AgentTurnOutcome::Failed {
                reason: reason.to_string(),
                memory: (reason.failure_stage == "provider_error").then_some(reason),
            }
        }
    };

    if !assistant_response.tool_intents.is_empty() {
        return AgentTurnOutcome::failed(
            "direct tool execution is unsupported on compatibility path; use coordinator loop",
        );
    }

    AgentTurnOutcome::Succeeded {
        output: assistant_response.text,
        messages: Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CollectedToolCall {
    tool_call_id: String,
    function_name: String,
    arguments_json: String,
}

fn parse_tool_intents(
    tool_calls: Vec<CollectedToolCall>,
    function_to_tool_id: &BTreeMap<String, String>,
) -> Result<Vec<AssistantToolIntent>, String> {
    let mut seen_tool_call_ids = BTreeSet::new();
    let mut intents = Vec::with_capacity(tool_calls.len());

    for tool_call in tool_calls {
        if non_empty_trimmed(&tool_call.tool_call_id).is_none() {
            return Err(format!(
                "provider emitted empty tool_call_id for `{}`",
                tool_call.function_name
            ));
        }
        if !seen_tool_call_ids.insert(tool_call.tool_call_id.clone()) {
            return Err(format!(
                "provider emitted duplicate tool_call_id `{}`",
                tool_call.tool_call_id
            ));
        }

        let Some(tool_id) = function_to_tool_id.get(&tool_call.function_name) else {
            return Err(format!(
                "provider emitted unmapped tool function `{}`",
                tool_call.function_name
            ));
        };

        let arguments = serde_json::from_str(&tool_call.arguments_json).map_err(|err| {
            format!(
                "provider emitted malformed tool args for `{}`: {err}",
                tool_call.function_name
            )
        })?;

        intents.push(AssistantToolIntent {
            tool_call_id: tool_call.tool_call_id.into(),
            function_name: tool_call.function_name,
            tool_id: tool_id.clone(),
            arguments_json: tool_call.arguments_json,
            arguments,
        });
    }

    Ok(intents)
}

async fn consume_provider_start_event(
    stream: &mut ProviderEventStream,
) -> (
    Option<ProviderStreamStartMetadata>,
    Option<ProviderStreamEvent>,
) {
    let mut start_metadata = None;

    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::Start => {}
            ProviderStreamEvent::Started { metadata } => {
                start_metadata = metadata;
            }
            event => return (start_metadata, Some(event)),
        }
    }

    (start_metadata, None)
}

async fn next_provider_event(
    pending_event: &mut Option<ProviderStreamEvent>,
    stream: &mut ProviderEventStream,
) -> Option<ProviderStreamEvent> {
    if pending_event.is_some() {
        return pending_event.take();
    }

    stream.next().await
}

fn provider_request_started_metadata(
    turn_request_id: &str,
    provider_request_id: &str,
    provider_metadata: Option<&ProviderStreamStartMetadata>,
    retry_metadata: Option<ProviderRequestRetryMetadata>,
    context_budget: Option<RequestBudgetSnapshot>,
    runtime_selection: Option<crate::session::CanonicalRuntimeSelection>,
) -> ProviderRequestStartedMetadata {
    ProviderRequestStartedMetadata {
        turn_id: Some(turn_request_id.to_string()),
        provider_call_id: Some(provider_request_id.to_string()),
        provider_session_id: provider_metadata.and_then(|metadata| {
            metadata
                .provider_session_id
                .as_deref()
                .and_then(non_empty_trimmed)
                .map(str::to_string)
        }),
        provider_cache_id: provider_metadata.and_then(|metadata| {
            metadata
                .provider_cache_id
                .as_deref()
                .and_then(non_empty_trimmed)
                .map(str::to_string)
        }),
        retry: retry_metadata,
        context_budget,
        runtime_selection: runtime_selection.map(Box::new),
    }
}

fn provider_finished_metadata(
    turn_request_id: &str,
    provider_request_id: &str,
    stop_reason: &str,
    output: &str,
    reasoning: &str,
    provider_metadata: Option<&ProviderStreamFinishedMetadata>,
) -> ProviderRequestFinishedMetadata {
    let assistant_message_id = provider_metadata.and_then(|metadata| {
        metadata
            .assistant_message_id
            .as_deref()
            .and_then(non_empty_trimmed)
            .map(str::to_string)
    });
    let assistant_message = (!output.is_empty()
        || !reasoning.is_empty()
        || assistant_message_id.is_some())
    .then(|| ProviderAssistantMessageMetadata {
        message_id: assistant_message_id,
        text_digest: (!output.is_empty()).then(|| digest12(output.as_bytes())),
        reasoning_digest: (!reasoning.is_empty()).then(|| digest12(reasoning.as_bytes())),
    });

    let thinking = provider_metadata
        .and_then(|metadata| metadata.thinking.as_ref())
        .and_then(provider_thinking_metadata)
        .or_else(|| {
            (!reasoning.is_empty()).then(|| ProviderThinkingMetadata {
                summary: None,
                summary_digest: Some(digest12(reasoning.as_bytes())),
                signature: None,
            })
        });

    ProviderRequestFinishedMetadata {
        turn_id: Some(turn_request_id.to_string()),
        provider_call_id: Some(provider_request_id.to_string()),
        provider_response_id: provider_metadata.and_then(|metadata| {
            metadata
                .provider_response_id
                .as_deref()
                .and_then(non_empty_trimmed)
                .map(str::to_string)
        }),
        provider_session_id: provider_metadata.and_then(|metadata| {
            metadata
                .provider_session_id
                .as_deref()
                .and_then(non_empty_trimmed)
                .map(str::to_string)
        }),
        provider_cache_id: provider_metadata.and_then(|metadata| {
            metadata
                .provider_cache_id
                .as_deref()
                .and_then(non_empty_trimmed)
                .map(str::to_string)
        }),
        provider_stop_reason: provider_metadata
            .and_then(|metadata| {
                metadata
                    .provider_stop_reason
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .map(str::to_string)
            })
            .or_else(|| Some(stop_reason.to_string())),
        cache_read_tokens: provider_metadata.and_then(|metadata| metadata.cache_read_tokens),
        cache_write_tokens: provider_metadata.and_then(|metadata| metadata.cache_write_tokens),
        assistant_message,
        thinking,
        provider_error_category: None,
        provider_error_remediation: None,
    }
}

fn provider_thinking_metadata(
    thinking: &ProviderStreamThinkingMetadata,
) -> Option<ProviderThinkingMetadata> {
    let metadata = ProviderThinkingMetadata {
        summary: thinking
            .summary
            .as_deref()
            .and_then(non_empty_trimmed)
            .map(str::to_string),
        summary_digest: thinking
            .summary_digest
            .as_deref()
            .and_then(non_empty_trimmed)
            .map(str::to_string),
        signature: thinking
            .signature
            .as_deref()
            .and_then(non_empty_trimmed)
            .map(str::to_string),
    };

    (metadata.summary.is_some()
        || metadata.summary_digest.is_some()
        || metadata.signature.is_some())
    .then_some(metadata)
}

pub fn default_model_settings_for_profile(profile_name: &str) -> AgentModelSettings {
    let Some(metadata) = registered_profile_model_metadata(profile_name) else {
        return AgentModelSettings::default();
    };

    AgentModelSettings {
        variant: metadata.variant,
        reasoning_effort: metadata.reasoning_effort.clone(),
        text_verbosity: metadata.text_verbosity,
        reasoning_summary: if metadata
            .resolution
            .capabilities
            .supports_reasoning_summaries
            && metadata.reasoning_effort.is_some()
        {
            Some("auto".to_string())
        } else {
            None
        },
        thinking: metadata.thinking.clone(),
    }
}

struct NullProvider;

#[async_trait]
impl Provider for NullProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<
        harness_providers::ProviderBudgetSemantics,
        harness_providers::ProviderRequestCostError,
    > {
        harness_providers::generic_request_budget_semantics(request, pending_prompt_index)
    }

    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        Box::pin(tokio_stream::iter(vec![ProviderStreamEvent::error(
            "no provider configured",
        )]))
    }
}

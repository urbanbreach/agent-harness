use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use harness_providers::{
    AssistantToolCall, CompletionMessage, CompletionRequest, CompletionUsage, MessageRole,
    Provider, ProviderEventStream, ProviderStreamEvent, ProviderStreamFinishedMetadata,
    ProviderStreamStartMetadata, ProviderStreamThinkingMetadata, ToolChoice, ToolDef,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use tokio_stream::StreamExt;

use crate::config::{registered_profile_model_metadata, ToolFailureMode};
use crate::conversation::{
    ConversationAssistantMessage, ConversationCheckpointMessage, ConversationMessage,
    ConversationToolResultMessage, ConversationUserMessage,
};
use crate::event::{
    EventArtifactRef, ProviderAssistantMessageMetadata, ProviderRequestFinishedMetadata,
    ProviderRequestStartedMetadata, ProviderThinkingMetadata,
};
use crate::tool::{
    build_tool_function_name_mapping, sanitize_tool_function_name, ToolRegistry, ToolResult,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentProfile {
    pub name: String,
    pub category: String,
    pub model_ref: String,
    pub system_prompt: String,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default = "default_agent_profile_max_iters")]
    pub max_iters: usize,
    pub tool_failure_mode: ToolFailureMode,
    pub toolset: Vec<String>,
}

impl AgentProfile {
    pub fn fallback(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            category: name.clone(),
            model_ref: "default:default".to_string(),
            system_prompt: String::new(),
            temperature: None,
            max_iters: default_agent_profile_max_iters(),
            tool_failure_mode: ToolFailureMode::FailTurn,
            toolset: Vec::new(),
            name,
        }
    }
}

fn default_agent_profile_max_iters() -> usize {
    12
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRequest {
    pub agent_id: String,
    pub prompt: String,
    pub model_ref: String,
    #[serde(default)]
    pub model_settings: AgentModelSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentModelSettings {
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub text_verbosity: Option<String>,
    #[serde(default)]
    pub reasoning_summary: Option<String>,
}

const PROVIDER_TURN_FAILURE_REASON_MAX_CHARS: usize = 240;
const ALLOWED_PROVIDER_TURN_FAILURE_STAGES: &[&str] = &[
    "provider_error",
    "provider_abort",
    "tool_failure",
    "overflow_retry_failed",
    "hook_failure",
    "cancelled",
    "unknown",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConversationTurnStatus {
    #[default]
    Completed,
    Failed,
    Aborted,
}

impl ProviderConversationTurnStatus {
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed)
    }

    fn marker_label(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
        }
    }
}

fn is_allowed_provider_turn_failure_stage(stage: &str) -> bool {
    ALLOWED_PROVIDER_TURN_FAILURE_STAGES.contains(&stage)
}

fn serialize_provider_turn_failure_stage<S>(
    stage: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match stage.as_deref() {
        Some(stage) if is_allowed_provider_turn_failure_stage(stage) => {
            serializer.serialize_some(stage)
        }
        Some(stage) => Err(serde::ser::Error::custom(format!(
            "unsupported provider turn failure stage `{stage}`"
        ))),
        None => serializer.serialize_none(),
    }
}

fn deserialize_provider_turn_failure_stage<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let stage = Option::<String>::deserialize(deserializer)?;
    if let Some(stage) = stage.as_deref() {
        if !is_allowed_provider_turn_failure_stage(stage) {
            return Err(serde::de::Error::custom(format!(
                "unsupported provider turn failure stage `{stage}`"
            )));
        }
    }
    Ok(stage)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderConversationTurn {
    pub user_prompt: String,
    pub assistant_response: String,
    #[serde(
        default,
        skip_serializing_if = "ProviderConversationTurnStatus::is_completed"
    )]
    pub status: ProviderConversationTurnStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_provider_turn_failure_stage",
        deserialize_with = "deserialize_provider_turn_failure_stage"
    )]
    pub failure_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<EventArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<ConversationMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderContextCheckpointMetadata {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub run_id: String,
    pub through_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_tokens_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction_tokens_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction_percent_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCompactionTurnFact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    pub user_excerpt: String,
    pub assistant_excerpt: String,
    #[serde(
        default,
        skip_serializing_if = "ProviderConversationTurnStatus::is_completed"
    )]
    pub status: ProviderConversationTurnStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_provider_turn_failure_stage",
        deserialize_with = "deserialize_provider_turn_failure_stage"
    )]
    pub failure_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<EventArtifactRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderFileOperationFact {
    pub path: String,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCompactionFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_checkpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compacted_turns: Vec<ProviderCompactionTurnFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relevant_artifacts: Vec<EventArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_files: Vec<ProviderFileOperationFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<ProviderFileOperationFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operation_facts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub touched_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_work: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCompactionTailBoundary {
    pub mode: String,
    pub preserved_turns: u32,
    pub preserved_tokens_estimate: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_from_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_from_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_prefix_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCompactionSummarySource {
    pub strategy: String,
    pub model_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_verbosity: Option<String>,
    pub previous_summary_used: bool,
    pub model_backed: bool,
    pub deterministic_fallback: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_contract_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_contract_enforced: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCompactionTimelineEntry {
    pub entry_type: String,
    pub summary: String,
    pub first_kept_request_id: Option<String>,
    pub compacted_turns: u32,
    pub preserved_turns: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after_estimate: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderContextCheckpoint {
    #[serde(flatten)]
    pub metadata: ProviderContextCheckpointMetadata,
    pub summary: String,
    #[serde(default)]
    pub recent_turns: Vec<ProviderConversationTurn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pruned_tool_artifacts: Vec<EventArtifactRef>,
    #[serde(default, skip_serializing_if = "ProviderCompactionFacts::is_empty")]
    pub facts: ProviderCompactionFacts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_boundary: Option<ProviderCompactionTailBoundary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_source: Option<ProviderCompactionSummarySource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_entry: Option<ProviderCompactionTimelineEntry>,
}

impl ProviderCompactionFacts {
    pub fn is_empty(&self) -> bool {
        self.previous_checkpoint_id.is_none()
            && self.compacted_turns.is_empty()
            && self.relevant_artifacts.is_empty()
            && self.read_files.is_empty()
            && self.modified_files.is_empty()
            && self.operation_facts.is_empty()
            && self.touched_files.is_empty()
            && self.pending_work.is_empty()
            && self.blockers.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderContext {
    pub compacted_summary: Option<String>,
    pub preserved_turns: Vec<ProviderConversationTurn>,
    pub checkpoint: Option<ProviderContextCheckpointMetadata>,
}

impl ProviderContext {
    pub fn from_turns(turns: Vec<ProviderConversationTurn>) -> Self {
        Self {
            compacted_summary: None,
            preserved_turns: turns,
            checkpoint: None,
        }
    }

    pub fn from_checkpoint(checkpoint: ProviderContextCheckpoint) -> Self {
        let summary =
            checkpoint_summary_with_operational_memory(&checkpoint.summary, &checkpoint.facts);
        Self {
            compacted_summary: Some(summary),
            preserved_turns: checkpoint.recent_turns,
            checkpoint: Some(checkpoint.metadata),
        }
    }

    pub fn push_turn(&mut self, turn: ProviderConversationTurn) {
        self.preserved_turns.push(turn);
    }

    pub fn is_empty(&self) -> bool {
        self.compacted_summary
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
            && self.preserved_turns.is_empty()
    }
}

fn checkpoint_summary_with_operational_memory(
    summary: &str,
    facts: &ProviderCompactionFacts,
) -> String {
    if summary.contains("## Operational Memory")
        || (facts.read_files.is_empty()
            && facts.modified_files.is_empty()
            && facts.operation_facts.is_empty())
    {
        return summary.to_string();
    }

    let mut lines = vec![summary.trim_end().to_string(), String::new()];
    lines.push("## Operational Memory".to_string());
    lines.push("Read files:".to_string());
    if facts.read_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .read_files
                .iter()
                .take(12)
                .map(|fact| format!("- {}", fact.path)),
        );
    }
    lines.push("Modified files:".to_string());
    if facts.modified_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .modified_files
                .iter()
                .take(12)
                .map(|fact| format!("- {}", fact.path)),
        );
    }
    for fact in facts.operation_facts.iter().take(20) {
        lines.push(format!("- {fact}"));
    }
    lines.join("\n")
}

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
    pub request_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_summary: String,
    pub request_digest: String,
    pub metadata: Option<ProviderRequestStartedMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequestFinished {
    pub request_id: String,
    pub finish_reason: String,
    pub output_digest: Option<String>,
    pub usage: Option<CompletionUsage>,
    pub metadata: Option<ProviderRequestFinishedMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRuntimeEvent {
    ProviderRequestStarted(ProviderRequestStarted),
    ProviderStreamDelta { request_id: String, delta: String },
    ProviderReasoningDelta { request_id: String, delta: String },
    ProviderRequestFinished(ProviderRequestFinished),
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
        }
    }

    pub fn provider_error(
        reason: impl Into<String>,
        partial_assistant_output: impl Into<String>,
        provider_request_id: String,
    ) -> Self {
        Self::new(
            ProviderConversationTurnStatus::Failed,
            "provider_error",
            reason,
            partial_assistant_output,
            Some(provider_request_id),
        )
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

pub fn default_provider() -> Arc<dyn Provider> {
    Arc::new(NullProvider)
}

pub(crate) const MAX_TOOL_CALLS_TOTAL: usize = 1000;

pub struct MultiTurnStreamingRequest<'a> {
    pub provider: Arc<dyn Provider>,
    pub tool_registry: Arc<ToolRegistry>,
    pub profile: &'a AgentProfile,
    pub request_id: String,
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
    pub prompt_summary: &'a str,
    pub context: ProviderBoundaryContext<'a>,
    pub tool_defs: &'a [ToolDef],
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantResponse {
    pub request_id: String,
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
    pub tool_call_id: String,
    pub function_name: Option<String>,
    pub arguments_delta: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantToolIntent {
    pub tool_call_id: String,
    pub function_name: String,
    pub tool_id: String,
    pub arguments_json: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub enum ProviderBoundaryContext<'a> {
    ProjectedHarness {
        messages: &'a [ConversationMessage],
        checkpoint: Option<&'a ProviderContextCheckpointMetadata>,
    },
    ProviderMessages {
        messages: &'a [CompletionMessage],
    },
}

#[derive(Debug, Clone)]
pub struct ProviderBoundaryInput<'a> {
    pub profile: &'a AgentProfile,
    pub model: AgentModelRef,
    pub model_settings: AgentModelSettings,
    pub context: ProviderBoundaryContext<'a>,
    pub tools: Option<Vec<ToolDef>>,
    pub tool_choice: Option<ToolChoice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderBoundaryOutput {
    pub messages: Vec<CompletionMessage>,
    pub request: CompletionRequest,
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
    let projected_context = project_provider_context_for_prompt(prior_context, &request.prompt);
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
    let completion_request = provider_boundary.request;
    let request_digest = digest12_completion_request(&completion_request);

    let mut stream = provider.stream_completion(completion_request).await;
    let (provider_start_metadata, mut pending_event) =
        consume_provider_start_event(&mut stream).await;
    emit(AgentRuntimeEvent::ProviderRequestStarted(
        ProviderRequestStarted {
            request_id: request_id.clone(),
            provider_id: model.provider_id,
            model_id: model.model_id,
            prompt_summary: truncate_summary(&request.prompt, 256),
            request_digest,
            metadata: Some(provider_request_started_metadata(
                &request_id,
                &request_id,
                provider_start_metadata.as_ref(),
            )),
        },
    ))
    .await;
    let mut output = String::new();

    loop {
        let Some(event) = next_provider_event(&mut pending_event, &mut stream).await else {
            break;
        };
        match event {
            ProviderStreamEvent::Start | ProviderStreamEvent::Started { .. } => {}
            ProviderStreamEvent::TextDelta(delta) => {
                output.push_str(&delta);
                emit(AgentRuntimeEvent::ProviderStreamDelta {
                    request_id: request_id.clone(),
                    delta,
                })
                .await;
            }
            ProviderStreamEvent::ReasoningDelta(delta) => {
                emit(AgentRuntimeEvent::ProviderReasoningDelta {
                    request_id: request_id.clone(),
                    delta,
                })
                .await;
            }
            ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. } => {}
            ProviderStreamEvent::Done { usage } => {
                emit(AgentRuntimeEvent::ProviderRequestFinished(
                    ProviderRequestFinished {
                        request_id: request_id.clone(),
                        finish_reason: "done".to_string(),
                        output_digest: Some(digest12(output.as_bytes())),
                        usage: Some(usage),
                        metadata: Some(provider_finished_metadata(
                            &request_id,
                            &request_id,
                            "done",
                            &output,
                            "",
                            None,
                        )),
                    },
                ))
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
                emit(AgentRuntimeEvent::ProviderRequestFinished(
                    ProviderRequestFinished {
                        request_id: request_id.clone(),
                        finish_reason: "done".to_string(),
                        output_digest: Some(digest12(output.as_bytes())),
                        usage: Some(usage),
                        metadata: Some(provider_finished_metadata(
                            &request_id,
                            &request_id,
                            "done",
                            &output,
                            "",
                            provider_metadata.as_ref(),
                        )),
                    },
                ))
                .await;

                return AgentTurnOutcome::Succeeded {
                    output,
                    messages: Vec::new(),
                };
            }
            ProviderStreamEvent::Error { message } => {
                emit(AgentRuntimeEvent::ProviderRequestFinished(
                    ProviderRequestFinished {
                        request_id: request_id.clone(),
                        finish_reason: "error".to_string(),
                        output_digest: None,
                        usage: None,
                        metadata: Some(provider_finished_metadata(
                            &request_id,
                            &request_id,
                            "error",
                            &output,
                            "",
                            None,
                        )),
                    },
                ))
                .await;

                return AgentTurnOutcome::failed_with_memory(
                    message.clone(),
                    AgentTurnFailure::provider_error(message, output, request_id.clone()),
                );
            }
        }
    }

    emit(AgentRuntimeEvent::ProviderRequestFinished(
        ProviderRequestFinished {
            request_id: request_id.clone(),
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
    ))
    .await;

    AgentTurnOutcome::Succeeded {
        output,
        messages: Vec::new(),
    }
}

pub async fn stream_assistant_response_once<F, Fut>(
    request: StreamAssistantResponseOnceRequest<'_>,
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
        prompt_summary,
        context,
        tool_defs,
    } = request;

    let function_to_tool_id = tool_defs
        .iter()
        .map(|tool| (tool.function_name.clone(), tool.tool_id.clone()))
        .collect::<BTreeMap<_, _>>();

    let provider_boundary = transform_context_for_provider(ProviderBoundaryInput {
        profile,
        model: model.clone(),
        model_settings,
        context,
        tools: (!tool_defs.is_empty()).then(|| tool_defs.to_vec()),
        tool_choice: (!tool_defs.is_empty()).then_some(ToolChoice::Auto),
    });
    let completion_request = provider_boundary.request;
    let request_digest = digest12_completion_request(&completion_request);

    let mut stream = provider.stream_completion(completion_request).await;
    let (provider_start_metadata, mut pending_event) =
        consume_provider_start_event(&mut stream).await;
    let started_metadata = provider_request_started_metadata(
        &turn_request_id,
        &provider_request_id,
        provider_start_metadata.as_ref(),
    );

    emit(AgentRuntimeEvent::ProviderRequestStarted(
        ProviderRequestStarted {
            request_id: provider_request_id.clone(),
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
            prompt_summary: truncate_summary(prompt_summary, 256),
            request_digest,
            metadata: Some(started_metadata.clone()),
        },
    ))
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

    loop {
        let Some(event) = next_provider_event(&mut pending_event, &mut stream).await else {
            break;
        };
        match event {
            ProviderStreamEvent::Start | ProviderStreamEvent::Started { .. } => {}
            ProviderStreamEvent::TextDelta(delta) => {
                output.push_str(&delta);
                emit(AgentRuntimeEvent::ProviderStreamDelta {
                    request_id: provider_request_id.clone(),
                    delta,
                })
                .await;
            }
            ProviderStreamEvent::ReasoningDelta(delta) => {
                reasoning.push_str(&delta);
                reasoning_deltas.push(delta.clone());
                emit(AgentRuntimeEvent::ProviderReasoningDelta {
                    request_id: provider_request_id.clone(),
                    delta,
                })
                .await;
            }
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id,
                function_name,
                arguments_delta,
            } => {
                tool_call_deltas.push(AssistantToolCallDelta {
                    tool_call_id,
                    function_name,
                    arguments_delta,
                });
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
                usage = Some(finished_usage);
                break;
            }
            ProviderStreamEvent::DoneWithMetadata {
                usage: finished_usage,
                metadata,
            } => {
                stop_reason = "done".to_string();
                usage = Some(finished_usage);
                finished_provider_metadata = metadata;
                break;
            }
            ProviderStreamEvent::Error { message } => {
                stop_reason = "error".to_string();
                provider_error = Some(message);
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
    let output_digest = if stop_reason == "error" {
        None
    } else {
        Some(digest12(output.as_bytes()))
    };
    emit(AgentRuntimeEvent::ProviderRequestFinished(
        ProviderRequestFinished {
            request_id: provider_request_id.clone(),
            finish_reason: stop_reason.clone(),
            output_digest,
            usage: usage.clone(),
            metadata: Some(finished_metadata.clone()),
        },
    ))
    .await;

    if let Some(reason) = provider_error {
        return Err(AgentTurnFailure::provider_error(
            reason,
            output,
            provider_request_id,
        ));
    }

    let tool_intents =
        parse_tool_intents(tool_calls, &function_to_tool_id).map_err(AgentTurnFailure::message)?;
    Ok(AssistantResponse {
        request_id: provider_request_id,
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
    let tool_defs = match build_provider_tool_defs(profile, tool_registry.as_ref()) {
        Ok(tool_defs) => tool_defs,
        Err(reason) => return AgentTurnOutcome::failed(reason),
    };

    let projected_context = project_provider_context_for_prompt(prior_context, &request.prompt);
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
            turn_request_id: request_id,
            provider_request_id,
            prompt_summary: &request.prompt,
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

pub fn build_provider_context_messages(
    profile: &AgentProfile,
    prior_context: &ProviderContext,
    prompt: &str,
) -> Vec<CompletionMessage> {
    let projected_context = project_provider_context_for_prompt(prior_context, prompt);
    transform_context_for_provider(ProviderBoundaryInput {
        profile,
        model: AgentModelRef::parse(&profile.model_ref),
        model_settings: AgentModelSettings::default(),
        context: ProviderBoundaryContext::ProjectedHarness {
            messages: &projected_context,
            checkpoint: prior_context.checkpoint.as_ref(),
        },
        tools: None,
        tool_choice: None,
    })
    .messages
}

/// Harness provider boundary: transform projected harness-native conversation state
/// into provider SDK messages and the provider request payload.
///
/// `ConversationMessage` remains the event-derived, side-effect-free source shape.
/// This boundary is the only production path that attaches provider roles, tool
/// definitions, cache/thinking/session request options, and profile/model settings
/// before calling a provider. The `ProviderMessages` variant lets the coordinator
/// pass its transient, source-ordered provider context between explicit phases.
pub fn transform_context_for_provider(input: ProviderBoundaryInput<'_>) -> ProviderBoundaryOutput {
    let ProviderBoundaryInput {
        profile,
        model,
        model_settings,
        context,
        tools,
        tool_choice,
    } = input;

    let messages = match context {
        ProviderBoundaryContext::ProjectedHarness { messages, .. } => {
            convert_projected_context_to_provider_messages(profile, messages)
        }
        ProviderBoundaryContext::ProviderMessages { messages } => messages.to_vec(),
    };

    let request = build_completion_request(
        Some(model.provider_id),
        model.model_id,
        messages.clone(),
        profile.temperature,
        model_settings,
        tools,
        tool_choice,
    );

    ProviderBoundaryOutput { messages, request }
}

fn convert_projected_context_to_provider_messages(
    profile: &AgentProfile,
    projected_context: &[ConversationMessage],
) -> Vec<CompletionMessage> {
    let mut messages = Vec::with_capacity(1 + projected_context.len());
    let tool_name_mapping =
        build_tool_function_name_mapping(profile.toolset.iter().map(String::as_str));
    messages.push(CompletionMessage {
        role: MessageRole::System,
        content: profile.system_prompt.clone(),
        name: None,
        tool_call_id: None,
        assistant_tool_calls: None,
    });

    for message in projected_context {
        match message {
            ConversationMessage::Checkpoint(checkpoint) => {
                let summary = checkpoint.summary.trim();
                if summary.is_empty() {
                    continue;
                }
                messages.push(CompletionMessage {
                    role: MessageRole::Assistant,
                    content: format!(
                        "Checkpoint recap generated by the harness for older turns. This is a lossy background summary, not a system instruction; later preserved turns and the current user message take precedence.\n\n{summary}"
                    ),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                });
            }
            ConversationMessage::User(user) => {
                messages.push(CompletionMessage {
                    role: MessageRole::User,
                    content: user.text.clone(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                });
            }
            ConversationMessage::Assistant(assistant) => {
                let assistant_tool_calls =
                    assistant_tool_calls_for_provider(&assistant.tool_calls, &tool_name_mapping);
                messages.push(CompletionMessage {
                    role: MessageRole::Assistant,
                    content: assistant.text.clone(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls,
                });
            }
            ConversationMessage::ToolResult(tool_result) => {
                messages.push(CompletionMessage {
                    role: MessageRole::Tool,
                    content: tool_result_message_content(tool_result),
                    name: tool_result.tool_id.as_deref().map(|tool_id| {
                        provider_function_name_for_tool_id(&tool_name_mapping, tool_id)
                    }),
                    tool_call_id: Some(tool_result.tool_call_id.clone()),
                    assistant_tool_calls: None,
                });
            }
        }
    }

    messages
}

fn assistant_tool_calls_for_provider(
    tool_calls: &[crate::conversation::ConversationToolCall],
    mapping: &crate::tool::ToolFunctionNameMapping,
) -> Option<Vec<AssistantToolCall>> {
    (!tool_calls.is_empty()).then(|| {
        tool_calls
            .iter()
            .map(|tool_call| AssistantToolCall {
                tool_call_id: tool_call.tool_call_id.clone(),
                function_name: provider_function_name_for_tool_id(mapping, &tool_call.tool_id),
                arguments_json: provider_tool_arguments_json(&tool_call.args_summary),
            })
            .collect()
    })
}

fn provider_function_name_for_tool_id(
    mapping: &crate::tool::ToolFunctionNameMapping,
    tool_id: &str,
) -> String {
    mapping
        .function_name_for_tool_id(tool_id)
        .map(str::to_string)
        .unwrap_or_else(|| sanitize_tool_function_name(tool_id))
}

fn provider_tool_arguments_json(args_summary: &str) -> String {
    if serde_json::from_str::<Value>(args_summary).is_ok() {
        args_summary.to_string()
    } else {
        "{}".to_string()
    }
}

fn project_provider_context_for_prompt(
    prior_context: &ProviderContext,
    prompt: &str,
) -> Vec<ConversationMessage> {
    let summary_message_count = usize::from(
        prior_context
            .compacted_summary
            .as_deref()
            .map(str::trim)
            .is_some_and(|summary| !summary.is_empty()),
    );
    let mut messages = Vec::with_capacity(
        1 + summary_message_count + prior_context.preserved_turns.len().saturating_mul(3),
    );

    if let Some(summary) = prior_context
        .compacted_summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
    {
        let checkpoint = prior_context.checkpoint.as_ref();
        messages.push(ConversationMessage::Checkpoint(
            ConversationCheckpointMessage {
                checkpoint_id: checkpoint
                    .map(|metadata| metadata.checkpoint_id.clone())
                    .unwrap_or_default(),
                agent_id: checkpoint
                    .map(|metadata| metadata.agent_id.clone())
                    .unwrap_or_default(),
                through_seq: checkpoint
                    .map(|metadata| metadata.through_seq)
                    .unwrap_or_default(),
                summary: summary.to_string(),
            },
        ));
    }

    for turn in &prior_context.preserved_turns {
        if !turn.messages.is_empty() {
            messages.extend(turn.messages.clone());
            continue;
        }

        let request_id = turn.request_id.clone().unwrap_or_default();
        messages.push(ConversationMessage::User(ConversationUserMessage {
            request_id: request_id.clone(),
            text: turn.user_prompt.clone(),
            seq: turn.first_seq,
            agent_id: prior_context
                .checkpoint
                .as_ref()
                .map(|metadata| metadata.agent_id.clone()),
        }));
        messages.push(ConversationMessage::Assistant(
            ConversationAssistantMessage {
                request_id,
                agent_id: prior_context
                    .checkpoint
                    .as_ref()
                    .map(|metadata| metadata.agent_id.clone()),
                text: provider_turn_assistant_projection_text(turn),
                tool_calls: Vec::new(),
                stop_reason: None,
                first_seq: turn.first_seq,
                last_seq: turn.last_seq,
                provider_id: prior_context
                    .checkpoint
                    .as_ref()
                    .and_then(|metadata| metadata.provider_id.clone()),
                model_id: prior_context
                    .checkpoint
                    .as_ref()
                    .and_then(|metadata| metadata.model_id.clone()),
                output_digest: None,
            },
        ));
    }

    messages.push(ConversationMessage::User(ConversationUserMessage {
        request_id: String::new(),
        text: prompt.to_string(),
        seq: None,
        agent_id: None,
    }));

    messages
}

fn provider_turn_assistant_projection_text(turn: &ProviderConversationTurn) -> String {
    if turn.status.is_completed() {
        return turn.assistant_response.clone();
    }

    let stage = turn
        .failure_stage
        .as_deref()
        .filter(|stage| is_allowed_provider_turn_failure_stage(stage))
        .unwrap_or("unknown");
    let mut lines = vec![
        "Harness preserved an incomplete provider turn for continuity. Do not treat it as a completed answer.".to_string(),
        format!("Status: {}", turn.status.marker_label()),
        format!("Stage: {stage}"),
    ];
    if let Some(reason) = turn
        .failure_reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
    {
        lines.push(format!(
            "Reason: {}",
            truncate_provider_turn_failure_reason(reason)
        ));
    }
    lines.push("Partial assistant output:".to_string());
    if turn.assistant_response.trim().is_empty() {
        lines.push("(none)".to_string());
    } else {
        lines.push(turn.assistant_response.clone());
    }
    lines.join("\n")
}

fn truncate_provider_turn_failure_reason(reason: &str) -> String {
    if reason.chars().count() <= PROVIDER_TURN_FAILURE_REASON_MAX_CHARS {
        return reason.to_string();
    }

    let mut truncated = reason
        .chars()
        .take(PROVIDER_TURN_FAILURE_REASON_MAX_CHARS)
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn tool_result_message_content(tool_result: &ConversationToolResultMessage) -> String {
    if let Some(output_summary) = tool_result
        .output_summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
    {
        return output_summary.to_string();
    }

    tool_result
        .output_json
        .as_ref()
        .map(Value::to_string)
        .unwrap_or_default()
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
        if tool_call.tool_call_id.trim().is_empty() {
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
            tool_call_id: tool_call.tool_call_id,
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
) -> ProviderRequestStartedMetadata {
    ProviderRequestStartedMetadata {
        turn_id: Some(turn_request_id.to_string()),
        provider_call_id: Some(provider_request_id.to_string()),
        provider_session_id: provider_metadata.and_then(|metadata| {
            metadata
                .provider_session_id
                .as_deref()
                .and_then(non_empty_str)
                .map(str::to_string)
        }),
        provider_cache_id: provider_metadata.and_then(|metadata| {
            metadata
                .provider_cache_id
                .as_deref()
                .and_then(non_empty_str)
                .map(str::to_string)
        }),
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
            .and_then(non_empty_str)
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
                .and_then(non_empty_str)
                .map(str::to_string)
        }),
        provider_session_id: provider_metadata.and_then(|metadata| {
            metadata
                .provider_session_id
                .as_deref()
                .and_then(non_empty_str)
                .map(str::to_string)
        }),
        provider_cache_id: provider_metadata.and_then(|metadata| {
            metadata
                .provider_cache_id
                .as_deref()
                .and_then(non_empty_str)
                .map(str::to_string)
        }),
        provider_stop_reason: provider_metadata
            .and_then(|metadata| {
                metadata
                    .provider_stop_reason
                    .as_deref()
                    .and_then(non_empty_str)
                    .map(str::to_string)
            })
            .or_else(|| Some(stop_reason.to_string())),
        cache_read_tokens: provider_metadata.and_then(|metadata| metadata.cache_read_tokens),
        cache_write_tokens: provider_metadata.and_then(|metadata| metadata.cache_write_tokens),
        assistant_message,
        thinking,
    }
}

fn provider_thinking_metadata(
    thinking: &ProviderStreamThinkingMetadata,
) -> Option<ProviderThinkingMetadata> {
    let metadata = ProviderThinkingMetadata {
        summary: thinking
            .summary
            .as_deref()
            .and_then(non_empty_str)
            .map(str::to_string),
        summary_digest: thinking
            .summary_digest
            .as_deref()
            .and_then(non_empty_str)
            .map(str::to_string),
        signature: thinking
            .signature
            .as_deref()
            .and_then(non_empty_str)
            .map(str::to_string),
    };

    (metadata.summary.is_some()
        || metadata.summary_digest.is_some()
        || metadata.signature.is_some())
    .then_some(metadata)
}

fn non_empty_str(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub fn build_provider_tool_defs(
    profile: &AgentProfile,
    tool_registry: &ToolRegistry,
) -> Result<Vec<ToolDef>, String> {
    let mapping = build_tool_function_name_mapping(profile.toolset.iter().map(String::as_str));
    let mut tools = Vec::new();

    for (tool_id, function_name) in mapping.tool_id_to_function_name() {
        let Some(tool) = tool_registry.get(tool_id) else {
            return Err(format!(
                "agent profile `{}` references unknown tool `{tool_id}`",
                profile.name
            ));
        };
        let parameters = tool.parameters_json_schema();
        if let Err(reason) = validate_provider_parameters_schema(&parameters) {
            return Err(format!(
                "tool `{tool_id}` exported invalid parameters schema for provider use: {reason}"
            ));
        }

        tools.push(ToolDef {
            tool_id: tool_id.clone(),
            function_name: function_name.clone(),
            description: Some(tool.description().to_string()),
            parameters,
        });
    }

    Ok(tools)
}

fn validate_provider_parameters_schema(parameters: &serde_json::Value) -> Result<(), &'static str> {
    if parameters.get("type").and_then(serde_json::Value::as_str) != Some("object") {
        return Err("expected top-level `type: object`");
    }

    for forbidden in ["oneOf", "anyOf", "allOf", "enum", "not"] {
        if parameters.get(forbidden).is_some() {
            return Err(
                "top-level combinators (`oneOf`/`anyOf`/`allOf`/`enum`/`not`) are not allowed",
            );
        }
    }

    Ok(())
}

pub(crate) fn tool_result_to_message_content(result: &ToolResult) -> String {
    if !result.display_text.trim().is_empty() {
        return result.display_text.clone();
    }

    let mut payload = serde_json::Map::new();
    if let Some(structured_output) = result.structured_json.clone() {
        payload.insert("structured_output".to_string(), structured_output);
    }
    if !result.artifacts.is_empty() {
        let artifacts = serde_json::to_value(&result.artifacts).unwrap_or(Value::Array(Vec::new()));
        payload.insert("artifacts".to_string(), artifacts);
    }

    if payload.is_empty() {
        String::new()
    } else {
        Value::Object(payload).to_string()
    }
}

fn build_completion_request(
    provider_id: Option<String>,
    model_id: String,
    messages: Vec<CompletionMessage>,
    temperature: Option<f32>,
    model_settings: AgentModelSettings,
    tools: Option<Vec<ToolDef>>,
    tool_choice: Option<ToolChoice>,
) -> CompletionRequest {
    let AgentModelSettings {
        variant,
        reasoning_effort,
        text_verbosity,
        reasoning_summary,
    } = model_settings;

    CompletionRequest {
        provider_id,
        model_id,
        messages,
        temperature,
        max_tokens: None,
        variant,
        reasoning_effort,
        text_verbosity,
        reasoning_summary,
        tools,
        tool_choice,
        stream: true,
    }
}

pub fn default_model_settings_for_profile(profile_name: &str) -> AgentModelSettings {
    let Some(metadata) = registered_profile_model_metadata(profile_name) else {
        return AgentModelSettings::default();
    };

    AgentModelSettings {
        variant: metadata.variant,
        reasoning_effort: metadata.reasoning_effort.clone(),
        text_verbosity: metadata.text_verbosity,
        reasoning_summary: metadata
            .reasoning_effort
            .as_ref()
            .map(|_| "auto".to_string()),
    }
}

fn truncate_summary(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut summary: String = text.chars().take(max_chars).collect();
    summary.push('…');
    summary
}

fn digest12_completion_request(request: &CompletionRequest) -> String {
    let bytes = serde_json::to_vec(request).unwrap_or_else(|_| b"null".to_vec());
    digest12(&bytes)
}

fn digest12(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().chars().take(12).collect()
}

struct NullProvider;

#[async_trait]
impl Provider for NullProvider {
    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        Box::pin(tokio_stream::iter(vec![ProviderStreamEvent::Error {
            message: "no provider configured".to_string(),
        }]))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use harness_providers::mock::{request_digest, MockProvider};
    use harness_providers::{CompletionRequest, CompletionUsage, MessageRole, ToolChoice};
    use serde_json::json;

    use super::{
        build_provider_context_messages, build_provider_tool_defs,
        project_provider_context_for_prompt, run_multi_turn_streaming,
        tool_result_to_message_content, transform_context_for_provider, AgentModelRef,
        AgentModelSettings, AgentProfile, AgentRequest, AgentTurnOutcome,
        MultiTurnStreamingRequest, ProviderBoundaryContext, ProviderBoundaryInput, ProviderContext,
        ProviderContextCheckpointMetadata, ProviderConversationTurn,
        ProviderConversationTurnStatus, MAX_TOOL_CALLS_TOTAL,
    };
    use crate::config::ToolFailureMode;
    use crate::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};

    #[tokio::test]
    async fn multi_turn_runner_returns_single_provider_response_without_tools() {
        let profile = test_profile();
        let request = test_request();
        let tool_registry = test_tool_registry();
        let tool_defs =
            build_provider_tool_defs(&profile, tool_registry.as_ref()).expect("build tool defs");

        let first_request = completion_request(
            "model-1",
            vec![
                completion_system_message("sys"),
                completion_user_message("Use a tool"),
            ],
            &tool_defs,
        );

        let mut scripted = BTreeMap::new();
        scripted.insert(
            request_digest(&first_request),
            vec![
                harness_providers::ProviderStreamEvent::Start,
                harness_providers::ProviderStreamEvent::TextDelta("plain response".to_string()),
                harness_providers::ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 10,
                        completion_tokens: 2,
                        total_tokens: 12,
                    },
                },
            ],
        );

        let provider = Arc::new(MockProvider::new(scripted));
        let seen_calls = Arc::new(Mutex::new(0usize));

        let outcome = run_multi_turn_streaming(
            MultiTurnStreamingRequest {
                provider,
                tool_registry,
                profile: &profile,
                request_id: "req_000001".to_string(),
                request,
                prior_context: &ProviderContext::default(),
            },
            test_provider_request_ids(),
            {
                let seen_calls = seen_calls.clone();
                move |_tool_id, _args_json| {
                    let seen_calls = seen_calls.clone();
                    async move {
                        *seen_calls.lock().expect("lock seen calls") += 1;
                        Ok(ToolResult::text("unused"))
                    }
                }
            },
            |_event| async {},
        )
        .await;

        assert_eq!(
            outcome,
            AgentTurnOutcome::Succeeded {
                output: "plain response".to_string(),
                messages: Vec::new(),
            }
        );
        assert_eq!(*seen_calls.lock().expect("lock seen calls"), 0);
    }

    #[tokio::test]
    async fn multi_turn_runner_rejects_tool_intents_without_executing_callback() {
        let profile = test_profile();
        let request = test_request();
        let tool_registry = test_tool_registry();
        let tool_defs =
            build_provider_tool_defs(&profile, tool_registry.as_ref()).expect("build tool defs");
        let function_name = tool_defs.first().expect("tool def").function_name.clone();

        let first_request = completion_request(
            "model-1",
            vec![
                completion_system_message("sys"),
                completion_user_message("Use a tool"),
            ],
            &tool_defs,
        );

        let mut scripted = BTreeMap::new();
        scripted.insert(
            request_digest(&first_request),
            vec![
                harness_providers::ProviderStreamEvent::Start,
                harness_providers::ProviderStreamEvent::TextDelta("calling tool".to_string()),
                harness_providers::ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "call_1".to_string(),
                    function_name,
                    arguments_json: r#"{"filePath":"/tmp/demo.txt"}"#.to_string(),
                },
                harness_providers::ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 10,
                        completion_tokens: 8,
                        total_tokens: 18,
                    },
                },
            ],
        );

        let provider = Arc::new(MockProvider::new(scripted));
        let seen_calls = Arc::new(Mutex::new(0usize));

        let outcome = run_multi_turn_streaming(
            MultiTurnStreamingRequest {
                provider,
                tool_registry,
                profile: &profile,
                request_id: "req_000001".to_string(),
                request,
                prior_context: &ProviderContext::default(),
            },
            test_provider_request_ids(),
            {
                let seen_calls = seen_calls.clone();
                move |_tool_id, _args_json| {
                    let seen_calls = seen_calls.clone();
                    async move {
                        *seen_calls.lock().expect("lock seen calls") += 1;
                        Ok(ToolResult::text("must not execute"))
                    }
                }
            },
            |_event| async {},
        )
        .await;

        match outcome {
            AgentTurnOutcome::Failed { reason, .. } => {
                assert!(reason.contains("direct tool execution is unsupported"));
                assert!(reason.contains("coordinator loop"));
            }
            other => panic!("expected failed outcome, got {other:?}"),
        }
        assert_eq!(*seen_calls.lock().expect("lock seen calls"), 0);
    }

    #[test]
    fn agent_model_ref_parse_accepts_colon_and_slash_refs() {
        let colon = AgentModelRef::parse("default:gpt-5.4-mini");
        assert_eq!(colon.provider_id, "default");
        assert_eq!(colon.model_id, "gpt-5.4-mini");

        let slash = AgentModelRef::parse("default/gpt-5.4-mini");
        assert_eq!(slash.provider_id, "default");
        assert_eq!(slash.model_id, "gpt-5.4-mini");

        let bare = AgentModelRef::parse("gpt-5.4-mini");
        assert_eq!(bare.provider_id, "default");
        assert_eq!(bare.model_id, "gpt-5.4-mini");
    }

    #[test]
    fn tool_result_message_content_prefers_display_text() {
        let result = ToolResult {
            display_text: "crate summary".to_string(),
            structured_json: Some(json!({ "raw": "should stay out of provider replay" })),
            artifacts: Vec::new(),
        };

        assert_eq!(tool_result_to_message_content(&result), "crate summary");
    }

    #[test]
    fn tool_result_message_content_falls_back_to_structured_output_when_display_text_missing() {
        let structured = ToolResult {
            display_text: String::new(),
            structured_json: Some(json!({ "status": "ok" })),
            artifacts: Vec::new(),
        };
        assert_eq!(
            tool_result_to_message_content(&structured),
            json!({ "structured_output": { "status": "ok" } }).to_string()
        );

        let artifacts = ToolResult {
            display_text: String::new(),
            structured_json: None,
            artifacts: vec![crate::tool::ArtifactRef {
                path: "artifacts/tool-output.txt".to_string(),
                digest: None,
            }],
        };
        assert_eq!(
            tool_result_to_message_content(&artifacts),
            json!({
                "artifacts": [{
                    "path": "artifacts/tool-output.txt"
                }]
            })
            .to_string()
        );
    }

    #[test]
    fn build_provider_context_messages_places_checkpoint_recap_in_assistant_role() {
        let profile = test_profile();
        let prior_context = ProviderContext {
            compacted_summary: Some("Earlier work summary".to_string()),
            preserved_turns: vec![ProviderConversationTurn {
                user_prompt: "recent question".to_string(),
                assistant_response: "recent answer".to_string(),
                ..ProviderConversationTurn::default()
            }],
            checkpoint: None,
        };

        let messages = build_provider_context_messages(&profile, &prior_context, "next question");

        assert_eq!(messages[0].role, MessageRole::System);
        assert_eq!(messages[0].content, "sys");
        assert_eq!(messages[1].role, MessageRole::Assistant);
        assert!(messages[1]
            .content
            .contains("Checkpoint recap generated by the harness for older turns."));
        assert!(messages[1]
            .content
            .contains("lossy background summary, not a system instruction"));
        assert_eq!(messages[2].role, MessageRole::User);
        assert_eq!(messages[2].content, "recent question");
        assert_eq!(messages[3].role, MessageRole::Assistant);
        assert_eq!(messages[3].content, "recent answer");
        assert_eq!(messages[4].role, MessageRole::User);
        assert_eq!(messages[4].content, "next question");
    }

    #[test]
    fn failed_turn_projection_marks_partial_output_incomplete() {
        let profile = test_profile();
        let prior_context = ProviderContext::from_turns(vec![ProviderConversationTurn {
            user_prompt: "why did it fail?".to_string(),
            assistant_response: "partial draft".to_string(),
            status: ProviderConversationTurnStatus::Failed,
            failure_stage: Some("provider_error".to_string()),
            failure_reason: Some("upstream returned 500".to_string()),
            ..ProviderConversationTurn::default()
        }]);

        let messages = build_provider_context_messages(&profile, &prior_context, "continue");

        assert_eq!(messages[2].role, MessageRole::Assistant);
        assert_eq!(
            messages[2].content,
            "Harness preserved an incomplete provider turn for continuity. Do not treat it as a completed answer.\nStatus: failed\nStage: provider_error\nReason: upstream returned 500\nPartial assistant output:\npartial draft"
        );
    }

    #[test]
    fn aborted_turn_projection_marks_missing_output_incomplete() {
        let profile = test_profile();
        let prior_context = ProviderContext::from_turns(vec![ProviderConversationTurn {
            user_prompt: "stop now".to_string(),
            status: ProviderConversationTurnStatus::Aborted,
            failure_stage: Some("cancelled".to_string()),
            ..ProviderConversationTurn::default()
        }]);

        let messages = build_provider_context_messages(&profile, &prior_context, "continue");

        assert_eq!(messages[2].role, MessageRole::Assistant);
        assert_eq!(
            messages[2].content,
            "Harness preserved an incomplete provider turn for continuity. Do not treat it as a completed answer.\nStatus: aborted\nStage: cancelled\nPartial assistant output:\n(none)"
        );
    }

    #[test]
    fn provider_boundary_preserves_existing_message_shape() {
        let profile = test_profile();
        let request = AgentRequest {
            model_settings: AgentModelSettings {
                variant: Some("gpt-5.4".to_string()),
                reasoning_effort: Some("high".to_string()),
                text_verbosity: Some("low".to_string()),
                reasoning_summary: Some("auto".to_string()),
            },
            ..test_request()
        };
        let prior_context = ProviderContext {
            compacted_summary: Some("Earlier work summary".to_string()),
            preserved_turns: vec![ProviderConversationTurn {
                user_prompt: "recent question".to_string(),
                assistant_response: "recent answer".to_string(),
                request_id: Some("req_prior".to_string()),
                first_seq: Some(7),
                last_seq: Some(9),
                artifacts: Vec::new(),
                ..ProviderConversationTurn::default()
            }],
            checkpoint: Some(ProviderContextCheckpointMetadata {
                checkpoint_id: "checkpoint_1".to_string(),
                agent_id: "agent_1".to_string(),
                run_id: "run_1".to_string(),
                through_seq: 9,
                through_request_id: Some("req_prior".to_string()),
                provider_id: Some("mock".to_string()),
                model_id: Some("model-1".to_string()),
                tokens_before: None,
                tokens_before_estimate: Some(100),
                tokens_after_estimate: Some(40),
                summary_tokens_estimate: Some(12),
                compacted_turns: Some(3),
                preserved_turns: Some(1),
                reduction_tokens_estimate: Some(60),
                reduction_percent_estimate: Some(60),
                trigger_reason: Some("test".to_string()),
            }),
        };
        let tool_defs = build_provider_tool_defs(&profile, test_tool_registry().as_ref())
            .expect("build provider tool defs");

        let projected_context =
            project_provider_context_for_prompt(&prior_context, &request.prompt);
        let boundary = transform_context_for_provider(ProviderBoundaryInput {
            profile: &profile,
            model: AgentModelRef::parse(&request.model_ref),
            model_settings: request.model_settings.clone(),
            context: ProviderBoundaryContext::ProjectedHarness {
                messages: &projected_context,
                checkpoint: prior_context.checkpoint.as_ref(),
            },
            tools: Some(tool_defs.clone()),
            tool_choice: Some(ToolChoice::Auto),
        });

        let existing_messages =
            build_provider_context_messages(&profile, &prior_context, &request.prompt);
        assert_eq!(boundary.messages, existing_messages);

        assert_eq!(boundary.messages[0], completion_system_message("sys"));
        assert_eq!(boundary.messages[1].role, MessageRole::Assistant);
        assert!(boundary.messages[1]
            .content
            .contains("Checkpoint recap generated by the harness for older turns."));
        assert!(boundary.messages[1]
            .content
            .contains("Earlier work summary"));
        assert_eq!(
            boundary.messages[2],
            completion_user_message("recent question")
        );
        assert_eq!(
            boundary.messages[3],
            harness_providers::CompletionMessage {
                role: MessageRole::Assistant,
                content: "recent answer".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            }
        );
        assert_eq!(boundary.messages[4], completion_user_message("Use a tool"));

        assert_eq!(
            boundary.request,
            CompletionRequest {
                provider_id: Some("mock".to_string()),
                model_id: "model-1".to_string(),
                messages: existing_messages,
                temperature: Some(0.1),
                max_tokens: None,
                variant: Some("gpt-5.4".to_string()),
                reasoning_effort: Some("high".to_string()),
                text_verbosity: Some("low".to_string()),
                reasoning_summary: Some("auto".to_string()),
                tools: Some(tool_defs),
                tool_choice: Some(ToolChoice::Auto),
                stream: true,
            }
        );
    }

    #[tokio::test]
    async fn multi_turn_runner_fails_closed_on_unmapped_function_name() {
        let profile = test_profile();
        let request = test_request();
        let tool_registry = test_tool_registry();
        let tool_defs =
            build_provider_tool_defs(&profile, tool_registry.as_ref()).expect("build tool defs");

        let first_request = completion_request(
            "model-1",
            vec![
                completion_system_message("sys"),
                completion_user_message("Use a tool"),
            ],
            &tool_defs,
        );

        let mut scripted = BTreeMap::new();
        scripted.insert(
            request_digest(&first_request),
            vec![
                harness_providers::ProviderStreamEvent::Start,
                harness_providers::ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "call_1".to_string(),
                    function_name: "missing_function".to_string(),
                    arguments_json: "{}".to_string(),
                },
                harness_providers::ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 4,
                        completion_tokens: 3,
                        total_tokens: 7,
                    },
                },
            ],
        );

        let provider = Arc::new(MockProvider::new(scripted));
        let call_count = Arc::new(Mutex::new(0usize));

        let outcome = run_multi_turn_streaming(
            MultiTurnStreamingRequest {
                provider,
                tool_registry,
                profile: &profile,
                request_id: "req_000002".to_string(),
                request,
                prior_context: &ProviderContext::default(),
            },
            test_provider_request_ids(),
            {
                let call_count = call_count.clone();
                move |_tool_id, _args_json| {
                    let call_count = call_count.clone();
                    async move {
                        let mut guard = call_count.lock().expect("lock call count");
                        *guard += 1;
                        Ok(ToolResult::text("unused"))
                    }
                }
            },
            |_event| async {},
        )
        .await;

        match outcome {
            AgentTurnOutcome::Failed { reason, .. } => {
                assert!(reason.contains("unmapped tool function"));
            }
            other => panic!("expected failed outcome, got {other:?}"),
        }

        assert_eq!(*call_count.lock().expect("lock call count"), 0);
    }

    #[tokio::test]
    async fn multi_turn_runner_fails_closed_on_malformed_tool_args_json() {
        let profile = test_profile();
        let request = test_request();
        let tool_registry = test_tool_registry();
        let tool_defs =
            build_provider_tool_defs(&profile, tool_registry.as_ref()).expect("build tool defs");
        let function_name = tool_defs.first().expect("tool def").function_name.clone();

        let first_request = completion_request(
            "model-1",
            vec![
                completion_system_message("sys"),
                completion_user_message("Use a tool"),
            ],
            &tool_defs,
        );

        let mut scripted = BTreeMap::new();
        scripted.insert(
            request_digest(&first_request),
            vec![
                harness_providers::ProviderStreamEvent::Start,
                harness_providers::ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "call_1".to_string(),
                    function_name,
                    arguments_json: "{\"filePath\":\"/tmp/demo.txt\"".to_string(),
                },
                harness_providers::ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 4,
                        completion_tokens: 3,
                        total_tokens: 7,
                    },
                },
            ],
        );

        let provider = Arc::new(MockProvider::new(scripted));
        let call_count = Arc::new(Mutex::new(0usize));

        let outcome = run_multi_turn_streaming(
            MultiTurnStreamingRequest {
                provider,
                tool_registry,
                profile: &profile,
                request_id: "req_000003".to_string(),
                request,
                prior_context: &ProviderContext::default(),
            },
            test_provider_request_ids(),
            {
                let call_count = call_count.clone();
                move |_tool_id, _args_json| {
                    let call_count = call_count.clone();
                    async move {
                        let mut guard = call_count.lock().expect("lock call count");
                        *guard += 1;
                        Ok(ToolResult::text("unused"))
                    }
                }
            },
            |_event| async {},
        )
        .await;

        match outcome {
            AgentTurnOutcome::Failed { reason, .. } => {
                assert!(reason.contains("malformed tool args"));
            }
            other => panic!("expected failed outcome, got {other:?}"),
        }

        assert_eq!(*call_count.lock().expect("lock call count"), 0);
    }

    fn test_profile() -> AgentProfile {
        profile_with_max_iters(12)
    }

    fn profile_with_max_iters(max_iters: usize) -> AgentProfile {
        AgentProfile {
            name: "worker".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            system_prompt: "sys".to_string(),
            max_iters,
            temperature: Some(0.1),
            tool_failure_mode: ToolFailureMode::FailTurn,
            toolset: vec!["read".to_string()],
        }
    }

    fn test_request() -> AgentRequest {
        AgentRequest {
            agent_id: "agent_1".to_string(),
            prompt: "Use a tool".to_string(),
            model_ref: "mock:model-1".to_string(),
            model_settings: AgentModelSettings::default(),
        }
    }

    fn test_provider_request_ids() -> impl FnMut() -> std::future::Ready<Result<String, String>> {
        let mut next_id = 1_u64;
        move || {
            let request_id = format!("req_provider_{next_id:06}");
            next_id += 1;
            std::future::ready(Ok(request_id))
        }
    }

    #[test]
    fn max_tool_calls_total_supports_tool_heavy_agents() {
        assert_eq!(MAX_TOOL_CALLS_TOTAL, 1000);
    }

    fn completion_request(
        model_id: &str,
        messages: Vec<harness_providers::CompletionMessage>,
        tool_defs: &[harness_providers::ToolDef],
    ) -> harness_providers::CompletionRequest {
        harness_providers::CompletionRequest {
            provider_id: Some("mock".to_string()),
            model_id: model_id.to_string(),
            messages,
            temperature: Some(0.1),
            max_tokens: None,
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            tools: Some(tool_defs.to_vec()),
            tool_choice: Some(ToolChoice::Auto),
            stream: true,
        }
    }

    fn completion_system_message(content: &str) -> harness_providers::CompletionMessage {
        harness_providers::CompletionMessage {
            role: harness_providers::MessageRole::System,
            content: content.to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }
    }

    fn completion_user_message(content: &str) -> harness_providers::CompletionMessage {
        harness_providers::CompletionMessage {
            role: harness_providers::MessageRole::User,
            content: content.to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }
    }

    fn test_tool_registry() -> Arc<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestReadTool));
        Arc::new(registry)
    }

    fn broken_schema_profile() -> AgentProfile {
        AgentProfile {
            toolset: vec!["broken.tool".to_string()],
            ..test_profile()
        }
    }

    fn broken_schema_tool_registry() -> Arc<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(BrokenSchemaTool));
        Arc::new(registry)
    }

    struct TestReadTool;

    struct BrokenSchemaTool;

    #[async_trait]
    impl Tool for TestReadTool {
        fn id(&self) -> &str {
            "read"
        }

        fn description(&self) -> &str {
            "Read file content by path"
        }

        fn parameters_json_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "filePath": {"type": "string"}
                },
                "required": ["filePath"],
                "additionalProperties": false
            })
        }

        fn capability(&self) -> ToolCapability {
            ToolCapability::ReadFs
        }

        async fn call(
            &self,
            _ctx: ToolContext,
            _args_json: serde_json::Value,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::text("unused"))
        }
    }

    #[async_trait]
    impl Tool for BrokenSchemaTool {
        fn id(&self) -> &str {
            "broken.tool"
        }

        fn description(&self) -> &str {
            "Broken provider schema test tool"
        }

        fn parameters_json_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "oneOf": [
                    {
                        "type": "object",
                        "required": ["value"],
                        "properties": {
                            "value": {"type": "string"}
                        }
                    }
                ]
            })
        }

        fn capability(&self) -> ToolCapability {
            ToolCapability::ReadFs
        }

        async fn call(
            &self,
            _ctx: ToolContext,
            _args_json: serde_json::Value,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::text("unused"))
        }
    }

    #[test]
    fn build_provider_tool_defs_rejects_top_level_combinator_schemas() {
        let err = build_provider_tool_defs(
            &broken_schema_profile(),
            broken_schema_tool_registry().as_ref(),
        )
        .expect_err("provider tool defs should reject top-level combinator schemas");

        assert!(err.contains("broken.tool"), "unexpected error: {err}");
        assert!(
            err.contains("top-level combinators"),
            "unexpected error: {err}"
        );
    }
}

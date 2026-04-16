use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use harness_providers::{
    AssistantToolCall, CompletionMessage, CompletionRequest, CompletionUsage, MessageRole,
    Provider, ProviderEventStream, ProviderStreamEvent, ToolChoice, ToolDef,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_stream::StreamExt;

use crate::config::{registered_profile_model_metadata, ToolFailureMode};
use crate::tool::{build_tool_function_name_mapping, ToolRegistry, ToolResult};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConversationTurn {
    pub user_prompt: String,
    pub assistant_response: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentModelRef {
    pub provider_id: String,
    pub model_id: String,
}

impl AgentModelRef {
    pub fn parse(model_ref: &str) -> Self {
        let mut parts = model_ref.splitn(2, ':');
        let provider_id = parts
            .next()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("default")
            .to_string();
        let model_id = parts
            .next()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("default")
            .to_string();

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequestFinished {
    pub request_id: String,
    pub finish_reason: String,
    pub output_digest: Option<String>,
    pub usage: Option<CompletionUsage>,
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
    Succeeded { output: String },
    Failed { reason: String },
}

pub fn default_provider() -> Arc<dyn Provider> {
    Arc::new(NullProvider)
}

const MAX_TOOL_CALLS_TOTAL: usize = 25;

pub struct MultiTurnStreamingRequest<'a> {
    pub provider: Arc<dyn Provider>,
    pub tool_registry: Arc<ToolRegistry>,
    pub profile: &'a AgentProfile,
    pub request_id: String,
    pub request: AgentRequest,
    pub prior_turns: &'a [ProviderConversationTurn],
}

pub async fn run_single_turn_streaming<F, Fut>(
    provider: Arc<dyn Provider>,
    profile: &AgentProfile,
    request_id: String,
    request: AgentRequest,
    prior_turns: &[ProviderConversationTurn],
    mut emit: F,
) -> AgentTurnOutcome
where
    F: FnMut(AgentRuntimeEvent) -> Fut,
    Fut: Future<Output = ()>,
{
    let model = AgentModelRef::parse(&request.model_ref);
    let messages = build_provider_context_messages(profile, prior_turns, &request.prompt);
    let completion_request = build_completion_request(
        Some(model.provider_id.clone()),
        model.model_id.clone(),
        messages,
        profile.temperature,
        request.model_settings.clone(),
        None,
        None,
    );

    emit(AgentRuntimeEvent::ProviderRequestStarted(
        ProviderRequestStarted {
            request_id: request_id.clone(),
            provider_id: model.provider_id,
            model_id: model.model_id,
            prompt_summary: truncate_summary(&request.prompt, 256),
            request_digest: digest12_completion_request(&completion_request),
        },
    ))
    .await;

    let mut stream = provider.stream_completion(completion_request).await;
    let mut output = String::new();

    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::Start => {}
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
                    },
                ))
                .await;

                return AgentTurnOutcome::Succeeded { output };
            }
            ProviderStreamEvent::Error { message } => {
                emit(AgentRuntimeEvent::ProviderRequestFinished(
                    ProviderRequestFinished {
                        request_id: request_id.clone(),
                        finish_reason: "error".to_string(),
                        output_digest: None,
                        usage: None,
                    },
                ))
                .await;

                return AgentTurnOutcome::Failed { reason: message };
            }
        }
    }

    emit(AgentRuntimeEvent::ProviderRequestFinished(
        ProviderRequestFinished {
            request_id,
            finish_reason: "stream_ended".to_string(),
            output_digest: Some(digest12(output.as_bytes())),
            usage: None,
        },
    ))
    .await;

    AgentTurnOutcome::Succeeded { output }
}

pub async fn run_multi_turn_streaming<F, Fut, T, TFut>(
    request: MultiTurnStreamingRequest<'_>,
    mut call_tool_and_wait: T,
    mut emit: F,
) -> AgentTurnOutcome
where
    F: FnMut(AgentRuntimeEvent) -> Fut,
    Fut: Future<Output = ()>,
    T: FnMut(String, Value) -> TFut,
    TFut: Future<Output = Result<ToolResult, String>>,
{
    let MultiTurnStreamingRequest {
        provider,
        tool_registry,
        profile,
        request_id,
        request,
        prior_turns,
    } = request;

    let model = AgentModelRef::parse(&request.model_ref);
    let tool_defs = match build_provider_tool_defs(profile, tool_registry.as_ref()) {
        Ok(tool_defs) => tool_defs,
        Err(reason) => return AgentTurnOutcome::Failed { reason },
    };

    let mut messages = build_provider_context_messages(profile, prior_turns, &request.prompt);

    let mut total_tool_calls = 0usize;

    for _iter in 1..=profile.max_iters {
        let turn_request_id = request_id.clone();

        let completion_request = build_completion_request(
            Some(model.provider_id.clone()),
            model.model_id.clone(),
            messages.clone(),
            profile.temperature,
            request.model_settings.clone(),
            (!tool_defs.is_empty()).then(|| tool_defs.clone()),
            (!tool_defs.is_empty()).then_some(ToolChoice::Auto),
        );

        emit(AgentRuntimeEvent::ProviderRequestStarted(
            ProviderRequestStarted {
                request_id: turn_request_id.clone(),
                provider_id: model.provider_id.clone(),
                model_id: model.model_id.clone(),
                prompt_summary: truncate_summary(&request.prompt, 256),
                request_digest: digest12_completion_request(&completion_request),
            },
        ))
        .await;

        let mut stream = provider.stream_completion(completion_request.clone()).await;
        let mut output = String::new();
        let mut tool_calls = Vec::new();
        let mut finished = false;

        while let Some(event) = stream.next().await {
            match event {
                ProviderStreamEvent::Start => {}
                ProviderStreamEvent::TextDelta(delta) => {
                    output.push_str(&delta);
                    emit(AgentRuntimeEvent::ProviderStreamDelta {
                        request_id: turn_request_id.clone(),
                        delta,
                    })
                    .await;
                }
                ProviderStreamEvent::ReasoningDelta(delta) => {
                    emit(AgentRuntimeEvent::ProviderReasoningDelta {
                        request_id: turn_request_id.clone(),
                        delta,
                    })
                    .await;
                }
                ProviderStreamEvent::ToolCallDelta { .. } => {}
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
                ProviderStreamEvent::Done { usage } => {
                    finished = true;
                    let usage = Some(usage);
                    emit(AgentRuntimeEvent::ProviderRequestFinished(
                        ProviderRequestFinished {
                            request_id: turn_request_id.clone(),
                            finish_reason: "done".to_string(),
                            output_digest: Some(digest12(output.as_bytes())),
                            usage,
                        },
                    ))
                    .await;
                    break;
                }
                ProviderStreamEvent::Error { message } => {
                    emit(AgentRuntimeEvent::ProviderRequestFinished(
                        ProviderRequestFinished {
                            request_id: turn_request_id,
                            finish_reason: "error".to_string(),
                            output_digest: None,
                            usage: None,
                        },
                    ))
                    .await;
                    return AgentTurnOutcome::Failed { reason: message };
                }
            }
        }

        if !finished {
            emit(AgentRuntimeEvent::ProviderRequestFinished(
                ProviderRequestFinished {
                    request_id: turn_request_id,
                    finish_reason: "stream_ended".to_string(),
                    output_digest: Some(digest12(output.as_bytes())),
                    usage: None,
                },
            ))
            .await;
        }

        let assistant_tool_calls = (!tool_calls.is_empty()).then(|| {
            tool_calls
                .iter()
                .map(|tool_call| AssistantToolCall {
                    tool_call_id: tool_call.tool_call_id.clone(),
                    function_name: tool_call.function_name.clone(),
                    arguments_json: tool_call.arguments_json.clone(),
                })
                .collect::<Vec<_>>()
        });

        messages.push(CompletionMessage {
            role: MessageRole::Assistant,
            content: output.clone(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls,
        });

        if tool_calls.is_empty() {
            return AgentTurnOutcome::Succeeded { output };
        }

        total_tool_calls += tool_calls.len();
        if total_tool_calls > MAX_TOOL_CALLS_TOTAL {
            return AgentTurnOutcome::Failed {
                reason: format!("agent turn exceeded MAX_TOOL_CALLS_TOTAL={MAX_TOOL_CALLS_TOTAL}"),
            };
        }

        let function_to_tool_id = completion_request
            .tools
            .as_ref()
            .map(|tools| {
                tools
                    .iter()
                    .map(|tool| (tool.function_name.clone(), tool.tool_id.clone()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();

        for tool_call in tool_calls {
            let Some(tool_id) = function_to_tool_id.get(&tool_call.function_name) else {
                return AgentTurnOutcome::Failed {
                    reason: format!(
                        "provider emitted unmapped tool function `{}`",
                        tool_call.function_name
                    ),
                };
            };

            let args_json: Value = match serde_json::from_str(&tool_call.arguments_json) {
                Ok(args_json) => args_json,
                Err(err) => {
                    return AgentTurnOutcome::Failed {
                        reason: format!(
                            "provider emitted malformed tool args for `{}`: {err}",
                            tool_call.function_name
                        ),
                    };
                }
            };

            let tool_result = match call_tool_and_wait(tool_id.clone(), args_json).await {
                Ok(result) => result,
                Err(reason) => {
                    if matches!(
                        profile.tool_failure_mode,
                        ToolFailureMode::ContinueAsToolMessage
                    ) {
                        let tool_error_result = ToolResult {
                            display_text: format!(
                                "tool call `{}` failed: {reason}",
                                tool_call.function_name
                            ),
                            structured_json: Some(serde_json::json!({
                                "error": reason,
                                "status": "failed"
                            })),
                            artifacts: Vec::new(),
                        };
                        messages.push(CompletionMessage {
                            role: MessageRole::Tool,
                            content: tool_result_to_message_content(&tool_error_result),
                            name: Some(tool_call.function_name),
                            tool_call_id: Some(tool_call.tool_call_id),
                            assistant_tool_calls: None,
                        });
                        continue;
                    }

                    return AgentTurnOutcome::Failed {
                        reason: format!(
                            "tool call `{}` failed closed: {reason}",
                            tool_call.function_name
                        ),
                    };
                }
            };

            messages.push(CompletionMessage {
                role: MessageRole::Tool,
                content: tool_result_to_message_content(&tool_result),
                name: Some(tool_call.function_name),
                tool_call_id: Some(tool_call.tool_call_id),
                assistant_tool_calls: None,
            });
        }
    }

    AgentTurnOutcome::Failed {
        reason: format!(
            "agent turn exceeded profile max_iters={}",
            profile.max_iters
        ),
    }
}

pub fn build_provider_context_messages(
    profile: &AgentProfile,
    prior_turns: &[ProviderConversationTurn],
    prompt: &str,
) -> Vec<CompletionMessage> {
    let mut messages = Vec::with_capacity(2 + prior_turns.len().saturating_mul(2));
    messages.push(CompletionMessage {
        role: MessageRole::System,
        content: profile.system_prompt.clone(),
        name: None,
        tool_call_id: None,
        assistant_tool_calls: None,
    });

    for turn in prior_turns {
        messages.push(CompletionMessage {
            role: MessageRole::User,
            content: turn.user_prompt.clone(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        });
        messages.push(CompletionMessage {
            role: MessageRole::Assistant,
            content: turn.assistant_response.clone(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        });
    }

    messages.push(CompletionMessage {
        role: MessageRole::User,
        content: prompt.to_string(),
        name: None,
        tool_call_id: None,
        assistant_tool_calls: None,
    });

    messages
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CollectedToolCall {
    tool_call_id: String,
    function_name: String,
    arguments_json: String,
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

fn tool_result_to_message_content(result: &ToolResult) -> String {
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
    use harness_providers::{CompletionUsage, ToolChoice};
    use serde_json::json;

    use super::{
        build_provider_tool_defs, run_multi_turn_streaming, tool_result_to_message_content,
        AgentModelSettings, AgentProfile, AgentRequest, AgentTurnOutcome,
        MultiTurnStreamingRequest,
    };
    use crate::config::ToolFailureMode;
    use crate::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};

    #[tokio::test]
    async fn multi_turn_runner_executes_tool_then_completes() {
        let profile = test_profile();
        let request = test_request();
        let tool_registry = test_tool_registry();
        let tool_defs =
            build_provider_tool_defs(&profile, tool_registry.as_ref()).expect("build tool defs");
        let function_name = tool_defs.first().expect("tool def").function_name.clone();

        let tool_result = ToolResult::text("file content");
        let tool_result_message = tool_result_to_message_content(&tool_result);

        let first_request = completion_request(
            "model-1",
            vec![
                completion_system_message("sys"),
                completion_user_message("Use a tool"),
            ],
            &tool_defs,
        );
        let second_request = completion_request(
            "model-1",
            vec![
                completion_system_message("sys"),
                completion_user_message("Use a tool"),
                completion_assistant_message_with_tool_call(
                    "calling tool",
                    &function_name,
                    "call_1",
                    r#"{"filePath":"/tmp/demo.txt"}"#,
                ),
                completion_tool_message(&tool_result_message, &function_name, "call_1"),
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
                    function_name: function_name.clone(),
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
        scripted.insert(
            request_digest(&second_request),
            vec![
                harness_providers::ProviderStreamEvent::Start,
                harness_providers::ProviderStreamEvent::TextDelta("final response".to_string()),
                harness_providers::ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 14,
                        completion_tokens: 5,
                        total_tokens: 19,
                    },
                },
            ],
        );

        let provider = Arc::new(MockProvider::new(scripted));
        let seen_calls = Arc::new(Mutex::new(Vec::<(String, serde_json::Value)>::new()));

        let outcome = run_multi_turn_streaming(
            MultiTurnStreamingRequest {
                provider,
                tool_registry,
                profile: &profile,
                request_id: "req_000001".to_string(),
                request,
                prior_turns: &[],
            },
            {
                let seen_calls = seen_calls.clone();
                move |tool_id, args_json| {
                    let seen_calls = seen_calls.clone();
                    let tool_result = tool_result.clone();
                    async move {
                        seen_calls
                            .lock()
                            .expect("lock seen calls")
                            .push((tool_id, args_json));
                        Ok(tool_result)
                    }
                }
            },
            |_event| async {},
        )
        .await;

        assert_eq!(
            outcome,
            AgentTurnOutcome::Succeeded {
                output: "final response".to_string(),
            }
        );

        let calls = seen_calls.lock().expect("lock seen calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "read");
        assert_eq!(calls[0].1, json!({"filePath": "/tmp/demo.txt"}));
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

    #[tokio::test]
    async fn multi_turn_runner_can_continue_with_structured_only_tool_result() {
        let profile = test_profile();
        let request = test_request();
        let tool_registry = test_tool_registry();
        let tool_defs =
            build_provider_tool_defs(&profile, tool_registry.as_ref()).expect("build tool defs");
        let function_name = tool_defs.first().expect("tool def").function_name.clone();

        let structured_only_result = ToolResult {
            display_text: String::new(),
            structured_json: Some(json!({
                "path": "docs/guide.md",
                "lines": ["1: Intro", "2: Body"],
                "truncated": false
            })),
            artifacts: Vec::new(),
        };
        let tool_result_message = tool_result_to_message_content(&structured_only_result);

        let first_request = completion_request(
            "model-1",
            vec![
                completion_system_message("sys"),
                completion_user_message("Use a tool"),
            ],
            &tool_defs,
        );
        let second_request = completion_request(
            "model-1",
            vec![
                completion_system_message("sys"),
                completion_user_message("Use a tool"),
                completion_assistant_message_with_tool_call(
                    "calling tool",
                    &function_name,
                    "call_1",
                    r#"{"filePath":"/tmp/demo.txt"}"#,
                ),
                completion_tool_message(&tool_result_message, &function_name, "call_1"),
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
                    function_name: function_name.clone(),
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
        scripted.insert(
            request_digest(&second_request),
            vec![
                harness_providers::ProviderStreamEvent::Start,
                harness_providers::ProviderStreamEvent::TextDelta("final response".to_string()),
                harness_providers::ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 14,
                        completion_tokens: 5,
                        total_tokens: 19,
                    },
                },
            ],
        );

        let provider = Arc::new(MockProvider::new(scripted));
        let outcome = run_multi_turn_streaming(
            MultiTurnStreamingRequest {
                provider,
                tool_registry,
                profile: &profile,
                request_id: "req_structured_tool_result".to_string(),
                request,
                prior_turns: &[],
            },
            {
                let structured_only_result = structured_only_result.clone();
                move |_tool_id, _args_json| {
                    let structured_only_result = structured_only_result.clone();
                    async move { Ok(structured_only_result) }
                }
            },
            |_event| async {},
        )
        .await;

        assert_eq!(
            outcome,
            AgentTurnOutcome::Succeeded {
                output: "final response".to_string(),
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
                prior_turns: &[],
            },
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
            AgentTurnOutcome::Failed { reason } => {
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
                prior_turns: &[],
            },
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
            AgentTurnOutcome::Failed { reason } => {
                assert!(reason.contains("malformed tool args"));
            }
            other => panic!("expected failed outcome, got {other:?}"),
        }

        assert_eq!(*call_count.lock().expect("lock call count"), 0);
    }

    #[tokio::test]
    async fn multi_turn_runner_fails_closed_on_tool_failure() {
        let outcome =
            run_with_single_tool_call_failure("tool execution failed: command failed").await;
        assert_failure_reason_contains(outcome, "command failed");
    }

    #[tokio::test]
    async fn multi_turn_runner_fails_closed_on_tool_permission_denied() {
        let outcome =
            run_with_single_tool_call_failure("tool call denied: policy denied request").await;
        assert_failure_reason_contains(outcome, "denied");
    }

    #[tokio::test]
    async fn multi_turn_runner_fails_closed_on_tool_timeout() {
        let outcome =
            run_with_single_tool_call_failure("tool call timed out: permission request timed out")
                .await;
        assert_failure_reason_contains(outcome, "timed out");
    }

    async fn run_with_single_tool_call_failure(error: &str) -> AgentTurnOutcome {
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
                    arguments_json: r#"{"filePath":"/tmp/demo.txt"}"#.to_string(),
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
        let error = error.to_string();

        run_multi_turn_streaming(
            MultiTurnStreamingRequest {
                provider,
                tool_registry,
                profile: &profile,
                request_id: "req_000004".to_string(),
                request,
                prior_turns: &[],
            },
            move |_tool_id, _args_json| {
                let error = error.clone();
                async move { Err(error) }
            },
            |_event| async {},
        )
        .await
    }

    fn assert_failure_reason_contains(outcome: AgentTurnOutcome, needle: &str) {
        match outcome {
            AgentTurnOutcome::Failed { reason } => {
                assert!(reason.contains(needle), "reason was: {reason}");
            }
            other => panic!("expected failed outcome, got {other:?}"),
        }
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

    fn continue_profile() -> AgentProfile {
        AgentProfile {
            tool_failure_mode: ToolFailureMode::ContinueAsToolMessage,
            ..test_profile()
        }
    }

    async fn run_with_tool_loop(profile: AgentProfile) -> AgentTurnOutcome {
        let request = test_request();
        let tool_registry = test_tool_registry();
        let tool_defs =
            build_provider_tool_defs(&profile, tool_registry.as_ref()).expect("build tool defs");
        let function_name = tool_defs.first().expect("tool def").function_name.clone();
        let requests = Arc::new(Mutex::new(
            Vec::<harness_providers::CompletionRequest>::new(),
        ));
        let scripted_events = (0..profile.max_iters)
            .map(|call_index| {
                vec![
                    harness_providers::ProviderStreamEvent::Start,
                    harness_providers::ProviderStreamEvent::TextDelta(format!(
                        "calling tool {call_index}"
                    )),
                    harness_providers::ProviderStreamEvent::ToolCallComplete {
                        tool_call_id: format!("call_{call_index}"),
                        function_name: function_name.clone(),
                        arguments_json: r#"{"filePath":"/tmp/demo.txt"}"#.to_string(),
                    },
                    harness_providers::ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 4,
                            completion_tokens: 3,
                            total_tokens: 7,
                        },
                    },
                ]
            })
            .collect();
        let provider = Arc::new(RecordingProvider::new(requests.clone(), scripted_events));
        let tool_call_count = Arc::new(Mutex::new(0usize));

        let outcome = run_multi_turn_streaming(
            MultiTurnStreamingRequest {
                provider,
                tool_registry,
                profile: &profile,
                request_id: "req_loop".to_string(),
                request,
                prior_turns: &[],
            },
            {
                let tool_call_count = tool_call_count.clone();
                move |_tool_id, _args_json| {
                    let tool_call_count = tool_call_count.clone();
                    async move {
                        *tool_call_count.lock().expect("lock tool call count") += 1;
                        Ok(ToolResult::text("loop"))
                    }
                }
            },
            |_event| async {},
        )
        .await;

        assert_eq!(
            requests.lock().expect("lock requests").len(),
            profile.max_iters
        );
        assert_eq!(
            *tool_call_count.lock().expect("lock tool call count"),
            profile.max_iters
        );

        outcome
    }

    struct RecordingProvider {
        requests: Arc<Mutex<Vec<harness_providers::CompletionRequest>>>,
        scripted_events: Vec<Vec<harness_providers::ProviderStreamEvent>>,
        next_call_index: Arc<Mutex<usize>>,
    }

    impl RecordingProvider {
        fn new(
            requests: Arc<Mutex<Vec<harness_providers::CompletionRequest>>>,
            scripted_events: Vec<Vec<harness_providers::ProviderStreamEvent>>,
        ) -> Self {
            Self {
                requests,
                scripted_events,
                next_call_index: Arc::new(Mutex::new(0)),
            }
        }
    }

    #[async_trait]
    impl harness_providers::Provider for RecordingProvider {
        async fn stream_completion(
            &self,
            req: harness_providers::CompletionRequest,
        ) -> harness_providers::ProviderEventStream {
            self.requests.lock().expect("lock requests").push(req);

            let mut next_call_index = self.next_call_index.lock().expect("lock call index");
            let call_index = *next_call_index;
            *next_call_index += 1;

            let events = self
                .scripted_events
                .get(call_index)
                .cloned()
                .unwrap_or_else(|| {
                    vec![harness_providers::ProviderStreamEvent::Error {
                        message: format!("unexpected stream_completion call index {call_index}"),
                    }]
                });

            Box::pin(tokio_stream::iter(events))
        }
    }

    #[tokio::test]
    async fn multi_turn_runner_can_continue_as_tool_message_on_tool_failure() {
        let profile = continue_profile();
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
        let second_request = completion_request(
            "model-1",
            vec![
                completion_system_message("sys"),
                completion_user_message("Use a tool"),
                completion_assistant_message_with_tool_call(
                    "calling tool",
                    &function_name,
                    "call_1",
                    r#"{"filePath":"/tmp/demo.txt"}"#,
                ),
                completion_tool_message(
                    &tool_result_to_message_content(&ToolResult {
                        display_text: format!(
                            "tool call `{}` failed: command failed",
                            function_name
                        ),
                        structured_json: Some(serde_json::json!({
                            "error": "command failed",
                            "status": "failed"
                        })),
                        artifacts: Vec::new(),
                    }),
                    &function_name,
                    "call_1",
                ),
            ],
            &tool_defs,
        );

        let requests = Arc::new(Mutex::new(
            Vec::<harness_providers::CompletionRequest>::new(),
        ));
        let provider = Arc::new(RecordingProvider::new(
            requests.clone(),
            vec![
                vec![
                    harness_providers::ProviderStreamEvent::Start,
                    harness_providers::ProviderStreamEvent::TextDelta("calling tool".to_string()),
                    harness_providers::ProviderStreamEvent::ToolCallComplete {
                        tool_call_id: "call_1".to_string(),
                        function_name: function_name.clone(),
                        arguments_json: r#"{"filePath":"/tmp/demo.txt"}"#.to_string(),
                    },
                    harness_providers::ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 4,
                            completion_tokens: 3,
                            total_tokens: 7,
                        },
                    },
                ],
                vec![
                    harness_providers::ProviderStreamEvent::Start,
                    harness_providers::ProviderStreamEvent::TextDelta("final response".to_string()),
                    harness_providers::ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 10,
                            completion_tokens: 3,
                            total_tokens: 13,
                        },
                    },
                ],
            ],
        ));

        let outcome = run_multi_turn_streaming(
            MultiTurnStreamingRequest {
                provider,
                tool_registry,
                profile: &profile,
                request_id: "req_000005".to_string(),
                request,
                prior_turns: &[],
            },
            move |_tool_id, _args_json| async move { Err("command failed".to_string()) },
            |_event| async {},
        )
        .await;

        assert_eq!(
            outcome,
            AgentTurnOutcome::Succeeded {
                output: "final response".to_string(),
            }
        );

        let requests = requests.lock().expect("lock requests");
        assert_eq!(requests.as_slice(), &[first_request, second_request]);
    }

    #[tokio::test]
    async fn multi_turn_runner_stops_after_default_profile_max_iters() {
        let profile = test_profile();
        let outcome = run_with_tool_loop(profile.clone()).await;

        assert_eq!(
            outcome,
            AgentTurnOutcome::Failed {
                reason: format!(
                    "agent turn exceeded profile max_iters={}",
                    profile.max_iters
                ),
            }
        );
    }

    #[tokio::test]
    async fn multi_turn_runner_stops_after_custom_profile_max_iters() {
        let profile = profile_with_max_iters(2);
        let outcome = run_with_tool_loop(profile.clone()).await;

        assert_eq!(
            outcome,
            AgentTurnOutcome::Failed {
                reason: format!(
                    "agent turn exceeded profile max_iters={}",
                    profile.max_iters
                ),
            }
        );
    }

    fn test_request() -> AgentRequest {
        AgentRequest {
            agent_id: "agent_1".to_string(),
            prompt: "Use a tool".to_string(),
            model_ref: "mock:model-1".to_string(),
            model_settings: AgentModelSettings::default(),
        }
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

    fn completion_assistant_message_with_tool_call(
        content: &str,
        function_name: &str,
        tool_call_id: &str,
        arguments_json: &str,
    ) -> harness_providers::CompletionMessage {
        harness_providers::CompletionMessage {
            role: harness_providers::MessageRole::Assistant,
            content: content.to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: Some(vec![harness_providers::AssistantToolCall {
                tool_call_id: tool_call_id.to_string(),
                function_name: function_name.to_string(),
                arguments_json: arguments_json.to_string(),
            }]),
        }
    }

    fn completion_tool_message(
        content: &str,
        function_name: &str,
        tool_call_id: &str,
    ) -> harness_providers::CompletionMessage {
        harness_providers::CompletionMessage {
            role: harness_providers::MessageRole::Tool,
            content: content.to_string(),
            name: Some(function_name.to_string()),
            tool_call_id: Some(tool_call_id.to_string()),
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

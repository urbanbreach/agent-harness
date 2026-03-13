use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use harness_providers::{
    AssistantToolCall, CompletionMessage, CompletionRequest, MessageRole, Provider,
    ProviderEventStream, ProviderStreamEvent, ToolChoice, ToolDef,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_stream::StreamExt;

use crate::tool::{build_tool_function_name_mapping, ToolRegistry, ToolResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfile {
    pub name: String,
    pub category: String,
    pub model_ref: String,
    pub system_prompt: String,
    pub toolset: Vec<String>,
}

impl AgentProfile {
    pub fn fallback(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            category: name.clone(),
            model_ref: "default:default".to_string(),
            system_prompt: String::new(),
            toolset: Vec::new(),
            name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRequest {
    pub agent_id: String,
    pub prompt: String,
    pub model_ref: String,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRuntimeEvent {
    ProviderRequestStarted(ProviderRequestStarted),
    ProviderStreamDelta { request_id: String, delta: String },
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

const MAX_ITERS: usize = 12;
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
    let completion_request = CompletionRequest {
        model_id: model.model_id.clone(),
        messages,
        temperature: Some(0.0),
        max_tokens: None,
        tools: None,
        tool_choice: None,
        stream: true,
    };

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
            ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. } => {}
            ProviderStreamEvent::Done { .. } => {
                emit(AgentRuntimeEvent::ProviderRequestFinished(
                    ProviderRequestFinished {
                        request_id: request_id.clone(),
                        finish_reason: "done".to_string(),
                        output_digest: Some(digest12(output.as_bytes())),
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

    for _iter in 1..=MAX_ITERS {
        let turn_request_id = request_id.clone();

        let completion_request = CompletionRequest {
            model_id: model.model_id.clone(),
            messages: messages.clone(),
            temperature: Some(0.0),
            max_tokens: None,
            tools: (!tool_defs.is_empty()).then(|| tool_defs.clone()),
            tool_choice: (!tool_defs.is_empty()).then_some(ToolChoice::Auto),
            stream: true,
        };

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
                ProviderStreamEvent::Done { .. } => {
                    finished = true;
                    break;
                }
                ProviderStreamEvent::Error { message } => {
                    emit(AgentRuntimeEvent::ProviderRequestFinished(
                        ProviderRequestFinished {
                            request_id: turn_request_id,
                            finish_reason: "error".to_string(),
                            output_digest: None,
                        },
                    ))
                    .await;
                    return AgentTurnOutcome::Failed { reason: message };
                }
            }
        }

        emit(AgentRuntimeEvent::ProviderRequestFinished(
            ProviderRequestFinished {
                request_id: turn_request_id,
                finish_reason: if finished {
                    "done".to_string()
                } else {
                    "stream_ended".to_string()
                },
                output_digest: Some(digest12(output.as_bytes())),
            },
        ))
        .await;

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
        reason: format!("agent turn exceeded MAX_ITERS={MAX_ITERS}"),
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

fn build_provider_tool_defs(
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

        tools.push(ToolDef {
            tool_id: tool_id.clone(),
            function_name: function_name.clone(),
            description: Some(tool.description().to_string()),
            parameters: tool.parameters_json_schema(),
        });
    }

    Ok(tools)
}

fn tool_result_to_message_content(result: &ToolResult) -> String {
    serde_json::to_string(result).unwrap_or_else(|_| result.display_text.clone())
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
        AgentProfile, AgentRequest, AgentTurnOutcome, MultiTurnStreamingRequest,
    };
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
        assert_eq!(calls[0].0, "fs.read");
        assert_eq!(calls[0].1, json!({"filePath": "/tmp/demo.txt"}));
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
        AgentProfile {
            name: "worker".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            system_prompt: "sys".to_string(),
            toolset: vec!["fs.read".to_string()],
        }
    }

    fn test_request() -> AgentRequest {
        AgentRequest {
            agent_id: "agent_1".to_string(),
            prompt: "Use a tool".to_string(),
            model_ref: "mock:model-1".to_string(),
        }
    }

    fn completion_request(
        model_id: &str,
        messages: Vec<harness_providers::CompletionMessage>,
        tool_defs: &[harness_providers::ToolDef],
    ) -> harness_providers::CompletionRequest {
        harness_providers::CompletionRequest {
            model_id: model_id.to_string(),
            messages,
            temperature: Some(0.0),
            max_tokens: None,
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
        registry.register(Arc::new(TestFsReadTool));
        Arc::new(registry)
    }

    struct TestFsReadTool;

    #[async_trait]
    impl Tool for TestFsReadTool {
        fn id(&self) -> &str {
            "fs.read"
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
}

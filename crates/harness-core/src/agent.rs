use harness_providers::CacheRetention;
use serde::{Deserialize, Serialize};

mod provider_boundary;
mod provider_context;
mod streaming;
#[cfg(test)]
pub(in crate::agent) use provider_boundary::project_provider_context_for_prompt;
pub(crate) use provider_boundary::tool_result_to_message_content;
pub use provider_boundary::{
    build_provider_context_messages, build_provider_tool_defs, transform_context_for_provider,
    ProviderBoundaryContext, ProviderBoundaryInput, ProviderBoundaryOutput,
};
pub(in crate::agent) use provider_context::{
    is_allowed_provider_turn_failure_stage, PROVIDER_TURN_FAILURE_REASON_MAX_CHARS,
};
pub use provider_context::{
    ProviderCompactionFacts, ProviderCompactionSummarySource, ProviderCompactionTailBoundary,
    ProviderCompactionTimelineEntry, ProviderCompactionTurnFact, ProviderContext,
    ProviderContextCheckpoint, ProviderContextCheckpointMetadata, ProviderConversationTurn,
    ProviderConversationTurnStatus, ProviderFileOperationFact,
};
pub(crate) use streaming::MAX_TOOL_CALLS_TOTAL;
pub use streaming::{
    default_model_settings_for_profile, default_provider, run_multi_turn_streaming,
    run_single_turn_streaming, stream_assistant_response_once, AgentModelRef, AgentRuntimeEvent,
    AgentTurnFailure, AgentTurnOutcome, AssistantResponse, AssistantToolCallDelta,
    AssistantToolIntent, MultiTurnStreamingRequest, ProviderRequestFinished,
    ProviderRequestStarted, StreamAssistantResponseOnceRequest,
};

use crate::config::ToolFailureMode;
use crate::file_tag::{SelectedAgentTag, SelectedFileTag, SelectedResourceTag};
use crate::text::non_empty_trimmed;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentProfile {
    pub name: String,
    pub category: String,
    pub model_ref: String,
    #[serde(default)]
    pub model_ref_explicit: bool,
    pub system_prompt: String,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub cache_retention: CacheRetention,
    #[serde(default)]
    pub max_iters: Option<usize>,
    pub tool_failure_mode: ToolFailureMode,
    pub toolset: Vec<String>,
}

impl AgentProfile {
    pub fn fallback(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            category: name.clone(),
            model_ref: "default:default".to_string(),
            model_ref_explicit: false,
            system_prompt: String::new(),
            temperature: None,
            cache_retention: CacheRetention::Short,
            max_iters: None,
            tool_failure_mode: ToolFailureMode::FailTurn,
            toolset: Vec::new(),
            name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRequest {
    pub agent_id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_context: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_file_tags: Vec<SelectedFileTag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_agent_tags: Vec<SelectedAgentTag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_resource_tags: Vec<SelectedResourceTag>,
    pub model_ref: String,
    #[serde(default)]
    pub model_settings: AgentModelSettings,
}

impl AgentRequest {
    pub fn provider_prompt(&self) -> String {
        match self.prompt_context.as_deref().and_then(non_empty_trimmed) {
            Some(context) => format!("{}\n\n{context}", self.prompt),
            None => self.prompt.clone(),
        }
    }
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
    use crate::conversation::{ConversationMessage, ConversationUserMessage};
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
        let result = ToolResult::structured(
            "crate summary",
            json!({ "raw": "should stay out of provider replay" }),
        );

        assert_eq!(tool_result_to_message_content(&result), "crate summary");
    }

    #[test]
    fn tool_result_message_content_falls_back_to_structured_output_when_display_text_missing() {
        let structured = ToolResult::structured("", json!({ "status": "ok" }));
        assert_eq!(
            tool_result_to_message_content(&structured),
            json!({ "structured_output": { "status": "ok" } }).to_string()
        );

        let artifacts = ToolResult::artifacts(
            "",
            vec![crate::tool::ArtifactRef {
                path: "artifacts/tool-output.txt".to_string(),
                digest: None,
            }],
        );
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
    fn max_iters_turn_round_trips_failure_stage_and_messages() {
        let turn = ProviderConversationTurn {
            user_prompt: "loop until capped".to_string(),
            assistant_response: "partial work".to_string(),
            status: ProviderConversationTurnStatus::Aborted,
            failure_stage: Some("max_iters".to_string()),
            failure_reason: Some("agent turn exceeded profile max_iters=2".to_string()),
            messages: vec![ConversationMessage::User(ConversationUserMessage {
                request_id: "req_000001".to_string(),
                text: "loop until capped".to_string(),
                seq: Some(3),
                agent_id: Some("agent_000001".to_string()),
            })],
            ..ProviderConversationTurn::default()
        };

        let serialized = serde_json::to_value(&turn).expect("serialize max_iters turn");
        let restored: ProviderConversationTurn =
            serde_json::from_value(serialized).expect("deserialize max_iters turn");

        assert_eq!(restored, turn);
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

        let provider_prompt = request.provider_prompt();
        let projected_context =
            project_provider_context_for_prompt(&prior_context, &provider_prompt);
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
            build_provider_context_messages(&profile, &prior_context, &request.provider_prompt());
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
                context: Default::default(),
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
            model_ref_explicit: true,
            system_prompt: "sys".to_string(),
            cache_retention: Default::default(),
            max_iters: Some(max_iters),
            temperature: Some(0.1),
            tool_failure_mode: ToolFailureMode::FailTurn,
            toolset: vec!["read".to_string()],
        }
    }

    fn test_request() -> AgentRequest {
        AgentRequest {
            agent_id: "agent_1".to_string(),
            prompt: "Use a tool".to_string(),
            prompt_context: None,
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
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
            context: Default::default(),
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

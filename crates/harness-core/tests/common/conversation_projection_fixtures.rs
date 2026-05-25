use harness_core::agent::{
    build_provider_context_messages, build_provider_tool_defs, transform_context_for_provider,
    AgentModelRef, AgentModelSettings, AgentProfile, AgentRequest, ProviderBoundaryContext,
    ProviderBoundaryInput, ProviderCompactionFacts, ProviderContext, ProviderContextCheckpoint,
    ProviderContextCheckpointMetadata, ProviderConversationTurn, ProviderConversationTurnStatus,
};
use harness_core::config::ToolFailureMode;
use harness_core::conversation::{
    project_conversation, ConversationAssistantMessage, ConversationMessage, ConversationToolCall,
    ConversationToolResultMessage, ConversationUserMessage,
};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use harness_providers::{CompletionMessage, CompletionRequest, MessageRole, ToolChoice};
use std::sync::Arc;

fn worker() -> EventActor {
    EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()))
}

fn envelope(
    seq: u64,
    actor: EventActor,
    correlation_id: Option<&str>,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:020}"),
        seq,
        run_id: "run_conversation_projection".to_string(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn boundary_profile() -> AgentProfile {
    AgentProfile {
        name: "worker".to_string(),
        category: "deep".to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "sys".to_string(),
        temperature: Some(0.1),
        max_iters: Some(12),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: vec!["read".to_string()],
    }
}

fn checkpoint_metadata() -> ProviderContextCheckpointMetadata {
    ProviderContextCheckpointMetadata {
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
    }
}

fn completion_message(role: MessageRole, content: &str) -> CompletionMessage {
    CompletionMessage {
        role,
        content: content.to_string(),
        name: None,
        tool_call_id: None,
        assistant_tool_calls: None,
    }
}

fn boundary_tool_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(BoundaryReadTool));
    Arc::new(registry)
}

struct BoundaryReadTool;

#[async_trait::async_trait]
impl Tool for BoundaryReadTool {
    fn id(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read file content by path"
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        serde_json::json!({
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

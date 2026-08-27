use super::*;
use harness_core::tool::{ToolError, ToolResult};

pub(crate) struct StaticTextTool {
    pub(super) id: String,
    pub(super) output: String,
}

#[async_trait]
impl Tool for StaticTextTool {
    fn id(&self) -> &str {
        &self.id
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(self.output.clone()))
    }
}

pub(crate) fn provider_tool_events(
    tool_call_id: &str,
    tool_id: &str,
    arguments_json: &str,
) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: tool_call_id.to_string(),
            function_name: tool_id.to_string(),
            arguments_json: arguments_json.to_string(),
        },
        ProviderStreamEvent::Done {
            usage: Some(CompletionUsage {
                prompt_tokens: 100,
                completion_tokens: 100,
                total_tokens: 200,
            }),
        },
    ]
}

pub(crate) fn session_compaction_values(events: &[EventEnvelopeV1]) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) => {
                Some(serde_json::to_value(payload).unwrap_or_abort())
            }
            _ => None,
        })
        .collect()
}

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, Provider, ProviderEventStream,
    ProviderStreamEvent, ToolDef,
};
use serde_json::{json, Value};

pub(crate) const RECOVERY_INSTRUCTION: &str =
    "Rewrite the JSON arguments to match this tool's schema.";
pub(crate) const SERDE_DETAIL: &str = "invalid type: integer `123`, expected a string";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CapturedDogfoodRequest {
    tools: Vec<CapturedToolDef>,
    messages: Vec<CompletionMessage>,
}

impl CapturedDogfoodRequest {
    pub(crate) fn tool(&self, tool_id: &str) -> Option<&CapturedToolDef> {
        self.tools.iter().find(|tool| tool.tool_id == tool_id)
    }

    pub(crate) fn messages_text(&self) -> String {
        serde_json::to_string(&self.messages).expect("serialize captured messages")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CapturedToolDef {
    pub(crate) tool_id: String,
    pub(crate) function_name: String,
    pub(crate) parameters: Value,
}

pub(crate) struct DogfoodPromptProvider {
    script: DogfoodScript,
    requests: Mutex<Vec<CapturedDogfoodRequest>>,
    next_response_index: Mutex<usize>,
}

enum DogfoodScript {
    VagueSelection,
    BadArgumentRecovery { target_path: String },
}

impl DogfoodPromptProvider {
    pub(crate) fn vague_selection() -> Arc<Self> {
        Arc::new(Self {
            script: DogfoodScript::VagueSelection,
            requests: Mutex::new(Vec::new()),
            next_response_index: Mutex::new(0),
        })
    }

    pub(crate) fn bad_argument_recovery(target_path: String) -> Arc<Self> {
        Arc::new(Self {
            script: DogfoodScript::BadArgumentRecovery { target_path },
            requests: Mutex::new(Vec::new()),
            next_response_index: Mutex::new(0),
        })
    }

    pub(crate) fn requests(&self) -> Vec<CapturedDogfoodRequest> {
        self.requests.lock().expect("provider requests").clone()
    }

    fn response_for(&self, request: &CompletionRequest) -> Vec<ProviderStreamEvent> {
        let target_tool_id = match &self.script {
            DogfoodScript::VagueSelection => "glob",
            DogfoodScript::BadArgumentRecovery { .. } => "read",
        };
        if tool_def(request, target_tool_id).is_none() {
            return text_events("Auxiliary prompt complete.");
        }

        let mut next_response_index = self
            .next_response_index
            .lock()
            .expect("provider response index");
        let response_index = *next_response_index;
        *next_response_index += 1;

        match &self.script {
            DogfoodScript::VagueSelection => match response_index {
                0 => tool_call_events(
                    "call_vague_glob",
                    &function_name_for(request, "glob"),
                    json!({"pattern": "*.md", "path": "fixtures"}),
                ),
                _ => text_events("Selected glob and found the markdown fixture."),
            },
            DogfoodScript::BadArgumentRecovery { target_path } => match response_index {
                0 => tool_call_events(
                    "call_bad_read",
                    &function_name_for(request, "read"),
                    json!({"filePath": 123}),
                ),
                1 => tool_call_events(
                    "call_fixed_read",
                    &function_name_for(request, "read"),
                    json!({
                        "filePath": target_path,
                        "offset": 1,
                        "limit": 20,
                        "hashlineAnchors": false,
                    }),
                ),
                _ => text_events("Recovered from malformed read arguments."),
            },
        }
    }
}

#[async_trait]
impl Provider for DogfoodPromptProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        self.requests
            .lock()
            .expect("provider requests")
            .push(CapturedDogfoodRequest::from(&req));
        let events = self.response_for(&req);
        Box::pin(tokio_stream::iter(events))
    }
}

impl From<&CompletionRequest> for CapturedDogfoodRequest {
    fn from(request: &CompletionRequest) -> Self {
        Self {
            tools: request
                .tools
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(CapturedToolDef::from)
                .collect(),
            messages: request.messages.clone(),
        }
    }
}

impl From<ToolDef> for CapturedToolDef {
    fn from(tool: ToolDef) -> Self {
        Self {
            tool_id: tool.tool_id,
            function_name: tool.function_name,
            parameters: tool.parameters,
        }
    }
}

fn function_name_for(request: &CompletionRequest, tool_id: &str) -> String {
    tool_def(request, tool_id)
        .map(|tool| tool.function_name.clone())
        .unwrap_or_else(|| tool_id.to_string())
}

fn tool_def<'a>(request: &'a CompletionRequest, tool_id: &str) -> Option<&'a ToolDef> {
    request
        .tools
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|tool| tool.tool_id == tool_id)
}

fn tool_call_events(
    tool_call_id: &str,
    function_name: &str,
    arguments: Value,
) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: tool_call_id.to_string(),
            function_name: function_name.to_string(),
            arguments_json: arguments.to_string(),
        },
        ProviderStreamEvent::Done { usage: usage() },
    ]
}

fn text_events(text: &str) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta(text.to_string()),
        ProviderStreamEvent::Done { usage: usage() },
    ]
}

fn usage() -> CompletionUsage {
    CompletionUsage {
        prompt_tokens: 12,
        completion_tokens: 4,
        total_tokens: 16,
    }
}

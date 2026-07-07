use harness_providers::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::agent::{build_provider_tool_defs, AgentProfile};
use harness_core::config::{McpConfig, McpServerConfig, ShellAllowlist, ToolFailureMode};
use harness_providers::openai::{
    OpenAiApiMode, OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig, OpenAiHttpResponse,
    OpenAiHttpTransport,
};
use harness_providers::{
    CacheRetention, CompletionMessage, CompletionRequest, MessageRole, ToolChoice, ToolDef,
};
use harness_testkit::fakes::{
    FakeHttpClient, HttpClient, HttpInvocation, HttpOutput, ScriptedHttpCall,
};
use harness_tools::{coordinator_registry, coordinator_registry_with_mcp};

#[derive(Debug)]
struct FakeOpenAiTransport {
    http: Arc<FakeHttpClient>,
}

#[async_trait]
impl OpenAiHttpTransport for FakeOpenAiTransport {
    async fn post_json(
        &self,
        endpoint: String,
        headers: reqwest::header::HeaderMap,
        bearer_token: String,
        body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String> {
        let headers = headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_string(), value.to_string()))
            })
            .collect();
        let output = self
            .http
            .send(
                HttpInvocation::new("POST", endpoint)
                    .headers(headers)
                    .bearer_token(bearer_token)
                    .body(body),
            )
            .map_err(|err| err.to_string())?;
        Ok(OpenAiHttpResponse::text(
            output.status,
            reqwest::header::HeaderMap::new(),
            output.body_text(),
        ))
    }
}

pub(crate) fn provider() -> (OpenAiCompatibleProvider, Arc<FakeHttpClient>) {
    let http = Arc::new(FakeHttpClient::new([ScriptedHttpCall::new(
        "POST",
        "http://127.0.0.1/v1/responses",
        HttpOutput::text(200, responses_done_sse_transcript()),
    )]));
    let provider = OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "http://127.0.0.1/v1".to_string(),
            api_key: "test-key".to_string(),
            api_mode: OpenAiApiMode::Responses,
            timeout_ms: 60_000,
            headers: BTreeMap::new(),
        },
        Arc::new(FakeOpenAiTransport {
            http: Arc::clone(&http),
        }),
    )
    .unwrap_or_abort();
    (provider, http)
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
pub(crate) fn real_tools(name: &str) -> Vec<ToolDef> {
    let native = coordinator_registry(ShellAllowlist::default());
    let mcp = coordinator_registry_with_mcp(ShellAllowlist::default(), mcp_config());
    let (profile, registry) = match name {
        "build" => (
            profile(
                "build",
                vec![
                    "read",
                    "edit",
                    "bash",
                    "shell.run",
                    "github.issue",
                    "lsp.rename",
                ],
            ),
            &native,
        ),
        "plan" => (
            profile(
                "plan",
                vec!["read", "glob", "grep", "list", "question", "plan_exit"],
            ),
            &native,
        ),
        "mcp" => (
            profile(
                "mcp",
                vec!["mcp.docs.rs.tools.list", "mcp.docs.rs.tool.call"],
            ),
            &mcp,
        ),
        _ => panic!("abort"),
    };
    build_provider_tool_defs(&profile, registry).unwrap_or_abort()
}

pub(crate) fn completion_request(
    provider: &str,
    model: &str,
    tools: Vec<ToolDef>,
) -> CompletionRequest {
    CompletionRequest {
        provider_id: Some(provider.to_string()),
        model_id: model.to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: "check provider schema compatibility".to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: Some(tools),
        tool_choice: Some(ToolChoice::Auto),
        context: Default::default(),
        stream: true,
    }
}

fn profile(name: &str, toolset: Vec<&str>) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: name.to_string(),
        model_ref: "openai/gpt-5.5".to_string(),
        model_ref_explicit: true,
        system_prompt: format!("{name} schema compatibility"),
        temperature: Some(0.0),
        cache_retention: CacheRetention::Short,
        max_iters: Some(3),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: toolset.into_iter().map(str::to_string).collect(),
    }
}

fn mcp_config() -> McpConfig {
    let mut servers = BTreeMap::new();
    servers.insert(
        "docs.rs".to_string(),
        McpServerConfig::Stdio {
            command: vec!["false".to_string()],
            env: BTreeMap::new(),
            cwd: None,
            timeout_secs: 1,
            enabled: true,
        },
    );
    McpConfig { servers }
}

fn responses_done_sse_transcript() -> String {
    concat!(
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

use std::collections::BTreeMap;
use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use harness_providers::openai::{
    OpenAiApiMode, OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig, OpenAiHttpResponse,
    OpenAiHttpTransport,
};
use harness_providers::{CompletionRequest, Provider};
use harness_testkit::fakes::{
    FakeHttpClient, HttpClient, HttpInvocation, HttpOutput, ScriptedHttpCall,
};
use tokio_stream::StreamExt;

use super::{digest_value, OpenAiRequestSnapshot};

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

pub(super) async fn capture_openai_request(
    api_mode: OpenAiApiMode,
    request: CompletionRequest,
) -> Result<OpenAiRequestSnapshot, Box<dyn Error>> {
    let endpoint_path = endpoint_path(api_mode);
    let http = Arc::new(FakeHttpClient::new([ScriptedHttpCall::new(
        "POST",
        format!("http://127.0.0.1{endpoint_path}"),
        HttpOutput::text(200, sse_transcript(api_mode)),
    )]));
    let provider = OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "http://127.0.0.1/v1".to_string(),
            api_key: "test-key".to_string(),
            api_mode,
            timeout_ms: 60_000,
            headers: BTreeMap::new(),
        },
        Arc::new(FakeOpenAiTransport { http: http.clone() }),
    )?;

    let events = provider
        .stream_completion(request)
        .await
        .collect::<Vec<_>>()
        .await;
    assert!(!events.is_empty());
    let calls = http.calls();
    assert_eq!(calls.len(), 1);
    let body = &calls[0].body;
    Ok(OpenAiRequestSnapshot {
        api_mode: api_mode_name(api_mode).to_string(),
        endpoint_path: endpoint_path.to_string(),
        bearer_token: calls[0]
            .bearer_token
            .as_ref()
            .map(|_| "<redacted>".to_string())
            .unwrap_or_else(|| "<missing>".to_string()),
        request_body_digest: digest_value(body),
        tool_function_names: tool_function_names(api_mode, body),
    })
}

fn tool_function_names(api_mode: OpenAiApiMode, body: &serde_json::Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| match api_mode {
            OpenAiApiMode::ChatCompletions => tool["function"]["name"].as_str(),
            OpenAiApiMode::Responses => tool["name"].as_str(),
            OpenAiApiMode::Auto => None,
        })
        .map(str::to_string)
        .collect()
}

fn sse_transcript(api_mode: OpenAiApiMode) -> String {
    match api_mode {
        OpenAiApiMode::ChatCompletions => concat!(
            "data: {\"id\":\"chatcmpl-snapshot\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string(),
        OpenAiApiMode::Responses => concat!(
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string(),
        OpenAiApiMode::Auto => unreachable!("snapshot test uses explicit API modes"),
    }
}

fn endpoint_path(api_mode: OpenAiApiMode) -> &'static str {
    match api_mode {
        OpenAiApiMode::ChatCompletions => "/v1/chat/completions",
        OpenAiApiMode::Responses => "/v1/responses",
        OpenAiApiMode::Auto => unreachable!("snapshot test uses explicit API modes"),
    }
}

fn api_mode_name(api_mode: OpenAiApiMode) -> &'static str {
    match api_mode {
        OpenAiApiMode::ChatCompletions => "chat_completions",
        OpenAiApiMode::Responses => "responses",
        OpenAiApiMode::Auto => "auto",
    }
}

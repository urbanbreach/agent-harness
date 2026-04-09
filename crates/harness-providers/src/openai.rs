use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::{self as stream, StreamExt};

use crate::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderEventStream, ProviderStreamEvent, ToolChoice, ToolDef,
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub api_mode: OpenAiApiMode,
    pub timeout_ms: u64,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiApiMode {
    Responses,
    ChatCompletions,
    #[default]
    Auto,
}

#[derive(Debug, Error)]
pub enum OpenAiCompatibleProviderError {
    #[error("failed to build HTTP client: {0}")]
    BuildHttpClient(#[source] reqwest::Error),
    #[error("invalid header name `{header}`: {source}")]
    InvalidHeaderName {
        header: String,
        #[source]
        source: reqwest::header::InvalidHeaderName,
    },
    #[error("invalid header value for `{header}`: {source}")]
    InvalidHeaderValue {
        header: String,
        #[source]
        source: reqwest::header::InvalidHeaderValue,
    },
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    api_mode: OpenAiApiMode,
    headers: HeaderMap,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        config: OpenAiCompatibleProviderConfig,
    ) -> Result<Self, OpenAiCompatibleProviderError> {
        let headers = parse_headers(&config.headers)?;

        let timeout = if config.timeout_ms == 0 {
            None
        } else {
            Some(Duration::from_millis(config.timeout_ms))
        };

        let mut builder = reqwest::Client::builder();
        if let Some(timeout) = timeout {
            builder = builder.timeout(timeout);
        }

        let client = builder
            .build()
            .map_err(OpenAiCompatibleProviderError::BuildHttpClient)?;

        Ok(Self {
            client,
            base_url: config.base_url,
            api_key: config.api_key,
            api_mode: config.api_mode,
            headers,
        })
    }

    fn chat_completions_endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/chat/completions")
    }

    fn responses_endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/responses")
    }

    fn is_loopback_base_url(&self) -> bool {
        reqwest::Url::parse(self.base_url.trim())
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost")
                    || host
                        .parse::<IpAddr>()
                        .map(|ip| ip.is_loopback())
                        .unwrap_or(false)
            })
    }

    async fn send_request<T: Serialize>(
        &self,
        endpoint: String,
        request: &T,
    ) -> Result<reqwest::Response, String> {
        self.client
            .post(endpoint)
            .headers(self.headers.clone())
            .bearer_auth(&self.api_key)
            .json(request)
            .send()
            .await
            .map_err(|_| "openai_compatible request failed before receiving response".to_string())
    }

    async fn send_chat_request(
        &self,
        request: &OpenAiChatCompletionsRequest,
    ) -> Result<reqwest::Response, String> {
        self.send_request(self.chat_completions_endpoint(), request)
            .await
    }

    async fn send_responses_request(
        &self,
        request: &OpenAiResponsesRequest,
    ) -> Result<reqwest::Response, String> {
        self.send_request(self.responses_endpoint(), request).await
    }

    async fn non_success_status_message(&self, response: reqwest::Response) -> String {
        let status = response.status();
        let body = response.text().await.ok();
        format_non_success_status_message(status.as_u16(), body.as_deref(), &self.api_key)
    }
}

fn format_non_success_status_message(status: u16, body: Option<&str>, api_key: &str) -> String {
    let detail = body
        .and_then(extract_provider_error_detail)
        .or_else(|| {
            body.map(str::trim)
                .filter(|body| !body.is_empty())
                .map(str::to_string)
        })
        .map(|body| sanitize_provider_error_detail(&body, api_key))
        .filter(|body| !body.is_empty());

    match detail {
        Some(detail) => format!("openai_compatible request failed with status {status}: {detail}"),
        None => format!("openai_compatible request failed with status {status}"),
    }
}

fn extract_provider_error_detail(body: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    parsed
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| parsed.get("message").and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

fn sanitize_provider_error_detail(detail: &str, api_key: &str) -> String {
    if detail.to_ascii_lowercase().contains("authorization") {
        return "provider error body redacted because it contained sensitive auth material"
            .to_string();
    }

    if api_key.is_empty() {
        return detail.to_string();
    }

    detail.replace(api_key, "[REDACTED]")
}

fn map_tools<T>(tools: Option<Vec<ToolDef>>) -> Option<Vec<T>>
where
    T: From<ToolDef>,
{
    tools.map(|tools| tools.into_iter().map(Into::into).collect())
}

fn openai_role(role: &MessageRole) -> String {
    role_to_openai(role).to_string()
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let chat_request = OpenAiChatCompletionsRequest::from(req.clone());
        let responses_request = OpenAiResponsesRequest::from(req);

        let strategy = match self.api_mode {
            OpenAiApiMode::ChatCompletions => RequestStrategy::ChatCompletions,
            OpenAiApiMode::Responses => RequestStrategy::Responses,
            OpenAiApiMode::Auto => RequestStrategy::Auto,
        };

        let (mode, response) = match strategy {
            RequestStrategy::ChatCompletions => match self.send_chat_request(&chat_request).await {
                Ok(response) => (OpenAiApiMode::ChatCompletions, response),
                Err(message) => {
                    return Box::pin(stream::iter(vec![ProviderStreamEvent::Error { message }]))
                }
            },
            RequestStrategy::Responses => {
                match self.send_responses_request(&responses_request).await {
                    Ok(response) => (OpenAiApiMode::Responses, response),
                    Err(message) => {
                        return Box::pin(stream::iter(vec![ProviderStreamEvent::Error { message }]))
                    }
                }
            }
            RequestStrategy::Auto => {
                let response = match self.send_responses_request(&responses_request).await {
                    Ok(response) => response,
                    Err(message) => {
                        return Box::pin(stream::iter(vec![ProviderStreamEvent::Error { message }]))
                    }
                };

                if matches!(response.status().as_u16(), 404 | 405)
                    || (response.status().as_u16() == 400 && self.is_loopback_base_url())
                {
                    match self.send_chat_request(&chat_request).await {
                        Ok(fallback_response) => {
                            (OpenAiApiMode::ChatCompletions, fallback_response)
                        }
                        Err(message) => {
                            return Box::pin(stream::iter(vec![ProviderStreamEvent::Error {
                                message,
                            }]))
                        }
                    }
                } else {
                    (OpenAiApiMode::Responses, response)
                }
            }
        };

        let status = response.status();
        if !status.is_success() {
            let message = self.non_success_status_message(response).await;
            return Box::pin(stream::iter(vec![ProviderStreamEvent::Error { message }]));
        }

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            match mode {
                OpenAiApiMode::ChatCompletions => consume_chat_sse_stream(response, tx).await,
                OpenAiApiMode::Responses | OpenAiApiMode::Auto => {
                    consume_responses_sse_stream(response, tx).await
                }
            }
        });

        Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
    }
}

#[derive(Debug, Clone, Copy)]
enum RequestStrategy {
    ChatCompletions,
    Responses,
    Auto,
}

async fn consume_chat_sse_stream(
    response: reqwest::Response,
    tx: mpsc::Sender<ProviderStreamEvent>,
) {
    if tx.send(ProviderStreamEvent::Start).await.is_err() {
        return;
    }

    let mut usage = zero_usage();
    let mut done_emitted = false;
    let mut tool_call_state = ChatToolCallState::default();
    let mut sse_stream = response.bytes_stream().eventsource();

    while let Some(next_event) = sse_stream.next().await {
        let event = match next_event {
            Ok(event) => event,
            Err(_) => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: "openai_compatible SSE stream transport error".to_string(),
                    })
                    .await;
                return;
            }
        };

        let data = event.data.trim();
        if data == "[DONE]" {
            if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
                return;
            }

            if !done_emitted {
                let _ = tx.send(ProviderStreamEvent::Done { usage }).await;
            }
            return;
        }

        let chunk: OpenAiChatCompletionsChunk = match serde_json::from_str(data) {
            Ok(chunk) => chunk,
            Err(_) => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: "openai_compatible returned invalid SSE JSON chunk".to_string(),
                    })
                    .await;
                return;
            }
        };

        if let Some(chunk_usage) = chunk.usage {
            usage = chunk_usage;
        }

        let mut finish_seen = false;
        for choice in chunk.choices {
            if let Some(reasoning) = choice.delta.reasoning_text {
                if !reasoning.is_empty()
                    && tx
                        .send(ProviderStreamEvent::ReasoningDelta(reasoning))
                        .await
                        .is_err()
                {
                    return;
                }
            }

            if let Some(content) = choice.delta.content {
                if !content.is_empty()
                    && tx
                        .send(ProviderStreamEvent::TextDelta(content))
                        .await
                        .is_err()
                {
                    return;
                }
            }

            if !consume_tool_call_deltas(&tx, &choice.delta.tool_calls, &mut tool_call_state).await
            {
                return;
            }

            if matches!(choice.finish_reason.as_deref(), Some("tool_calls"))
                && !emit_tool_call_completions(&tx, &mut tool_call_state).await
            {
                return;
            }

            if choice.finish_reason.is_some() {
                finish_seen = true;
            }
        }

        if finish_seen && !done_emitted {
            if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
                return;
            }

            done_emitted = true;
            if tx
                .send(ProviderStreamEvent::Done {
                    usage: usage.clone(),
                })
                .await
                .is_err()
            {
                return;
            }
        }
    }

    if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
        return;
    }

    if !done_emitted {
        let _ = tx.send(ProviderStreamEvent::Done { usage }).await;
    }
}

#[derive(Debug, Default)]
struct ChatToolCallState {
    accumulators: BTreeMap<String, ToolCallAccumulator>,
    call_ids_by_index: BTreeMap<usize, String>,
}

#[derive(Debug, Default)]
struct ToolCallAccumulator {
    function_name: Option<String>,
    arguments_json: String,
}

async fn consume_tool_call_deltas(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    tool_calls: &[OpenAiChatToolCallDeltaChunk],
    state: &mut ChatToolCallState,
) -> bool {
    for tool_call in tool_calls {
        let Some(tool_call_id) = resolve_tool_call_id(tool_call, state) else {
            let _ = tx
                .send(ProviderStreamEvent::Error {
                    message:
                        "openai_compatible stream omitted tool_call_id for chat tool call delta"
                            .to_string(),
                })
                .await;
            return false;
        };

        let accumulator = state.accumulators.entry(tool_call_id.clone()).or_default();

        let mut function_name_delta = None;
        let mut arguments_delta = String::new();
        if let Some(function) = &tool_call.function {
            if let Some(name) = function.name.clone().filter(|name| !name.is_empty()) {
                accumulator.function_name = Some(name.clone());
                function_name_delta = Some(name);
            }

            if let Some(arguments) = function
                .arguments
                .as_ref()
                .filter(|value| !value.is_empty())
            {
                accumulator.arguments_json.push_str(arguments);
                arguments_delta = arguments.clone();
            }
        }

        if (function_name_delta.is_some() || !arguments_delta.is_empty())
            && tx
                .send(ProviderStreamEvent::ToolCallDelta {
                    tool_call_id,
                    function_name: function_name_delta,
                    arguments_delta,
                })
                .await
                .is_err()
        {
            return false;
        }
    }

    true
}

fn resolve_tool_call_id(
    tool_call: &OpenAiChatToolCallDeltaChunk,
    state: &mut ChatToolCallState,
) -> Option<String> {
    if let Some(tool_call_id) = tool_call.id.as_ref().filter(|id| !id.is_empty()) {
        let tool_call_id = tool_call_id.clone();
        state
            .call_ids_by_index
            .insert(tool_call.index, tool_call_id.clone());
        return Some(tool_call_id);
    }

    state.call_ids_by_index.get(&tool_call.index).cloned()
}

async fn emit_tool_call_completions(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    state: &mut ChatToolCallState,
) -> bool {
    if state.accumulators.is_empty() {
        return true;
    }

    let pending = std::mem::take(&mut state.accumulators);
    state.call_ids_by_index.clear();

    for (tool_call_id, accumulator) in pending {
        let Some(function_name) = accumulator
            .function_name
            .as_ref()
            .filter(|name| !name.is_empty())
            .cloned()
        else {
            let _ = tx
                .send(ProviderStreamEvent::Error {
                    message: format!(
                        "openai_compatible chat tool call `{tool_call_id}` missing function name"
                    ),
                })
                .await;
            return false;
        };

        if serde_json::from_str::<serde_json::Value>(&accumulator.arguments_json).is_err() {
            let _ = tx
                .send(ProviderStreamEvent::Error {
                    message: format!(
                        "openai_compatible chat tool call `{tool_call_id}` produced invalid arguments JSON"
                    ),
                })
                .await;
            return false;
        }

        if tx
            .send(ProviderStreamEvent::ToolCallComplete {
                tool_call_id,
                function_name,
                arguments_json: accumulator.arguments_json,
            })
            .await
            .is_err()
        {
            return false;
        }
    }

    true
}

async fn consume_responses_sse_stream(
    response: reqwest::Response,
    tx: mpsc::Sender<ProviderStreamEvent>,
) {
    if tx.send(ProviderStreamEvent::Start).await.is_err() {
        return;
    }

    let mut usage = zero_usage();
    let mut done_emitted = false;
    let mut sse_stream = response.bytes_stream().eventsource();
    let mut tool_calls = BTreeMap::<String, ResponsesToolCallAccumulator>::new();

    while let Some(next_event) = sse_stream.next().await {
        let event = match next_event {
            Ok(event) => event,
            Err(_) => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: "openai_compatible SSE stream transport error".to_string(),
                    })
                    .await;
                return;
            }
        };

        let data = event.data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            if !done_emitted {
                let _ = tx.send(ProviderStreamEvent::Done { usage }).await;
            }
            return;
        }

        let parsed: OpenAiResponsesEvent = match serde_json::from_str(data) {
            Ok(parsed) => parsed,
            Err(err) => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: format!(
                            "openai_compatible returned invalid SSE JSON chunk: {err}; sample={}",
                            summarize_sse_data(data)
                        ),
                    })
                    .await;
                return;
            }
        };

        match parsed.event_type.as_str() {
            "response.reasoning_summary_text.delta" => {
                if let Some(delta) = parsed.delta {
                    if !delta.is_empty()
                        && tx
                            .send(ProviderStreamEvent::ReasoningDelta(delta))
                            .await
                            .is_err()
                    {
                        return;
                    }
                }
            }
            "response.output_text.delta" => {
                if let Some(delta) = parsed.delta {
                    if !delta.is_empty()
                        && tx
                            .send(ProviderStreamEvent::TextDelta(delta))
                            .await
                            .is_err()
                    {
                        return;
                    }
                }
            }
            "response.output_item.added" => {
                if !handle_responses_tool_item_added(&tx, &mut tool_calls, parsed).await {
                    return;
                }
            }
            "response.function_call_arguments.delta" => {
                if !handle_responses_arguments_delta(&tx, &mut tool_calls, parsed).await {
                    return;
                }
            }
            "response.output_item.done" => {
                if !handle_responses_tool_item_done(&tx, &mut tool_calls, parsed).await {
                    return;
                }
            }
            "response.completed" | "response.done" | "response.incomplete" => {
                if let Some(response) = parsed.response {
                    if let Some(completion_usage) = response
                        .usage
                        .map(OpenAiResponsesUsage::into_completion_usage)
                    {
                        usage = completion_usage;
                    }
                }

                let pending_tool_calls = std::mem::take(&mut tool_calls);
                for (state_key, state) in pending_tool_calls {
                    if let Err(message) =
                        emit_responses_tool_call_complete(&tx, &state_key, state).await
                    {
                        let _ = tx.send(ProviderStreamEvent::Error { message }).await;
                        return;
                    }
                }

                if !done_emitted {
                    done_emitted = true;
                    if tx
                        .send(ProviderStreamEvent::Done {
                            usage: usage.clone(),
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
            "response.error" => {
                let _ = tx
                    .send(ProviderStreamEvent::Error {
                        message: "openai_compatible responses stream returned error event"
                            .to_string(),
                    })
                    .await;
                return;
            }
            _ => {}
        }
    }

    if !done_emitted {
        let _ = tx.send(ProviderStreamEvent::Done { usage }).await;
    }
}

async fn handle_responses_tool_item_added(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    tool_calls: &mut BTreeMap<String, ResponsesToolCallAccumulator>,
    event: OpenAiResponsesEvent,
) -> bool {
    let Some(item) = event.item else {
        return true;
    };

    if item.item_type != "function_call" {
        return true;
    }

    let Some(key) = item.id.clone().or_else(|| item.call_id.clone()) else {
        let _ = tx
            .send(ProviderStreamEvent::Error {
                message:
                    "openai_compatible responses tool call is missing both item id and call id"
                        .to_string(),
            })
            .await;
        return false;
    };

    let state = tool_calls.entry(key.clone()).or_default();
    if let Some(call_id) = item.call_id {
        state.tool_call_id = Some(call_id);
    }
    if let Some(function_name) = item.name {
        state.function_name = Some(function_name);
    }

    if let Some(arguments_delta) = item.arguments.filter(|value| !value.is_empty()) {
        state.arguments_json.push_str(&arguments_delta);
        let tool_call_id = state.tool_call_id.clone().unwrap_or_else(|| key.clone());

        if tx
            .send(ProviderStreamEvent::ToolCallDelta {
                tool_call_id,
                function_name: state.function_name.clone(),
                arguments_delta,
            })
            .await
            .is_err()
        {
            return false;
        }
    }

    true
}

async fn handle_responses_arguments_delta(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    tool_calls: &mut BTreeMap<String, ResponsesToolCallAccumulator>,
    event: OpenAiResponsesEvent,
) -> bool {
    let Some(item_id) = event.item_id else {
        let _ = tx
            .send(ProviderStreamEvent::Error {
                message:
                    "openai_compatible responses function_call_arguments.delta missing item_id"
                        .to_string(),
            })
            .await;
        return false;
    };

    let Some(arguments_delta) = event.delta.filter(|value| !value.is_empty()) else {
        return true;
    };

    let state_key =
        find_responses_tool_call_key(tool_calls, &item_id).unwrap_or_else(|| item_id.clone());
    let state = tool_calls.entry(state_key.clone()).or_default();
    state.arguments_json.push_str(&arguments_delta);

    let tool_call_id = state
        .tool_call_id
        .clone()
        .unwrap_or_else(|| state_key.clone());

    tx.send(ProviderStreamEvent::ToolCallDelta {
        tool_call_id,
        function_name: None,
        arguments_delta,
    })
    .await
    .is_ok()
}

async fn handle_responses_tool_item_done(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    tool_calls: &mut BTreeMap<String, ResponsesToolCallAccumulator>,
    event: OpenAiResponsesEvent,
) -> bool {
    let Some(item) = event.item else {
        return true;
    };

    if item.item_type != "function_call" {
        return true;
    }

    let Some(key) = item
        .id
        .clone()
        .or_else(|| item.call_id.clone())
        .or(event.item_id)
    else {
        let _ = tx
            .send(ProviderStreamEvent::Error {
                message:
                    "openai_compatible responses tool completion missing both item id and call id"
                        .to_string(),
            })
            .await;
        return false;
    };

    let state_key = find_responses_tool_call_key(tool_calls, &key).unwrap_or(key);
    let state = tool_calls.entry(state_key.clone()).or_default();

    if let Some(call_id) = item.call_id {
        state.tool_call_id = Some(call_id);
    }
    if let Some(function_name) = item.name {
        state.function_name = Some(function_name);
    }
    if let Some(arguments_json) = item.arguments.filter(|value| !value.is_empty()) {
        state.arguments_json = arguments_json;
    }

    let Some(completed_state) = tool_calls.remove(&state_key) else {
        return true;
    };

    if let Err(message) = emit_responses_tool_call_complete(tx, &state_key, completed_state).await {
        let _ = tx.send(ProviderStreamEvent::Error { message }).await;
        return false;
    }

    true
}

fn find_responses_tool_call_key(
    tool_calls: &BTreeMap<String, ResponsesToolCallAccumulator>,
    item_or_call_id: &str,
) -> Option<String> {
    if tool_calls.contains_key(item_or_call_id) {
        return Some(item_or_call_id.to_string());
    }

    tool_calls.iter().find_map(|(key, state)| {
        (state.tool_call_id.as_deref() == Some(item_or_call_id)).then(|| key.clone())
    })
}

async fn emit_responses_tool_call_complete(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    state_key: &str,
    state: ResponsesToolCallAccumulator,
) -> Result<(), String> {
    let tool_call_id = state.tool_call_id.unwrap_or_else(|| state_key.to_string());

    let Some(function_name) = state.function_name.filter(|value| !value.is_empty()) else {
        return Err(format!(
            "openai_compatible responses tool call `{tool_call_id}` missing function name"
        ));
    };

    let arguments_json = normalize_responses_arguments_json(state.arguments_json);
    serde_json::from_str::<serde_json::Value>(&arguments_json).map_err(|err| {
        format!(
            "openai_compatible responses tool call `{tool_call_id}` has malformed arguments JSON: {err}"
        )
    })?;

    tx.send(ProviderStreamEvent::ToolCallComplete {
        tool_call_id,
        function_name,
        arguments_json,
    })
    .await
    .map_err(|_| {
        "openai_compatible stream receiver closed while sending tool completion".to_string()
    })
}

fn normalize_responses_arguments_json(arguments_json: String) -> String {
    if arguments_json.trim().is_empty() {
        "{}".to_string()
    } else {
        arguments_json
    }
}

fn summarize_sse_data(data: &str) -> String {
    let mut snippet = data
        .chars()
        .take(160)
        .collect::<String>()
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    if data.chars().count() > 160 {
        snippet.push('…');
    }
    snippet
}

fn parse_headers(
    headers: &BTreeMap<String, String>,
) -> Result<HeaderMap, OpenAiCompatibleProviderError> {
    let mut parsed = HeaderMap::new();

    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|source| {
            OpenAiCompatibleProviderError::InvalidHeaderName {
                header: name.clone(),
                source,
            }
        })?;

        let header_value = HeaderValue::from_str(value).map_err(|source| {
            OpenAiCompatibleProviderError::InvalidHeaderValue {
                header: name.clone(),
                source,
            }
        })?;

        parsed.insert(header_name, header_value);
    }

    Ok(parsed)
}

fn zero_usage() -> CompletionUsage {
    CompletionUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatCompletionsRequest {
    model: String,
    messages: Vec<OpenAiChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_verbosity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolChoice>,
    stream: bool,
}

impl From<CompletionRequest> for OpenAiChatCompletionsRequest {
    fn from(request: CompletionRequest) -> Self {
        let CompletionRequest {
            provider_id: _,
            model_id,
            messages,
            temperature,
            max_tokens,
            variant: _,
            reasoning_effort,
            text_verbosity,
            reasoning_summary: _,
            tools,
            tool_choice,
            stream,
        } = request;

        Self {
            model: model_id,
            messages: messages.into_iter().map(Into::into).collect(),
            temperature,
            max_tokens,
            reasoning_effort,
            text_verbosity,
            tools: map_tools(tools),
            tool_choice,
            stream,
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiChatMessageToolCall>>,
}

impl From<CompletionMessage> for OpenAiChatMessage {
    fn from(message: CompletionMessage) -> Self {
        let CompletionMessage {
            role,
            content,
            name,
            tool_call_id,
            assistant_tool_calls,
        } = message;

        let tool_calls = assistant_tool_calls
            .filter(|calls| !calls.is_empty())
            .map(|calls| {
                calls
                    .into_iter()
                    .map(|call| OpenAiChatMessageToolCall {
                        id: call.tool_call_id,
                        kind: "function",
                        function: OpenAiChatMessageToolCallFunction {
                            name: call.function_name,
                            arguments: call.arguments_json,
                        },
                    })
                    .collect()
            });

        Self {
            role: openai_role(&role),
            content,
            name,
            tool_call_id,
            tool_calls,
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatMessageToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiChatMessageToolCallFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiChatMessageToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAiChatTool {
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiChatToolFunction,
}

impl From<ToolDef> for OpenAiChatTool {
    fn from(tool: ToolDef) -> Self {
        Self {
            kind: "function",
            function: OpenAiChatToolFunction {
                name: tool.function_name,
                description: tool.description,
                parameters: tool.parameters,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatToolFunction {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesRequest {
    model: String,
    input: Vec<OpenAiResponsesInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<OpenAiResponsesReasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<OpenAiResponsesText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolChoice>,
    stream: bool,
}

impl From<CompletionRequest> for OpenAiResponsesRequest {
    fn from(request: CompletionRequest) -> Self {
        let CompletionRequest {
            provider_id: _,
            model_id,
            messages,
            temperature,
            max_tokens,
            variant: _,
            reasoning_effort,
            text_verbosity,
            reasoning_summary,
            tools,
            tool_choice,
            stream,
        } = request;

        Self {
            model: model_id,
            input: serialize_responses_input(messages),
            temperature,
            max_output_tokens: max_tokens,
            reasoning: (reasoning_effort.is_some() || reasoning_summary.is_some()).then_some(
                OpenAiResponsesReasoning {
                    effort: reasoning_effort,
                    summary: reasoning_summary,
                },
            ),
            text: text_verbosity.map(|verbosity| OpenAiResponsesText { verbosity }),
            tools: map_tools(tools),
            tool_choice,
            stream,
        }
    }
}

fn serialize_responses_input(messages: Vec<CompletionMessage>) -> Vec<OpenAiResponsesInputItem> {
    messages
        .into_iter()
        .flat_map(OpenAiResponsesInputItem::from_completion_message)
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OpenAiResponsesInputItem {
    Message {
        role: String,
        content: Vec<OpenAiResponsesContentItem>,
    },
    FunctionCall {
        #[serde(rename = "type")]
        item_type: &'static str,
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        #[serde(rename = "type")]
        item_type: &'static str,
        call_id: String,
        output: String,
    },
}

impl OpenAiResponsesInputItem {
    fn from_completion_message(message: CompletionMessage) -> Vec<Self> {
        let CompletionMessage {
            role,
            content,
            name: _,
            tool_call_id,
            assistant_tool_calls,
        } = message;

        if matches!(role, MessageRole::Tool) {
            return vec![Self::FunctionCallOutput {
                item_type: "function_call_output",
                call_id: tool_call_id.unwrap_or_default(),
                output: content,
            }];
        }

        let item_type = match role {
            MessageRole::Assistant => "output_text",
            MessageRole::System | MessageRole::User => "input_text",
            MessageRole::Tool => unreachable!("tool messages handled above"),
        };

        let mut items = vec![Self::Message {
            role: openai_role(&role),
            content: vec![OpenAiResponsesContentItem {
                item_type: item_type.to_string(),
                text: content,
            }],
        }];

        if matches!(role, MessageRole::Assistant) {
            if let Some(tool_calls) = assistant_tool_calls {
                for tool_call in tool_calls {
                    items.push(Self::FunctionCall {
                        item_type: "function_call",
                        call_id: tool_call.tool_call_id,
                        name: tool_call.function_name,
                        arguments: tool_call.arguments_json,
                    });
                }
            }
        }

        items
    }
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesContentItem {
    #[serde(rename = "type")]
    item_type: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesText {
    verbosity: String,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesTool {
    #[serde(rename = "type")]
    kind: &'static str,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parameters: serde_json::Value,
}

impl From<ToolDef> for OpenAiResponsesTool {
    fn from(tool: ToolDef) -> Self {
        Self {
            kind: "function",
            name: tool.function_name,
            description: tool.description.filter(|value| !value.trim().is_empty()),
            parameters: tool.parameters,
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<String>,
    #[serde(default)]
    item_id: Option<String>,
    #[serde(default)]
    item: Option<OpenAiResponsesOutputItem>,
    #[serde(default)]
    response: Option<OpenAiResponsesResponsePayload>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesOutputItem {
    #[serde(rename = "type")]
    item_type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesResponsePayload {
    #[serde(default)]
    usage: Option<OpenAiResponsesUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
    #[serde(default)]
    total_tokens: Option<u32>,
}

impl OpenAiResponsesUsage {
    fn into_completion_usage(self) -> CompletionUsage {
        let prompt_tokens = self.prompt_tokens.or(self.input_tokens).unwrap_or(0);
        let completion_tokens = self.completion_tokens.or(self.output_tokens).unwrap_or(0);
        let total_tokens = self
            .total_tokens
            .unwrap_or(prompt_tokens.saturating_add(completion_tokens));

        CompletionUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        }
    }
}

#[derive(Debug, Default)]
struct ResponsesToolCallAccumulator {
    tool_call_id: Option<String>,
    function_name: Option<String>,
    arguments_json: String,
}

fn role_to_openai(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChatCompletionsChunk {
    #[serde(default)]
    choices: Vec<OpenAiChatChoiceChunk>,
    #[serde(default)]
    usage: Option<CompletionUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatChoiceChunk {
    #[serde(default)]
    delta: OpenAiChatDeltaChunk,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiChatDeltaChunk {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_text: Option<String>,
    #[serde(default)]
    tool_calls: Vec<OpenAiChatToolCallDeltaChunk>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatToolCallDeltaChunk {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAiChatToolFunctionDeltaChunk>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiChatToolFunctionDeltaChunk {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, env, fs, path::PathBuf, time::Duration};

    use serde::Deserialize;
    use serde_json::json;
    use tokio::time::timeout;
    use tokio_stream::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::{
        OpenAiApiMode, OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig,
        OpenAiResponsesRequest,
    };
    use crate::{
        CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
        ProviderStreamEvent, ToolChoice, ToolDef,
    };

    #[tokio::test]
    async fn openai_compatible_offline_wiremock_parses_sse_deltas() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(deterministic_sse_transcript(), "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = provider_for_base_url(format!("{}/v1", server.uri()), "test-secret-key");
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("Hello".to_string()),
                ProviderStreamEvent::TextDelta(" world".to_string()),
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 4,
                        completion_tokens: 2,
                        total_tokens: 6,
                    }
                },
            ]
        );

        let requests = server
            .received_requests()
            .await
            .expect("request recording must be enabled");
        assert_eq!(requests.len(), 1);

        let request = &requests[0];
        let authorization = request
            .headers
            .get("authorization")
            .expect("authorization header")
            .to_str()
            .expect("authorization header is utf-8");
        assert_eq!(authorization, "Bearer test-secret-key");

        let body: serde_json::Value = request.body_json().expect("request body must be JSON");
        assert_eq!(body.get("stream"), Some(&serde_json::Value::Bool(true)));
        assert_eq!(
            body.get("model"),
            Some(&serde_json::Value::String("gpt-4o-mini".to_string()))
        );
        assert!(body.get("api_key").is_none());
    }

    #[tokio::test]
    async fn openai_responses_offline_wiremock_streams_tool_call_complete() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(responses_tool_call_sse_transcript(), "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = provider_for_base_url_with_mode(
            format!("{}/v1", server.uri()),
            "test-secret-key",
            OpenAiApiMode::Responses,
        );
        let events = collect_events(&provider, request_with_single_tool("gpt-4o-mini")).await;

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::ToolCallDelta {
                    tool_call_id: "call_resp_1".to_string(),
                    function_name: Some("filesystem_read".to_string()),
                    arguments_delta: "{\"filePath\":\"/tmp".to_string(),
                },
                ProviderStreamEvent::ToolCallDelta {
                    tool_call_id: "call_resp_1".to_string(),
                    function_name: None,
                    arguments_delta: "/demo.txt\"}".to_string(),
                },
                ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "call_resp_1".to_string(),
                    function_name: "filesystem_read".to_string(),
                    arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
                },
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 9,
                        completion_tokens: 3,
                        total_tokens: 12,
                    }
                },
            ]
        );

        let requests = server
            .received_requests()
            .await
            .expect("request recording must be enabled");
        assert_eq!(requests.len(), 1);

        let authorization = requests[0]
            .headers
            .get("authorization")
            .expect("authorization header")
            .to_str()
            .expect("authorization header is utf-8");
        assert_eq!(authorization, "Bearer test-secret-key");

        let body: serde_json::Value = requests[0].body_json().expect("request body must be JSON");
        assert_eq!(
            body.get("tool_choice"),
            Some(&serde_json::Value::String("auto".to_string()))
        );

        let tools = body
            .get("tools")
            .and_then(|value| value.as_array())
            .expect("responses tools array should be serialized");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].get("type"),
            Some(&serde_json::Value::String("function".to_string()))
        );
        assert_eq!(
            tools[0].get("name"),
            Some(&serde_json::Value::String("filesystem_read".to_string()))
        );
        assert!(tools[0].get("function").is_none());
    }

    #[tokio::test]
    async fn openai_auto_loopback_falls_back_to_chat_completions_on_400() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(deterministic_sse_transcript(), "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = provider_for_base_url_with_mode(
            format!("{}/v1", server.uri()),
            "test-secret-key",
            OpenAiApiMode::Auto,
        );
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("Hello".to_string()),
                ProviderStreamEvent::TextDelta(" world".to_string()),
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 4,
                        completion_tokens: 2,
                        total_tokens: 6,
                    }
                },
            ]
        );

        let requests = server
            .received_requests()
            .await
            .expect("request recording must be enabled");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url.path(), "/v1/responses");
        assert_eq!(requests[1].url.path(), "/v1/chat/completions");
    }

    #[test]
    fn openai_responses_request_replays_assistant_tool_call_before_function_call_output() {
        let request = CompletionRequest {
            provider_id: None,
            model_id: "gpt-4o-mini".to_string(),
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: "sys".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: "Use a tool".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::Assistant,
                    content: "calling tool".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: Some(vec![crate::AssistantToolCall {
                        tool_call_id: "call_1".to_string(),
                        function_name: "filesystem_read".to_string(),
                        arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
                    }]),
                },
                CompletionMessage {
                    role: MessageRole::Tool,
                    content: "{\"display_text\":\"ok\"}".to_string(),
                    name: Some("filesystem_read".to_string()),
                    tool_call_id: Some("call_1".to_string()),
                    assistant_tool_calls: None,
                },
            ],
            temperature: Some(0.0),
            max_tokens: None,
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            tools: None,
            tool_choice: None,
            stream: true,
        };

        let body = serde_json::to_value(OpenAiResponsesRequest::from(request))
            .expect("serialize responses request");
        let input = body
            .get("input")
            .and_then(serde_json::Value::as_array)
            .expect("responses request input array");

        assert!(input
            .iter()
            .take(3)
            .all(|item| item.get("role").is_some() && item.get("content").is_some()));
        assert_eq!(
            input[0]
                .get("content")
                .and_then(serde_json::Value::as_array)
                .and_then(|content| content.first())
                .and_then(|item| item.get("type")),
            Some(&serde_json::Value::String("input_text".to_string()))
        );
        assert_eq!(
            input[1]
                .get("content")
                .and_then(serde_json::Value::as_array)
                .and_then(|content| content.first())
                .and_then(|item| item.get("type")),
            Some(&serde_json::Value::String("input_text".to_string()))
        );
        assert_eq!(
            input[2]
                .get("content")
                .and_then(serde_json::Value::as_array)
                .and_then(|content| content.first())
                .and_then(|item| item.get("type")),
            Some(&serde_json::Value::String("output_text".to_string()))
        );

        let function_call_index = input
            .iter()
            .position(|item| {
                item.get("type") == Some(&serde_json::Value::String("function_call".to_string()))
            })
            .expect("assistant tool call should replay as function_call item");
        let function_call_output_index = input
            .iter()
            .position(|item| {
                item.get("type")
                    == Some(&serde_json::Value::String(
                        "function_call_output".to_string(),
                    ))
            })
            .expect("tool result should serialize as function_call_output item");

        assert!(
            function_call_index < function_call_output_index,
            "function_call replay must precede function_call_output"
        );
        assert_eq!(
            input[function_call_index].get("call_id"),
            Some(&serde_json::Value::String("call_1".to_string()))
        );
        assert_eq!(
            input[function_call_index].get("name"),
            Some(&serde_json::Value::String("filesystem_read".to_string()))
        );
        assert_eq!(
            input[function_call_index].get("arguments"),
            Some(&serde_json::Value::String(
                "{\"filePath\":\"/tmp/demo.txt\"}".to_string()
            ))
        );
        assert_eq!(
            input[function_call_output_index].get("call_id"),
            Some(&serde_json::Value::String("call_1".to_string()))
        );
        assert_eq!(
            input[function_call_output_index].get("output"),
            Some(&serde_json::Value::String(
                "{\"display_text\":\"ok\"}".to_string()
            ))
        );
    }

    #[test]
    fn openai_chat_request_replays_assistant_tool_call_in_tool_calls_field() {
        let request = CompletionRequest {
            provider_id: None,
            model_id: "gpt-4o-mini".to_string(),
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: "sys".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: "Use a tool".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::Assistant,
                    content: "calling tool".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: Some(vec![crate::AssistantToolCall {
                        tool_call_id: "call_1".to_string(),
                        function_name: "filesystem_read".to_string(),
                        arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
                    }]),
                },
                CompletionMessage {
                    role: MessageRole::Tool,
                    content: "{\"display_text\":\"ok\"}".to_string(),
                    name: Some("filesystem_read".to_string()),
                    tool_call_id: Some("call_1".to_string()),
                    assistant_tool_calls: None,
                },
            ],
            temperature: Some(0.0),
            max_tokens: None,
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            tools: None,
            tool_choice: None,
            stream: true,
        };

        let body = serde_json::to_value(super::OpenAiChatCompletionsRequest::from(request))
            .expect("serialize chat request");
        let messages = body
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .expect("chat request messages array");

        let assistant = &messages[2];
        let tool_calls = assistant
            .get("tool_calls")
            .and_then(serde_json::Value::as_array)
            .expect("assistant message should include tool_calls replay");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(
            tool_calls[0].get("id"),
            Some(&serde_json::Value::String("call_1".to_string()))
        );
        assert_eq!(
            tool_calls[0].get("type"),
            Some(&serde_json::Value::String("function".to_string()))
        );
        assert_eq!(
            tool_calls[0]
                .get("function")
                .and_then(|value| value.get("name")),
            Some(&serde_json::Value::String("filesystem_read".to_string()))
        );
        assert_eq!(
            tool_calls[0]
                .get("function")
                .and_then(|value| value.get("arguments")),
            Some(&serde_json::Value::String(
                "{\"filePath\":\"/tmp/demo.txt\"}".to_string()
            ))
        );

        let tool_message = &messages[3];
        assert_eq!(
            tool_message.get("tool_call_id"),
            Some(&serde_json::Value::String("call_1".to_string()))
        );
    }

    #[tokio::test]
    async fn openai_responses_offline_wiremock_malformed_args_fail_closed() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(
                        responses_malformed_tool_args_sse_transcript(),
                        "text/event-stream",
                    ),
            )
            .mount(&server)
            .await;

        let provider = provider_for_base_url_with_mode(
            format!("{}/v1", server.uri()),
            "test-secret-key",
            OpenAiApiMode::Responses,
        );
        let events = collect_events(&provider, request_with_single_tool("gpt-4o-mini")).await;

        assert!(matches!(events.first(), Some(ProviderStreamEvent::Start)));
        assert!(events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::ToolCallDelta { .. })));
        assert!(events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::Error { message } if message.contains("malformed arguments JSON"))));
        assert!(!events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::ToolCallComplete { .. })));
        assert!(!events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::Done { .. })));
    }

    #[tokio::test]
    async fn openai_compatible_offline_wiremock_streams_chat_tool_calls() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(tool_call_sse_transcript(), "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = provider_for_base_url(format!("{}/v1", server.uri()), "test-secret-key");
        let events = collect_events(&provider, request_with_single_tool("gpt-4o-mini")).await;

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::ToolCallDelta {
                    tool_call_id: "call_1".to_string(),
                    function_name: Some("filesystem_read".to_string()),
                    arguments_delta: "{\"filePath\":\"".to_string(),
                },
                ProviderStreamEvent::ToolCallDelta {
                    tool_call_id: "call_1".to_string(),
                    function_name: None,
                    arguments_delta: "/tmp/demo.txt\"}".to_string(),
                },
                ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "call_1".to_string(),
                    function_name: "filesystem_read".to_string(),
                    arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
                },
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 12,
                        completion_tokens: 4,
                        total_tokens: 16,
                    }
                },
            ]
        );

        let requests = server
            .received_requests()
            .await
            .expect("request recording must be enabled");
        assert_eq!(requests.len(), 1);

        let body: serde_json::Value = requests[0].body_json().expect("request body must be JSON");
        assert_eq!(
            body.get("tool_choice"),
            Some(&serde_json::Value::String("auto".to_string()))
        );

        let tools = body
            .get("tools")
            .and_then(|value| value.as_array())
            .expect("tools array should be serialized");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].get("type"),
            Some(&serde_json::Value::String("function".to_string()))
        );
        assert_eq!(
            tools[0].get("function").and_then(|value| value.get("name")),
            Some(&serde_json::Value::String("filesystem_read".to_string()))
        );
    }

    #[tokio::test]
    async fn openai_compatible_offline_wiremock_chat_tool_calls_fail_closed_on_invalid_arguments() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(malformed_tool_call_sse_transcript(), "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = provider_for_base_url(format!("{}/v1", server.uri()), "test-secret-key");
        let events = collect_events(&provider, request_with_single_tool("gpt-4o-mini")).await;

        assert!(matches!(events.first(), Some(ProviderStreamEvent::Start)));
        assert!(events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::ToolCallDelta { .. })));
        assert!(events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::Error { .. })));
        assert!(!events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::ToolCallComplete { .. })));
        assert!(!events
            .iter()
            .any(|event| matches!(event, ProviderStreamEvent::Done { .. })));
    }

    #[tokio::test]
    async fn openai_compatible_errors_do_not_leak_auth_secrets() {
        let server = MockServer::start().await;
        let api_key = "test-secret-key";

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_string(format!("Authorization: Bearer {api_key} should never leak")),
            )
            .mount(&server)
            .await;

        let provider = provider_for_base_url(format!("{}/v1", server.uri()), api_key);
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

        assert_eq!(events.len(), 1);
        let ProviderStreamEvent::Error { message } = &events[0] else {
            panic!("expected an error event for non-success response")
        };

        assert!(message.contains("status 401"));
        assert!(!message.contains(api_key));
        assert!(!message.to_ascii_lowercase().contains("authorization"));
    }

    #[tokio::test]
    async fn openai_compatible_errors_include_response_body_detail() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {
                    "message": "Invalid schema for function 'plan_exit': object schema missing properties"
                }
            })))
            .mount(&server)
            .await;

        let provider = provider_for_base_url(format!("{}/v1", server.uri()), "test-secret-key");
        let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

        assert_eq!(events.len(), 1);
        let ProviderStreamEvent::Error { message } = &events[0] else {
            panic!("expected an error event for non-success response")
        };

        assert!(message.contains("status 400"));
        assert!(message.contains("Invalid schema for function 'plan_exit'"));
        assert!(message.contains("object schema missing properties"));
    }

    #[tokio::test]
    #[ignore = "requires HARNESS_LIVE_PROXY=1 and local proxy access"]
    async fn openai_compatible_live_proxy_config_file_smoke() {
        if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
            return;
        }

        let config_path = env::var("HARNESS_LIVE_PROXY_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_live_config_path());
        let provider_name =
            env::var("HARNESS_LIVE_PROXY_PROVIDER").unwrap_or_else(|_| "default".to_string());

        let live_config = load_live_config(&config_path).unwrap_or_else(|err| panic!("{err}"));

        let provider_config = live_config
            .providers
            .get(&provider_name)
            .unwrap_or_else(|| panic!("provider `{provider_name}` missing in live config"));

        assert_eq!(provider_config.provider_type, "openai_compatible");

        let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
            base_url: provider_config.base_url.clone(),
            api_key: resolve_env_reference(&provider_config.api_key),
            api_mode: provider_config.api_mode,
            timeout_ms: provider_config.timeout_ms,
            headers: provider_config.headers.clone(),
        })
        .expect("build live provider");

        let model_id = env::var("HARNESS_LIVE_PROXY_MODEL")
            .ok()
            .or_else(|| provider_config.models.keys().next().cloned())
            .expect("HARNESS_LIVE_PROXY_MODEL or at least one configured model is required");

        let mut stream = provider.stream_completion(basic_request(&model_id)).await;

        let mut saw_start = false;
        let mut saw_done = false;
        let mut delta_chars = 0usize;

        timeout(Duration::from_secs(45), async {
            while let Some(event) = stream.next().await {
                match event {
                    ProviderStreamEvent::Start => saw_start = true,
                    ProviderStreamEvent::ReasoningDelta(_) => {}
                    ProviderStreamEvent::TextDelta(delta) => {
                        delta_chars += delta.len();
                    }
                    ProviderStreamEvent::ToolCallDelta { .. }
                    | ProviderStreamEvent::ToolCallComplete { .. } => {}
                    ProviderStreamEvent::Done { .. } => {
                        saw_done = true;
                        break;
                    }
                    ProviderStreamEvent::Error { message } => {
                        panic!("live proxy returned provider error: {message}")
                    }
                }
            }
        })
        .await
        .expect("timed out waiting for live SSE response");

        assert!(saw_start, "expected a start event");
        assert!(saw_done, "expected a done event");
        assert!(delta_chars > 0, "expected at least one text delta");
    }

    fn provider_for_base_url(base_url: String, api_key: &str) -> OpenAiCompatibleProvider {
        provider_for_base_url_with_mode(base_url, api_key, OpenAiApiMode::ChatCompletions)
    }

    fn provider_for_base_url_with_mode(
        base_url: String,
        api_key: &str,
        api_mode: OpenAiApiMode,
    ) -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
            base_url,
            api_key: api_key.to_string(),
            api_mode,
            timeout_ms: 15_000,
            headers: std::collections::BTreeMap::new(),
        })
        .expect("build provider")
    }

    fn basic_request(model_id: &str) -> CompletionRequest {
        CompletionRequest {
            provider_id: None,
            model_id: model_id.to_string(),
            messages: vec![CompletionMessage {
                role: MessageRole::User,
                content: "Say hello from test".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            }],
            temperature: Some(0.0),
            max_tokens: Some(32),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            tools: None,
            tool_choice: None,
            stream: true,
        }
    }

    fn request_with_single_tool(model_id: &str) -> CompletionRequest {
        CompletionRequest {
            provider_id: None,
            model_id: model_id.to_string(),
            messages: vec![CompletionMessage {
                role: MessageRole::User,
                content: "Read /tmp/demo.txt".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            }],
            temperature: Some(0.0),
            max_tokens: Some(64),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            tools: Some(vec![ToolDef {
                tool_id: "fs.read".to_string(),
                function_name: "filesystem_read".to_string(),
                description: Some("Read file content".to_string()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "filePath": {"type": "string"}
                    },
                    "required": ["filePath"],
                    "additionalProperties": false
                }),
            }]),
            tool_choice: Some(ToolChoice::Auto),
            stream: true,
        }
    }

    async fn collect_events(
        provider: &OpenAiCompatibleProvider,
        request: CompletionRequest,
    ) -> Vec<ProviderStreamEvent> {
        provider.stream_completion(request).await.collect().await
    }

    fn deterministic_sse_transcript() -> String {
        concat!(
            "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}],\"usage\":null}\n\n",
            "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}],\"usage\":null}\n\n",
            "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
    }

    fn tool_call_sse_transcript() -> String {
        concat!(
            "data: {\"id\":\"chatcmpl-tool-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"filesystem_read\",\"arguments\":\"{\\\"filePath\\\":\\\"\"}}]},\"finish_reason\":null}],\"usage\":null}\n\n",
            "data: {\"id\":\"chatcmpl-tool-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"/tmp/demo.txt\\\"}\"}}]},\"finish_reason\":null}],\"usage\":null}\n\n",
            "data: {\"id\":\"chatcmpl-tool-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":4,\"total_tokens\":16}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
    }

    fn malformed_tool_call_sse_transcript() -> String {
        concat!(
            "data: {\"id\":\"chatcmpl-tool-2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_bad\",\"type\":\"function\",\"function\":{\"name\":\"filesystem_read\",\"arguments\":\"{\\\"filePath\\\":\"}}]},\"finish_reason\":null}],\"usage\":null}\n\n",
            "data: {\"id\":\"chatcmpl-tool-2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":3,\"total_tokens\":14}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
    }

    fn responses_tool_call_sse_transcript() -> String {
        concat!(
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_resp_1\",\"call_id\":\"call_resp_1\",\"name\":\"filesystem_read\",\"arguments\":\"{\\\"filePath\\\":\\\"/tmp\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_resp_1\",\"delta\":\"/demo.txt\\\"}\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_resp_1\",\"call_id\":\"call_resp_1\",\"name\":\"filesystem_read\",\"arguments\":\"{\\\"filePath\\\":\\\"/tmp/demo.txt\\\"}\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":9,\"output_tokens\":3,\"total_tokens\":12}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
    }

    fn responses_malformed_tool_args_sse_transcript() -> String {
        concat!(
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_resp_bad\",\"call_id\":\"call_resp_bad\",\"name\":\"filesystem_read\",\"arguments\":\"{\\\"filePath\\\":\\\"/tmp\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_resp_bad\",\"delta\":\"/demo.txt\\\"\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":9,\"output_tokens\":3,\"total_tokens\":12}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string()
    }

    fn default_live_config_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("configs")
            .join("harness.example.jsonc")
    }

    fn load_live_config(path: &PathBuf) -> Result<LiveHarnessConfig, String> {
        let raw = fs::read_to_string(path)
            .map_err(|err| format!("failed reading live config {}: {err}", path.display()))?;
        json5::from_str(&raw)
            .map_err(|err| format!("failed parsing live config {}: {err}", path.display()))
    }

    fn resolve_env_reference(value: &str) -> String {
        if !(value.starts_with("${") && value.ends_with('}')) {
            return value.to_string();
        }

        let reference = &value[2..value.len() - 1];
        if reference.is_empty() {
            return value.to_string();
        }

        if let Some((key, fallback)) = reference.split_once(":-") {
            if key.is_empty() {
                return value.to_string();
            }
            return env::var(key)
                .ok()
                .filter(|resolved| !resolved.is_empty())
                .unwrap_or_else(|| fallback.to_string());
        }

        env::var(reference).unwrap_or_else(|_| value.to_string())
    }

    #[allow(unsafe_code)]
    fn with_env_var_state<T>(name: &str, value: Option<&str>, run: impl FnOnce() -> T) -> T {
        let previous = env::var_os(name);

        match value {
            Some(value) => unsafe { env::set_var(name, value) },
            None => unsafe { env::remove_var(name) },
        }

        let result = run();

        match previous {
            Some(value) => unsafe { env::set_var(name, value) },
            None => unsafe { env::remove_var(name) },
        }

        result
    }

    #[test]
    fn live_smoke_env_reference_supports_default_fallback_syntax() {
        with_env_var_state("HARNESS_PROVIDER_TEST_API_KEY", None, || {
            assert_eq!(
                resolve_env_reference("${HARNESS_PROVIDER_TEST_API_KEY:-sk-zerolimit}"),
                "sk-zerolimit"
            );
        });

        with_env_var_state("HARNESS_PROVIDER_TEST_API_KEY", Some(""), || {
            assert_eq!(
                resolve_env_reference("${HARNESS_PROVIDER_TEST_API_KEY:-sk-zerolimit}"),
                "sk-zerolimit"
            );
        });

        with_env_var_state("HARNESS_PROVIDER_TEST_API_KEY", Some("real-key"), || {
            assert_eq!(
                resolve_env_reference("${HARNESS_PROVIDER_TEST_API_KEY:-sk-zerolimit}"),
                "real-key"
            );
        });
    }

    fn default_timeout_ms() -> u64 {
        60_000
    }

    fn default_api_mode() -> OpenAiApiMode {
        OpenAiApiMode::Auto
    }

    #[derive(Debug, Deserialize)]
    struct LiveHarnessConfig {
        providers: BTreeMap<String, LiveProviderConfig>,
    }

    #[derive(Debug, Deserialize)]
    struct LiveProviderConfig {
        #[serde(rename = "type")]
        provider_type: String,
        #[serde(alias = "baseUrl")]
        base_url: String,
        #[serde(alias = "apiKey")]
        api_key: String,
        #[serde(default = "default_timeout_ms", alias = "timeoutMs")]
        timeout_ms: u64,
        #[serde(default = "default_api_mode", alias = "apiMode")]
        api_mode: OpenAiApiMode,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        models: BTreeMap<String, serde_json::Value>,
    }
}

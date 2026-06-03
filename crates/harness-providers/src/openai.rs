use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::{self as stream, Stream, StreamExt};

use crate::{
    CompletionRequest, CompletionUsage, Provider, ProviderBearerToken, ProviderCredentialKind,
    ProviderCredentialSource, ProviderErrorCategory, ProviderEventStream, ProviderRequestContext,
    ProviderStreamEvent, ProviderStreamStartMetadata,
};

mod endpoint;
mod error;
mod header;
mod request;
mod sse;
mod stream_event;
mod stream_payload;
mod tool_call;

pub use self::endpoint::{CODEX_API_ENDPOINT, COPILOT_API_BASE};

use self::endpoint::{
    apply_codex_gpt5_response_defaults, chat_completions_endpoint, copilot_base_url,
    is_loopback_base_url, responses_endpoint, rewrite_codex_endpoint, rewrite_endpoint_base,
    supports_long_prompt_cache_retention,
};
use self::error::{
    categorize_non_success_status, format_non_success_status_message, format_transport_error,
};
use self::header::{insert_static_header, parse_headers, remove_header_case_insensitive};
use self::request::{OpenAiChatCompletionsRequest, OpenAiResponsesRequest};
use self::sse::{collect_body_text, next_sse_event};
use self::stream_event::{
    malformed_stream_error, non_empty_finished_metadata,
    provider_stream_finished_metadata_from_start, provider_stream_start_metadata_from_headers,
    transport_failure_error, unsupported_tool_call_error,
};
use self::stream_payload::{OpenAiChatCompletionsChunk, OpenAiResponsesEvent};
use self::tool_call::{
    consume_tool_call_deltas, emit_pending_responses_tool_call_completions,
    emit_tool_call_completions, handle_responses_arguments_delta, handle_responses_tool_item_added,
    handle_responses_tool_item_done, ChatToolCallState, ResponsesToolCallState,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiAuthProfile {
    Codex,
    GithubCopilot,
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

#[derive(Clone)]
pub struct OpenAiCompatibleProvider {
    transport: Arc<dyn OpenAiHttpTransport>,
    base_url: String,
    api_key: String,
    credential_source: Option<Arc<dyn ProviderCredentialSource>>,
    auth_profile: Option<OpenAiAuthProfile>,
    api_mode: OpenAiApiMode,
    headers: HeaderMap,
}

pub type OpenAiResponseBody = Pin<Box<dyn Stream<Item = Result<Vec<u8>, String>> + Send>>;

pub struct OpenAiHttpResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: OpenAiResponseBody,
}

impl OpenAiHttpResponse {
    pub fn new(status: u16, headers: HeaderMap, body: OpenAiResponseBody) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    pub fn text(status: u16, headers: HeaderMap, body: impl Into<String>) -> Self {
        Self::new(
            status,
            headers,
            Box::pin(stream::iter(vec![Ok(body.into().into_bytes())])),
        )
    }
}

#[async_trait]
pub trait OpenAiHttpTransport: Send + Sync {
    async fn post_json(
        &self,
        endpoint: String,
        headers: HeaderMap,
        bearer_token: String,
        body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String>;
}

#[derive(Debug, Clone)]
struct ReqwestOpenAiHttpTransport {
    client: reqwest::Client,
}

#[async_trait]
impl OpenAiHttpTransport for ReqwestOpenAiHttpTransport {
    async fn post_json(
        &self,
        endpoint: String,
        headers: HeaderMap,
        bearer_token: String,
        body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String> {
        let response = self
            .client
            .post(endpoint)
            .headers(headers)
            .bearer_auth(bearer_token)
            .json(&body)
            .send()
            .await
            .map_err(format_transport_error)?;

        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.bytes_stream().map(|chunk| {
            chunk
                .map(|bytes| bytes.to_vec())
                .map_err(format_transport_error)
        });

        Ok(OpenAiHttpResponse::new(status, headers, Box::pin(body)))
    }
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
            transport: Arc::new(ReqwestOpenAiHttpTransport { client }),
            base_url: config.base_url,
            api_key: config.api_key,
            credential_source: None,
            auth_profile: None,
            api_mode: config.api_mode,
            headers,
        })
    }

    pub fn with_transport(
        config: OpenAiCompatibleProviderConfig,
        transport: Arc<dyn OpenAiHttpTransport>,
    ) -> Result<Self, OpenAiCompatibleProviderError> {
        let headers = parse_headers(&config.headers)?;
        Ok(Self {
            transport,
            base_url: config.base_url,
            api_key: config.api_key,
            credential_source: None,
            auth_profile: None,
            api_mode: config.api_mode,
            headers,
        })
    }

    pub fn with_credential_source(
        mut self,
        credential_source: Arc<dyn ProviderCredentialSource>,
    ) -> Self {
        self.credential_source = Some(credential_source);
        self
    }

    pub fn with_auth_profile(mut self, auth_profile: OpenAiAuthProfile) -> Self {
        self.auth_profile = Some(auth_profile);
        self
    }

    fn chat_completions_endpoint(&self) -> String {
        chat_completions_endpoint(&self.base_url)
    }

    fn responses_endpoint(&self) -> String {
        responses_endpoint(&self.base_url)
    }

    fn is_loopback_base_url(&self) -> bool {
        is_loopback_base_url(&self.base_url)
    }

    async fn provider_credential(&self) -> Result<ProviderBearerToken, ProviderStreamEvent> {
        if let Some(source) = &self.credential_source {
            let credential = source
                .bearer_token()
                .await
                .map_err(|err| ProviderStreamEvent::categorized_error(err.message, err.category))?;
            if credential.token.trim().is_empty() {
                return Err(ProviderStreamEvent::categorized_error(
                    "openai_compatible credential source returned an empty bearer token",
                    ProviderErrorCategory::MissingCredentials,
                ));
            }
            return Ok(credential);
        }

        if self.api_key.trim().is_empty() {
            return Err(ProviderStreamEvent::categorized_error(
                "openai_compatible credentials are missing",
                ProviderErrorCategory::MissingCredentials,
            ));
        }

        Ok(ProviderBearerToken {
            token: self.api_key.clone(),
            kind: ProviderCredentialKind::InlineApiKey,
            account_id: None,
            enterprise_url: None,
        })
    }

    async fn send_request<T: Serialize>(
        &self,
        endpoint: String,
        request: &T,
        credential: &ProviderBearerToken,
        context: &ProviderRequestContext,
    ) -> Result<OpenAiHttpResponse, String> {
        let mut body = serde_json::to_value(request)
            .map_err(|err| format!("failed to serialize openai_compatible request: {err}"))?;
        if matches!(self.auth_profile, Some(OpenAiAuthProfile::Codex)) {
            if let serde_json::Value::Object(body) = &mut body {
                body.insert("store".to_string(), serde_json::Value::Bool(false));
                body.remove("max_output_tokens");
                body.remove("max_tokens");
                apply_codex_gpt5_response_defaults(body);
            }
        }
        let (endpoint, headers) = self.decorate_request(endpoint, credential, context)?;
        self.transport
            .post_json(endpoint, headers, credential.token.clone(), body)
            .await
    }

    async fn send_chat_request(
        &self,
        request: &OpenAiChatCompletionsRequest,
        credential: &ProviderBearerToken,
        context: &ProviderRequestContext,
    ) -> Result<OpenAiHttpResponse, String> {
        self.send_request(
            self.chat_completions_endpoint(),
            request,
            credential,
            context,
        )
        .await
    }

    async fn send_responses_request(
        &self,
        request: &OpenAiResponsesRequest,
        credential: &ProviderBearerToken,
        context: &ProviderRequestContext,
    ) -> Result<OpenAiHttpResponse, String> {
        self.send_request(self.responses_endpoint(), request, credential, context)
            .await
    }

    fn decorate_request(
        &self,
        endpoint: String,
        credential: &ProviderBearerToken,
        context: &ProviderRequestContext,
    ) -> Result<(String, HeaderMap), String> {
        let mut headers = self.headers.clone();
        remove_header_case_insensitive(&mut headers, "authorization");

        if self.auth_profile.is_none() {
            return Ok((endpoint, headers));
        }

        match self.auth_profile {
            Some(OpenAiAuthProfile::Codex) => {
                insert_static_header(&mut headers, "originator", "harness")?;
                insert_static_header(
                    &mut headers,
                    "user-agent",
                    concat!("harness/", env!("CARGO_PKG_VERSION")),
                )?;
                if let Some(session_id) = context.session_id.as_deref().and_then(non_empty_string) {
                    insert_static_header(&mut headers, "session-id", session_id)?;
                }
                if let Some(request_id) = context.request_id.as_deref().and_then(non_empty_string) {
                    insert_static_header(&mut headers, "request-id", request_id)?;
                }
                if let Some(account_id) =
                    credential.account_id.as_deref().and_then(non_empty_string)
                {
                    insert_static_header(&mut headers, "chatgpt-account-id", account_id)?;
                }

                let rewritten = rewrite_codex_endpoint(&endpoint).unwrap_or(endpoint);
                Ok((rewritten, headers))
            }
            Some(OpenAiAuthProfile::GithubCopilot) => {
                remove_header_case_insensitive(&mut headers, "x-api-key");
                insert_static_header(
                    &mut headers,
                    "x-initiator",
                    match context.initiator {
                        crate::ProviderRequestInitiator::Agent => "agent",
                        crate::ProviderRequestInitiator::User => "user",
                    },
                )?;
                insert_static_header(&mut headers, "Openai-Intent", "conversation-edits")?;
                insert_static_header(
                    &mut headers,
                    "user-agent",
                    concat!("harness/", env!("CARGO_PKG_VERSION")),
                )?;
                if context.has_media {
                    insert_static_header(&mut headers, "Copilot-Vision-Request", "true")?;
                }
                let base = copilot_base_url(credential.enterprise_url.as_deref())?;
                let rewritten = rewrite_endpoint_base(&endpoint, &base);
                Ok((rewritten, headers))
            }
            None => Ok((endpoint, headers)),
        }
    }

    fn supports_long_prompt_cache_retention(&self) -> bool {
        supports_long_prompt_cache_retention(&self.base_url)
    }

    async fn non_success_status_error(
        &self,
        response: OpenAiHttpResponse,
        bearer_token: &str,
    ) -> ProviderStreamEvent {
        let status = response.status;
        let body = collect_body_text(response.body).await.ok();
        let message = format_non_success_status_message(status, body.as_deref(), bearer_token);
        let category = categorize_non_success_status(status, body.as_deref(), bearer_token);
        ProviderStreamEvent::categorized_error(message, category)
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let credential = match self.provider_credential().await {
            Ok(credential) => credential,
            Err(event) => return Box::pin(stream::iter(vec![event])),
        };
        let context = req.context.clone();
        let supports_long_cache_retention = self.supports_long_prompt_cache_retention();
        let responses_system_as_instructions =
            matches!(self.auth_profile, Some(OpenAiAuthProfile::Codex));
        let response_result = match self.api_mode {
            OpenAiApiMode::ChatCompletions => {
                let chat_request = OpenAiChatCompletionsRequest::from_completion_request(
                    req,
                    supports_long_cache_retention,
                );
                self.send_chat_request(&chat_request, &credential, &context)
                    .await
                    .map(|response| (OpenAiApiMode::ChatCompletions, response))
            }
            OpenAiApiMode::Responses => {
                let responses_request = OpenAiResponsesRequest::from_completion_request(
                    req,
                    supports_long_cache_retention,
                    responses_system_as_instructions,
                );
                self.send_responses_request(&responses_request, &credential, &context)
                    .await
                    .map(|response| (OpenAiApiMode::Responses, response))
            }
            OpenAiApiMode::Auto => {
                let responses_request = OpenAiResponsesRequest::from_completion_request(
                    req.clone(),
                    supports_long_cache_retention,
                    responses_system_as_instructions,
                );
                match self
                    .send_responses_request(&responses_request, &credential, &context)
                    .await
                {
                    Ok(response)
                        if matches!(response.status, 404 | 405)
                            || (response.status == 400 && self.is_loopback_base_url()) =>
                    {
                        let chat_request = OpenAiChatCompletionsRequest::from_completion_request(
                            req,
                            supports_long_cache_retention,
                        );
                        self.send_chat_request(&chat_request, &credential, &context)
                            .await
                            .map(|fallback_response| {
                                (OpenAiApiMode::ChatCompletions, fallback_response)
                            })
                    }
                    Ok(response) => Ok((OpenAiApiMode::Responses, response)),
                    Err(message) => Err(message),
                }
            }
        };

        let (mode, response) = match response_result {
            Ok(response) => response,
            Err(message) => {
                return Box::pin(stream::iter(vec![ProviderStreamEvent::categorized_error(
                    message,
                    ProviderErrorCategory::TransportFailure,
                )]));
            }
        };

        if !(200..300).contains(&response.status) {
            let error = self
                .non_success_status_error(response, &credential.token)
                .await;
            return Box::pin(stream::iter(vec![error]));
        }

        let start_metadata = provider_stream_start_metadata_from_headers(&response.headers);

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            match mode {
                OpenAiApiMode::ChatCompletions => {
                    consume_chat_sse_stream(response, tx, start_metadata).await
                }
                OpenAiApiMode::Responses | OpenAiApiMode::Auto => {
                    consume_responses_sse_stream(response, tx, start_metadata).await
                }
            }
        });

        Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
    }
}

fn warn_stream_send_failure(context: &str) {
    tracing::warn!(
        context,
        "provider stream receiver dropped before event delivery"
    );
}

fn warn_stream_processing_failure(context: &str, message: &str) {
    tracing::warn!(
        context,
        message,
        "openai_compatible stream processing failed"
    );
}

async fn consume_chat_sse_stream(
    response: OpenAiHttpResponse,
    tx: mpsc::Sender<ProviderStreamEvent>,
    start_metadata: Option<ProviderStreamStartMetadata>,
) {
    if tx
        .send(ProviderStreamEvent::Started {
            metadata: start_metadata.clone(),
        })
        .await
        .is_err()
    {
        warn_stream_send_failure("chat.start");
        return;
    }

    let mut usage = zero_usage();
    let mut finished_metadata = provider_stream_finished_metadata_from_start(start_metadata);
    let mut done_emitted = false;
    let mut tool_call_state = ChatToolCallState::default();
    let mut body = response.body;
    let mut sse_buffer = Vec::new();

    loop {
        let event = match next_sse_event(&mut body, &mut sse_buffer).await {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(_) => {
                warn_stream_processing_failure(
                    "chat.transport",
                    "openai_compatible SSE stream transport error",
                );
                let _ = tx
                    .send(transport_failure_error(
                        "openai_compatible SSE stream transport error",
                    ))
                    .await;
                return;
            }
        };

        let data = event.data.trim();
        if data == "[DONE]" {
            if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
                return;
            }

            if !done_emitted
                && tx
                    .send(ProviderStreamEvent::DoneWithMetadata {
                        usage,
                        metadata: non_empty_finished_metadata(finished_metadata),
                    })
                    .await
                    .is_err()
            {
                warn_stream_send_failure("chat.done");
            }
            return;
        }

        let chunk: OpenAiChatCompletionsChunk = match serde_json::from_str(data) {
            Ok(chunk) => chunk,
            Err(_) => {
                warn_stream_processing_failure(
                    "chat.invalid_json",
                    "openai_compatible returned invalid SSE JSON chunk",
                );
                let _ = tx
                    .send(malformed_stream_error(
                        "openai_compatible returned invalid SSE JSON chunk",
                    ))
                    .await;
                return;
            }
        };

        if let Some(id) = chunk
            .id
            .as_deref()
            .filter(|id| non_empty_string(id).is_some())
        {
            finished_metadata
                .provider_response_id
                .get_or_insert_with(|| id.to_string());
        }

        if let Some(chunk_usage) = chunk.usage {
            usage = chunk_usage.completion_usage();
            chunk_usage.merge_finished_metadata(&mut finished_metadata);
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

            if let Some(finish_reason) = choice.finish_reason.as_deref() {
                finished_metadata.provider_stop_reason = Some(finish_reason.to_string());
                finish_seen = true;
            }
        }

        if finish_seen && !done_emitted {
            if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
                return;
            }

            done_emitted = true;
            if tx
                .send(ProviderStreamEvent::DoneWithMetadata {
                    usage: usage.clone(),
                    metadata: non_empty_finished_metadata(finished_metadata.clone()),
                })
                .await
                .is_err()
            {
                warn_stream_send_failure("chat.done_after_finish_reason");
                return;
            }
        }
    }

    if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
        return;
    }

    if !done_emitted
        && tx
            .send(ProviderStreamEvent::DoneWithMetadata {
                usage,
                metadata: non_empty_finished_metadata(finished_metadata),
            })
            .await
            .is_err()
    {
        warn_stream_send_failure("chat.done_after_stream_end");
    }
}

async fn consume_responses_sse_stream(
    response: OpenAiHttpResponse,
    tx: mpsc::Sender<ProviderStreamEvent>,
    start_metadata: Option<ProviderStreamStartMetadata>,
) {
    if tx
        .send(ProviderStreamEvent::Started {
            metadata: start_metadata.clone(),
        })
        .await
        .is_err()
    {
        warn_stream_send_failure("responses.start");
        return;
    }

    let mut usage = zero_usage();
    let mut finished_metadata = provider_stream_finished_metadata_from_start(start_metadata);
    let mut done_emitted = false;
    let mut body = response.body;
    let mut sse_buffer = Vec::new();
    let mut tool_calls = ResponsesToolCallState::default();

    loop {
        let event = match next_sse_event(&mut body, &mut sse_buffer).await {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(message) => {
                warn_stream_processing_failure(
                    "responses.transport",
                    &format!("openai_compatible SSE stream transport error: {message}"),
                );
                let _ = tx
                    .send(transport_failure_error(format!(
                        "openai_compatible SSE stream transport error: {message}"
                    )))
                    .await;
                return;
            }
        };

        let data = event.data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            if !done_emitted
                && tx
                    .send(ProviderStreamEvent::DoneWithMetadata {
                        usage,
                        metadata: non_empty_finished_metadata(finished_metadata),
                    })
                    .await
                    .is_err()
            {
                warn_stream_send_failure("responses.done");
            }
            return;
        }

        let parsed: OpenAiResponsesEvent = match serde_json::from_str(data) {
            Ok(parsed) => parsed,
            Err(err) => {
                warn_stream_processing_failure(
                    "responses.invalid_json",
                    &format!(
                        "openai_compatible returned invalid SSE JSON chunk: {err}; sample={}",
                        summarize_sse_data(data)
                    ),
                );
                let _ = tx
                    .send(malformed_stream_error(format!(
                        "openai_compatible returned invalid SSE JSON chunk: {err}; sample={}",
                        summarize_sse_data(data)
                    )))
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
                finished_metadata.provider_stop_reason = Some(parsed.event_type.clone());
                if let Some(response) = parsed.response {
                    response.merge_finished_metadata(&mut finished_metadata);
                    if let Some(completion_usage) =
                        response.usage.map(|usage| usage.completion_usage())
                    {
                        usage = completion_usage;
                    }
                }

                if let Err(message) =
                    emit_pending_responses_tool_call_completions(&tx, &mut tool_calls).await
                {
                    warn_stream_processing_failure("responses.tool_completion", &message);
                    let _ = tx.send(unsupported_tool_call_error(message)).await;
                    return;
                }

                if !done_emitted {
                    done_emitted = true;
                    if tx
                        .send(ProviderStreamEvent::DoneWithMetadata {
                            usage: usage.clone(),
                            metadata: non_empty_finished_metadata(finished_metadata.clone()),
                        })
                        .await
                        .is_err()
                    {
                        warn_stream_send_failure("responses.done_after_completion");
                        return;
                    }
                }
            }
            "response.error" => {
                warn_stream_processing_failure(
                    "responses.error_event",
                    "openai_compatible responses stream returned error event",
                );
                let _ = tx
                    .send(malformed_stream_error(
                        "openai_compatible responses stream returned error event",
                    ))
                    .await;
                return;
            }
            _ => {}
        }
    }

    if !done_emitted
        && tx
            .send(ProviderStreamEvent::DoneWithMetadata {
                usage,
                metadata: non_empty_finished_metadata(finished_metadata),
            })
            .await
            .is_err()
    {
        warn_stream_send_failure("responses.done_after_stream_end");
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

fn zero_usage() -> CompletionUsage {
    CompletionUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    }
}

fn non_empty_string(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests;

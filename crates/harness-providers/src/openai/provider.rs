// allow: SIZE_OK — cohesive OpenAI-compatible provider definition (constructor + credential resolution + request dispatch + auth-profile header decoration + Provider trait impl)
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::HeaderMap;

use crate::request_budget::{openai_request_budget_semantics, OpenAiBudgetMode};
use crate::{
    CompletionRequest, Provider, ProviderBearerToken, ProviderBudgetSemantics,
    ProviderCredentialKind, ProviderCredentialSource, ProviderErrorCategory, ProviderEventStream,
    ProviderRequestContext, ProviderRequestCostError, ProviderStreamEvent,
};

use super::config::{
    OpenAiApiMode, OpenAiAuthProfile, OpenAiCompatibleProviderConfig, OpenAiCompatibleProviderError,
};
use super::endpoint::{
    apply_codex_gpt5_response_defaults, chat_completions_endpoint, copilot_base_url,
    is_loopback_base_url, responses_endpoint, rewrite_codex_endpoint, rewrite_endpoint_base,
    supports_long_prompt_cache_retention,
};
use super::error::{
    categorize_non_success_status, format_non_success_status_message, format_transport_error,
    retry_after_ms,
};
use super::header::{insert_static_header, parse_headers, remove_header_case_insensitive};
use super::request::{OpenAiChatCompletionsRequest, OpenAiResponsesRequest};
use super::sse::collect_body_text;
use super::stream_event::{
    non_empty_finished_metadata, provider_stream_start_metadata_from_headers,
};
use super::transport::{OpenAiHttpResponse, OpenAiHttpTransport, ReqwestOpenAiHttpTransport};

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
            transport: Arc::new(ReqwestOpenAiHttpTransport::new(client)),
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

    pub(crate) fn api_mode(&self) -> OpenAiApiMode {
        self.api_mode
    }

    pub(crate) fn is_codex_profile(&self) -> bool {
        matches!(self.auth_profile, Some(OpenAiAuthProfile::Codex))
    }

    pub(crate) fn is_loopback_base_url(&self) -> bool {
        is_loopback_base_url(&self.base_url)
    }

    pub(crate) fn supports_long_prompt_cache_retention(&self) -> bool {
        supports_long_prompt_cache_retention(&self.base_url)
    }

    pub(crate) async fn provider_credential(
        &self,
    ) -> Result<ProviderBearerToken, Box<ProviderStreamEvent>> {
        if let Some(source) = &self.credential_source {
            let credential = source.bearer_token().await.map_err(|err| {
                Box::new(ProviderStreamEvent::categorized_error(
                    err.message,
                    err.category,
                ))
            })?;
            if credential.token.trim().is_empty() {
                return Err(Box::new(ProviderStreamEvent::categorized_error(
                    "openai_compatible credential source returned an empty bearer token",
                    ProviderErrorCategory::MissingCredentials,
                )));
            }
            return Ok(credential);
        }

        if self.api_key.trim().is_empty() {
            return Err(Box::new(ProviderStreamEvent::categorized_error(
                "openai_compatible credentials are missing",
                ProviderErrorCategory::MissingCredentials,
            )));
        }

        Ok(ProviderBearerToken {
            token: self.api_key.clone(),
            kind: ProviderCredentialKind::InlineApiKey,
            account_id: None,
            enterprise_url: None,
        })
    }

    pub(crate) async fn send_request<T: serde::Serialize>(
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

    pub(crate) async fn send_chat_request(
        &self,
        request: &OpenAiChatCompletionsRequest,
        credential: &ProviderBearerToken,
        context: &ProviderRequestContext,
    ) -> Result<OpenAiHttpResponse, String> {
        self.send_request(
            chat_completions_endpoint(&self.base_url),
            request,
            credential,
            context,
        )
        .await
    }

    pub(crate) async fn send_responses_request(
        &self,
        request: &OpenAiResponsesRequest,
        credential: &ProviderBearerToken,
        context: &ProviderRequestContext,
    ) -> Result<OpenAiHttpResponse, String> {
        self.send_request(
            responses_endpoint(&self.base_url),
            request,
            credential,
            context,
        )
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
                if let Some(session_id) = context
                    .session_id
                    .as_deref()
                    .and_then(super::stream::non_empty_string)
                {
                    insert_static_header(&mut headers, "session-id", session_id)?;
                }
                if let Some(request_id) = context
                    .request_id
                    .as_deref()
                    .and_then(super::stream::non_empty_string)
                {
                    insert_static_header(&mut headers, "request-id", request_id)?;
                }
                if let Some(account_id) = credential
                    .account_id
                    .as_deref()
                    .and_then(super::stream::non_empty_string)
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

    pub(crate) async fn non_success_status_error(
        &self,
        response: OpenAiHttpResponse,
        bearer_token: &str,
    ) -> ProviderStreamEvent {
        let status = response.status;
        let retry_after_ms = retry_after_ms(&response.headers);
        let body = collect_body_text(response.body).await.ok();
        let message = format_non_success_status_message(status, body.as_deref(), bearer_token);
        let category = categorize_non_success_status(status, body.as_deref(), bearer_token);
        ProviderStreamEvent::categorized_error_with_retry_after_ms(
            message,
            category,
            retry_after_ms,
        )
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
        let mode = match self.api_mode {
            OpenAiApiMode::ChatCompletions => OpenAiBudgetMode::Chat,
            OpenAiApiMode::Responses => OpenAiBudgetMode::Responses,
            OpenAiApiMode::Auto => OpenAiBudgetMode::Auto,
        };
        openai_request_budget_semantics(
            request,
            pending_prompt_index,
            mode,
            self.is_codex_profile(),
        )
    }

    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        super::stream::stream_completion(self, req).await
    }
}

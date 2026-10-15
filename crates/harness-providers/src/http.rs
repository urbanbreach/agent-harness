use crate::{
    sse::Decoder,
    wire::{encode, StreamDecoder},
};
use crate::{
    CompletionRequest, Protocol, ProviderCredentialSource, ProviderErrorCategory as Category,
    ProviderEventStream, ProviderStreamEvent as Event,
};
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub struct HttpProvider {
    pub(crate) client: reqwest::Client,
    pub(crate) endpoint: reqwest::Url,
    pub(crate) protocol: Protocol,
    pub(crate) credentials: Option<Arc<dyn ProviderCredentialSource>>,
    pub(crate) headers: reqwest::header::HeaderMap,
    pub(crate) auth_profile: Option<crate::ProviderAuthProfile>,
    chat_fallback: bool,
}

impl HttpProvider {
    pub(crate) fn protocol(&self, request: &CompletionRequest) -> Protocol {
        if self.chat_fallback
            && self.auth_profile == Some(crate::ProviderAuthProfile::GithubCopilot)
            && request.model_id.starts_with("claude-")
        {
            Protocol::Anthropic
        } else {
            self.protocol
        }
    }
    pub(crate) fn budget(
        &self,
        request: &CompletionRequest,
        pending: usize,
    ) -> Result<crate::ProviderBudgetSemantics, crate::ProviderRequestCostError> {
        let mut budget = crate::request_budget::request_budget_semantics_at(
            request,
            pending,
            self.protocol(request),
            self.endpoint.host_str(),
        )?;
        if self.auth_profile == Some(crate::ProviderAuthProfile::Codex) {
            budget.output_cap_disposition = crate::ProviderOutputCapDisposition::ProviderControlled;
        }
        Ok(budget)
    }
    pub fn new(endpoint: &str, protocol: Protocol, timeout: Duration) -> Result<Self, String> {
        let endpoint = reqwest::Url::parse(endpoint).map_err(|_| "invalid provider URL")?;
        if !matches!(endpoint.scheme(), "https" | "http")
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
            || timeout.is_zero()
        {
            return Err("provider URL must use HTTP(S) without embedded credentials; timeout must be positive".into());
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(timeout.min(Duration::from_secs(30)))
            .read_timeout(timeout)
            .build()
            .map_err(|_| "could not initialize provider HTTP client")?;
        Ok(Self {
            client,
            endpoint,
            protocol,
            credentials: None,
            headers: reqwest::header::HeaderMap::new(),
            auth_profile: None,
            chat_fallback: false,
        })
    }

    pub fn with_credentials(mut self, credentials: Arc<dyn ProviderCredentialSource>) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub fn with_headers(mut self, headers: reqwest::header::HeaderMap) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_auth_profile(mut self, profile: crate::ProviderAuthProfile) -> Self {
        self.auth_profile = Some(profile);
        if profile == crate::ProviderAuthProfile::Codex {
            self.chat_fallback = false;
            self.protocol = Protocol::Responses;
            if let Some(base) = self.endpoint.path().strip_suffix("/chat/completions") {
                self.endpoint.set_path(&format!("{base}/responses"));
            }
        }
        self
    }

    pub fn with_chat_fallback(mut self) -> Self {
        self.chat_fallback = self.protocol == Protocol::Responses
            && self.auth_profile != Some(crate::ProviderAuthProfile::Codex);
        self
    }

    async fn send(
        &self,
        request: &CompletionRequest,
        protocol: Protocol,
        token: Option<&crate::ProviderBearerToken>,
    ) -> Result<reqwest::Response, (&'static str, Category)> {
        let mut sanitized;
        let request = if protocol == Protocol::Anthropic
            && request.messages.iter().any(|m| {
                m.continuation
                    .as_deref()
                    .is_some_and(|s| s.data["type"] == "anthropicCompaction")
            })
            && !crate::native_compaction::supported_anthropic(&self.endpoint)
        {
            sanitized = request.clone();
            for message in &mut sanitized.messages {
                if message
                    .continuation
                    .as_deref()
                    .is_some_and(|s| s.data["type"] == "anthropicCompaction")
                {
                    message.continuation = None;
                }
            }
            &sanitized
        } else {
            request
        };
        let mut body = crate::wire::encode_at(request, protocol, self.endpoint.host_str())
            .map_err(|e| (e, Category::UnsupportedToolCall))?;
        let mut headers = self.headers.clone();
        let mut endpoint = self.endpoint.clone();
        if protocol != self.protocol {
            let base = endpoint
                .path()
                .strip_suffix("/responses")
                .ok_or(("automatic endpoint must end in /responses", Category::Other))?;
            let suffix = if protocol == Protocol::Anthropic {
                "/messages"
            } else {
                "/chat/completions"
            };
            endpoint.set_path(&format!("{base}{suffix}"));
        }
        if protocol == Protocol::Anthropic
            && request.messages.iter().any(|m| {
                m.continuation.as_deref().is_some_and(|s| {
                    s.provider_id == request.provider_id && s.data["type"] == "anthropicCompaction"
                })
            })
            && crate::native_compaction::anthropic_capable(&request.model_id)
        {
            crate::native_compaction::route_anthropic_beta(&mut body, &mut headers, &endpoint)
                .map_err(|_| ("invalid beta header", Category::Other))?;
        }
        if let Some(token) = token {
            headers.remove("authorization");
            if protocol == Protocol::Anthropic {
                headers.remove("x-api-key");
            }
            if let Some(profile) = self.auth_profile {
                profile
                    .decorate(&mut endpoint, &mut headers, &mut body, request, token)
                    .map_err(|e| (e, Category::InvalidCredentials))?;
            }
        }
        if request.context.cache_retention == crate::CacheRetention::Long
            && protocol != Protocol::Anthropic
            && self.auth_profile.is_none()
            && endpoint.host_str() == Some("api.openai.com")
        {
            body["prompt_cache_retention"] = "24h".into();
        }
        let mut post = self
            .client
            .post(endpoint)
            .headers(headers)
            .header("accept", "text/event-stream")
            .json(&body);
        if protocol == Protocol::Anthropic {
            post = post.header("anthropic-version", "2023-06-01");
        }
        if let Some(token) = token {
            post = if protocol == Protocol::Anthropic
                && self.auth_profile != Some(crate::ProviderAuthProfile::GithubCopilot)
            {
                post.header("x-api-key", &token.token)
            } else {
                post.bearer_auth(&token.token)
            };
        }
        post.send()
            .await
            .map_err(|_| ("provider connection failed", Category::TransportFailure))
    }

    pub fn stream(&self, request: CompletionRequest) -> ProviderEventStream {
        let provider = self.clone();
        Box::pin(async_stream::stream! {
            if provider.auth_profile == Some(crate::ProviderAuthProfile::Codex) && !crate::codex_model_allowed(&request.model_id) {
                yield Event::error("Model is not supported by Codex subscriptions; select a supported model or an API-key provider."); return;
            }
            let token = if let Some(credentials) = &provider.credentials {
                let token = match credentials.bearer_token().await {
                    Ok(token) => token,
                    Err(error) => { yield Event::categorized_error("provider credential is unavailable", error.category); return; }
                };
                Some(token)
            } else { None };
            if provider.auth_profile.is_some() && token.is_none() {
                yield Event::categorized_error("subscription credential is unavailable", Category::MissingCredentials); return;
            }
            let mut protocol = provider.protocol(&request);
            let mut response = match provider.send(&request, protocol, token.as_ref()).await {
                Ok(response) => response,
                Err((message, category)) => { yield Event::categorized_error(message, category); return; }
            };
            let local = provider.endpoint.host_str().is_some_and(|host| host == "localhost"
                || host.trim_matches(['[', ']']).parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()));
            if provider.chat_fallback && protocol == Protocol::Responses && (matches!(response.status().as_u16(), 404 | 405)
                || response.status().as_u16() == 400 && local) {
                drop(response);
                protocol = Protocol::Chat;
                response = match provider.send(&request, protocol, token.as_ref()).await {
                    Ok(response) => response,
                    Err((message, category)) => { yield Event::categorized_error(message, category); return; }
                };
            }
            if !response.status().is_success() {
                yield failure::event(response, token.as_ref()).await;
                return;
            }
            let is_sse = response.headers().get("content-type").is_none_or(|value|
                value.to_str().is_ok_and(|s| s.split(';').next().is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))));
            if !is_sse { yield Event::categorized_error("provider response is not an event stream", Category::MalformedStream); return; }
            yield Event::Start;
            let mut frames = Decoder::default();
            let mut decoder = StreamDecoder::new(protocol);
            loop {
                let chunk = match response.chunk().await {
                    Ok(Some(chunk)) => chunk,
                    Ok(None) => break,
                    Err(_) => { yield Event::categorized_error("provider stream connection failed", Category::TransportFailure); return; }
                };
                let frames = match frames.push(&chunk) {
                    Ok(frames) => frames,
                    Err(error) => { yield Event::categorized_error(error, Category::MalformedStream); return; }
                };
                for frame in frames {
                    match decoder.frame(&frame) {
                        Ok(events) => for mut event in events {
                            if let Event::DoneWithMetadata { metadata: Some(metadata), .. } = &mut event
                                && let Some(state) = &mut metadata.continuation {
                                state.provider_id.clone_from(&request.provider_id);
                                state.model_id.clone_from(&request.model_id);
                            }
                            yield event;
                        },
                        Err(error) => {
                            yield decoder.take_failure().unwrap_or_else(|| Event::categorized_error(error, Category::MalformedStream));
                            return;
                        }
                    }
                    if decoder.done { return; }
                }
            }
            yield Event::categorized_error("provider stream ended before completion", Category::MalformedStream);
        })
    }
}

#[cfg(test)]
mod cache_tests;
mod failure;
#[cfg(test)]
mod tests;

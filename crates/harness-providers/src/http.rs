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
    client: reqwest::Client,
    endpoint: reqwest::Url,
    protocol: Protocol,
    credentials: Option<Arc<dyn ProviderCredentialSource>>,
    headers: reqwest::header::HeaderMap,
    auth_profile: Option<crate::ProviderAuthProfile>,
    chat_fallback: bool,
}

impl HttpProvider {
    fn protocol(&self, request: &CompletionRequest) -> Protocol {
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
        let mut budget = crate::request_budget::request_budget_semantics(
            request,
            pending,
            self.protocol(request),
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
            .timeout(timeout)
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
        let mut body = encode(request, protocol).map_err(|e| (e, Category::UnsupportedToolCall))?;
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
                let status = response.status().as_u16();
                let retry_after = response.headers().get("retry-after").and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok()).map(|n| n.saturating_mul(1000));
                let mut category = match status {
                    401 | 403 => Category::InvalidCredentials,
                    429 => Category::RateLimited,
                    500..=599 => Category::TransportFailure,
                    _ => Category::Other,
                };
                if status == 400 || status == 413 {
                    let mut bytes = Vec::new();
                    while let Ok(Some(chunk)) = response.chunk().await {
                        if bytes.len() + chunk.len() > 65_536 { break; }
                        bytes.extend_from_slice(&chunk);
                    }
                    if let Ok(error) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        let code = error.pointer("/error/code").and_then(|v| v.as_str()).unwrap_or("");
                        let message = error.pointer("/error/message").and_then(|v| v.as_str()).unwrap_or("");
                        if matches!(code, "context_length_exceeded" | "prompt_too_long") || message.contains("prompt is too long") {
                            category = Category::ContextWindowExceeded;
                        }
                    }
                }
                yield Event::categorized_error_with_retry_after_ms(format!("provider returned HTTP {status}"), category, retry_after);
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
                        Ok(events) => for event in events { yield event; },
                        Err(error) => { yield Event::categorized_error(error, Category::MalformedStream); return; }
                    }
                    if decoder.done { return; }
                }
            }
            yield Event::categorized_error("provider stream ended before completion", Category::MalformedStream);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProviderStreamEvent;
    use crate::{CompletionMessage, MessageRole, ProviderErrorCategory};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn http_stream_reports_truncation_and_keeps_server_errors_private(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (status, content_type, body, category) in [
            (
                "200 OK",
                "Content-Type: text/event-stream\r\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
                Some(ProviderErrorCategory::MalformedStream),
            ),
            (
                "401 Unauthorized",
                "Content-Type: text/event-stream\r\n",
                "secret-server-body",
                Some(ProviderErrorCategory::InvalidCredentials),
            ),
            ("200 OK", "", "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n", None),
            (
                "200 OK",
                "Content-Type: text/html\r\n",
                "data: [DONE]\n\n",
                Some(ProviderErrorCategory::MalformedStream),
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let (protocol, path) = if status.starts_with("401") {
                (Protocol::Responses, "/v1/responses")
            } else {
                (Protocol::Chat, "/v1/chat/completions")
            };
            let endpoint = format!("http://{}{path}", listener.local_addr()?);
            let server = tokio::spawn(reply(listener, status, content_type, body, path));
            let provider = HttpProvider::new(&endpoint, protocol, Duration::from_secs(2))?
                .with_chat_fallback();
            let request = CompletionRequest {
                model_id: "fixture".into(),
                messages: vec![CompletionMessage::text(MessageRole::User, "hello")],
                ..Default::default()
            };
            let events = provider.stream(request).collect::<Vec<_>>().await;
            match category {
                Some(category) => assert!(
                    matches!(events.last(), Some(ProviderStreamEvent::Error { category: Some(actual), .. }) if *actual == category)
                ),
                None => assert!(matches!(
                    events.last(),
                    Some(ProviderStreamEvent::DoneWithMetadata { .. })
                )),
            }
            assert!(!format!("{events:?}").contains("secret-server-body"));
            server.await??;
        }
        assert!(HttpProvider::new(
            "http://name:password@localhost/",
            Protocol::Chat,
            Duration::from_secs(2)
        )
        .is_err());
        Ok(())
    }

    async fn reply(
        listener: tokio::net::TcpListener,
        status: &str,
        content_type: &str,
        body: &str,
        path: &str,
    ) -> std::io::Result<()> {
        let (mut connection, _) = listener.accept().await?;
        let mut request = vec![0; 8192];
        let count = connection.read(&mut request).await?;
        if !request[..count].starts_with(format!("POST {path} ").as_bytes()) {
            return Err(std::io::Error::other("expected completion request"));
        }
        let reply = format!("HTTP/1.1 {status}\r\n{content_type}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        connection.write_all(reply.as_bytes()).await
    }
}

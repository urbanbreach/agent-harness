use async_trait::async_trait;

use super::{
    anthropic_messages_url, build_anthropic_request, parse_anthropic_response,
    parse_anthropic_sse_stream, AnthropicProvider, ANTHROPIC_VERSION,
};
use crate::request_budget::anthropic_request_budget_semantics;
use crate::{
    CompletionRequest, Provider, ProviderBudgetSemantics, ProviderErrorCategory,
    ProviderEventStream, ProviderRequestCostError, ProviderStreamEvent,
};

#[async_trait]
impl Provider for AnthropicProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
        anthropic_request_budget_semantics(request, pending_prompt_index)
    }

    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        let body = build_anthropic_request(&request);
        let url = anthropic_messages_url(&self.base_url);
        let mut http_request = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body);

        for (key, value) in &self.headers {
            http_request = http_request.header(key.as_str(), value.as_str());
        }

        match http_request.send().await {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    let message = format!("anthropic request to {url} returned status {status}");
                    return Box::pin(tokio_stream::iter(vec![
                        ProviderStreamEvent::categorized_error(
                            message,
                            ProviderErrorCategory::TransportFailure,
                        ),
                    ]));
                }
                let bytes = match response.bytes().await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        return Box::pin(tokio_stream::iter(vec![
                            ProviderStreamEvent::categorized_error(
                                format!("failed to read anthropic response body: {error}"),
                                ProviderErrorCategory::TransportFailure,
                            ),
                        ]));
                    }
                };
                let raw = String::from_utf8_lossy(&bytes);
                let events = if request.stream {
                    parse_anthropic_sse_stream(&raw)
                } else {
                    parse_anthropic_response(&raw)
                };
                Box::pin(tokio_stream::iter(events))
            }
            Err(error) => {
                let message = format!("anthropic transport error: {error}");
                Box::pin(tokio_stream::iter(vec![
                    ProviderStreamEvent::categorized_error(
                        message,
                        ProviderErrorCategory::TransportFailure,
                    ),
                ]))
            }
        }
    }
}

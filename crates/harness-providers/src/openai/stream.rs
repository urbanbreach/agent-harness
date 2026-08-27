use tokio::sync::mpsc;
use tokio_stream::{self as stream};

use crate::{ProviderErrorCategory, ProviderEventStream, ProviderStreamEvent};

use super::config::OpenAiApiMode;
use super::request::{OpenAiChatCompletionsRequest, OpenAiResponsesRequest};
use super::stream_event::provider_stream_start_metadata_from_headers;

mod chat_sse;
mod responses_sse;

pub async fn stream_completion(
    provider: &super::provider::OpenAiCompatibleProvider,
    req: crate::CompletionRequest,
) -> crate::ProviderEventStream {
    let credential = match provider.provider_credential().await {
        Ok(credential) => credential,
        Err(event) => return Box::pin(stream::iter(vec![*event])),
    };
    let req = match crate::schema_compat::prepare_request_tools(req) {
        Ok(req) => req,
        Err(err) => {
            return Box::pin(stream::iter(vec![ProviderStreamEvent::categorized_error(
                err.to_string(),
                ProviderErrorCategory::UnsupportedToolCall,
            )]));
        }
    };
    let context = req.context.clone();
    let supports_long_cache_retention = provider.supports_long_prompt_cache_retention();
    let responses_system_as_instructions = provider.is_codex_profile();
    let response_result = match provider.api_mode() {
        OpenAiApiMode::ChatCompletions => {
            let chat_request = OpenAiChatCompletionsRequest::from_completion_request(
                req,
                supports_long_cache_retention,
            );
            provider
                .send_chat_request(&chat_request, &credential, &context)
                .await
                .map(|response| (OpenAiApiMode::ChatCompletions, response))
        }
        OpenAiApiMode::Responses => {
            let responses_request = OpenAiResponsesRequest::from_completion_request(
                req,
                supports_long_cache_retention,
                responses_system_as_instructions,
            );
            provider
                .send_responses_request(&responses_request, &credential, &context)
                .await
                .map(|response| (OpenAiApiMode::Responses, response))
        }
        OpenAiApiMode::Auto => {
            let responses_request = OpenAiResponsesRequest::from_completion_request(
                req.clone(),
                supports_long_cache_retention,
                responses_system_as_instructions,
            );
            match provider
                .send_responses_request(&responses_request, &credential, &context)
                .await
            {
                Ok(response)
                    if matches!(response.status, 404 | 405)
                        || (response.status == 400 && provider.is_loopback_base_url()) =>
                {
                    let chat_request = OpenAiChatCompletionsRequest::from_completion_request(
                        req,
                        supports_long_cache_retention,
                    );
                    provider
                        .send_chat_request(&chat_request, &credential, &context)
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
        let error = provider
            .non_success_status_error(response, &credential.token)
            .await;
        return Box::pin(stream::iter(vec![error]));
    }

    let start_metadata = provider_stream_start_metadata_from_headers(&response.headers);

    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        match mode {
            OpenAiApiMode::ChatCompletions => {
                chat_sse::consume_chat_sse_stream(response, tx, start_metadata).await
            }
            OpenAiApiMode::Responses | OpenAiApiMode::Auto => {
                responses_sse::consume_responses_sse_stream(response, tx, start_metadata).await
            }
        }
    });

    Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
}

pub(crate) fn warn_stream_send_failure(context: &str) {
    tracing::warn!(
        context,
        "provider stream receiver dropped before event delivery"
    );
}

pub(crate) fn warn_stream_processing_failure(context: &str, message: &str) {
    tracing::warn!(
        context,
        message,
        "openai_compatible stream processing failed"
    );
}

pub(crate) fn non_empty_string(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

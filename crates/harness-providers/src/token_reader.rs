//! The token-pricing identity of the model that will read a request.
use crate::{CompletionRequest, Protocol};
use harness_tokenizer::{Reader, ReaderApi};

/// The tokenizer's reader api for a wire protocol.
pub(crate) fn reader_api(protocol: Protocol) -> ReaderApi {
    match protocol {
        Protocol::Chat => ReaderApi::OpenAiChat,
        Protocol::Responses => ReaderApi::OpenAiResponses,
        Protocol::Anthropic => ReaderApi::Anthropic,
    }
}

/// Whether `host` is the first-party endpoint of the protocol's vendor.
fn official_endpoint(protocol: Protocol, host: Option<&str>) -> bool {
    match protocol {
        Protocol::Anthropic => host == Some("api.anthropic.com"),
        Protocol::Chat | Protocol::Responses => host == Some("api.openai.com"),
    }
}

/// The reader for `model_id` reached over `protocol` at `host` for the request's provider.
/// The provider layer knows no compatibility flags, so prefix binding is off and reasoning
/// history is not filtered.
pub(crate) fn reader_for(
    request: &CompletionRequest,
    model_id: &str,
    protocol: Protocol,
    host: Option<&str>,
) -> Reader {
    Reader {
        api: reader_api(protocol),
        provider: request.provider_id.clone().unwrap_or_default(),
        model_id: model_id.to_owned(),
        official_endpoint: official_endpoint(protocol, host),
        prefix_binding: false,
        filter_reasoning_history: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_selects_the_reader_api_and_host_the_official_flag() {
        let request = CompletionRequest {
            provider_id: Some("openai".into()),
            model_id: "gpt-5.5".into(),
            ..Default::default()
        };
        let cases = [
            (
                Protocol::Chat,
                Some("api.openai.com"),
                ReaderApi::OpenAiChat,
                true,
            ),
            (
                Protocol::Chat,
                Some("gateway.example"),
                ReaderApi::OpenAiChat,
                false,
            ),
            (
                Protocol::Responses,
                Some("api.openai.com"),
                ReaderApi::OpenAiResponses,
                true,
            ),
            (
                Protocol::Anthropic,
                Some("api.openai.com"),
                ReaderApi::Anthropic,
                false,
            ),
            (
                Protocol::Anthropic,
                Some("api.anthropic.com"),
                ReaderApi::Anthropic,
                true,
            ),
            (Protocol::Anthropic, None, ReaderApi::Anthropic, false),
        ];
        for (protocol, host, api, official) in cases {
            let reader = reader_for(&request, "gpt-5.5", protocol, host);
            assert_eq!((reader.api, reader.official_endpoint), (api, official));
            assert_eq!(reader.provider, "openai");
        }
    }

    #[test]
    fn an_openai_model_routed_over_chat_completions_reads_as_chat() {
        use crate::Provider;
        let provider = crate::HttpProvider::new(
            "https://gateway.example/v1/chat/completions",
            Protocol::Chat,
            std::time::Duration::from_secs(5),
        )
        .expect("valid endpoint");
        let request = CompletionRequest {
            provider_id: Some("gateway".into()),
            model_id: "gpt-5.5".into(),
            ..Default::default()
        };
        let reader = provider
            .token_reader(&request)
            .expect("http providers report a reader");
        assert_eq!(reader.api, ReaderApi::OpenAiChat);
        assert!(!reader.official_endpoint);
        assert_eq!(
            crate::mock::MockProvider::script([]).token_reader(&request),
            None
        );
    }
}

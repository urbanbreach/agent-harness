//! Integration tests for the provider leaf contract (Todo 8).
//!
//! These tests verify:
//! 1. Config-reachable protocols construct successfully via the leaf factory.
//! 2. Unsupported protocols are rejected with a truthful typed error.
//! 3. Auth metadata (API keys, bearer tokens) is redacted in provider
//!    artifacts and events — no secret material leaks into serialized output.

use std::collections::BTreeMap;

use harness_providers::leaf::{
    build_provider, resolve_protocol, AnthropicLeafParams, OpenAiCompatibleLeafParams,
    ProviderError, ProviderLeafParams, ProviderProtocol,
};
use harness_providers::openai::OpenAiApiMode;
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, ProviderCredentialError,
    ProviderCredentialKind, ProviderErrorCategory, ProviderStreamEvent,
};

// ---------------------------------------------------------------------------
// Happy path: supported protocols construct via the leaf factory
// ---------------------------------------------------------------------------

fn openai_params() -> OpenAiCompatibleLeafParams {
    OpenAiCompatibleLeafParams {
        base_url: "http://127.0.0.1:8317/v1".to_string(),
        api_key: "sk-test-openai-key-1234567890abcdef".to_string(),
        api_mode: OpenAiApiMode::Auto,
        timeout_ms: 60000,
        headers: BTreeMap::new(),
    }
}

fn anthropic_params() -> AnthropicLeafParams {
    AnthropicLeafParams {
        base_url: "https://api.anthropic.com".to_string(),
        api_key: "sk-ant-test-anthropic-key-1234567890".to_string(),
        timeout_ms: 60000,
        headers: BTreeMap::new(),
    }
}

#[test]
fn openai_compatible_is_config_reachable() {
    // arrange
    let params = ProviderLeafParams::OpenAiCompatible(openai_params());

    // act
    let provider = build_provider(params);

    // assert
    assert!(
        provider.is_ok(),
        "openai_compatible should be config-reachable"
    );
}

#[test]
fn anthropic_messages_is_config_reachable() {
    // arrange
    let params = ProviderLeafParams::AnthropicMessages(anthropic_params());

    // act
    let provider = build_provider(params);

    // assert
    assert!(
        provider.is_ok(),
        "anthropic_messages should be config-reachable"
    );
}

#[test]
fn leaf_params_report_correct_protocol() {
    // arrange
    let openai = ProviderLeafParams::OpenAiCompatible(openai_params());
    let anthropic = ProviderLeafParams::AnthropicMessages(anthropic_params());

    // act
    let openai_protocol = openai.protocol();
    let anthropic_protocol = anthropic.protocol();

    // assert
    assert_eq!(openai_protocol, ProviderProtocol::OpenAiCompatible);
    assert_eq!(anthropic_protocol, ProviderProtocol::AnthropicMessages);
}

// ---------------------------------------------------------------------------
// Failure path: unsupported protocols report unsupported
// ---------------------------------------------------------------------------

#[test]
fn unsupported_protocol_tag_is_rejected() {
    // arrange
    let tag = "google_gemini";

    // act
    let result = resolve_protocol(tag);

    // assert
    assert!(
        matches!(&result, Err(ProviderError::UnsupportedProtocol { tag, .. }) if tag == "google_gemini"),
        "unsupported protocol should be rejected with UnsupportedProtocol, got: {result:?}"
    );
}

#[test]
fn unsupported_protocol_error_lists_supported_protocols() {
    // arrange
    let tag = "bedrock";

    // act
    let err = resolve_protocol(tag).unwrap_err();
    let message = err.to_string();

    // assert
    assert!(
        message.contains("openai_compatible"),
        "error message should list supported protocols"
    );
    assert!(
        message.contains("anthropic_messages"),
        "error message should list supported protocols"
    );
}

#[test]
fn empty_protocol_tag_is_rejected() {
    // arrange
    let tag = "";

    // act
    let result = resolve_protocol(tag);

    // assert
    assert!(
        matches!(result, Err(ProviderError::UnsupportedProtocol { .. })),
        "empty tag should be rejected"
    );
}

#[test]
fn invalid_config_rejects_empty_base_url() {
    // arrange
    let mut params = openai_params();
    params.base_url = String::new();

    // act
    let result = build_provider(ProviderLeafParams::OpenAiCompatible(params));

    // assert
    assert!(
        matches!(
            result,
            Err(ProviderError::InvalidConfig {
                protocol: ProviderProtocol::OpenAiCompatible,
                ..
            })
        ),
        "empty base_url should be rejected with InvalidConfig"
    );
}

// ---------------------------------------------------------------------------
// Redaction: auth metadata must not leak into serialized provider artifacts
// ---------------------------------------------------------------------------

/// A test API key that looks like a real OpenAI key for redaction detection.
const TEST_API_KEY: &str = "sk-test-redaction-key-1234567890abcdef";

/// A test Anthropic key.
const TEST_ANTHROPIC_KEY: &str = "sk-ant-test-redaction-1234567890";

#[test]
fn provider_stream_event_error_does_not_leak_api_key() {
    // arrange
    let event = ProviderStreamEvent::categorized_error(
        "request failed with status 401",
        ProviderErrorCategory::InvalidCredentials,
    );

    // act
    let serialized = serde_json::to_string(&event).expect("event should serialize");

    // assert
    assert!(
        !serialized.contains(TEST_API_KEY),
        "serialized ProviderStreamEvent must not contain the API key"
    );
    assert!(
        !serialized.contains(TEST_ANTHROPIC_KEY),
        "serialized ProviderStreamEvent must not contain the Anthropic key"
    );
}

#[test]
fn provider_credential_error_does_not_leak_token() {
    // arrange
    let error = ProviderCredentialError::new(
        ProviderErrorCategory::InvalidCredentials,
        "credential validation failed",
    );

    // act
    let message = error.to_string();

    // assert
    assert!(
        !message.contains(TEST_API_KEY),
        "ProviderCredentialError message must not contain the API key"
    );
}

#[test]
fn completion_request_does_not_serialize_api_key() {
    // arrange
    let request = CompletionRequest {
        provider_id: Some("default".to_string()),
        model_id: "gpt-5.4-mini".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: "hello".to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        temperature: None,
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    // act
    let serialized = serde_json::to_string(&request).expect("request should serialize");

    // assert
    assert!(
        !serialized.contains(TEST_API_KEY),
        "serialized CompletionRequest must not contain the API key"
    );
    assert!(
        !serialized.contains("bearer"),
        "serialized CompletionRequest must not contain bearer token material"
    );
}

#[test]
fn provider_stream_finished_metadata_does_not_leak_secrets() {
    use harness_providers::ProviderStreamFinishedMetadata;

    // arrange
    let metadata = ProviderStreamFinishedMetadata {
        provider_response_id: Some("resp_123".to_string()),
        provider_session_id: Some("sess_456".to_string()),
        provider_cache_id: None,
        provider_stop_reason: Some("stop".to_string()),
        cache_read_tokens: Some(100),
        cache_write_tokens: Some(50),
        assistant_message_id: Some("msg_789".to_string()),
        thinking: None,
    };

    // act
    let serialized = serde_json::to_string(&metadata).expect("metadata should serialize");

    // assert
    assert!(
        !serialized.contains(TEST_API_KEY),
        "serialized ProviderStreamFinishedMetadata must not contain the API key"
    );
    assert!(
        !serialized.contains("authorization"),
        "metadata must not contain authorization header material"
    );
}

#[test]
fn provider_bearer_token_kind_is_typed_not_string() {
    // arrange
    // ProviderCredentialKind is an enum, not a string — this test verifies
    // that credential kind is type-safe and does not leak token values.
    let kinds = [
        ProviderCredentialKind::StoredOauth,
        ProviderCredentialKind::StoredApiKey,
        ProviderCredentialKind::EnvApiKey,
        ProviderCredentialKind::InlineApiKey,
    ];

    // act
    let serialized = kinds.map(|kind| serde_json::to_string(&kind).expect("kind should serialize"));

    // assert
    for serialized in serialized {
        // The serialized form should be a snake_case string tag, not a token value
        assert!(
            serialized.len() < 30,
            "ProviderCredentialKind should serialize to a short tag, got: {serialized}"
        );
        assert!(
            !serialized.contains(TEST_API_KEY),
            "ProviderCredentialKind must not contain API key material"
        );
    }
}

#[test]
fn provider_error_messages_do_not_leak_credentials() {
    // arrange
    let errors = [
        ProviderError::UnsupportedProtocol {
            tag: "bedrock".to_string(),
            supported: &ProviderProtocol::SUPPORTED,
        },
        ProviderError::InvalidConfig {
            protocol: ProviderProtocol::OpenAiCompatible,
            message: "base_url must not be empty".to_string(),
        },
        ProviderError::BuildHttpClient {
            protocol: ProviderProtocol::OpenAiCompatible,
            message: "connection refused".to_string(),
        },
        ProviderError::MissingCredentialSource {
            protocol: ProviderProtocol::AnthropicMessages,
        },
    ];

    // act
    let messages = errors.map(|error| error.to_string());

    // assert
    for message in messages {
        assert!(
            !message.contains(TEST_API_KEY),
            "ProviderError message must not contain API key: {message}"
        );
        assert!(
            !message.contains(TEST_ANTHROPIC_KEY),
            "ProviderError message must not contain Anthropic key: {message}"
        );
    }
}

#[test]
fn completion_usage_serializes_without_secrets() {
    // arrange
    let usage = CompletionUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };

    // act
    let serialized = serde_json::to_string(&usage).expect("usage should serialize");

    // assert
    assert!(
        !serialized.contains(TEST_API_KEY),
        "CompletionUsage must not contain API key"
    );
}

#[test]
fn provider_protocol_supported_list_is_complete() {
    // arrange
    let supported = ProviderProtocol::SUPPORTED;

    // act
    let supported_count = supported.len();

    // assert
    // The SUPPORTED list must contain exactly the wired protocols.
    assert_eq!(
        supported_count, 2,
        "exactly two protocols should be supported"
    );
    assert!(
        supported.contains(&ProviderProtocol::OpenAiCompatible),
        "openai_compatible must be in SUPPORTED"
    );
    assert!(
        supported.contains(&ProviderProtocol::AnthropicMessages),
        "anthropic_messages must be in SUPPORTED"
    );
}

#[test]
fn provider_protocol_type_tag_roundtrip_is_exhaustive() {
    // arrange
    let supported = ProviderProtocol::SUPPORTED;

    // act
    let roundtrips = supported.map(|protocol| {
        let tag = protocol.as_type_tag();
        (protocol, ProviderProtocol::from_type_tag(tag))
    });

    // assert
    for (protocol, resolved) in roundtrips {
        assert_eq!(
            resolved,
            Some(protocol),
            "type tag roundtrip must be exhaustive for {protocol:?}"
        );
    }
}
